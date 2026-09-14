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

/** Single Gateway instance, explicit content database; not the v1 metadata store. */
export class RemoteContentStore {
  private db: DatabaseSync;
  constructor(path: string) {
    this.db = new DatabaseSync(path);
    this.db
      .exec(`PRAGMA locking_mode=EXCLUSIVE; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS remote_runners (runner TEXT PRIMARY KEY, owner TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, command_seq INTEGER NOT NULL DEFAULT 0, command_ack INTEGER NOT NULL DEFAULT 0, hello TEXT);
      CREATE TABLE IF NOT EXISTS remote_commands (runner TEXT NOT NULL, cmd_id TEXT NOT NULL, seq INTEGER NOT NULL, body TEXT NOT NULL, receipt TEXT, PRIMARY KEY(runner,cmd_id), UNIQUE(runner,seq));
      CREATE TABLE IF NOT EXISTS remote_sessions (owner TEXT NOT NULL, runner TEXT NOT NULL, hub TEXT NOT NULL, native TEXT NOT NULL, binding TEXT NOT NULL, PRIMARY KEY(runner,hub), UNIQUE(runner,native));
      CREATE TABLE IF NOT EXISTS remote_events (id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, runner TEXT NOT NULL, runner_seq INTEGER NOT NULL, hub TEXT, payload TEXT NOT NULL, UNIQUE(runner,runner_seq));
      CREATE INDEX IF NOT EXISTS remote_event_owner ON remote_events(owner,id);
      CREATE INDEX IF NOT EXISTS remote_event_session ON remote_events(owner,runner,hub,id);`);
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
          "INSERT INTO remote_commands(runner,cmd_id,seq,body) VALUES (?,?,?,?)",
        )
        .run(runner, command.client_cmd_id, seq, JSON.stringify(command));
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
    return this.db
      .prepare(
        "SELECT body,receipt FROM remote_commands WHERE runner=? ORDER BY seq",
      )
      .all(runner)
      .map((row) => ({
        command: JSON.parse(String(row.body)) as RemoteCommand,
        receipt: row.receipt
          ? (JSON.parse(String(row.receipt)) as unknown)
          : null,
      }))
      .filter((row) => row.command.hub_session_id === hub);
  }
  acknowledgeCommands(runner: string, seq: number): void {
    const watermarks = this.watermarks(runner);
    if (
      !Number.isSafeInteger(seq) ||
      seq < watermarks.command_ack ||
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
          ],
          "invalid_event",
        );
        validateRemoteEventFields(event);
        const isApprovalEvent =
          event.kind === "approval_requested" ||
          event.kind === "approval_resolved" ||
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
          const currentHubs = new Set(
            sessions.map((session) => session.hub_session_id),
          );
          for (const session of this.sessions(owner).filter(
            (item) => item.runner_id === runner,
          )) {
            if (!currentHubs.has(session.binding.hub_session_id))
              this.saveBinding(owner, runner, {
                ...session.binding,
                control_connected: false,
                execution_state: "unknown",
                current_turn_id: null,
              });
          }
          for (const session of sessions)
            this.saveBinding(owner, runner, session);
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
        for (const session of this.sessions(owner).filter(
          (item) => item.runner_id === runner,
        )) {
          this.saveBinding(owner, runner, {
            ...session.binding,
            control_connected: false,
            execution_state: "unknown",
          });
        }
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
  private saveBinding(
    owner: string,
    runner: string,
    binding: RemoteBinding,
  ): void {
    const old = this.session(owner, runner, binding.hub_session_id);
    if (old && old.provider_session_id !== binding.provider_session_id)
      throw new RemoteStoreError("native_identity_changed");
    this.db
      .prepare(
        "INSERT INTO remote_sessions(owner,runner,hub,native,binding) VALUES (?,?,?,?,?) ON CONFLICT(runner,hub) DO UPDATE SET binding=excluded.binding",
      )
      .run(
        owner,
        runner,
        binding.hub_session_id,
        binding.provider_session_id,
        JSON.stringify(binding),
      );
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
  for (const key of ["hub_session_id", "provider_session_id", "approval_id"])
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
        itemKeys.some((key) => !["id", "type"].includes(key))) ||
      !["agentMessage", "commandExecution"].includes(typed.type as string)
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
