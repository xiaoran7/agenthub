// Type-only imports: pulling runtime values from @agent-platform/contracts
// would drag its node:crypto dependency into the browser bundle.
import type { AgentType, SyncEvent } from "@agent-platform/contracts";

export type {
  ActivityStatus,
  AgentType,
  Confidence,
  ObservationSource,
  Presence,
  SessionState,
  SyncEvent,
  WaitingReason,
} from "@agent-platform/contracts";

/** Session snapshot or timeline event as returned by the gateway read API. */
export interface SessionRecord extends SyncEvent {
  received_at: string;
}

/** Device as returned by `GET /v1/devices`. */
export interface DeviceRecord {
  device_id: string;
  created_at: string;
  last_seen_at: string;
  online: boolean;
}

/** Stable identity of one session across snapshot and timeline queries. */
export interface SessionRef {
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
}

export function sessionRef(session: SessionRecord): SessionRef {
  return {
    device_id: session.device_id,
    agent_type: session.agent_type,
    agent_session_id: session.agent_session_id,
  };
}

export function sessionKey(ref: SessionRef): string {
  return `${ref.device_id}:${ref.agent_type}:${ref.agent_session_id}`;
}
