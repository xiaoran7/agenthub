import { DatabaseSync } from "node:sqlite";
import type {
  ActivityStatus,
  AgentType,
  AuthSnapshot,
  AuthSyncBatch,
  DeviceHeartbeat,
  Presence,
  SyncBatch,
} from "@agent-pulse/contracts";
import type { PushSubscription } from "./push.js";
import type {
  AuthFilters,
  AuthIngestResult,
  DeviceRegistrationResult,
  DeviceView,
  GatewayStore,
  HeartbeatResult,
  IngestResult,
  RegisteredDevice,
  SessionEventFilters,
  SessionFilters,
  StoredAuthSnapshot,
  StoredSession,
} from "./store.js";
import { GatewayStoreError } from "./store.js";
import type { AccountStore } from "./accounts.js";

export class SqliteGatewayStore implements GatewayStore {
  private db: DatabaseSync;
  private now: () => Date;

  constructor(path: string, options: { now?: () => Date } = {}) {
    this.db = new DatabaseSync(path);
    this.now = options.now ?? (() => new Date());
    this.initTables();
  }

  private initTables(): void {
    this.db.exec(`
      PRAGMA journal_mode=WAL;
      PRAGMA synchronous=NORMAL;
      CREATE TABLE IF NOT EXISTS registered_devices (
        device_id TEXT PRIMARY KEY,
        public_key TEXT NOT NULL,
        owner_id TEXT NOT NULL,
        created_at TEXT NOT NULL,
        last_seen_at TEXT NOT NULL,
        revoked_at TEXT
      );
      CREATE INDEX IF NOT EXISTS idx_devices_owner ON registered_devices(owner_id);

      CREATE TABLE IF NOT EXISTS binding_codes (
        code_digest TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        claimed_device_id TEXT,
        issued_by TEXT
      );

      CREATE TABLE IF NOT EXISTS push_subscriptions (
        endpoint TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        payload TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_push_owner ON push_subscriptions(owner_id);
    `);
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

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
    _actorUserId?: string,
  ): Promise<DeviceRegistrationResult> {
    const existing = await this.getRegisteredDevice(deviceId);
    if (existing !== undefined) {
      if (existing.revoked_at !== undefined) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (existing.public_key !== publicKey || existing.owner_id !== ownerId) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
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

  async ingestBatch(_batch: SyncBatch): Promise<IngestResult> {
    return {
      accepted: 0,
      owner_id: "default",
      changes: [],
    };
  }

  async ingestAuthBatch(_batch: AuthSyncBatch): Promise<AuthIngestResult> {
    return {
      accepted: 0,
      owner_id: "default",
      snapshots: [],
    };
  }

  async recordHeartbeat(
    heartbeat: DeviceHeartbeat,
    _receivedAt: string,
  ): Promise<HeartbeatResult> {
    const device = await this.getRegisteredDevice(heartbeat.device_id);
    if (!device || device.revoked_at !== undefined) {
      throw new GatewayStoreError(
        "device_not_registered",
        "device is not registered",
      );
    }
    const nowIso = this.now().toISOString();
    this.db
      .prepare(
        "UPDATE registered_devices SET last_seen_at = ? WHERE device_id = ?",
      )
      .run(nowIso, heartbeat.device_id);
    return {
      accepted: true,
      owner_id: device.owner_id,
      last_seen_at: nowIso,
    };
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const rows = this.db
      .prepare(
        "SELECT * FROM registered_devices WHERE owner_id = ? ORDER BY created_at DESC",
      )
      .all(ownerId) as Array<{
      device_id: string;
      created_at: string;
      last_seen_at: string;
      revoked_at: string | null;
    }>;
    const nowMs = this.now().getTime();
    return rows.map((r) => ({
      device_id: r.device_id,
      created_at: r.created_at,
      last_seen_at: r.last_seen_at,
      online: nowMs - Date.parse(r.last_seen_at) < 300_000,
      revoked: r.revoked_at !== null,
    }));
  }

  async listSessions(
    _ownerId: string,
    _filters: SessionFilters,
  ): Promise<StoredSession[]> {
    return [];
  }

  async listSessionEvents(
    _ownerId: string,
    _filters: SessionEventFilters,
  ): Promise<StoredSession[]> {
    return [];
  }

  async listAuthSnapshots(
    _ownerId: string,
    _filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    return [];
  }

  async listAuth(
    _ownerId: string,
    _filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    return [];
  }
}
