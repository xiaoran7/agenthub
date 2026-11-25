import {
  type AuthSnapshot,
  type DeviceHeartbeat,
} from "@agent-pulse/contracts";
import type postgres from "postgres";
import {
  isoTimestamp,
  registeredDevice,
  syncEvent,
  type DeviceRow,
} from "../postgres-helpers.js";
import {
  GatewayStoreError,
  type AuthFilters,
  type DeviceView,
  type HeartbeatResult,
  type RegisteredDevice,
  type SessionEventFilters,
  type SessionFilters,
  type StoredAuthSnapshot,
  type StoredSession,
} from "../store.js";
import { type AuthSnapshotRow, type SessionRow } from "./pg-shared.js";

export class PostgresSyncReadRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  async recordHeartbeat(
    heartbeat: DeviceHeartbeat,
    receivedAt: string,
  ): Promise<HeartbeatResult> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{
          owner_id: string;
          public_key: string;
          revoked_at: unknown | null;
          last_seen_at: unknown;
          last_heartbeat_at: unknown | null;
        }>
      >`
        SELECT owner_id, public_key, revoked_at, last_seen_at, last_heartbeat_at
        FROM agent_platform_devices
        WHERE device_id = ${heartbeat.device_id}
        FOR UPDATE
      `;
      const device = rows[0];
      if (device === undefined) {
        throw new GatewayStoreError(
          "device_not_registered",
          "device is not registered",
        );
      }
      if (device.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (device.public_key !== heartbeat.public_key) {
        throw new GatewayStoreError(
          "device_key_mismatch",
          "heartbeat public key does not match registered device",
        );
      }
      const previous =
        device.last_heartbeat_at === null
          ? undefined
          : isoTimestamp(device.last_heartbeat_at);
      const accepted =
        previous === undefined ||
        Date.parse(heartbeat.sent_at) > Date.parse(previous);
      if (accepted) {
        await sql`
          UPDATE agent_platform_devices
          SET last_seen_at = ${receivedAt}, last_heartbeat_at = ${heartbeat.sent_at}
          WHERE device_id = ${heartbeat.device_id}
        `;
        return {
          accepted: true,
          owner_id: device.owner_id,
          last_seen_at: receivedAt,
        };
      }
      return {
        accepted: false,
        owner_id: device.owner_id,
        last_seen_at: isoTimestamp(device.last_seen_at),
      };
    });
  }

  async getRegisteredDevice(
    deviceId: string,
  ): Promise<RegisteredDevice | undefined> {
    const rows = await this.#sql<
      DeviceRow[]
    >`SELECT * FROM agent_platform_devices WHERE device_id = ${deviceId}`;
    return rows[0] === undefined ? undefined : registeredDevice(rows[0]);
  }

  async getRegisteredDevices(
    deviceIds: readonly string[],
  ): Promise<Map<string, RegisteredDevice>> {
    if (deviceIds.length === 0) {
      return new Map();
    }
    const rows = await this.#sql<DeviceRow[]>`
      SELECT * FROM agent_platform_devices
      WHERE device_id = ANY(${deviceIds as string[]})
    `;
    return new Map(
      rows.map((row) => [row.device_id, registeredDevice(row)] as const),
    );
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const rows = await this.#sql<
      Array<{
        device_id: string;
        created_at: unknown;
        last_seen_at: unknown;
        online: boolean;
        revoked_at: unknown | null;
      }>
    >`
      SELECT
        device_id,
        created_at,
        last_seen_at,
        revoked_at,
        revoked_at IS NULL
          AND last_seen_at >= clock_timestamp() - interval '45 seconds' AS online
      FROM agent_platform_devices
      WHERE owner_id = ${ownerId}
      ORDER BY device_id
    `;
    return rows.map((row) => ({
      device_id: row.device_id,
      created_at: isoTimestamp(row.created_at),
      last_seen_at: isoTimestamp(row.last_seen_at),
      online: row.online,
      revoked: row.revoked_at !== null,
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
    const agentFilter =
      filters.agent_type === undefined
        ? this.#sql``
        : this.#sql`AND sessions.agent_type = ${filters.agent_type}`;
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
        ${agentFilter}
        ${activityFilter}
        ${presenceFilter}
      ORDER BY sessions.observed_at DESC, sessions.sequence DESC
    `;
    return rows.map((row) => ({
      ...syncEvent(row.event_data),
      received_at: isoTimestamp(row.received_at),
    }));
  }

  async listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]> {
    const rows = await this.#sql<SessionRow[]>`
      SELECT events.event_data, events.received_at
      FROM agent_platform_events AS events
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = events.device_id
      WHERE
        devices.owner_id = ${ownerId}
        AND events.device_id = ${filters.device_id}
        AND events.event_data->>'agent_type' = ${filters.agent_type}
        AND events.event_data->>'agent_session_id' = ${filters.agent_session_id}
      ORDER BY events.sequence DESC
      LIMIT ${filters.limit}
    `;
    return rows.map((row) => ({
      ...syncEvent(row.event_data),
      received_at: isoTimestamp(row.received_at),
    }));
  }

  async listAuth(
    ownerId: string,
    filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    const deviceFilter =
      filters.device_id === undefined
        ? this.#sql``
        : this.#sql`AND snapshots.device_id = ${filters.device_id}`;
    const rows = await this.#sql<AuthSnapshotRow[]>`
      SELECT
        snapshots.device_id,
        snapshots.agent_type,
        snapshots.checked_at,
        snapshots.received_at,
        snapshots.snapshot_data
      FROM agent_platform_auth_snapshots AS snapshots
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = snapshots.device_id
      WHERE devices.owner_id = ${ownerId}
        ${deviceFilter}
      ORDER BY snapshots.checked_at DESC, snapshots.agent_type
    `;
    return rows.map((row) => {
      const snapshot = row.snapshot_data as AuthSnapshot;
      return {
        ...snapshot,
        device_id: row.device_id,
        received_at: isoTimestamp(row.received_at),
      };
    });
  }

  async pruneExpiredData(nowIso: string, historyDays = 90): Promise<void> {
    const cutoff = new Date(
      Date.parse(nowIso) - historyDays * 86_400_000,
    ).toISOString();
    const loginLimitCutoff = new Date(
      Date.parse(nowIso) - 86_400_000,
    ).toISOString();
    const auditCutoff = new Date(
      Date.parse(nowIso) - 365 * 86_400_000,
    ).toISOString();
    await this
      .#sql`DELETE FROM agent_pulse_web_sessions WHERE expires_at <= ${nowIso}::timestamptz`;
    await this
      .#sql`DELETE FROM agent_pulse_email_verifications WHERE expires_at <= ${nowIso}::timestamptz`;
    await this
      .#sql`DELETE FROM agent_pulse_password_resets WHERE expires_at <= ${nowIso}::timestamptz`;
    await this
      .#sql`DELETE FROM agent_pulse_team_invitations WHERE expires_at <= ${nowIso}::timestamptz AND consumed_at IS NULL`;
    await this
      .#sql`DELETE FROM agent_platform_binding_codes WHERE expires_at <= ${nowIso}::timestamptz AND claimed_at IS NULL`;
    await this.#sql`
      DELETE FROM agent_pulse_login_limits
      WHERE (blocked_until IS NOT NULL AND blocked_until <= ${nowIso}::timestamptz)
         OR (blocked_until IS NULL AND window_started_at <= ${loginLimitCutoff}::timestamptz)
    `;
    await this
      .#sql`DELETE FROM agent_pulse_audit_events WHERE created_at < ${auditCutoff}::timestamptz`;
    await this.#sql`
      DELETE FROM agent_platform_events e
      USING agent_platform_devices d
      WHERE e.device_id = d.device_id
        AND e.sequence < d.last_sequence
        AND e.observed_at < ${cutoff}::timestamptz
    `;
  }
}
