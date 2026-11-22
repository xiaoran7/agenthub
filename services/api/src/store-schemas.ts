import { createHash } from "node:crypto";
import type { ActivityStatus, SyncEvent } from "@agent-pulse/contracts";
import type { SessionStatusChange, StoredSession } from "./store.js";

const CONFIDENCE_RANK: Record<SyncEvent["confidence"], number> = {
  unknown: 0,
  inferred: 1,
  observed: 2,
  authoritative: 3,
};

export function bindingCodeDigest(code: string): string {
  return createHash("sha256").update(code, "utf8").digest("hex");
}

export function sessionKey(event: SyncEvent): string {
  return `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
}

export function sessionStatusChange(
  event: SyncEvent,
  previousActivityStatus: ActivityStatus | undefined,
): SessionStatusChange {
  const change: SessionStatusChange = {
    device_id: event.device_id,
    agent_type: event.agent_type,
    agent_session_id: event.agent_session_id,
    project_display_name: event.payload.project.display_name,
    presence: event.payload.state.presence,
    activity_status: event.payload.state.activity_status,
    observed_at: event.observed_at,
  };
  if (event.payload.state.waiting_reason !== undefined) {
    change.waiting_reason = event.payload.state.waiting_reason;
  }
  if (previousActivityStatus !== undefined) {
    change.previous_activity_status = previousActivityStatus;
  }
  return change;
}

export function shouldReplaceSnapshot(
  current: StoredSession,
  incoming: SyncEvent,
): boolean {
  const currentTime = Date.parse(current.observed_at);
  const incomingTime = Date.parse(incoming.observed_at);
  if (incomingTime !== currentTime) return incomingTime > currentTime;
  return CONFIDENCE_RANK[incoming.confidence] !==
    CONFIDENCE_RANK[current.confidence]
    ? CONFIDENCE_RANK[incoming.confidence] > CONFIDENCE_RANK[current.confidence]
    : incoming.sequence > current.sequence;
}

export const SQLITE_GATEWAY_SCHEMA = `
  PRAGMA busy_timeout=5000;
  PRAGMA journal_mode=WAL;
  PRAGMA synchronous=NORMAL;
  CREATE TABLE IF NOT EXISTS registered_devices (
    device_id TEXT PRIMARY KEY,
    public_key TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    revoked_at TEXT,
    last_sequence INTEGER NOT NULL DEFAULT 0,
    last_heartbeat_at TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_devices_owner ON registered_devices(owner_id);
  CREATE TABLE IF NOT EXISTS binding_codes (
    code_digest TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    claimed_device_id TEXT,
    issued_by TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_binding_owner ON binding_codes(owner_id, expires_at);
  CREATE TABLE IF NOT EXISTS push_subscriptions (
    endpoint TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    payload TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_push_owner ON push_subscriptions(owner_id);
  CREATE TABLE IF NOT EXISTS sync_events (
    event_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    observed_at TEXT NOT NULL,
    received_at TEXT NOT NULL,
    event_data TEXT NOT NULL,
    UNIQUE(device_id, sequence)
  );
  CREATE INDEX IF NOT EXISTS idx_sync_events_device_seq ON sync_events(device_id, sequence DESC);
  CREATE TABLE IF NOT EXISTS sync_sessions (
    device_id TEXT NOT NULL,
    agent_type TEXT NOT NULL,
    agent_session_id TEXT NOT NULL,
    observed_at TEXT NOT NULL,
    received_at TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    confidence TEXT NOT NULL,
    presence TEXT NOT NULL,
    activity_status TEXT NOT NULL,
    event_data TEXT NOT NULL,
    PRIMARY KEY(device_id, agent_type, agent_session_id)
  );
  CREATE INDEX IF NOT EXISTS idx_sync_sessions_filter ON sync_sessions(device_id, presence, activity_status, observed_at DESC);
  CREATE TABLE IF NOT EXISTS auth_snapshots (
    device_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    checked_at TEXT NOT NULL,
    received_at TEXT NOT NULL,
    snapshot_data TEXT NOT NULL,
    PRIMARY KEY(device_id, provider)
  );
`;

export const SQLITE_ACCOUNT_SCHEMA = `
  PRAGMA busy_timeout=5000;
  PRAGMA journal_mode=WAL;
  PRAGMA synchronous=NORMAL;
  CREATE TABLE IF NOT EXISTS agent_pulse_users (
    user_id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    email_verified_at TEXT,
    avatar_data_url TEXT,
    created_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_users_email ON agent_pulse_users(email);
  CREATE TABLE IF NOT EXISTS agent_pulse_web_sessions (
    digest TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    workspace_owner_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_sessions_user ON agent_pulse_web_sessions(user_id);
  CREATE TABLE IF NOT EXISTS agent_pulse_login_attempts (
    scope_digest TEXT PRIMARY KEY,
    attempt_count INTEGER NOT NULL,
    window_started_at TEXT NOT NULL,
    blocked_until TEXT
  );
  CREATE TABLE IF NOT EXISTS agent_pulse_email_verifications (
    digest TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_email_verifications_user ON agent_pulse_email_verifications(user_id, created_at DESC);
  CREATE TABLE IF NOT EXISTS agent_pulse_password_resets (
    digest TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_password_resets_user ON agent_pulse_password_resets(user_id, created_at DESC);
`;
