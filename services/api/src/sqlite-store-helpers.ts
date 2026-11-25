import type { DatabaseSync } from "node:sqlite";
import type { PushSubscription } from "./push.js";
import { SQLITE_GATEWAY_SCHEMA } from "./store-schemas.js";
import type {
  AuthFilters,
  DeviceView,
  StoredAuthSnapshot,
  StoredPushSubscription,
} from "./store-types.js";

export function runSqliteGatewayTransaction<T>(
  db: DatabaseSync,
  work: () => T,
): T {
  db.exec("BEGIN IMMEDIATE");
  try {
    const res = work();
    db.exec("COMMIT");
    return res;
  } catch (error) {
    try {
      db.exec("ROLLBACK");
    } catch {
      // Ignore rollback errors if SQLite already rolled back the transaction.
    }
    throw error;
  }
}

export function initSqliteGatewayTables(db: DatabaseSync): void {
  db.exec(SQLITE_GATEWAY_SCHEMA);
  const deviceCols = db
    .prepare("PRAGMA table_info(registered_devices)")
    .all() as Array<{ name: string }>;
  if (!deviceCols.some((c) => c.name === "last_sequence")) {
    db.exec(
      "ALTER TABLE registered_devices ADD COLUMN last_sequence INTEGER NOT NULL DEFAULT 0",
    );
  }
  if (!deviceCols.some((c) => c.name === "last_heartbeat_at")) {
    db.exec("ALTER TABLE registered_devices ADD COLUMN last_heartbeat_at TEXT");
  }
  const pushCols = db
    .prepare("PRAGMA table_info(push_subscriptions)")
    .all() as Array<{ name: string }>;
  if (!pushCols.some((c) => c.name === "subscriber_user_id")) {
    db.exec(
      "ALTER TABLE push_subscriptions ADD COLUMN subscriber_user_id TEXT",
    );
  }
  db.exec(
    "CREATE INDEX IF NOT EXISTS idx_push_owner_subscriber ON push_subscriptions(owner_id, subscriber_user_id)",
  );
}

export function saveSqlitePushSubscription(
  db: DatabaseSync,
  ownerId: string,
  subscription: PushSubscription,
  subscriberUserId?: string,
): void {
  db.prepare(
    `INSERT INTO push_subscriptions (endpoint, owner_id, subscriber_user_id, payload)
     VALUES (?, ?, ?, ?)
     ON CONFLICT(endpoint) DO UPDATE SET
       owner_id = excluded.owner_id,
       subscriber_user_id = excluded.subscriber_user_id,
       payload = excluded.payload`,
  ).run(
    subscription.endpoint,
    ownerId,
    subscriberUserId ?? null,
    JSON.stringify(subscription),
  );
}

export function deleteSqlitePushSubscription(
  db: DatabaseSync,
  ownerId: string,
  endpoint: string,
  subscriberUserId?: string,
): void {
  if (subscriberUserId !== undefined) {
    db.prepare(
      "DELETE FROM push_subscriptions WHERE endpoint = ? AND owner_id = ? AND subscriber_user_id = ?",
    ).run(endpoint, ownerId, subscriberUserId);
    return;
  }
  db.prepare(
    "DELETE FROM push_subscriptions WHERE endpoint = ? AND owner_id = ?",
  ).run(endpoint, ownerId);
}

export function deleteSqlitePushSubscriptionsForSubscriber(
  db: DatabaseSync,
  ownerId: string,
  subscriberUserId: string,
): void {
  db.prepare(
    "DELETE FROM push_subscriptions WHERE owner_id = ? AND subscriber_user_id = ?",
  ).run(ownerId, subscriberUserId);
}

export function listSqlitePushSubscriptions(
  db: DatabaseSync,
  ownerId: string,
  subscriberUserId?: string,
): StoredPushSubscription[] {
  const rows = (
    subscriberUserId !== undefined
      ? db
          .prepare(
            "SELECT payload, subscriber_user_id FROM push_subscriptions WHERE owner_id = ? AND subscriber_user_id = ?",
          )
          .all(ownerId, subscriberUserId)
      : db
          .prepare(
            "SELECT payload, subscriber_user_id FROM push_subscriptions WHERE owner_id = ?",
          )
          .all(ownerId)
  ) as Array<{ payload: string; subscriber_user_id: string | null }>;
  return rows.map((r) => ({
    ...(JSON.parse(r.payload) as PushSubscription),
    ...(r.subscriber_user_id !== null
      ? { subscriber_user_id: r.subscriber_user_id }
      : {}),
  }));
}

export function pruneSqliteExpiredData(
  db: DatabaseSync,
  nowIso: string,
  historyDays = 90,
): void {
  const cutoff = new Date(
    Date.parse(nowIso) - historyDays * 86_400_000,
  ).toISOString();
  db.prepare(
    "DELETE FROM binding_codes WHERE expires_at <= ? AND claimed_device_id IS NULL",
  ).run(nowIso);
  db.prepare(
    `DELETE FROM sync_events WHERE rowid IN (
      SELECT e.rowid FROM sync_events e
      JOIN registered_devices d ON d.device_id = e.device_id
      WHERE e.sequence < d.last_sequence AND e.observed_at < ?
    )`,
  ).run(cutoff);
}

export function listSqliteDevices(
  db: DatabaseSync,
  ownerId: string,
  nowMs: number,
): DeviceView[] {
  const rows = db
    .prepare(
      "SELECT device_id, created_at, last_seen_at, revoked_at FROM registered_devices WHERE owner_id = ? ORDER BY device_id ASC",
    )
    .all(ownerId) as Array<{
    device_id: string;
    created_at: string;
    last_seen_at: string;
    revoked_at: string | null;
  }>;
  const offlineBefore = nowMs - 45_000;
  return rows.map((r) => ({
    device_id: r.device_id,
    created_at: r.created_at,
    last_seen_at: r.last_seen_at,
    online:
      r.revoked_at === null && Date.parse(r.last_seen_at) >= offlineBefore,
    revoked: r.revoked_at !== null,
  }));
}

export function listSqliteAuth(
  db: DatabaseSync,
  ownerId: string,
  filters: AuthFilters,
): StoredAuthSnapshot[] {
  const rows = db
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
