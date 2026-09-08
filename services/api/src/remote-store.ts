import { DatabaseSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import {
  parseRemoteBinding,
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
        const event = payload.event as Record<string, unknown> | undefined;
        if (!event || typeof event !== "object")
          throw new RemoteStoreError("invalid_event");
        if (event.binding) binding = parseRemoteBinding(event.binding);
        hub =
          typeof event.hub_session_id === "string"
            ? event.hub_session_id
            : null;
        if (binding && hub !== binding.hub_session_id)
          throw new RemoteStoreError("binding_conflict");
      } else if (payload.type === "receipt") {
        const receipt = payload.receipt as Record<string, unknown> | undefined;
        if (!receipt || typeof receipt.client_cmd_id !== "string")
          throw new RemoteStoreError("invalid_receipt");
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
        const result = receipt.result as Record<string, unknown> | undefined;
        if (result?.binding) binding = parseRemoteBinding(result.binding);
        if (binding && binding.hub_session_id !== hub)
          throw new RemoteStoreError("binding_conflict");
        this.db
          .prepare(
            "UPDATE remote_commands SET receipt=? WHERE runner=? AND cmd_id=?",
          )
          .run(JSON.stringify(receipt), runner, receipt.client_cmd_id);
      } else if (payload.type === "ready") {
        if (
          !Array.isArray(payload.sessions) ||
          !Array.isArray(payload.workspaces)
        )
          throw new RemoteStoreError("invalid_ready");
        for (const item of payload.sessions)
          this.saveBinding(owner, runner, parseRemoteBinding(item));
        this.db
          .prepare("UPDATE remote_runners SET hello=? WHERE runner=?")
          .run(JSON.stringify(payload), runner);
      } else if (payload.type !== "host_lost" && payload.type !== "models")
        throw new RemoteStoreError("invalid_content_type");
      if (binding) this.saveBinding(owner, runner, binding);
      if (payload.type === "host_lost") {
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
        "SELECT id,runner,payload FROM remote_events WHERE owner=? AND id>? AND (? IS NULL OR runner=?) AND (? IS NULL OR hub=?) ORDER BY id LIMIT 256",
      )
      .all(
        owner,
        after,
        runner ?? null,
        runner ?? null,
        hub ?? null,
        hub ?? null,
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
