import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync, type StatementSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import {
  type AgentPlugin,
  type RemoteBinding,
  type RemoteCommand,
  type RemotePluginsResponse,
} from "@agent-pulse/contracts";
import {
  normalizePluginList,
  queryRunnerPluginsResponse,
  upsertRunnerPluginsRow,
} from "./remote-plugins.js";
import {
  findRemoteResumePoint,
  migrateRemoteStoreSchema,
  receiveRemoteFrame,
  type CachedRunnerHello,
  type RemoteIngestContext,
} from "./remote-store-ingest.js";
import {
  REMOTE_STORE_SCHEMA,
  RUNNER_HELLO_LIST_FIELDS,
  RemoteStoreError,
  commandDigest,
  isSameProviderRebind,
  isWorkspaceAuthorizedInHello,
  projectRunnerHello,
} from "./remote-store-validation.js";

export { RUNNER_HELLO_LIST_FIELDS, RemoteStoreError, projectRunnerHello };

export interface RemoteSession {
  runner_id: string;
  binding: RemoteBinding;
}
export interface ContentEvent {
  cursor: number;
  runner_id: string;
  payload: Record<string, unknown>;
}

const BINDING_INSERT_COLUMNS = 5;
const BINDING_INSERT_PARAMETER_LIMIT = 32_000;
const SNAPSHOT_LOOKBACK = 2000;
const EVENT_RETENTION = 20_000;
const MAX_HELLO_CACHE_ENTRIES = 256;

export type RemoteChangeListener = (owner: string) => void;
export type RemotePluginChangeListener = (
  owner: string,
  runner: string,
) => void;

export class RemoteContentStore {
  private db: DatabaseSync;
  private closed = false;
  private readonly listeners = new Set<RemoteChangeListener>();
  private readonly pluginListeners = new Set<RemotePluginChangeListener>();
  private readonly retention: number;
  private readonly statements = new Map<string, StatementSync>();
  private readonly runnerEventCounts = new Map<string, number>();
  private readonly runnerHelloCache = new Map<string, CachedRunnerHello>();

  constructor(path: string, eventRetention: number = EVENT_RETENTION) {
    this.retention = Math.max(SNAPSHOT_LOOKBACK, eventRetention);
    const dir = dirname(path);
    if (dir && dir !== "." && dir !== "") {
      mkdirSync(dir, { recursive: true });
    }
    this.db = new DatabaseSync(path);
    this.db.exec(REMOTE_STORE_SCHEMA);
    migrateRemoteStoreSchema(this.db);
  }

  private stmt(sql: string): StatementSync {
    let cached = this.statements.get(sql);
    if (cached === undefined) {
      cached = this.db.prepare(sql);
      this.statements.set(sql, cached);
    }
    return cached;
  }

  saveRunnerPlugins(
    owner: string,
    runner: string,
    registryUrl: string | undefined,
    plugins: AgentPlugin[],
  ): void {
    upsertRunnerPluginsRow(
      (sql) => this.stmt(sql),
      owner,
      runner,
      registryUrl,
      plugins,
    );
    this.notifyPlugins(owner, runner);
    this.notify(owner);
  }

  deleteRunnerPlugins(runner: string, expectedOwner?: string): void {
    if (expectedOwner !== undefined) {
      this.stmt(
        "DELETE FROM remote_runner_plugins WHERE runner=? AND owner=?",
      ).run(runner, expectedOwner);
      return;
    }
    this.stmt("DELETE FROM remote_runner_plugins WHERE runner=?").run(runner);
  }

  runnerOwner(runner: string): string | undefined {
    const row = this.stmt(
      "SELECT owner FROM remote_runners WHERE runner=?",
    ).get(runner) as { owner: string } | undefined;
    return row ? String(row.owner) : undefined;
  }

  onPluginChange(listener: RemotePluginChangeListener): () => void {
    this.pluginListeners.add(listener);
    return () => {
      this.pluginListeners.delete(listener);
    };
  }

  private notifyPlugins(owner: string, runner: string): void {
    for (const listener of [...this.pluginListeners]) {
      try {
        listener(owner, runner);
      } catch {
        // Plugin SSE wakeup failures must never break persistence.
      }
    }
  }

  listRunnerPlugins(
    owner: string,
    runnerFilter?: string,
  ): RemotePluginsResponse {
    return queryRunnerPluginsResponse(
      (sql) => this.stmt(sql),
      owner,
      runnerFilter,
    );
  }

  onChange(listener: RemoteChangeListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private notify(owner: string): void {
    for (const listener of [...this.listeners]) {
      try {
        listener(owner);
      } catch {
        // Reader wakeup failures must never break durable Runner frame handling.
      }
    }
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.statements.clear();
    this.runnerEventCounts.clear();
    this.runnerHelloCache.clear();
    this.db.close();
  }

  register(owner: string, runner: string): void {
    const prior = this.stmt(
      "SELECT owner FROM remote_runners WHERE runner=?",
    ).get(runner);
    if (prior && prior.owner !== owner)
      throw new RemoteStoreError("runner_owner_conflict");
    if (!prior) {
      this.stmt(
        "INSERT OR IGNORE INTO remote_runners(runner,owner) VALUES (?,?)",
      ).run(runner, owner);
      this.runnerHelloCache.delete(runner);
    }
  }

  watermarks(runner: string): {
    received: number;
    command_seq: number;
    command_ack: number;
  } {
    const row = this.stmt(
      "SELECT received,command_seq,command_ack FROM remote_runners WHERE runner=?",
    ).get(runner);
    if (!row) throw new RemoteStoreError("runner_not_found");
    return {
      received: Number(row.received),
      command_seq: Number(row.command_seq),
      command_ack: Number(row.command_ack),
    };
  }

  private cacheRunnerHello(runner: string, entry: CachedRunnerHello): void {
    this.runnerHelloCache.delete(runner);
    if (this.runnerHelloCache.size >= MAX_HELLO_CACHE_ENTRIES) {
      const oldest = this.runnerHelloCache.keys().next().value;
      if (oldest !== undefined) this.runnerHelloCache.delete(oldest);
    }
    this.runnerHelloCache.set(runner, entry);
  }

  private loadRunnerHelloEntry(runner: string): CachedRunnerHello | undefined {
    const cached = this.runnerHelloCache.get(runner);
    if (cached !== undefined) return cached;
    const row = this.stmt(
      "SELECT owner,hello FROM remote_runners WHERE runner=?",
    ).get(runner) as { owner: string; hello: string | null } | undefined;
    if (!row) return undefined;
    let full: Record<string, unknown> | null = null;
    if (row.hello) {
      try {
        full = JSON.parse(String(row.hello)) as Record<string, unknown>;
      } catch {
        full = null;
      }
    }
    const entry: CachedRunnerHello = {
      owner: String(row.owner),
      full,
      thin: projectRunnerHello(full),
    };
    this.cacheRunnerHello(runner, entry);
    return entry;
  }

  runners(
    owner: string,
  ): Array<{ runner_id: string; hello: Record<string, unknown> | null }> {
    const rows = this.stmt(
      "SELECT runner FROM remote_runners WHERE owner=? ORDER BY runner",
    ).all(owner) as Array<{ runner: string }>;
    return rows.map((row) => {
      const id = String(row.runner);
      const entry = this.loadRunnerHelloEntry(id);
      return {
        runner_id: id,
        hello: entry && entry.owner === owner ? entry.full : null,
      };
    });
  }

  /**
   * Returns each Runner for `owner` with only the list-projected `hello` fields,
   * served from an in-memory cache so polling `/v3/remote/sessions` never copies
   * or `JSON.parse`s multi-hundred-kilobyte `hello` blobs from SQLite.
   */
  thinRunners(
    owner: string,
  ): Array<{ runner_id: string; hello: Record<string, unknown> | null }> {
    const rows = this.stmt(
      "SELECT runner FROM remote_runners WHERE owner=? ORDER BY runner",
    ).all(owner) as Array<{ runner: string }>;
    return rows.map((row) => {
      const id = String(row.runner);
      const entry = this.loadRunnerHelloEntry(id);
      return {
        runner_id: id,
        hello:
          entry && entry.owner === owner && entry.thin
            ? { ...entry.thin }
            : null,
      };
    });
  }

  deviceNames(owner: string): Map<string, string> {
    const names = new Map<string, string>();
    const rows = this.stmt(
      "SELECT runner, device_name FROM remote_runners WHERE owner=? ORDER BY runner",
    ).all(owner) as Array<{ runner: string; device_name: string | null }>;
    for (const row of rows) {
      const id = String(row.runner);
      const entry = this.loadRunnerHelloEntry(id);
      const helloName =
        entry && entry.owner === owner ? entry.thin?.device_name : undefined;
      if (typeof helloName === "string" && helloName.trim() !== "") {
        names.set(id, helloName.trim());
      } else if (
        typeof row.device_name === "string" &&
        row.device_name.trim() !== ""
      ) {
        names.set(id, row.device_name.trim());
      }
    }
    return names;
  }

  recordDeviceName(owner: string, runner: string, deviceName: string): void {
    const trimmed = deviceName.trim();
    if (!trimmed) return;
    this.register(owner, runner);
    this.stmt(
      "UPDATE remote_runners SET device_name=? WHERE runner=? AND owner=?",
    ).run(trimmed, runner, owner);
    const row = this.stmt(
      "SELECT hello FROM remote_runners WHERE runner=? AND owner=?",
    ).get(runner, owner) as { hello: string | null } | undefined;
    if (row?.hello) {
      try {
        const helloObj = JSON.parse(String(row.hello)) as Record<
          string,
          unknown
        >;
        if (
          typeof helloObj.host_generation === "string" &&
          helloObj.host_generation.trim() !== ""
        ) {
          helloObj.device_name = trimmed;
          this.stmt(
            "UPDATE remote_runners SET hello=? WHERE runner=? AND owner=?",
          ).run(JSON.stringify(helloObj), runner, owner);
        }
      } catch {
        // leave malformed hello untouched
      }
    }
    this.runnerHelloCache.delete(runner);
  }

  sessions(owner: string): RemoteSession[] {
    return this.stmt(
      "SELECT runner,binding FROM remote_sessions WHERE owner=? ORDER BY runner,hub",
    )
      .all(owner)
      .map((row) => ({
        runner_id: String(row.runner),
        binding: JSON.parse(String(row.binding)) as RemoteBinding,
      }));
  }

  session(
    owner: string,
    runner: string,
    hub: string,
  ): RemoteBinding | undefined {
    const row = this.stmt(
      "SELECT binding FROM remote_sessions WHERE owner=? AND runner=? AND hub=?",
    ).get(owner, runner, hub);
    return row ? (JSON.parse(String(row.binding)) as RemoteBinding) : undefined;
  }

  workspaceStatus(
    owner: string,
    runner: string,
    generation: string,
    workspace: string,
  ):
    | "ready"
    | "runner_not_ready"
    | "host_generation_mismatch"
    | "workspace_not_authorized" {
    const hello = this.runnerHello(owner, runner);
    if (!hello) return "runner_not_ready";
    if (hello.host_generation !== generation) return "host_generation_mismatch";
    return isWorkspaceAuthorizedInHello(hello, workspace)
      ? "ready"
      : "workspace_not_authorized";
  }

  supersedeStaleCommands(
    runner: string,
    generation: string,
  ): { commandSeq: number; dropped: number } {
    return this.transaction(() => {
      const pending = this.stmt(
        "SELECT seq,body FROM remote_commands WHERE runner=? AND receipt IS NULL ORDER BY seq",
      ).all(runner) as Array<{ seq: number; body: string }>;
      const staleSeqs: number[] = [];
      for (const row of pending) {
        let addressedTo: unknown;
        try {
          addressedTo = (JSON.parse(row.body) as RemoteCommand).host_generation;
        } catch {
          addressedTo = undefined;
        }
        if (addressedTo !== generation) staleSeqs.push(Number(row.seq));
      }
      for (let offset = 0; offset < staleSeqs.length; offset += 500) {
        const batch = staleSeqs.slice(offset, offset + 500);
        const placeholders = batch.map(() => "?").join(",");
        this.db
          .prepare(
            `DELETE FROM remote_commands WHERE runner=? AND seq IN (${placeholders})`,
          )
          .run(runner, ...batch);
      }
      const maxRow = this.stmt(
        "SELECT COALESCE(MAX(seq),0) AS seq FROM remote_commands WHERE runner=?",
      ).get(runner) as { seq: number };
      const commandSeq = Number(maxRow.seq);
      if (staleSeqs.length > 0) {
        this.stmt(
          "UPDATE remote_runners SET command_seq=?, command_ack=MIN(command_ack,?) WHERE runner=?",
        ).run(commandSeq, commandSeq, runner);
      }
      return { commandSeq, dropped: staleSeqs.length };
    });
  }

  submit(
    owner: string,
    runner: string,
    command: RemoteCommand,
  ): { seq: number; command: RemoteCommand } {
    this.register(owner, runner);
    const digest = commandDigest(command);
    return this.transaction(() => {
      const old = this.stmt(
        "SELECT seq,cmd_digest,body FROM remote_commands WHERE runner=? AND cmd_id=?",
      ).get(runner, command.client_cmd_id) as
        { seq: number; cmd_digest: string | null; body: string } | undefined;
      if (old) {
        const same =
          old.cmd_digest !== null
            ? old.cmd_digest === digest
            : isDeepStrictEqual(JSON.parse(String(old.body)), command);
        if (!same) throw new RemoteStoreError("command_id_conflict");
        return { seq: Number(old.seq), command };
      }
      const seq = this.watermarks(runner).command_seq + 1;
      this.stmt(
        "INSERT INTO remote_commands(runner,cmd_id,seq,hub,cmd_digest,body) VALUES (?,?,?,?,?,?)",
      ).run(
        runner,
        command.client_cmd_id,
        seq,
        command.hub_session_id,
        digest,
        JSON.stringify(command),
      );
      this.stmt("UPDATE remote_runners SET command_seq=? WHERE runner=?").run(
        seq,
        runner,
      );
      return { seq, command };
    });
  }

  commands(
    runner: string,
    after: number,
  ): Array<{ seq: number; command: RemoteCommand }> {
    return this.stmt(
      "SELECT seq,body FROM remote_commands WHERE runner=? AND seq>? AND receipt IS NULL ORDER BY seq LIMIT 128",
    )
      .all(runner, after)
      .map((row) => ({
        seq: Number(row.seq),
        command: JSON.parse(String(row.body)) as RemoteCommand,
      }));
  }

  existingCommand(
    owner: string,
    runner: string,
    command: RemoteCommand,
  ): number | undefined {
    this.register(owner, runner);
    const old = this.stmt(
      "SELECT seq,cmd_digest,body FROM remote_commands WHERE runner=? AND cmd_id=?",
    ).get(runner, command.client_cmd_id) as
      { seq: number; cmd_digest: string | null; body: string } | undefined;
    if (!old) return undefined;
    const same =
      old.cmd_digest !== null
        ? old.cmd_digest === commandDigest(command)
        : isDeepStrictEqual(JSON.parse(String(old.body)), command);
    if (!same) throw new RemoteStoreError("command_id_conflict");
    return Number(old.seq);
  }

  commandHistory(
    owner: string,
    runner: string,
    hub: string,
  ): Array<{ command: RemoteCommand; receipt: unknown }> {
    if (!this.thinRunners(owner).some((item) => item.runner_id === runner))
      return [];
    return this.commandHistoryRows(runner, hub)
      .reverse()
      .map((row) => ({
        command: JSON.parse(String(row.body)) as RemoteCommand,
        receipt: row.receipt
          ? (JSON.parse(String(row.receipt)) as unknown)
          : null,
      }));
  }

  private commandHistoryRows(
    runner: string,
    hub: string,
  ): Array<{ body: unknown; receipt: unknown }> {
    return this.stmt(
      "SELECT body,receipt FROM remote_commands WHERE runner=? AND hub=? ORDER BY seq DESC LIMIT 200",
    ).all(runner, hub) as Array<{ body: unknown; receipt: unknown }>;
  }

  acknowledgeCommands(runner: string, seq: number, force = false): void {
    const watermarks = this.watermarks(runner);
    if (
      !Number.isSafeInteger(seq) ||
      (!force && seq < watermarks.command_ack) ||
      seq > watermarks.command_seq
    )
      throw new RemoteStoreError("invalid_command_ack");
    this.stmt("UPDATE remote_runners SET command_ack=? WHERE runner=?").run(
      seq,
      runner,
    );
  }

  receive(
    owner: string,
    runner: string,
    seq: number,
    payload: Record<string, unknown>,
    activeGeneration?: string,
  ): number {
    const ctx: RemoteIngestContext = {
      retention: this.retention,
      runnerEventCounts: this.runnerEventCounts,
      runnerHelloCache: this.runnerHelloCache,
      stmt: (sql) => this.stmt(sql),
      register: (o, r) => this.register(o, r),
      watermarks: (r) => this.watermarks(r),
      transaction: (work) => this.transaction(work),
      runnerBindings: (o, r) => this.runnerBindings(o, r),
      assertNativeIdentity: (existing, incoming) =>
        this.assertNativeIdentity(existing, incoming),
      writeBindings: (o, r, bindings) => this.writeBindings(o, r, bindings),
      saveBinding: (o, r, binding) => this.saveBinding(o, r, binding),
      cacheRunnerHello: (r, entry) => this.cacheRunnerHello(r, entry),
      notify: (o) => this.notify(o),
      notifyPlugins: (o, r) => this.notifyPlugins(o, r),
    };
    return receiveRemoteFrame(
      ctx,
      owner,
      runner,
      seq,
      payload,
      activeGeneration,
    );
  }

  events(
    owner: string,
    after: number,
    runner?: string,
    hub?: string,
    limit = 256,
  ): ContentEvent[] {
    const boundedLimit = Math.min(Math.max(1, Number(limit) || 256), 1000);
    const rows =
      runner !== undefined && hub !== undefined
        ? this.stmt(
            `SELECT id,runner,payload FROM (
               SELECT id,runner,payload FROM remote_events
                 WHERE owner=? AND runner=? AND hub=? AND id>?
               UNION ALL
               SELECT id,runner,payload FROM remote_events
                 WHERE owner=? AND runner=? AND hub IS NULL AND id>?
             ) ORDER BY id LIMIT ?`,
          ).all(owner, runner, hub, after, owner, runner, after, boundedLimit)
        : this.stmt(
            "SELECT id,runner,payload FROM remote_events WHERE owner=? AND id>? AND (? IS NULL OR runner=?) ORDER BY id LIMIT ?",
          ).all(owner, after, runner ?? null, runner ?? null, boundedLimit);
    return rows.map((row) => ({
      cursor: Number(row.id),
      runner_id: String(row.runner),
      payload: JSON.parse(String(row.payload)) as Record<string, unknown>,
    }));
  }

  minCursor(owner: string, runner?: string): number | null {
    const row = this.stmt(
      "SELECT MIN(id) AS min_id FROM remote_events WHERE owner=? AND (? IS NULL OR runner=?)",
    ).get(owner, runner ?? null, runner ?? null) as
      { min_id: number | null } | undefined;
    return row?.min_id == null ? null : Number(row.min_id);
  }

  sessionCursor(owner: string, runner: string, hub: string): number {
    return Number(
      this.stmt(
        `SELECT COALESCE(MAX(id),0) AS cursor FROM (
           SELECT MAX(id) AS id FROM remote_events
             WHERE owner=? AND runner=? AND hub=?
           UNION ALL
           SELECT MAX(id) FROM remote_events
             WHERE owner=? AND runner=? AND hub IS NULL
         )`,
      ).get(owner, runner, hub, owner, runner)?.cursor ?? 0,
    );
  }

  /**
   * Two indexed reverse cursor walks on `remote_event_session(owner,runner,hub,id)`
   * that stop on the first hit instead of scanning `remote_event_owner` across
   * multi-megabyte payloads when `(hub=? OR hub IS NULL)` defeats the session index.
   */
  resumePoint(
    owner: string,
    runner: string,
    hub: string,
  ): { transcript: number | null; approvals: number | null } {
    return findRemoteResumePoint(
      (sql) => this.stmt(sql),
      owner,
      runner,
      hub,
      SNAPSHOT_LOOKBACK,
    );
  }

  cursor(owner: string): number {
    return Number(
      this.stmt(
        "SELECT COALESCE(MAX(id),0) AS cursor FROM remote_events WHERE owner=?",
      ).get(owner)?.cursor ?? 0,
    );
  }

  runnerHello(owner: string, runner: string): Record<string, unknown> | null {
    const entry = this.loadRunnerHelloEntry(runner);
    if (!entry || entry.owner !== owner) return null;
    return entry.full;
  }

  private runnerBindings(
    owner: string,
    runner: string,
  ): Map<string, RemoteBinding> {
    const rows = this.stmt(
      "SELECT hub,binding FROM remote_sessions WHERE owner=? AND runner=?",
    ).all(owner, runner);
    const found = new Map<string, RemoteBinding>();
    for (const row of rows) {
      found.set(
        String(row.hub),
        JSON.parse(String(row.binding)) as RemoteBinding,
      );
    }
    return found;
  }

  private assertNativeIdentity(
    existing: ReadonlyMap<string, RemoteBinding>,
    incoming: RemoteBinding,
  ): void {
    const previous = existing.get(incoming.hub_session_id);
    if (
      previous &&
      previous.provider_session_id !== incoming.provider_session_id &&
      !isSameProviderRebind(
        previous.provider_session_id,
        incoming.provider_session_id,
      )
    )
      throw new RemoteStoreError("native_identity_changed");
  }

  private writeBindings(
    owner: string,
    runner: string,
    bindings: readonly RemoteBinding[],
  ): void {
    if (bindings.length === 0) return;
    const merged = new Map<string, RemoteBinding>();
    for (const binding of bindings) {
      merged.set(binding.hub_session_id, binding);
    }
    const rows = [...merged.values()];
    const perStatement = Math.max(
      1,
      Math.floor(BINDING_INSERT_PARAMETER_LIMIT / BINDING_INSERT_COLUMNS),
    );
    for (let offset = 0; offset < rows.length; offset += perStatement) {
      const chunk = rows.slice(offset, offset + perStatement);
      const placeholders = chunk.map(() => "(?,?,?,?,?)").join(",");
      const values: (string | number | null)[] = [];
      for (const binding of chunk) {
        values.push(
          owner,
          runner,
          binding.hub_session_id,
          binding.provider_session_id,
          JSON.stringify(binding),
        );
      }
      const sql = `INSERT INTO remote_sessions(owner,runner,hub,native,binding) VALUES ${placeholders} ON CONFLICT(runner,hub) DO UPDATE SET native=excluded.native, binding=excluded.binding`;
      (chunk.length === 1 ? this.stmt(sql) : this.db.prepare(sql)).run(
        ...values,
      );
    }
  }

  private saveBinding(
    owner: string,
    runner: string,
    binding: RemoteBinding,
  ): void {
    const old = this.session(owner, runner, binding.hub_session_id);
    if (
      old &&
      old.provider_session_id !== binding.provider_session_id &&
      !isSameProviderRebind(
        old.provider_session_id,
        binding.provider_session_id,
      )
    )
      throw new RemoteStoreError("native_identity_changed");
    this.writeBindings(owner, runner, [binding]);
  }

  private transaction<T>(work: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const result = work();
      this.db.exec("COMMIT");
      return result;
    } catch (error) {
      try {
        this.db.exec("ROLLBACK");
      } catch {
        // Ignore rollback errors if SQLite already rolled back the transaction.
      }
      throw error;
    }
  }
}
