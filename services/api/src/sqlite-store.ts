import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import type {
  ActivityStatus,
  AuthSyncBatch,
  DeviceHeartbeat,
  SyncBatch,
  SyncEvent,
} from "@agent-pulse/contracts";
import type { PushSubscription } from "./push.js";
import {
  SQLITE_GATEWAY_SCHEMA,
  sessionStatusChange,
  shouldReplaceSnapshot,
} from "./store-schemas.js";
import {
  GatewayStoreError,
  type AuthFilters,
  type AuthIngestResult,
  type DeviceRegistrationResult,
  type DeviceView,
  type GatewayStore,
  type HeartbeatResult,
  type IngestResult,
  type RegisteredDevice,
  type SessionEventFilters,
  type SessionFilters,
  type SessionStatusChange,
  type StoredAuthSnapshot,
  type StoredSession,
} from "./store.js";
import type { AccountStore } from "./accounts.js";

export class SqliteGatewayStore implements GatewayStore {
  private db: DatabaseSync;
  private now: () => Date;
  private closed = false;

  constructor(path: string, options: { now?: () => Date } = {}) {
    const dir = dirname(path);
    if (dir && dir !== "." && dir !== "") {
      mkdirSync(dir, { recursive: true });
    }
    this.db = new DatabaseSync(path);
    this.now = options.now ?? (() => new Date());
    this.initTables();
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.db.close();
  }

  private transaction<T>(work: () => T): T {
    this.db.exec("BEGIN IMMEDIATE");
    try {
      const res = work();
      this.db.exec("COMMIT");
      return res;
    } catch (error) {
      this.db.exec("ROLLBACK");
      throw error;
    }
  }

  private initTables(): void {
    this.db.exec(SQLITE_GATEWAY_SCHEMA);
    const cols = this.db
      .prepare("PRAGMA table_info(registered_devices)")
      .all() as Array<{ name: string }>;
    if (!cols.some((c) => c.name === "last_sequence")) {
      this.db.exec(
        "ALTER TABLE registered_devices ADD COLUMN last_sequence INTEGER NOT NULL DEFAULT 0",
      );
    }
    if (!cols.some((c) => c.name === "last_heartbeat_at")) {
      this.db.exec(
        "ALTER TABLE registered_devices ADD COLUMN last_heartbeat_at TEXT",
      );
    }
  }

  setAuditRecorder?(_recorder: Pick<AccountStore, "recordAuditEvent">): void {}

  async getRegisteredDevice(
    deviceId: string,
  ): Promise<RegisteredDevice | undefined> {
    const row = this.db
      .prepare("SELECT * FROM registered_devices WHERE device_id = ?")
      .get(deviceId) as
      | {
          device_id: string;
          public_key: string;
          owner_id: string;
          created_at: string;
          last_seen_at: string;
          revoked_at: string | null;
        }
      | undefined;
    if (!row) return undefined;
    return {
      device_id: row.device_id,
      public_key: row.public_key,
      owner_id: row.owner_id,
      created_at: row.created_at,
      last_seen_at: row.last_seen_at,
      ...(row.revoked_at ? { revoked_at: row.revoked_at } : {}),
    };
  }

  async getRegisteredDevices(
    deviceIds: readonly string[],
  ): Promise<Map<string, RegisteredDevice>> {
    const map = new Map<string, RegisteredDevice>();
    if (deviceIds.length === 0) return map;
    const placeholders = deviceIds.map(() => "?").join(",");
    const rows = this.db
      .prepare(
        `SELECT * FROM registered_devices WHERE device_id IN (${placeholders})`,
      )
      .all(...deviceIds) as Array<{
      device_id: string;
      public_key: string;
      owner_id: string;
      created_at: string;
      last_seen_at: string;
      revoked_at: string | null;
    }>;
    for (const row of rows) {
      map.set(row.device_id, {
        device_id: row.device_id,
        public_key: row.public_key,
        owner_id: row.owner_id,
        created_at: row.created_at,
        last_seen_at: row.last_seen_at,
        ...(row.revoked_at ? { revoked_at: row.revoked_at } : {}),
      });
    }
    return map;
  }

  async createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
    actorUserId?: string,
  ): Promise<void> {
    const nowIso = this.now().toISOString();
    this.db
      .prepare(
        "DELETE FROM binding_codes WHERE expires_at <= ? AND claimed_device_id IS NULL",
      )
      .run(nowIso);

    const existing = this.db
      .prepare("SELECT 1 FROM binding_codes WHERE code_digest = ?")
      .get(codeDigest);
    if (existing) {
      throw new GatewayStoreError(
        "binding_code_conflict",
        "binding code already exists",
      );
    }

    this.db
      .prepare(
        "INSERT INTO binding_codes (code_digest, owner_id, expires_at, issued_by) VALUES (?, ?, ?, ?)",
      )
      .run(codeDigest, ownerId, expiresAt, actorUserId ?? null);
  }

  async registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    const binding = this.db
      .prepare("SELECT * FROM binding_codes WHERE code_digest = ?")
      .get(codeDigest) as
      | {
          code_digest: string;
          owner_id: string;
          expires_at: string;
          claimed_device_id: string | null;
          issued_by: string | null;
        }
      | undefined;

    if (!binding) {
      throw new GatewayStoreError(
        "invalid_binding_code",
        "binding code is invalid or expired",
      );
    }
    if (binding.claimed_device_id !== null) {
      if (binding.claimed_device_id !== deviceId) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      return this.registerDevice(
        binding.owner_id,
        deviceId,
        publicKey,
        binding.issued_by ?? undefined,
        false,
      );
    }
    if (Date.parse(binding.expires_at) <= this.now().getTime()) {
      throw new GatewayStoreError(
        "invalid_binding_code",
        "binding code is invalid or expired",
      );
    }

    this.db
      .prepare(
        "UPDATE binding_codes SET claimed_device_id = ? WHERE code_digest = ?",
      )
      .run(deviceId, codeDigest);

    try {
      return await this.registerDevice(
        binding.owner_id,
        deviceId,
        publicKey,
        binding.issued_by ?? undefined,
        true,
      );
    } catch (err) {
      this.db
        .prepare(
          "UPDATE binding_codes SET claimed_device_id = NULL WHERE code_digest = ?",
        )
        .run(codeDigest);
      throw err;
    }
  }

  async revokeDevice(
    ownerId: string,
    deviceId: string,
    _actorUserId?: string,
  ): Promise<boolean> {
    const device = await this.getRegisteredDevice(deviceId);
    if (
      !device ||
      device.owner_id !== ownerId ||
      device.revoked_at !== undefined
    ) {
      return false;
    }
    const nowIso = this.now().toISOString();
    this.db
      .prepare(
        "UPDATE registered_devices SET revoked_at = ? WHERE device_id = ?",
      )
      .run(nowIso, deviceId);
    return true;
  }

  async savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
  ): Promise<void> {
    this.db
      .prepare(
        "INSERT INTO push_subscriptions (endpoint, owner_id, payload) VALUES (?, ?, ?) ON CONFLICT(endpoint) DO UPDATE SET owner_id = excluded.owner_id, payload = excluded.payload",
      )
      .run(subscription.endpoint, ownerId, JSON.stringify(subscription));
  }

  async deletePushSubscription(
    ownerId: string,
    endpoint: string,
  ): Promise<void> {
    this.db
      .prepare(
        "DELETE FROM push_subscriptions WHERE endpoint = ? AND owner_id = ?",
      )
      .run(endpoint, ownerId);
  }

  async listPushSubscriptions(ownerId: string): Promise<PushSubscription[]> {
    const rows = this.db
      .prepare("SELECT payload FROM push_subscriptions WHERE owner_id = ?")
      .all(ownerId) as Array<{ payload: string }>;
    return rows.map((r) => JSON.parse(r.payload) as PushSubscription);
  }

  async countActiveDevices(ownerId: string): Promise<number> {
    const row = this.db
      .prepare(
        "SELECT COUNT(*) as count FROM registered_devices WHERE owner_id = ? AND revoked_at IS NULL",
      )
      .get(ownerId) as { count: number };
    return row.count;
  }

  async countPendingBindingCodes(
    ownerId: string,
    nowIso: string,
  ): Promise<number> {
    const row = this.db
      .prepare(
        "SELECT COUNT(*) as count FROM binding_codes WHERE owner_id = ? AND claimed_device_id IS NULL AND expires_at > ?",
      )
      .get(ownerId, nowIso) as { count: number };
    return Number(row.count ?? 0);
  }

  async pruneExpiredData(nowIso: string, historyDays = 90): Promise<void> {
    const cutoff = new Date(
      Date.parse(nowIso) - historyDays * 86_400_000,
    ).toISOString();
    this.db
      .prepare(
        "DELETE FROM binding_codes WHERE expires_at <= ? AND claimed_device_id IS NULL",
      )
      .run(nowIso);
    this.db
      .prepare(
        `DELETE FROM sync_events WHERE rowid IN (
          SELECT e.rowid FROM sync_events e
          JOIN registered_devices d ON d.device_id = e.device_id
          WHERE e.sequence < d.last_sequence AND e.observed_at < ?
        )`,
      )
      .run(cutoff);
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
    _actorUserId?: string,
    allowReactivate = false,
  ): Promise<DeviceRegistrationResult> {
    const existing = await this.getRegisteredDevice(deviceId);
    if (existing !== undefined) {
      if (existing.public_key !== publicKey || existing.owner_id !== ownerId) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      if (existing.revoked_at !== undefined) {
        if (!allowReactivate) {
          throw new GatewayStoreError("device_revoked", "device is revoked");
        }
        this.db
          .prepare(
            "UPDATE registered_devices SET revoked_at = NULL, last_seen_at = ? WHERE device_id = ?",
          )
          .run(this.now().toISOString(), deviceId);
        delete existing.revoked_at;
      }
      return { device: existing, created: false };
    }
    const nowIso = this.now().toISOString();
    const device: RegisteredDevice = {
      device_id: deviceId,
      public_key: publicKey,
      owner_id: ownerId,
      created_at: nowIso,
      last_seen_at: nowIso,
    };
    this.db
      .prepare(
        "INSERT INTO registered_devices (device_id, public_key, owner_id, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?)",
      )
      .run(deviceId, publicKey, ownerId, nowIso, nowIso);
    return { device, created: true };
  }

  async ingestBatch(batch: SyncBatch): Promise<IngestResult> {
    return this.transaction(() => {
      const device = this.db
        .prepare("SELECT * FROM registered_devices WHERE device_id = ?")
        .get(batch.device_id) as
        | {
            device_id: string;
            public_key: string;
            owner_id: string;
            revoked_at: string | null;
            last_sequence: number;
          }
        | undefined;
      if (!device) {
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
          "sync batch public key does not match registered device",
        );
      }
      const newEvents: SyncEvent[] = [];
      for (const event of batch.events) {
        const byId = this.db
          .prepare("SELECT event_data FROM sync_events WHERE event_id = ?")
          .get(event.event_id) as { event_data: string } | undefined;
        if (byId) {
          if (!isDeepStrictEqual(JSON.parse(byId.event_data), event)) {
            throw new GatewayStoreError(
              "event_id_conflict",
              "event_id was reused for different content",
            );
          }
          continue;
        }
        const bySeq = this.db
          .prepare(
            "SELECT event_id FROM sync_events WHERE device_id = ? AND sequence = ?",
          )
          .get(event.device_id, event.sequence) as
          { event_id: string } | undefined;
        if (bySeq && bySeq.event_id !== event.event_id) {
          throw new GatewayStoreError(
            "sequence_conflict",
            "device sequence was reused by another event",
          );
        }
        newEvents.push(event);
      }
      if (
        newEvents.length > 0 &&
        newEvents[0]!.sequence !== device.last_sequence + 1
      ) {
        throw new GatewayStoreError(
          "sequence_gap",
          "new events do not continue the stored device sequence",
        );
      }
      const before = new Map<string, ActivityStatus>();
      for (const event of newEvents) {
        const key = `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
        if (before.has(key)) continue;
        const row = this.db
          .prepare(
            "SELECT activity_status FROM sync_sessions WHERE device_id = ? AND agent_type = ? AND agent_session_id = ?",
          )
          .get(event.device_id, event.agent_type, event.agent_session_id) as
          { activity_status: ActivityStatus } | undefined;
        if (row) before.set(key, row.activity_status);
      }
      const receivedAt = this.now().toISOString();
      const touched = new Map<string, StoredSession>();
      const insertEvent = this.db.prepare(
        "INSERT INTO sync_events (event_id, device_id, sequence, observed_at, received_at, event_data) VALUES (?, ?, ?, ?, ?, ?)",
      );
      const getSession = this.db.prepare(
        "SELECT received_at, event_data FROM sync_sessions WHERE device_id = ? AND agent_type = ? AND agent_session_id = ?",
      );
      const upsertSession = this.db.prepare(
        `INSERT INTO sync_sessions (device_id, agent_type, agent_session_id, observed_at, received_at, sequence, confidence, presence, activity_status, event_data)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(device_id, agent_type, agent_session_id) DO UPDATE SET
           observed_at = excluded.observed_at,
           received_at = excluded.received_at,
           sequence = excluded.sequence,
           confidence = excluded.confidence,
           presence = excluded.presence,
           activity_status = excluded.activity_status,
           event_data = excluded.event_data`,
      );
      let nextSeq = device.last_sequence;
      for (const event of newEvents) {
        const rawEvent = JSON.stringify(event);
        insertEvent.run(
          event.event_id,
          event.device_id,
          event.sequence,
          event.observed_at,
          receivedAt,
          rawEvent,
        );
        nextSeq = Math.max(nextSeq, event.sequence);
        const key = `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
        const existingRow = getSession.get(
          event.device_id,
          event.agent_type,
          event.agent_session_id,
        ) as { received_at: string; event_data: string } | undefined;
        const currentStored: StoredSession | undefined = existingRow
          ? {
              ...(JSON.parse(existingRow.event_data) as SyncEvent),
              received_at: existingRow.received_at,
            }
          : undefined;
        if (!currentStored || shouldReplaceSnapshot(currentStored, event)) {
          const updated: StoredSession = { ...event, received_at: receivedAt };
          upsertSession.run(
            event.device_id,
            event.agent_type,
            event.agent_session_id,
            event.observed_at,
            receivedAt,
            event.sequence,
            event.confidence,
            event.payload.state.presence,
            event.payload.state.activity_status,
            rawEvent,
          );
          touched.set(key, updated);
        }
      }
      this.db
        .prepare(
          "UPDATE registered_devices SET last_seen_at = ?, last_sequence = ? WHERE device_id = ?",
        )
        .run(receivedAt, nextSeq, batch.device_id);
      const changes: SessionStatusChange[] = [];
      for (const [key, snapshot] of touched) {
        const previous = before.get(key);
        if (previous === snapshot.payload.state.activity_status) continue;
        changes.push(sessionStatusChange(snapshot, previous));
      }
      return { accepted: newEvents.length, owner_id: device.owner_id, changes };
    });
  }

  async ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult> {
    return this.transaction(() => {
      const device = this.db
        .prepare(
          "SELECT owner_id, public_key, revoked_at FROM registered_devices WHERE device_id = ?",
        )
        .get(batch.device_id) as
        | { owner_id: string; public_key: string; revoked_at: string | null }
        | undefined;
      if (!device) {
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
      const receivedAt = this.now().toISOString();
      const accepted: StoredAuthSnapshot[] = [];
      let acceptedCount = 0;
      const getSnap = this.db.prepare(
        "SELECT checked_at, received_at, snapshot_data FROM auth_snapshots WHERE device_id = ? AND provider = ?",
      );
      const upsertSnap = this.db.prepare(
        `INSERT INTO auth_snapshots (device_id, provider, checked_at, received_at, snapshot_data)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(device_id, provider) DO UPDATE SET
           checked_at = excluded.checked_at,
           received_at = excluded.received_at,
           snapshot_data = excluded.snapshot_data`,
      );
      for (const snapshot of batch.snapshots) {
        const existingRow = getSnap.get(batch.device_id, snapshot.provider) as
          | { checked_at: string; received_at: string; snapshot_data: string }
          | undefined;
        if (existingRow) {
          const parsedExisting = JSON.parse(existingRow.snapshot_data);
          const existingChecked = Date.parse(existingRow.checked_at);
          const incomingChecked = Date.parse(snapshot.checked_at);
          if (
            existingChecked > incomingChecked ||
            (existingChecked === incomingChecked &&
              isDeepStrictEqual(parsedExisting, snapshot))
          ) {
            accepted.push({
              ...parsedExisting,
              device_id: batch.device_id,
              received_at: existingRow.received_at,
            });
            continue;
          }
        }
        upsertSnap.run(
          batch.device_id,
          snapshot.provider,
          snapshot.checked_at,
          receivedAt,
          JSON.stringify(snapshot),
        );
        accepted.push({
          ...snapshot,
          device_id: batch.device_id,
          received_at: receivedAt,
        });
        acceptedCount += 1;
      }
      this.db
        .prepare(
          "UPDATE registered_devices SET last_seen_at = ? WHERE device_id = ?",
        )
        .run(receivedAt, batch.device_id);
      return {
        accepted: acceptedCount,
        owner_id: device.owner_id,
        snapshots: accepted,
      };
    });
  }

  async recordHeartbeat(
    heartbeat: DeviceHeartbeat,
    receivedAt: string,
  ): Promise<HeartbeatResult> {
    const device = this.db
      .prepare(
        "SELECT owner_id, public_key, revoked_at, last_seen_at, last_heartbeat_at FROM registered_devices WHERE device_id = ?",
      )
      .get(heartbeat.device_id) as
      | {
          owner_id: string;
          public_key: string;
          revoked_at: string | null;
          last_seen_at: string;
          last_heartbeat_at: string | null;
        }
      | undefined;
    if (!device) {
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
    const accepted =
      device.last_heartbeat_at === null ||
      heartbeat.sent_at > device.last_heartbeat_at;
    const seenAt = accepted ? receivedAt : device.last_seen_at;
    if (accepted) {
      this.db
        .prepare(
          "UPDATE registered_devices SET last_seen_at = ?, last_heartbeat_at = ? WHERE device_id = ?",
        )
        .run(receivedAt, heartbeat.sent_at, heartbeat.device_id);
    }
    return { accepted, owner_id: device.owner_id, last_seen_at: seenAt };
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const rows = this.db
      .prepare(
        "SELECT device_id, created_at, last_seen_at, revoked_at FROM registered_devices WHERE owner_id = ? ORDER BY device_id ASC",
      )
      .all(ownerId) as Array<{
      device_id: string;
      created_at: string;
      last_seen_at: string;
      revoked_at: string | null;
    }>;
    const offlineBefore = this.now().getTime() - 45_000;
    return rows.map((r) => ({
      device_id: r.device_id,
      created_at: r.created_at,
      last_seen_at: r.last_seen_at,
      online:
        r.revoked_at === null && Date.parse(r.last_seen_at) >= offlineBefore,
      revoked: r.revoked_at !== null,
    }));
  }

  async listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]> {
    const rows = this.db
      .prepare(
        `SELECT s.received_at, s.event_data
         FROM sync_sessions s
         JOIN registered_devices d ON d.device_id = s.device_id
         WHERE d.owner_id = ?
           AND (? IS NULL OR s.device_id = ?)
           AND (? IS NULL OR s.agent_type = ?)
           AND (? IS NULL OR s.activity_status = ?)
           AND (? IS NULL OR s.presence = ?)
         ORDER BY s.observed_at DESC, s.sequence DESC`,
      )
      .all(
        ownerId,
        filters.device_id ?? null,
        filters.device_id ?? null,
        filters.agent_type ?? null,
        filters.agent_type ?? null,
        filters.activity_status ?? null,
        filters.activity_status ?? null,
        filters.presence ?? null,
        filters.presence ?? null,
      ) as Array<{ received_at: string; event_data: string }>;
    return rows.map((r) => ({
      ...(JSON.parse(r.event_data) as SyncEvent),
      received_at: r.received_at,
    }));
  }

  async listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]> {
    const device = await this.getRegisteredDevice(filters.device_id);
    if (!device || device.owner_id !== ownerId) return [];
    const rows = this.db
      .prepare(
        `SELECT received_at, event_data FROM sync_events
         WHERE device_id = ?
           AND json_extract(event_data, '$.agent_type') = ?
           AND json_extract(event_data, '$.agent_session_id') = ?
         ORDER BY sequence DESC LIMIT ?`,
      )
      .all(
        filters.device_id,
        filters.agent_type,
        filters.agent_session_id,
        filters.limit,
      ) as Array<{
      received_at: string;
      event_data: string;
    }>;
    return rows.map((r) => ({
      ...(JSON.parse(r.event_data) as SyncEvent),
      received_at: r.received_at,
    }));
  }

  async listAuth(
    ownerId: string,
    filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    const rows = this.db
      .prepare(
        `SELECT a.device_id, a.received_at, a.snapshot_data
         FROM auth_snapshots a
         JOIN registered_devices d ON d.device_id = a.device_id
         WHERE d.owner_id = ?
           AND (? IS NULL OR a.device_id = ?)
         ORDER BY a.checked_at DESC, a.provider ASC`,
      )
      .all(
        ownerId,
        filters.device_id ?? null,
        filters.device_id ?? null,
      ) as Array<{
      device_id: string;
      received_at: string;
      snapshot_data: string;
    }>;
    return rows.map((r) => ({
      ...JSON.parse(r.snapshot_data),
      device_id: r.device_id,
      received_at: r.received_at,
    }));
  }
}
