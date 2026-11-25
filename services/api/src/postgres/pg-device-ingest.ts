import { isDeepStrictEqual } from "node:util";
import {
  type ActivityStatus,
  type AuthSnapshot,
  type AuthSyncBatch,
  type SyncBatch,
  type SyncEvent,
} from "@agent-pulse/contracts";
import type postgres from "postgres";
import {
  AccountError,
  entitlementsFor,
  type SubscriptionRecord,
} from "../accounts.js";
import {
  bestSessionEvents,
  classifyPostgresConflict,
  isoTimestamp,
  normalizeSubscription,
  registeredDevice,
  syncEvent,
  type DeviceRow,
} from "../postgres-helpers.js";
import {
  GatewayStoreError,
  sessionStatusChange,
  type AuthIngestResult,
  type DeviceRegistrationResult,
  type IngestResult,
  type SessionStatusChange,
} from "../store.js";
import {
  UUID_RE,
  insertWorkspaceAuditIfVisible,
  type AuthSnapshotRow,
  type BindingCodeRow,
  type ExistingEventRow,
  type SessionStatusRow,
} from "./pg-shared.js";

export class PostgresDeviceIngestRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  async registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    return this.#sql.begin(async (sql) => {
      const bindings = await sql<BindingCodeRow[]>`
        SELECT
          owner_id,
          issued_by,
          expires_at <= clock_timestamp() AS expired,
          claimed_device_id
        FROM agent_platform_binding_codes
        WHERE code_digest = ${codeDigest}
        FOR UPDATE
      `;
      const binding = bindings[0];
      if (binding === undefined) {
        throw new GatewayStoreError(
          "invalid_binding_code",
          "binding code is invalid or expired",
        );
      }
      const isFreshClaim = binding.claimed_device_id === null;
      if (
        binding.claimed_device_id !== null &&
        binding.claimed_device_id !== deviceId
      ) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      if (binding.claimed_device_id === null && binding.expired) {
        throw new GatewayStoreError(
          "invalid_binding_code",
          "binding code is invalid or expired",
        );
      }

      if (isFreshClaim && UUID_RE.test(binding.owner_id)) {
        const subRows = await sql<SubscriptionRecord[]>`
          SELECT plan, status, current_period_end
          FROM agent_pulse_subscriptions
          WHERE user_id = ${binding.owner_id}::uuid
          FOR UPDATE
        `;
        if (subRows[0] !== undefined) {
          const maxDevices = entitlementsFor(
            normalizeSubscription(subRows[0]),
          ).max_devices;
          const existingDevice = await sql<
            Array<{ revoked_at: unknown | null }>
          >`
            SELECT revoked_at FROM agent_platform_devices WHERE device_id = ${deviceId} FOR UPDATE
          `;
          const needsSlot =
            existingDevice.length === 0 ||
            existingDevice[0]!.revoked_at !== null;
          if (needsSlot) {
            const activeRows = await sql<{ count: string }[]>`
              SELECT COUNT(*)::text AS count
              FROM agent_platform_devices
              WHERE owner_id = ${binding.owner_id} AND revoked_at IS NULL
            `;
            if (Number(activeRows[0]?.count ?? "0") >= maxDevices) {
              throw new AccountError(
                "device_limit_reached",
                "subscription device limit reached",
                403,
              );
            }
          }
        }
      }

      const inserted = await sql<DeviceRow[]>`
        INSERT INTO agent_platform_devices (
          device_id,
          public_key,
          owner_id
        )
        VALUES (${deviceId}, ${publicKey}, ${binding.owner_id})
        ON CONFLICT (device_id) DO NOTHING
        RETURNING
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence,
          revoked_at
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
                last_sequence,
                revoked_at
              FROM agent_platform_devices
              WHERE device_id = ${deviceId}
              FOR UPDATE
            `;
      const device = rows[0];
      if (device === undefined) {
        throw new Error("registered device could not be loaded");
      }
      if (
        device.public_key !== publicKey ||
        device.owner_id !== binding.owner_id
      ) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      const wasRevoked = device.revoked_at !== null;
      if (wasRevoked) {
        if (!isFreshClaim) {
          throw new GatewayStoreError("device_revoked", "device is revoked");
        }
        await sql`
          UPDATE agent_platform_devices
          SET
            revoked_at = NULL,
            last_seen_at = clock_timestamp()
          WHERE device_id = ${deviceId}
        `;
        device.revoked_at = null;
      }
      if (binding.claimed_device_id === null) {
        await sql`
          UPDATE agent_platform_binding_codes
          SET
            claimed_at = clock_timestamp(),
            claimed_device_id = ${deviceId}
          WHERE code_digest = ${codeDigest}
        `;
      }
      if (created) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: binding.owner_id,
          actorUserId: binding.issued_by ?? undefined,
          action: "device.registered",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(device.created_at),
        });
      } else if (isFreshClaim && wasRevoked) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: binding.owner_id,
          actorUserId: binding.issued_by ?? undefined,
          action: "device.reactivated",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(new Date()),
        });
      }
      return { device: registeredDevice(device), created };
    });
  }

  async revokeDevice(
    ownerId: string,
    deviceId: string,
    actorUserId?: string,
  ): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ device_id: string; revoked_at: unknown }>>`
        UPDATE agent_platform_devices
        SET revoked_at = clock_timestamp()
        WHERE
          device_id = ${deviceId}
          AND owner_id = ${ownerId}
          AND revoked_at IS NULL
        RETURNING device_id, revoked_at
      `;
      if (rows.length !== 1) return false;
      await insertWorkspaceAuditIfVisible(sql, {
        workspaceOwnerId: ownerId,
        actorUserId,
        action: "device.revoked",
        targetType: "device",
        targetId: deviceId,
        metadata: {},
        createdAt: isoTimestamp(rows[0]!.revoked_at),
      });
      return true;
    });
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
    actorUserId?: string,
    allowReactivate = false,
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
          last_sequence,
          revoked_at
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
                last_sequence,
                revoked_at
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
      const wasRevoked = row.revoked_at !== null;
      if (wasRevoked) {
        if (!allowReactivate) {
          throw new GatewayStoreError("device_revoked", "device is revoked");
        }
        await sql`
          UPDATE agent_platform_devices
          SET
            revoked_at = NULL,
            last_seen_at = clock_timestamp()
          WHERE device_id = ${deviceId}
        `;
        row.revoked_at = null;
      }
      if (created) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: ownerId,
          actorUserId,
          action: "device.registered",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(row.created_at),
        });
      } else if (allowReactivate && wasRevoked) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: ownerId,
          actorUserId,
          action: "device.reactivated",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(new Date()),
        });
      }
      return { device: registeredDevice(row), created };
    });
  }

  async ingestBatch(batch: SyncBatch): Promise<IngestResult> {
    try {
      return await this.#sql.begin(async (sql) => {
        const devices = await sql<DeviceRow[]>`
          SELECT
            device_id,
            public_key,
            owner_id,
            created_at,
            last_seen_at,
            last_sequence,
            revoked_at
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
        if (device.revoked_at !== null) {
          throw new GatewayStoreError("device_revoked", "device is revoked");
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
        const changes: SessionStatusChange[] = [];
        if (newEvents.length > 0) {
          const snapshotEvents = bestSessionEvents(newEvents);
          // The device row is already locked FOR UPDATE, so no concurrent
          // batch can move these snapshots between the before/after reads.
          const before = new Map<string, ActivityStatus>(
            (
              await sql<SessionStatusRow[]>`
                SELECT agent_type, agent_session_id, activity_status, event_data
                FROM agent_platform_sessions
                WHERE
                  device_id = ${batch.device_id}
                  AND agent_session_id IN ${sql(
                    snapshotEvents.map((event) => event.agent_session_id),
                  )}
              `
            ).map(
              (row) =>
                [
                  `${row.agent_type}:${row.agent_session_id}`,
                  row.activity_status,
                ] as const,
            ),
          );
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

          const after = await sql<SessionStatusRow[]>`
            SELECT agent_type, agent_session_id, activity_status, event_data
            FROM agent_platform_sessions
            WHERE
              device_id = ${batch.device_id}
              AND agent_session_id IN ${sql(
                snapshotEvents.map((event) => event.agent_session_id),
              )}
          `;
          const touched = new Set<string>(
            snapshotEvents.map(
              (event) => `${event.agent_type}:${event.agent_session_id}`,
            ),
          );
          for (const row of after) {
            const key = `${row.agent_type}:${row.agent_session_id}`;
            if (!touched.has(key)) {
              continue;
            }
            const previous = before.get(key);
            if (previous === row.activity_status) {
              continue;
            }
            changes.push(
              sessionStatusChange(syncEvent(row.event_data), previous),
            );
          }
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
        return {
          accepted: newEvents.length,
          owner_id: device.owner_id,
          changes,
        };
      });
    } catch (error) {
      throw classifyPostgresConflict(error);
    }
  }

  async ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult> {
    return this.#sql.begin(async (sql) => {
      const devices = await sql<DeviceRow[]>`
        SELECT
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence,
          revoked_at
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
      if (device.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (device.public_key !== batch.public_key) {
        throw new GatewayStoreError(
          "device_key_mismatch",
          "auth batch public key does not match registered device",
        );
      }

      const receivedAt = new Date();
      // One read for the whole batch instead of a lookup per snapshot.
      const existingRows = await sql<
        Array<{
          agent_type: string;
          checked_at: unknown;
          snapshot_data: unknown;
        }>
      >`
        SELECT agent_type, checked_at, snapshot_data
        FROM agent_platform_auth_snapshots
        WHERE device_id = ${batch.device_id}
      `;
      const existingByProvider = new Map(
        existingRows.map((row) => [row.agent_type, row] as const),
      );
      let accepted = 0;
      for (const snapshot of batch.snapshots) {
        const existing = existingByProvider.get(snapshot.provider);
        const existingCheckedAt = existing?.checked_at;
        const existingSnapshot = existing?.snapshot_data as
          AuthSnapshot | undefined;
        if (
          existingCheckedAt !== undefined &&
          (Date.parse(isoTimestamp(existingCheckedAt)) >
            Date.parse(snapshot.checked_at) ||
            (Date.parse(isoTimestamp(existingCheckedAt)) ===
              Date.parse(snapshot.checked_at) &&
              existingSnapshot !== undefined &&
              isDeepStrictEqual(existingSnapshot, snapshot)))
        ) {
          continue;
        }
        await sql`
          INSERT INTO agent_platform_auth_snapshots (
            device_id,
            agent_type,
            checked_at,
            received_at,
            snapshot_data
          )
          VALUES (
            ${batch.device_id},
            ${snapshot.provider},
            ${snapshot.checked_at},
            ${receivedAt},
            ${sql.json(snapshot as unknown as postgres.JSONValue)}
          )
          ON CONFLICT (device_id, agent_type)
          DO UPDATE SET
            checked_at = EXCLUDED.checked_at,
            received_at = EXCLUDED.received_at,
            snapshot_data = EXCLUDED.snapshot_data
          WHERE EXCLUDED.checked_at >= agent_platform_auth_snapshots.checked_at
        `;
        accepted += 1;
      }
      await sql`
        UPDATE agent_platform_devices
        SET last_seen_at = ${receivedAt}
        WHERE device_id = ${batch.device_id}
      `;
      const rows = await sql<AuthSnapshotRow[]>`
        SELECT device_id, agent_type, checked_at, received_at, snapshot_data
        FROM agent_platform_auth_snapshots
        WHERE device_id = ${batch.device_id}
        ORDER BY checked_at DESC, agent_type
      `;
      return {
        accepted,
        owner_id: device.owner_id,
        snapshots: rows.map((row) => ({
          ...(row.snapshot_data as AuthSnapshot),
          device_id: batch.device_id,
          received_at: isoTimestamp(row.received_at),
        })),
      };
    });
  }
}
