// Type-only imports: pulling runtime values from @agent-pulse/contracts
// would drag its node:crypto dependency into the browser bundle.
import type {
  AgentType,
  AuthSnapshot,
  SyncEvent,
} from "@agent-pulse/contracts";

export type {
  ActivityStatus,
  AgentType,
  Confidence,
  ObservationSource,
  Presence,
  SessionState,
  SyncEvent,
  WaitingReason,
} from "@agent-pulse/contracts";

export type {
  AuthSnapshot,
  AuthState,
  CredentialSource,
  QuotaSnapshot,
  QuotaState,
  QuotaWindow,
} from "@agent-pulse/contracts";

/** Session snapshot or timeline event as returned by the gateway read API. */
export interface SessionRecord extends SyncEvent {
  received_at: string;
}

/** Redacted authentication/quota view returned by `GET /v1/device-auth`. */
export interface AuthRecord extends AuthSnapshot {
  device_id: string;
  received_at: string;
}

/** Device as returned by `GET /v1/devices`. */
export interface DeviceRecord {
  device_id: string;
  created_at: string;
  last_seen_at: string;
  online: boolean;
  revoked: boolean;
  /** Optional presentation metadata returned by newer gateway versions. */
  display_name?: string;
  os?: string;
  runner_version?: string;
}

/** Agent engines valid for remote/notification SessionRef handoff. */
export type SessionAgentType = AgentType;

/** Stable identity of one session across snapshot and timeline queries. */
export interface SessionRef {
  device_id: string;
  agent_type: SessionAgentType;
  agent_session_id: string;
}

const AGENT_TYPE_PATTERN = /^[a-z][a-z0-9_-]{1,63}$/u;

/**
 * Validates a session reference that arrived from outside React state — today
 * a service worker message carrying a notification's payload.
 */
export function parseSessionRef(value: unknown): SessionRef | undefined {
  if (typeof value !== "object" || value === null) {
    return undefined;
  }
  const candidate = value as Record<string, unknown>;
  const { device_id: deviceId, agent_session_id: sessionId } = candidate;
  const agentType = candidate.agent_type;
  if (
    typeof deviceId !== "string" ||
    deviceId.length === 0 ||
    typeof sessionId !== "string" ||
    sessionId.length === 0 ||
    typeof agentType !== "string" ||
    !AGENT_TYPE_PATTERN.test(agentType) ||
    agentType === "toString"
  ) {
    return undefined;
  }
  return {
    device_id: deviceId,
    agent_type: agentType as SessionAgentType,
    agent_session_id: sessionId,
  };
}
