import type { SyncEvent } from "@agent-pulse/contracts";
import {
  auditMetadata,
  type AccountUser,
  type AuditEvent,
  type AuditMetadataValue,
  type SubscriptionEvent,
  type SubscriptionRecord,
} from "./accounts.js";
import { GatewayStoreError, type RegisteredDevice } from "./store.js";

export interface DeviceRow {
  device_id: string;
  public_key: string;
  owner_id: string;
  created_at: unknown;
  last_seen_at: unknown;
  last_sequence?: string | number;
  revoked_at: unknown | null;
  last_heartbeat_at?: unknown | null;
}

export interface AuditEventRow {
  event_id: string;
  scope: AuditEvent["scope"];
  actor_user_id: string | null;
  workspace_owner_id: string | null;
  action: AuditEvent["action"];
  target_type: Exclude<AuditEvent["target_type"], undefined> | null;
  target_id: string | null;
  metadata: unknown;
  created_at: unknown;
}

export function registeredDevice(row: DeviceRow): RegisteredDevice {
  return {
    device_id: row.device_id,
    public_key: row.public_key,
    owner_id: row.owner_id,
    created_at: isoTimestamp(row.created_at),
    last_seen_at: isoTimestamp(row.last_seen_at),
    ...(row.revoked_at === null
      ? {}
      : { revoked_at: isoTimestamp(row.revoked_at) }),
  };
}

export function isoTimestamp(value: unknown): string {
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

export function syncEvent(value: unknown): SyncEvent {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("database returned invalid event data");
  }
  return value as SyncEvent;
}

export function bestSessionEvents(events: SyncEvent[]): SyncEvent[] {
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

export function shouldReplaceEvent(
  current: SyncEvent,
  incoming: SyncEvent,
): boolean {
  const currentTime = Date.parse(current.observed_at);
  const incomingTime = Date.parse(incoming.observed_at);
  if (incomingTime !== currentTime) return incomingTime > currentTime;
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

export function postgresConstraint(error: unknown): string | undefined {
  if (typeof error !== "object" || error === null) return undefined;
  if ("constraint_name" in error && typeof error.constraint_name === "string") {
    return error.constraint_name;
  }
  if ("constraint" in error && typeof error.constraint === "string") {
    return error.constraint;
  }
  return undefined;
}

export function classifyPostgresConflict(error: unknown): unknown {
  if (
    typeof error !== "object" ||
    error === null ||
    !("code" in error) ||
    error.code !== "23505"
  ) {
    return error;
  }
  const constraint = postgresConstraint(error);
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
  if (constraint === "agent_platform_binding_codes_pkey") {
    return new GatewayStoreError(
      "binding_code_conflict",
      "binding code digest already exists",
    );
  }
  return error;
}

export function normalizeAuditEvent(row: AuditEventRow): AuditEvent {
  if (
    typeof row.metadata !== "object" ||
    row.metadata === null ||
    Array.isArray(row.metadata)
  ) {
    throw new Error("stored audit metadata is invalid");
  }
  return {
    event_id: row.event_id,
    scope: row.scope,
    ...(row.actor_user_id === null ? {} : { actor_user_id: row.actor_user_id }),
    ...(row.workspace_owner_id === null
      ? {}
      : { workspace_owner_id: row.workspace_owner_id }),
    action: row.action,
    ...(row.target_type === null ? {} : { target_type: row.target_type }),
    ...(row.target_id === null ? {} : { target_id: row.target_id }),
    metadata: auditMetadata(row.metadata as Record<string, AuditMetadataValue>),
    created_at: isoTimestamp(row.created_at),
  };
}

export function normalizeAccountUser(row: AccountUser): AccountUser {
  return {
    user_id: row.user_id,
    email: row.email,
    display_name: row.display_name,
    password_hash: row.password_hash,
    status: row.status,
    ...(row.email_verified_at === undefined || row.email_verified_at === null
      ? {}
      : { email_verified_at: isoTimestamp(row.email_verified_at) }),
    ...(row.oauth_provider === undefined || row.oauth_provider === null
      ? {}
      : { oauth_provider: row.oauth_provider }),
    ...(row.oauth_sub === undefined || row.oauth_sub === null
      ? {}
      : { oauth_sub: row.oauth_sub }),
    ...(row.avatar_data_url === undefined || row.avatar_data_url === null
      ? {}
      : { avatar_data_url: row.avatar_data_url }),
    created_at: isoTimestamp(row.created_at),
  };
}

export function normalizeSubscription(
  row: SubscriptionRecord,
): SubscriptionRecord {
  return {
    plan: row.plan,
    status: row.status,
    ...(row.current_period_end === undefined || row.current_period_end === null
      ? {}
      : { current_period_end: isoTimestamp(row.current_period_end) }),
    ...(row.provider === undefined || row.provider === null
      ? {}
      : { provider: row.provider }),
    ...(row.provider_customer_id === undefined ||
    row.provider_customer_id === null
      ? {}
      : { provider_customer_id: row.provider_customer_id }),
    ...(row.provider_subscription_id === undefined ||
    row.provider_subscription_id === null
      ? {}
      : { provider_subscription_id: row.provider_subscription_id }),
    ...(row.last_event_at === undefined || row.last_event_at === null
      ? {}
      : { last_event_at: isoTimestamp(row.last_event_at) }),
    ...(row.last_event_id === undefined || row.last_event_id === null
      ? {}
      : { last_event_id: row.last_event_id }),
  };
}

export function normalizeSubscriptionEvent(
  row: SubscriptionEvent & { current_period_end: unknown | null },
): SubscriptionEvent {
  return {
    event_id: row.event_id,
    user_id: row.user_id,
    provider: row.provider,
    provider_customer_id: row.provider_customer_id,
    provider_subscription_id: row.provider_subscription_id,
    plan: row.plan,
    status: row.status,
    ...(row.current_period_end === null
      ? {}
      : { current_period_end: isoTimestamp(row.current_period_end) }),
    occurred_at: isoTimestamp(row.occurred_at),
  };
}
