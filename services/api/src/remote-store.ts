import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import {
  isRemoteWorkspaceId,
  parseRemoteBinding,
  parseRemotePendingApproval,
  type RemoteBinding,
  type RemoteCommand,
} from "@agent-pulse/contracts";

export class RemoteStoreError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

export interface RemoteSession {
  runner_id: string;
  binding: RemoteBinding;
}
export interface ContentEvent {
  cursor: number;
  runner_id: string;
  payload: Record<string, unknown>;
}

/** SQLite allows 32,000 host parameters per statement; 5 per binding row. */
const BINDING_INSERT_COLUMNS = 5;
const BINDING_INSERT_PARAMETER_LIMIT = 32_000;

/** Single Gateway instance, explicit content database; not the v1 metadata store. */
export class RemoteContentStore {
  private db: DatabaseSync;
  constructor(path: string) {
    const dir = dirname(path);
    if (dir && dir !== "." && dir !== "") {
      mkdirSync(dir, { recursive: true });
    }
    this.db = new DatabaseSync(path);
    this.db
      .exec(`PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS remote_runners (runner TEXT PRIMARY KEY, owner TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, command_seq INTEGER NOT NULL DEFAULT 0, command_ack INTEGER NOT NULL DEFAULT 0, hello TEXT);
      CREATE TABLE IF NOT EXISTS remote_commands (runner TEXT NOT NULL, cmd_id TEXT NOT NULL, seq INTEGER NOT NULL, hub TEXT, body TEXT NOT NULL, receipt TEXT, PRIMARY KEY(runner,cmd_id), UNIQUE(runner,seq));
      CREATE TABLE IF NOT EXISTS remote_sessions (owner TEXT NOT NULL, runner TEXT NOT NULL, hub TEXT NOT NULL, native TEXT NOT NULL, binding TEXT NOT NULL, PRIMARY KEY(runner,hub), UNIQUE(runner,native));
      CREATE TABLE IF NOT EXISTS remote_events (id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, runner TEXT NOT NULL, runner_seq INTEGER NOT NULL, hub TEXT, payload TEXT NOT NULL, UNIQUE(runner,runner_seq));
      CREATE INDEX IF NOT EXISTS remote_event_owner ON remote_events(owner,id);
      CREATE INDEX IF NOT EXISTS remote_event_session ON remote_events(owner,runner,hub,id);`);
    this.migrateCommandHub();
  }
  /**
   * Brings an existing `remote_commands` table up to the current shape and
   * indexes the session column.
   *
   * `CREATE TABLE IF NOT EXISTS` leaves an already-created table alone, so a
   * database written by an older build has no `hub` column and no index. The
   * column is added and backfilled from `body` — the same value the read path
   * verifies against — so the first poll after an upgrade is indexed rather
   * than a full scan. `ALTER TABLE ADD COLUMN` is supported from SQLite 3.35
   * and errors if the column is already present, so both steps are guarded.
   */
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
          // A row that is not parseable has no hub to project; it stays null
          // and the read path falls back to the same parse failure as before.
        }
        backfill.run(hub, row.body);
      }
    }
    this.db.exec(
      "CREATE INDEX IF NOT EXISTS remote_command_session ON remote_commands(runner,hub,seq)",
    );
  }
  close(): void {
    this.db.close();
  }
  register(owner: string, runner: string): void {
    const prior = this.db
      .prepare("SELECT owner FROM remote_runners WHERE runner=?")
      .get(runner);
    if (prior && prior.owner !== owner)
      throw new RemoteStoreError("runner_owner_conflict");
    this.db
      .prepare(
        "INSERT OR IGNORE INTO remote_runners(runner,owner) VALUES (?,?)",
      )
      .run(runner, owner);
  }
  watermarks(runner: string): {
    received: number;
    command_seq: number;
    command_ack: number;
  } {
    const row = this.db
      .prepare(
        "SELECT received,command_seq,command_ack FROM remote_runners WHERE runner=?",
      )
      .get(runner);
    if (!row) throw new RemoteStoreError("runner_not_found");
    return {
      received: Number(row.received),
      command_seq: Number(row.command_seq),
      command_ack: Number(row.command_ack),
    };
  }
  runners(
    owner: string,
  ): Array<{ runner_id: string; hello: Record<string, unknown> | null }> {
    return this.db
      .prepare(
        "SELECT runner,hello FROM remote_runners WHERE owner=? ORDER BY runner",
      )
      .all(owner)
      .map((row) => ({
        runner_id: String(row.runner),
        hello: row.hello
          ? (JSON.parse(String(row.hello)) as Record<string, unknown>)
          : null,
      }));
  }
  sessions(owner: string): RemoteSession[] {
    return this.db
      .prepare(
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
    const row = this.db
      .prepare(
        "SELECT binding FROM remote_sessions WHERE owner=? AND runner=? AND hub=?",
      )
      .get(owner, runner, hub);
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
    const row = this.db
      .prepare("SELECT hello FROM remote_runners WHERE owner=? AND runner=?")
      .get(owner, runner);
    if (!row?.hello) return "runner_not_ready";
    const hello = JSON.parse(String(row.hello)) as Record<string, unknown>;
    if (hello.host_generation !== generation) return "host_generation_mismatch";
    const discoveryCandidates = (
      hello.discovery as { candidates?: Array<{ workspace_path?: string }> }
    )?.candidates;
    const isDiscovered =
      Array.isArray(discoveryCandidates) &&
      discoveryCandidates.some((c) => c.workspace_path === workspace);
    if (
      !isDiscovered &&
      (!Array.isArray(hello.workspaces) ||
        !hello.workspaces.includes(workspace))
    )
      return "workspace_not_authorized";
    return "ready";
  }
  submit(
    owner: string,
    runner: string,
    command: RemoteCommand,
  ): { seq: number; command: RemoteCommand } {
    this.register(owner, runner);
    return this.transaction(() => {
      const old = this.db
        .prepare(
          "SELECT seq,body FROM remote_commands WHERE runner=? AND cmd_id=?",
        )
        .get(runner, command.client_cmd_id);
      if (old) {
        if (!isDeepStrictEqual(JSON.parse(String(old.body)), command))
          throw new RemoteStoreError("command_id_conflict");
        return { seq: Number(old.seq), command };
      }
      const seq = this.watermarks(runner).command_seq + 1;
      this.db
        .prepare(
          "INSERT INTO remote_commands(runner,cmd_id,seq,hub,body) VALUES (?,?,?,?,?)",
        )
        .run(
          runner,
          command.client_cmd_id,
          seq,
          command.hub_session_id,
          JSON.stringify(command),
        );
      this.db
        .prepare("UPDATE remote_runners SET command_seq=? WHERE runner=?")
        .run(seq, runner);
      return { seq, command };
    });
  }
  commands(
    runner: string,
    after: number,
  ): Array<{ seq: number; command: RemoteCommand }> {
    return this.db
      .prepare(
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
    const old = this.db
      .prepare(
        "SELECT seq,body FROM remote_commands WHERE runner=? AND cmd_id=?",
      )
      .get(runner, command.client_cmd_id);
    if (!old) return undefined;
    if (!isDeepStrictEqual(JSON.parse(String(old.body)), command))
      throw new RemoteStoreError("command_id_conflict");
    return Number(old.seq);
  }
  commandHistory(
    owner: string,
    runner: string,
    hub: string,
  ): Array<{ command: RemoteCommand; receipt: unknown }> {
    if (!this.runners(owner).some((item) => item.runner_id === runner))
      return [];
    return this.commandHistoryRows(runner, hub).map((row) => ({
      command: JSON.parse(String(row.body)) as RemoteCommand,
      receipt: row.receipt
        ? (JSON.parse(String(row.receipt)) as unknown)
        : null,
    }));
  }
  /**
   * The rows behind one session's command history, newest last.
   *
   * The obvious query — every row for the runner, then filter on the hub parsed
   * out of `body` — reads and parses the runner's whole command log on each
   * poll. The hub id is the one stable field of a command, so it is projected
   * into its own column at insert time (see `submit`) and the filter becomes
   * one indexed range scan. The projection is verified against `body` rather
   * than trusted, so a row that somehow disagrees still matches what the
   * caller would have seen before.
   */
  private commandHistoryRows(
    runner: string,
    hub: string,
  ): Array<{ body: unknown; receipt: unknown }> {
    return this.db
      .prepare(
        "SELECT body,receipt FROM remote_commands WHERE runner=? AND hub=? ORDER BY seq",
      )
      .all(runner, hub)
      .filter((row) => {
        try {
          return (
            (JSON.parse(String(row.body)) as RemoteCommand).hub_session_id ===
            hub
          );
        } catch {
          return false;
        }
      })
      .map((row) => ({ body: row.body, receipt: row.receipt }));
  }
  acknowledgeCommands(runner: string, seq: number, force = false): void {
    const watermarks = this.watermarks(runner);
    if (
      !Number.isSafeInteger(seq) ||
      (!force && seq < watermarks.command_ack) ||
      seq > watermarks.command_seq
    )
      throw new RemoteStoreError("invalid_command_ack");
    this.db
      .prepare("UPDATE remote_runners SET command_ack=? WHERE runner=?")
      .run(seq, runner);
  }
  receive(
    owner: string,
    runner: string,
    seq: number,
    payload: Record<string, unknown>,
    activeGeneration?: string,
  ): void {
    this.register(owner, runner);
    this.transaction(() => {
      const watermarks = this.watermarks(runner);
      if (!Number.isSafeInteger(seq) || seq < 1)
        throw new RemoteStoreError("invalid_sequence");
      if (seq <= watermarks.received) {
        const old = this.db
          .prepare(
            "SELECT payload FROM remote_events WHERE runner=? AND runner_seq=?",
          )
          .get(runner, seq);
        if (
          !old ||
          !isDeepStrictEqual(JSON.parse(String(old.payload)), payload)
        )
          throw new RemoteStoreError("sequence_conflict");
        return;
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
          event.kind === "question_requested" ||
          event.kind === "question_resolved" ||
          event.kind === "unsupported_provider_request";
        if (isApprovalEvent) validateRemoteApprovalEvent(event);
        validateRemoteEventRaw(event, isApprovalEvent);
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
        const queued = this.db
          .prepare(
            "SELECT body FROM remote_commands WHERE runner=? AND cmd_id=?",
          )
          .get(runner, receipt.client_cmd_id);
        if (!queued) throw new RemoteStoreError("unknown_command_receipt");
        const command = JSON.parse(String(queued.body)) as RemoteCommand;
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
        this.db
          .prepare(
            "UPDATE remote_commands SET receipt=? WHERE runner=? AND cmd_id=?",
          )
          .run(JSON.stringify(receipt), runner, receipt.client_cmd_id);
      } else if (payload.type === "ready") {
        requireOnlyKeys(
          payload,
          [
            "type",
            "provider",
            "host_generation",
            "workspaces",
            "sessions",
            "models",
            "discovery",
            "pending_approvals",
            "pending_questions",
            "attach_running",
          ],
          "invalid_ready",
        );
        if (
          !Array.isArray(payload.sessions) ||
          !Array.isArray(payload.workspaces) ||
          typeof payload.host_generation !== "string" ||
          !payload.host_generation ||
          payload.host_generation.length > 128 ||
          payload.workspaces.some(
            (workspace) => !isRemoteWorkspaceId(workspace),
          ) ||
          new Set(payload.workspaces).size !== payload.workspaces.length
        )
          throw new RemoteStoreError("invalid_ready");
        if (payload.pending_approvals !== undefined) {
          if (!Array.isArray(payload.pending_approvals))
            throw new RemoteStoreError("invalid_ready");
          const approvals = payload.pending_approvals.map((item) => {
            try {
              return parseRemotePendingApproval(item);
            } catch {
              throw new RemoteStoreError("invalid_ready");
            }
          });
          if (
            new Set(approvals.map((approval) => approval.approval_id)).size !==
              approvals.length ||
            approvals.some(
              (approval) =>
                approval.host_generation !== payload.host_generation,
            )
          )
            throw new RemoteStoreError("invalid_ready");
        }
        const sessions = payload.sessions.map((item) => {
          try {
            return parseRemoteBinding(item);
          } catch {
            throw new RemoteStoreError("invalid_ready");
          }
        });
        if (
          sessions.some(
            (session) => session.host_generation !== payload.host_generation,
          )
        )
          throw new RemoteStoreError("invalid_ready");
        if (
          activeGeneration === undefined ||
          payload.host_generation === activeGeneration
        ) {
          // Read this runner's bindings once instead of scanning every session
          // the owner owns, then write all changes in a single statement.
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
          this.db
            .prepare("UPDATE remote_runners SET hello=? WHERE runner=?")
            .run(JSON.stringify(payload), runner);
        }
      } else if (payload.type !== "host_lost" && payload.type !== "models")
        throw new RemoteStoreError("invalid_content_type");
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
      this.db
        .prepare(
          "INSERT INTO remote_events(owner,runner,runner_seq,hub,payload) VALUES (?,?,?,?,?)",
        )
        .run(owner, runner, seq, hub, JSON.stringify(payload));
      this.db
        .prepare("UPDATE remote_runners SET received=? WHERE runner=?")
        .run(seq, runner);
    });
  }
  events(
    owner: string,
    after: number,
    runner?: string,
    hub?: string,
  ): ContentEvent[] {
    return this.db
      .prepare(
        "SELECT id,runner,payload FROM remote_events WHERE owner=? AND id>? AND (? IS NULL OR runner=?) AND (? IS NULL OR hub=? OR (? IS NOT NULL AND runner=? AND hub IS NULL)) ORDER BY id LIMIT 256",
      )
      .all(
        owner,
        after,
        runner ?? null,
        runner ?? null,
        hub ?? null,
        hub ?? null,
        hub ?? null,
        runner ?? null,
      )
      .map((row) => ({
        cursor: Number(row.id),
        runner_id: String(row.runner),
        payload: JSON.parse(String(row.payload)) as Record<string, unknown>,
      }));
  }
  cursor(owner: string): number {
    return Number(
      this.db
        .prepare(
          "SELECT COALESCE(MAX(id),0) AS cursor FROM remote_events WHERE owner=?",
        )
        .get(owner)?.cursor ?? 0,
    );
  }
  /** Every binding one runner currently has, keyed by hub session id. */
  private runnerBindings(
    owner: string,
    runner: string,
  ): Map<string, RemoteBinding> {
    const rows = this.db
      .prepare(
        "SELECT hub,binding FROM remote_sessions WHERE owner=? AND runner=?",
      )
      .all(owner, runner);
    const found = new Map<string, RemoteBinding>();
    for (const row of rows) {
      found.set(
        String(row.hub),
        JSON.parse(String(row.binding)) as RemoteBinding,
      );
    }
    return found;
  }

  /**
   * A hub session may not be re-pointed at a different native session: the
   * cached history behind it would silently belong to another conversation.
   */
  private assertNativeIdentity(
    existing: ReadonlyMap<string, RemoteBinding>,
    incoming: RemoteBinding,
  ): void {
    const previous = existing.get(incoming.hub_session_id);
    if (
      previous &&
      previous.provider_session_id !== incoming.provider_session_id
    )
      throw new RemoteStoreError("native_identity_changed");
  }

  /** Writes every binding in one statement instead of one round trip each. */
  private writeBindings(
    owner: string,
    runner: string,
    bindings: readonly RemoteBinding[],
  ): void {
    if (bindings.length === 0) return;
    // Later entries win, so a binding that is both detached and re-announced
    // in the same batch keeps the incoming state.
    const merged = new Map<string, RemoteBinding>();
    for (const binding of bindings) {
      merged.set(binding.hub_session_id, binding);
    }
    const rows = [...merged.values()];
    // SQLite caps bound parameters per statement, so very wide ready payloads
    // are split across several statements rather than failing outright.
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
      this.db
        .prepare(
          `INSERT INTO remote_sessions(owner,runner,hub,native,binding) VALUES ${placeholders} ON CONFLICT(runner,hub) DO UPDATE SET binding=excluded.binding`,
        )
        .run(...values);
    }
  }

  private saveBinding(
    owner: string,
    runner: string,
    binding: RemoteBinding,
  ): void {
    const old = this.session(owner, runner, binding.hub_session_id);
    if (old && old.provider_session_id !== binding.provider_session_id)
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

function requireOnlyKeys(
  value: Record<string, unknown>,
  keys: string[],
  code: string,
): void {
  if (Object.keys(value).some((key) => !keys.includes(key)))
    throw new RemoteStoreError(code);
}

function validateRemoteEventFields(event: Record<string, unknown>): void {
  for (const key of ["event_id", "host_generation", "kind"])
    if (
      typeof event[key] !== "string" ||
      !(event[key] as string).trim() ||
      (event[key] as string).length > 256
    )
      throw new RemoteStoreError("invalid_event");
  for (const key of [
    "hub_session_id",
    "provider_session_id",
    "approval_id",
    "question_id",
  ])
    if (
      event[key] !== null &&
      event[key] !== undefined &&
      (typeof event[key] !== "string" ||
        !(event[key] as string).trim() ||
        (event[key] as string).length > 256)
    )
      throw new RemoteStoreError("invalid_event");
  if (event.binding !== null && event.binding !== undefined) {
    try {
      const binding = parseRemoteBinding(event.binding);
      if (
        binding.host_generation !== event.host_generation ||
        binding.hub_session_id !== event.hub_session_id ||
        binding.provider_session_id !== event.provider_session_id
      )
        throw new RemoteStoreError("binding_conflict");
    } catch (error) {
      if (error instanceof RemoteStoreError) throw error;
      throw new RemoteStoreError("invalid_event");
    }
  }
}

function validateRemoteApprovalEvent(event: Record<string, unknown>): void {
  if ("raw" in event) throw new RemoteStoreError("unsafe_approval_event");
  if (
    typeof event.event_id !== "string" ||
    !event.event_id ||
    event.event_id.length > 128 ||
    typeof event.host_generation !== "string" ||
    !event.host_generation ||
    event.host_generation.length > 128 ||
    typeof event.hub_session_id !== "string" ||
    !event.hub_session_id ||
    event.hub_session_id.length > 128 ||
    typeof event.approval_id !== "string" ||
    !/^approval-[0-9a-f]{64}$/u.test(event.approval_id)
  )
    throw new RemoteStoreError("invalid_approval_event");
  if (event.kind === "approval_requested") {
    let approval;
    try {
      approval = parseRemotePendingApproval(event.approval);
    } catch {
      throw new RemoteStoreError("invalid_approval_event");
    }
    if (
      approval.approval_id !== event.approval_id ||
      approval.hub_session_id !== event.hub_session_id ||
      approval.host_generation !== event.host_generation
    )
      throw new RemoteStoreError("invalid_approval_event");
  } else if (event.approval !== undefined) {
    throw new RemoteStoreError("invalid_approval_event");
  }
}

function validateRemoteEventRaw(
  event: Record<string, unknown>,
  isApprovalEvent: boolean,
): void {
  if (isApprovalEvent) return;
  if (event.raw === undefined) return;
  if (!event.raw || typeof event.raw !== "object" || Array.isArray(event.raw))
    throw new RemoteStoreError("unsafe_provider_event");
  const raw = event.raw as Record<string, unknown>;
  if (Object.keys(raw).some((key) => key !== "params"))
    throw new RemoteStoreError("unsafe_provider_event");
  if (
    !raw.params ||
    typeof raw.params !== "object" ||
    Array.isArray(raw.params)
  )
    throw new RemoteStoreError("unsafe_provider_event");
  const params = raw.params as Record<string, unknown>;
  const only = (keys: string[]): boolean =>
    Object.keys(params).every((key) => keys.includes(key));
  const kind = event.kind;
  if (kind === "turn/started" || kind === "turn/completed") {
    const turn = params.turn;
    if (
      !only(["turn"]) ||
      !turn ||
      typeof turn !== "object" ||
      Array.isArray(turn) ||
      Object.keys(turn).some((key) => key !== "id") ||
      typeof (turn as Record<string, unknown>).id !== "string"
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "item/agentMessage/delta") {
    if (
      !only(["itemId", "delta"]) ||
      typeof params.itemId !== "string" ||
      typeof params.delta !== "string"
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "thread/tokenUsage/updated") {
    // Context capacity is two integers. Anything richer than that is provider
    // bookkeeping the console has no use for, so it is rejected rather than
    // stored.
    const usage = params.tokenUsage;
    if (
      !only(["tokenUsage"]) ||
      !usage ||
      typeof usage !== "object" ||
      Array.isArray(usage)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = usage as Record<string, unknown>;
    const isCount = (value: unknown) =>
      value === undefined ||
      (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
    if (
      Object.keys(typed).some((key) => !["used", "window"].includes(key)) ||
      typeof typed.used !== "number" ||
      !Number.isSafeInteger(typed.used) ||
      typed.used < 0 ||
      !isCount(typed.window)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "history/snapshot") {
    if (!only(["items"]) || !Array.isArray(params.items))
      throw new RemoteStoreError("unsafe_provider_event");
    for (const item of params.items) {
      if (!item || typeof item !== "object" || Array.isArray(item))
        throw new RemoteStoreError("unsafe_provider_event");
      const entry = item as Record<string, unknown>;
      if (
        (entry.role !== undefined && typeof entry.role !== "string") ||
        (entry.kind !== undefined && typeof entry.kind !== "string") ||
        (entry.text !== undefined && typeof entry.text !== "string") ||
        (entry.label !== undefined && typeof entry.label !== "string") ||
        (entry.status !== undefined && typeof entry.status !== "string")
      )
        throw new RemoteStoreError("unsafe_provider_event");
    }
    return;
  }
  if (kind === "item/started") {
    // An item that announces itself before it has any output. Only the fields
    // a client renders are accepted, so a live "执行中" row is possible without
    // letting an arbitrary provider object ride along.
    const item = params.item;
    if (
      !only(["item"]) ||
      !item ||
      typeof item !== "object" ||
      Array.isArray(item)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = item as Record<string, unknown>;
    const itemKeys = Object.keys(typed);
    if (
      typeof typed.id !== "string" ||
      itemKeys.some(
        (key) =>
          ![
            "id",
            "type",
            "command",
            "commandLine",
            "toolName",
            "name",
            "path",
            "status",
            "text",
          ].includes(key),
      ) ||
      !["commandExecution", "fileChange", "reasoning", "thinking"].includes(
        typed.type as string,
      )
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "item/completed") {
    const item = params.item;
    if (
      !only(["item"]) ||
      !item ||
      typeof item !== "object" ||
      Array.isArray(item)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = item as Record<string, unknown>;
    const itemKeys = Object.keys(typed);
    if (
      typeof typed.id !== "string" ||
      (typed.type === "agentMessage" &&
        (typeof typed.text !== "string" ||
          itemKeys.some((key) => !["id", "type", "text"].includes(key)))) ||
      (typed.type === "commandExecution" &&
        itemKeys.some(
          (key) =>
            !["id", "type", "command", "output", "status", "exitCode"].includes(
              key,
            ),
        )) ||
      (typed.type === "thinking" &&
        ((typed.text !== undefined && typeof typed.text !== "string") ||
          itemKeys.some((key) => !["id", "type", "text"].includes(key)))) ||
      (typed.type === "userMessage" &&
        itemKeys.some(
          (key) => !["id", "type", "text", "content"].includes(key),
        )) ||
      !["agentMessage", "commandExecution", "userMessage", "thinking"].includes(
        typed.type as string,
      )
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  throw new RemoteStoreError("unsafe_provider_event");
}

interface RemoteReceipt {
  client_cmd_id: string;
  hub_session_id: string;
  status: "accepted" | "rejected" | "failed" | "indeterminate";
  code: string | null;
  result: {
    binding?: RemoteBinding;
    turn?: { id: string };
    approval_id?: string;
    response_sent?: boolean;
  } | null;
}

function parseRemoteReceipt(value: unknown): RemoteReceipt {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new RemoteStoreError("invalid_receipt");
  const receipt = value as Record<string, unknown>;
  if (
    Object.keys(receipt).some(
      (key) =>
        ![
          "client_cmd_id",
          "hub_session_id",
          "status",
          "code",
          "result",
        ].includes(key),
    ) ||
    typeof receipt.client_cmd_id !== "string" ||
    !receipt.client_cmd_id.trim() ||
    receipt.client_cmd_id.length > 128 ||
    typeof receipt.hub_session_id !== "string" ||
    !receipt.hub_session_id.trim() ||
    receipt.hub_session_id.length > 128 ||
    !["accepted", "rejected", "failed", "indeterminate"].includes(
      receipt.status as string,
    ) ||
    (receipt.code !== null &&
      (typeof receipt.code !== "string" || receipt.code.length > 128))
  )
    throw new RemoteStoreError("invalid_receipt");
  let result: RemoteReceipt["result"] = null;
  if (receipt.result !== null) {
    if (
      !receipt.result ||
      typeof receipt.result !== "object" ||
      Array.isArray(receipt.result)
    )
      throw new RemoteStoreError("invalid_receipt");
    const rawResult = receipt.result as Record<string, unknown>;
    if (
      Object.keys(rawResult).some(
        (key) =>
          !["binding", "turn", "approval_id", "response_sent"].includes(key),
      )
    )
      throw new RemoteStoreError("invalid_receipt");
    result = {};
    if (rawResult.binding !== undefined) {
      try {
        result.binding = parseRemoteBinding(rawResult.binding);
      } catch {
        throw new RemoteStoreError("invalid_receipt");
      }
    }
    if (rawResult.turn !== undefined) {
      const turn = rawResult.turn as Record<string, unknown>;
      if (
        !rawResult.turn ||
        typeof rawResult.turn !== "object" ||
        Array.isArray(rawResult.turn) ||
        Object.keys(turn).some((key) => key !== "id") ||
        typeof turn.id !== "string" ||
        !turn.id ||
        turn.id.length > 256
      )
        throw new RemoteStoreError("invalid_receipt");
      result.turn = { id: turn.id };
    }
    if (rawResult.approval_id !== undefined) {
      if (
        typeof rawResult.approval_id !== "string" ||
        !/^approval-[0-9a-f]{64}$/u.test(rawResult.approval_id)
      )
        throw new RemoteStoreError("invalid_receipt");
      result.approval_id = rawResult.approval_id;
    }
    if (rawResult.response_sent !== undefined) {
      if (typeof rawResult.response_sent !== "boolean")
        throw new RemoteStoreError("invalid_receipt");
      result.response_sent = rawResult.response_sent;
    }
  }
  return {
    client_cmd_id: receipt.client_cmd_id,
    hub_session_id: receipt.hub_session_id,
    status: receipt.status as RemoteReceipt["status"],
    code: receipt.code as string | null,
    result,
  };
}
