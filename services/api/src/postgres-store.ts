import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";

import type { SyncBatch, SyncEvent } from "@agent-platform/contracts";
import postgres from "postgres";

import {
  GatewayStoreError,
  type DeviceRegistrationResult,
  type DeviceView,
  type GatewayStore,
  type RegisteredDevice,
  type SessionFilters,
  type StoredSession,
} from "./store.js";

const MIGRATIONS = [
  {
    version: 1,
    url: new URL("../migrations/0001_gateway.sql", import.meta.url),
  },
] as const;

interface DeviceRow {
  device_id: string;
  public_key: string;
  owner_id: string;
  created_at: unknown;
  last_seen_at: unknown;
  last_sequence: string | number;
}

interface ExistingEventRow {
  event_id: string;
  device_id: string;
  sequence: string | number;
  event_data: unknown;
}

interface SessionRow {
  event_data: unknown;
  received_at: unknown;
}

export class PostgresGatewayStore implements GatewayStore {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  static async connect(databaseUrl: string): Promise<PostgresGatewayStore> {
    const sql = postgres(databaseUrl, {
      max: 10,
      connect_timeout: 10,
      idle_timeout: 20,
      onnotice: () => {},
      connection: { application_name: "agent-platform-api" },
    });
    try {
      await migratePostgres(sql);
      return new PostgresGatewayStore(sql);
    } catch (error) {
      await sql.end({ timeout: 1 });
      throw error;
    }
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    return this.#sql.begin(async (sql) => {
      const inserted = await sql<DeviceRow[]>`
        INSERT INTO agent_platform_devices (
          device_id,
          public_key,
          owner_id
        )
        VALUES (${deviceId}, ${publicKey}, ${ownerId})
        ON CONFLICT (device_id) DO NOTHING
        RETURNING
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence
      `;
      const created = inserted.length === 1;
      const rows =
        inserted.length > 0
          ? inserted
          : await sql<DeviceRow[]>`
              SELECT
                device_id,
                public_key,
                owner_id,
                created_at,
                last_seen_at,
                last_sequence
              FROM agent_platform_devices
              WHERE device_id = ${deviceId}
              FOR UPDATE
            `;
      const row = rows[0];
      if (row === undefined) {
        throw new Error("registered device could not be loaded");
      }
      if (row.public_key !== publicKey || row.owner_id !== ownerId) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      return { device: registeredDevice(row), created };
    });
  }

  async ingestBatch(batch: SyncBatch): Promise<number> {
    try {
      return await this.#sql.begin(async (sql) => {
        const devices = await sql<DeviceRow[]>`
          SELECT
            device_id,
            public_key,
            owner_id,
            created_at,
            last_seen_at,
            last_sequence
          FROM agent_platform_devices
          WHERE device_id = ${batch.device_id}
          FOR UPDATE
        `;
        const device = devices[0];
        if (device === undefined) {
          throw new GatewayStoreError(
            "device_not_registered",
            "device is not registered",
          );
        }
        if (device.public_key !== batch.public_key) {
          throw new GatewayStoreError(
            "device_key_mismatch",
            "batch public key does not match registered device",
          );
        }

        const eventIds = batch.events.map((event) => event.event_id);
        const existingRows = await sql<ExistingEventRow[]>`
          SELECT event_id::text, device_id, sequence, event_data
          FROM agent_platform_events
          WHERE
            event_id IN ${sql(eventIds)}
            OR (
              device_id = ${batch.device_id}
              AND sequence BETWEEN ${batch.first_sequence} AND ${batch.last_sequence}
            )
        `;
        const byId = new Map(
          existingRows.map((row) => [row.event_id, row] as const),
        );
        const bySequence = new Map(
          existingRows
            .filter((row) => row.device_id === batch.device_id)
            .map((row) => [Number(row.sequence), row] as const),
        );
        const newEvents: SyncEvent[] = [];
        for (const event of batch.events) {
          const existingById = byId.get(event.event_id);
          if (existingById !== undefined) {
            if (!isDeepStrictEqual(existingById.event_data, event)) {
              throw new GatewayStoreError(
                "event_conflict",
                "event ID was reused with different data",
              );
            }
            continue;
          }
          const existingAtSequence = bySequence.get(event.sequence);
          if (
            existingAtSequence !== undefined &&
            existingAtSequence.event_id !== event.event_id
          ) {
            throw new GatewayStoreError(
              "sequence_conflict",
              "device sequence was reused by another event",
            );
          }
          newEvents.push(event);
        }

        const lastSequence = Number(device.last_sequence);
        if (
          newEvents.length > 0 &&
          newEvents[0]?.sequence !== lastSequence + 1
        ) {
          throw new GatewayStoreError(
            "sequence_gap",
            "new events do not continue the stored device sequence",
          );
        }

        const receivedAt = new Date();
        if (newEvents.length > 0) {
          const eventRows = newEvents.map((event) => ({
            event_id: event.event_id,
            device_id: event.device_id,
            sequence: event.sequence,
            observed_at: event.observed_at,
            received_at: receivedAt,
            event_data: sql.json(event as unknown as postgres.JSONValue),
          }));
          await sql`
            INSERT INTO agent_platform_events ${sql(
              eventRows,
              "event_id",
              "device_id",
              "sequence",
              "observed_at",
              "received_at",
              "event_data",
            )}
          `;

          const snapshotEvents = bestSessionEvents(newEvents);
          const sessionRows = snapshotEvents.map((event) => ({
            device_id: event.device_id,
            agent_type: event.agent_type,
            agent_session_id: event.agent_session_id,
            observed_at: event.observed_at,
            received_at: receivedAt,
            sequence: event.sequence,
            confidence: event.confidence,
            presence: event.payload.state.presence,
            activity_status: event.payload.state.activity_status,
            event_data: sql.json(event as unknown as postgres.JSONValue),
          }));
          await sql`
            INSERT INTO agent_platform_sessions ${sql(
              sessionRows,
              "device_id",
              "agent_type",
              "agent_session_id",
              "observed_at",
              "received_at",
              "sequence",
              "confidence",
              "presence",
              "activity_status",
              "event_data",
            )}
            ON CONFLICT (device_id, agent_type, agent_session_id)
            DO UPDATE SET
              observed_at = EXCLUDED.observed_at,
              received_at = EXCLUDED.received_at,
              sequence = EXCLUDED.sequence,
              confidence = EXCLUDED.confidence,
              presence = EXCLUDED.presence,
              activity_status = EXCLUDED.activity_status,
              event_data = EXCLUDED.event_data
            WHERE
              EXCLUDED.observed_at > agent_platform_sessions.observed_at
              OR (
                EXCLUDED.observed_at = agent_platform_sessions.observed_at
                AND (
                  CASE EXCLUDED.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                ) > (
                  CASE agent_platform_sessions.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                )
              )
              OR (
                EXCLUDED.observed_at = agent_platform_sessions.observed_at
                AND (
                  CASE EXCLUDED.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                ) = (
                  CASE agent_platform_sessions.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                )
                AND EXCLUDED.sequence > agent_platform_sessions.sequence
              )
          `;
        }

        const acceptedSequence =
          newEvents.at(-1)?.sequence ?? Number(device.last_sequence);
        await sql`
          UPDATE agent_platform_devices
          SET
            last_seen_at = ${receivedAt},
            last_sequence = ${acceptedSequence}
          WHERE device_id = ${batch.device_id}
        `;
        return newEvents.length;
      });
    } catch (error) {
      throw classifyPostgresConflict(error);
    }
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const rows = await this.#sql<
      Array<{
        device_id: string;
        created_at: unknown;
        last_seen_at: unknown;
        online: boolean;
      }>
    >`
      SELECT
        device_id,
        created_at,
        last_seen_at,
        last_seen_at >= clock_timestamp() - interval '45 seconds' AS online
      FROM agent_platform_devices
      WHERE owner_id = ${ownerId}
      ORDER BY device_id
    `;
    return rows.map((row) => ({
      device_id: row.device_id,
      created_at: isoTimestamp(row.created_at),
      last_seen_at: isoTimestamp(row.last_seen_at),
      online: row.online,
    }));
  }

  async listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]> {
    const deviceFilter =
      filters.device_id === undefined
        ? this.#sql``
        : this.#sql`AND sessions.device_id = ${filters.device_id}`;
    const activityFilter =
      filters.activity_status === undefined
        ? this.#sql``
        : this.#sql`AND sessions.activity_status = ${filters.activity_status}`;
    const presenceFilter =
      filters.presence === undefined
        ? this.#sql``
        : this.#sql`AND sessions.presence = ${filters.presence}`;
    const rows = await this.#sql<SessionRow[]>`
      SELECT sessions.event_data, sessions.received_at
      FROM agent_platform_sessions AS sessions
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = sessions.device_id
      WHERE
        devices.owner_id = ${ownerId}
        ${deviceFilter}
        ${activityFilter}
        ${presenceFilter}
      ORDER BY sessions.observed_at DESC, sessions.sequence DESC
    `;
    return rows.map((row) => ({
      ...syncEvent(row.event_data),
      received_at: isoTimestamp(row.received_at),
    }));
  }

  async close(): Promise<void> {
    await this.#sql.end({ timeout: 5 });
  }
}

export async function migratePostgres(sql: postgres.Sql): Promise<void> {
  await sql.begin(async (transaction) => {
    await transaction`SELECT pg_advisory_xact_lock(1701733484, 1886152045)`;
    await transaction.unsafe(`
      CREATE TABLE IF NOT EXISTS agent_platform_schema_migrations (
        version integer PRIMARY KEY,
        checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
        applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
      )
    `);
    for (const migration of MIGRATIONS) {
      const source = await readFile(migration.url, "utf8");
      const checksum = createHash("sha256").update(source).digest("hex");
      const existing = await transaction<
        Array<{ version: number; checksum: string }>
      >`
        SELECT version, checksum
        FROM agent_platform_schema_migrations
        WHERE version = ${migration.version}
      `;
      if (existing.length > 0) {
        if (existing[0]?.checksum !== checksum) {
          throw new Error(
            `database migration ${migration.version} checksum mismatch`,
          );
        }
        continue;
      }
      await transaction.unsafe(source);
      await transaction`
        INSERT INTO agent_platform_schema_migrations (version, checksum)
        VALUES (${migration.version}, ${checksum})
      `;
    }
  });
}

function registeredDevice(row: DeviceRow): RegisteredDevice {
  return {
    device_id: row.device_id,
    public_key: row.public_key,
    owner_id: row.owner_id,
    created_at: isoTimestamp(row.created_at),
    last_seen_at: isoTimestamp(row.last_seen_at),
  };
}

function isoTimestamp(value: unknown): string {
  const date =
    value instanceof Date
      ? value
      : typeof value === "string" || typeof value === "number"
        ? new Date(value)
        : undefined;
  if (date === undefined || Number.isNaN(date.getTime())) {
    throw new Error("database returned an invalid timestamp");
  }
  return date.toISOString();
}

function syncEvent(value: unknown): SyncEvent {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("database returned invalid event data");
  }
  return value as SyncEvent;
}

function bestSessionEvents(events: SyncEvent[]): SyncEvent[] {
  const sessions = new Map<string, SyncEvent>();
  for (const event of events) {
    const key = `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
    const current = sessions.get(key);
    if (current === undefined || shouldReplaceEvent(current, event)) {
      sessions.set(key, event);
    }
  }
  return [...sessions.values()];
}

function shouldReplaceEvent(current: SyncEvent, incoming: SyncEvent): boolean {
  const currentTime = Date.parse(current.observed_at);
  const incomingTime = Date.parse(incoming.observed_at);
  if (incomingTime !== currentTime) {
    return incomingTime > currentTime;
  }
  const confidenceRank: Record<SyncEvent["confidence"], number> = {
    unknown: 0,
    inferred: 1,
    observed: 2,
    authoritative: 3,
  };
  const rankDifference =
    confidenceRank[incoming.confidence] - confidenceRank[current.confidence];
  return rankDifference === 0
    ? incoming.sequence > current.sequence
    : rankDifference > 0;
}

function classifyPostgresConflict(error: unknown): unknown {
  if (
    typeof error !== "object" ||
    error === null ||
    !("code" in error) ||
    error.code !== "23505"
  ) {
    return error;
  }
  const constraint =
    "constraint_name" in error && typeof error.constraint_name === "string"
      ? error.constraint_name
      : "constraint" in error && typeof error.constraint === "string"
        ? error.constraint
        : undefined;
  if (constraint === "agent_platform_events_pkey") {
    return new GatewayStoreError(
      "event_conflict",
      "event ID was reused with different data",
    );
  }
  if (constraint === "agent_platform_events_device_id_sequence_key") {
    return new GatewayStoreError(
      "sequence_conflict",
      "device sequence was reused by another event",
    );
  }
  return error;
}
