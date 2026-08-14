import { randomUUID } from "node:crypto";

import postgres from "postgres";

/**
 * Owner-scoped change notifications for the read panel.
 *
 * The stream carries a "something changed" signal, never session payloads:
 * clients re-read the existing query endpoints, so the realtime channel cannot
 * drift from the field policy those endpoints already enforce.
 */
export interface ChangeMessage {
  device_id: string;
  accepted: number;
  auth_snapshots?: boolean;
  remote_tasks?: boolean;
}

export type ChangeListener = (message: ChangeMessage) => void;

export class ChangeBroker {
  readonly #listeners = new Map<string, Set<ChangeListener>>();

  subscribe(ownerId: string, listener: ChangeListener): () => void {
    let listeners = this.#listeners.get(ownerId);
    if (listeners === undefined) {
      listeners = new Set();
      this.#listeners.set(ownerId, listeners);
    }
    listeners.add(listener);
    return () => {
      const current = this.#listeners.get(ownerId);
      if (current === undefined) {
        return;
      }
      current.delete(listener);
      if (current.size === 0) {
        this.#listeners.delete(ownerId);
      }
    };
  }

  subscriberCount(ownerId: string): number {
    return this.#listeners.get(ownerId)?.size ?? 0;
  }

  publish(ownerId: string, message: ChangeMessage): void {
    const listeners = this.#listeners.get(ownerId);
    if (listeners === undefined) {
      return;
    }
    // Copy first: a listener may unsubscribe itself while being notified.
    for (const listener of [...listeners]) {
      listener(message);
    }
  }

  close(): void | Promise<void> {
    // The in-process broker owns no external resources.
  }
}

const POSTGRES_CHANNEL = "agent_pulse_changes_v1";
const MAX_NOTIFY_BYTES = 7_500;

interface PostgresChangeEnvelope {
  origin: string;
  owner_id: string;
  message: ChangeMessage;
}

/**
 * Cross-instance fan-out backed by PostgreSQL LISTEN/NOTIFY.
 *
 * The payload is deliberately limited to the same metadata used by the SSE
 * frame. It is a wake-up signal only; clients still re-read authenticated
 * query endpoints for the actual data.
 */
export class PostgresChangeBroker extends ChangeBroker {
  readonly #sql: postgres.Sql;
  readonly #origin = randomUUID();
  readonly #ready: Promise<void>;
  #unlisten: (() => Promise<unknown>) | undefined;

  private constructor(sql: postgres.Sql) {
    super();
    this.#sql = sql;
    this.#ready = sql
      .listen(POSTGRES_CHANNEL, (payload) => this.#receive(payload))
      .then((handle) => {
        this.#unlisten = handle.unlisten;
      });
  }

  static async connect(databaseUrl: string): Promise<PostgresChangeBroker> {
    const sql = postgres(databaseUrl, {
      max: 1,
      connect_timeout: 10,
      idle_timeout: 0,
      max_lifetime: 0,
      onnotice: () => {},
      connection: { application_name: "agent-pulse-broker" },
    });
    const broker = new PostgresChangeBroker(sql);
    try {
      await broker.#ready;
      return broker;
    } catch (error) {
      await sql.end({ timeout: 1 }).catch(() => undefined);
      throw error;
    }
  }

  override publish(ownerId: string, message: ChangeMessage): void {
    super.publish(ownerId, message);
    const payload = JSON.stringify({
      origin: this.#origin,
      owner_id: ownerId,
      message,
    } satisfies PostgresChangeEnvelope);
    if (Buffer.byteLength(payload, "utf8") > MAX_NOTIFY_BYTES) {
      return;
    }
    void this.#sql.notify(POSTGRES_CHANNEL, payload).catch(() => undefined);
  }

  override async close(): Promise<void> {
    await this.#ready.catch(() => undefined);
    await this.#unlisten?.().catch(() => undefined);
    await this.#sql.end({ timeout: 1 }).catch(() => undefined);
  }

  #receive(payload: string): void {
    let envelope: PostgresChangeEnvelope;
    try {
      envelope = JSON.parse(payload) as PostgresChangeEnvelope;
    } catch {
      return;
    }
    if (
      typeof envelope !== "object" ||
      envelope === null ||
      envelope.origin === this.#origin ||
      typeof envelope.origin !== "string" ||
      typeof envelope.owner_id !== "string" ||
      !isChangeMessage(envelope.message)
    ) {
      return;
    }
    super.publish(envelope.owner_id, envelope.message);
  }
}

function isChangeMessage(value: unknown): value is ChangeMessage {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const message = value as Record<string, unknown>;
  if (
    typeof message.device_id !== "string" ||
    !/^dev_[0-9a-f]{32}$/.test(message.device_id) ||
    typeof message.accepted !== "number" ||
    !Number.isSafeInteger(message.accepted) ||
    message.accepted < 0 ||
    message.accepted > 1_000
  ) {
    return false;
  }
  return (
    (message.auth_snapshots === undefined ||
      typeof message.auth_snapshots === "boolean") &&
    (message.remote_tasks === undefined ||
      typeof message.remote_tasks === "boolean")
  );
}
