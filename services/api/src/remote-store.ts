import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync, type StatementSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import {
  parseRemoteBinding,
  parseRemoteReady,
  type AgentPlugin,
  type RemoteBinding,
  type RemoteCommand,
  type RemotePluginsResponse,
  type RemoteReady,
} from "@agent-pulse/contracts";
import {
  normalizePluginList,
  queryRunnerPluginsResponse,
  updateSingleRunnerPluginStatus,
  upsertRunnerPluginsRow,
} from "./remote-plugins.js";
import {
  REMOTE_STORE_SCHEMA,
  RUNNER_HELLO_LIST_FIELDS,
  RemoteStoreError,
  commandDigest,
  dehydrateRemoteCommandBody,
  isSameProviderRebind,
  isWorkspaceAuthorizedInHello,
  parseRemoteReceipt,
  projectRunnerHello,
  requireOnlyKeys,
  storableFrame,
  validateRemoteApprovalEvent,
  validateRemoteEventFields,
  validateRemoteEventRaw,
  validateRemoteMetaFrame,
  validateRemoteQuestionEvent,
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

type CachedRunnerHello = {
  owner: string;
  full: Record<string, unknown> | null;
  thin: Record<string, unknown> | null;
};
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
    this.migrateCommandHub();
  }

  private stmt(sql: string): StatementSync {
    let cached = this.statements.get(sql);
    if (cached === undefined) {
      cached = this.db.prepare(sql);
      this.statements.set(sql, cached);
    }
    return cached;
  }

  private migrateCommandHub(): void {
    const columns = this.db
      .prepare("PRAGMA table_info(remote_commands)")
      .all() as Array<{ name: string }>;
    if (!columns.some((column) => column.name === "hub")) {
      this.db.exec("ALTER TABLE remote_commands ADD COLUMN hub TEXT");
      const rows = this.db
        .prepare("SELECT body FROM remote_commands")
        .all() as Array<{ body: string }>;
      const backfill = this.db.prepare(
        "UPDATE remote_commands SET hub=? WHERE body=?",
      );
      for (const row of rows) {
        let hub: string | null = null;
        try {
          hub = (JSON.parse(row.body) as RemoteCommand).hub_session_id ?? null;
        } catch {
          // leave null on unparseable rows
        }
        backfill.run(hub, row.body);
      }
    }
    if (!columns.some((column) => column.name === "cmd_digest")) {
      this.db.exec("ALTER TABLE remote_commands ADD COLUMN cmd_digest TEXT");
    }
    this.db.exec(
      "CREATE INDEX IF NOT EXISTS remote_command_session ON remote_commands(runner,hub,seq)",
    );
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

  deleteRunnerPlugins(runner: string): void {
    this.stmt("DELETE FROM remote_runner_plugins WHERE runner=?").run(runner);
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
    for (const runner of this.thinRunners(owner)) {
      const name = runner.hello?.device_name;
      if (typeof name === "string" && name.trim() !== "") {
        names.set(runner.runner_id, name.trim());
      }
    }
    return names;
  }

  recordDeviceName(owner: string, runner: string, deviceName: string): void {
    const trimmed = deviceName.trim();
    if (!trimmed) return;
    this.stmt(
      "INSERT OR IGNORE INTO remote_runners(runner,owner) VALUES (?,?)",
    ).run(runner, owner);
    const row = this.stmt(
      "SELECT hello FROM remote_runners WHERE runner=?",
    ).get(runner) as { hello: string | null } | undefined;
    let helloObj: Record<string, unknown> = {};
    if (row?.hello) {
      try {
        helloObj = JSON.parse(String(row.hello)) as Record<string, unknown>;
      } catch {
        helloObj = {};
      }
    }
    helloObj.device_name = trimmed;
    this.stmt("UPDATE remote_runners SET hello=? WHERE runner=?").run(
      JSON.stringify(helloObj),
      runner,
    );
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
      if (staleSeqs.length > 0) {
        const placeholders = staleSeqs.map(() => "?").join(",");
        this.stmt(
          `DELETE FROM remote_commands WHERE runner=? AND seq IN (${placeholders})`,
        ).run(runner, ...staleSeqs);
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
      "SELECT seq,body FROM remote_commands WHERE runner=? AND seq>? ORDER BY seq LIMIT 128",
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
    this.register(owner, runner);
    let committedCount: number | undefined;
    let helloMutated = false;
    try {
      const receivedWatermark = this.transaction(() => {
        const watermarks = this.watermarks(runner);
        if (!Number.isSafeInteger(seq) || seq < 1)
          throw new RemoteStoreError("invalid_sequence");
        if (seq <= watermarks.received) {
          const old = this.stmt(
            "SELECT payload FROM remote_events WHERE runner=? AND runner_seq=?",
          ).get(runner, seq);
          if (
            !old ||
            !isDeepStrictEqual(
              JSON.parse(String(old.payload)),
              storableFrame(payload),
            )
          )
            throw new RemoteStoreError("sequence_conflict");
          return watermarks.received;
        }
        if (seq !== watermarks.received + 1)
          throw new RemoteStoreError("sequence_gap");
        let binding: RemoteBinding | undefined;
        let hub: string | null = null;
        if (payload.type === "event") {
          requireOnlyKeys(payload, ["type", "event"], "invalid_event");
          const event = payload.event as Record<string, unknown> | undefined;
          if (!event || typeof event !== "object" || Array.isArray(event))
            throw new RemoteStoreError("invalid_event");
          requireOnlyKeys(
            event,
            [
              "event_id",
              "host_generation",
              "binding",
              "hub_session_id",
              "provider_session_id",
              "kind",
              "raw",
              "approval_id",
              "approval",
              "question_id",
              "question",
            ],
            "invalid_event",
          );
          validateRemoteEventFields(event);
          const isApprovalEvent =
            event.kind === "approval_requested" ||
            event.kind === "approval_resolved" ||
            event.kind === "unsupported_provider_request";
          const isQuestionEvent =
            event.kind === "question_requested" ||
            event.kind === "question_resolved";
          if (isApprovalEvent) validateRemoteApprovalEvent(event);
          if (isQuestionEvent) validateRemoteQuestionEvent(event);
          validateRemoteEventRaw(event, isApprovalEvent || isQuestionEvent);
          if (event.binding) binding = parseRemoteBinding(event.binding);
          hub =
            typeof event.hub_session_id === "string"
              ? event.hub_session_id
              : null;
          if (binding && hub !== binding.hub_session_id)
            throw new RemoteStoreError("binding_conflict");
          if (
            binding &&
            activeGeneration !== undefined &&
            binding.host_generation !== activeGeneration
          )
            binding = undefined;
        } else if (payload.type === "receipt") {
          const receipt = parseRemoteReceipt(payload.receipt);
          const queued = this.stmt(
            "SELECT body FROM remote_commands WHERE runner=? AND cmd_id=?",
          ).get(runner, receipt.client_cmd_id);
          if (!queued) throw new RemoteStoreError("unknown_command_receipt");
          const rawBody = String(queued.body);
          const command = JSON.parse(rawBody) as RemoteCommand;
          if (receipt.hub_session_id !== command.hub_session_id)
            throw new RemoteStoreError("receipt_session_conflict");
          hub = command.hub_session_id;
          if (receipt.result?.binding) binding = receipt.result.binding;
          if (binding && binding.hub_session_id !== hub)
            throw new RemoteStoreError("binding_conflict");
          if (
            binding &&
            activeGeneration !== undefined &&
            binding.host_generation !== activeGeneration
          )
            binding = undefined;
          const dehydrated = dehydrateRemoteCommandBody(rawBody);
          this.stmt(
            "UPDATE remote_commands SET receipt=?, body=? WHERE runner=? AND cmd_id=?",
          ).run(
            JSON.stringify(receipt),
            dehydrated,
            runner,
            receipt.client_cmd_id,
          );
        } else if (payload.type === "ready") {
          let ready: RemoteReady;
          try {
            ready = parseRemoteReady(payload);
          } catch {
            throw new RemoteStoreError("invalid_ready");
          }
          const sessions = ready.sessions;
          if (
            activeGeneration === undefined ||
            ready.host_generation === activeGeneration
          ) {
            const existing = this.runnerBindings(owner, runner);
            const currentHubs = new Set(
              sessions.map((session) => session.hub_session_id),
            );
            const updates: RemoteBinding[] = [];
            for (const binding of existing.values()) {
              if (!currentHubs.has(binding.hub_session_id))
                updates.push({
                  ...binding,
                  control_connected: false,
                  execution_state: "unknown",
                  current_turn_id: null,
                });
            }
            for (const session of sessions) {
              this.assertNativeIdentity(existing, session);
              updates.push(session);
            }
            this.writeBindings(owner, runner, updates);
            this.stmt("UPDATE remote_runners SET hello=? WHERE runner=?").run(
              JSON.stringify(payload),
              runner,
            );
            if ("plugins" in payload) {
              upsertRunnerPluginsRow(
                (sql) => this.stmt(sql),
                owner,
                runner,
                undefined,
                normalizePluginList(payload.plugins, runner),
              );
              this.notifyPlugins(owner, runner);
            }
            helloMutated = true;
          }
        } else if (payload.type === "plugin_status") {
          validateRemoteMetaFrame(payload);
          const pluginId = String(payload.plugin_id);
          const runtimeStatus = String(
            payload.runtime_status ?? "idle",
          ) as AgentPlugin["runtimeStatus"];
          const err =
            typeof payload.error === "string" ? payload.error : undefined;
          updateSingleRunnerPluginStatus(
            (sql) => this.stmt(sql),
            owner,
            runner,
            pluginId,
            runtimeStatus,
            err,
          );
        } else {
          validateRemoteMetaFrame(payload);
        }
        if (binding) this.saveBinding(owner, runner, binding);
        if (
          payload.type === "host_lost" &&
          (activeGeneration === undefined ||
            payload.host_generation === activeGeneration)
        ) {
          const existing = this.runnerBindings(owner, runner);
          this.writeBindings(
            owner,
            runner,
            [...existing.values()].map((binding) => ({
              ...binding,
              control_connected: false,
              execution_state: "unknown",
            })),
          );
        }
        this.stmt(
          "INSERT INTO remote_events(owner,runner,runner_seq,hub,payload) VALUES (?,?,?,?,?)",
        ).run(owner, runner, seq, hub, JSON.stringify(storableFrame(payload)));
        let count = this.runnerEventCounts.get(runner);
        if (count === undefined) {
          const row = this.stmt(
            "SELECT COUNT(*) AS cnt FROM remote_events WHERE runner=?",
          ).get(runner) as { cnt: number } | undefined;
          count = Number(row?.cnt ?? 0);
        } else {
          count += 1;
        }
        if (count > this.retention) {
          const excess = count - this.retention;
          const deleted = this.stmt(
            "DELETE FROM remote_events WHERE id IN (SELECT id FROM remote_events WHERE runner=? ORDER BY id ASC LIMIT ?)",
          ).run(runner, excess);
          count -= Number(deleted.changes);
        }
        committedCount = count;
        this.stmt("UPDATE remote_runners SET received=? WHERE runner=?").run(
          seq,
          runner,
        );
        return seq;
      });
      if (committedCount !== undefined) {
        this.runnerEventCounts.set(runner, committedCount);
      }
      if (helloMutated) {
        this.cacheRunnerHello(runner, {
          owner,
          full: payload,
          thin: projectRunnerHello(payload),
        });
      }
      this.notify(owner);
      return receivedWatermark;
    } catch (error) {
      this.runnerEventCounts.delete(runner);
      if (helloMutated) this.runnerHelloCache.delete(runner);
      throw error;
    }
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
    let transcript: number | null = null;
    const hubIter = this.stmt(
      `SELECT id,payload FROM remote_events
         WHERE owner=? AND runner=? AND hub=?
         ORDER BY id DESC LIMIT ${SNAPSHOT_LOOKBACK}`,
    ).iterate(owner, runner, hub) as Iterable<{ id: number; payload: string }>;
    for (const row of hubIter) {
      if (row.payload.includes('"kind":"history/snapshot"')) {
        transcript = Number(row.id);
        break;
      }
    }
    let approvals: number | null = null;
    const runnerIter = this.stmt(
      `SELECT id,payload FROM remote_events
         WHERE owner=? AND runner=? AND hub IS NULL
         ORDER BY id DESC LIMIT ${SNAPSHOT_LOOKBACK}`,
    ).iterate(owner, runner) as Iterable<{ id: number; payload: string }>;
    for (const row of runnerIter) {
      if (row.payload.includes('"pending_approvals"')) {
        approvals = Number(row.id);
        break;
      }
    }
    return { transcript, approvals };
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
      this.stmt(
        `INSERT INTO remote_sessions(owner,runner,hub,native,binding) VALUES ${placeholders} ON CONFLICT(runner,hub) DO UPDATE SET native=excluded.native, binding=excluded.binding`,
      ).run(...values);
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
      this.db.exec("ROLLBACK");
      throw error;
    }
  }
}
