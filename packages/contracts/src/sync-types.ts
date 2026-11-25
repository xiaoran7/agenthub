export const SYNC_PROTOCOL_VERSION = 1;
export const MAX_SYNC_BATCH_EVENTS = 1_000;

export const coreAgentTypes = [
  "antigravity",
  "codex",
  "claude_desktop",
  "claude_code",
  "deepseek_harness",
] as const;

export const agentTypes = coreAgentTypes;
export type CoreAgentType = (typeof coreAgentTypes)[number];
export const presences = ["LIVE", "PERSISTED", "UNKNOWN"] as const;
export const activityStatuses = [
  "WORKING",
  "WAITING_INPUT",
  "WAITING_PERMISSION",
  "IDLE",
  "COMPLETED",
  "FAILED",
  "STOPPED",
  "UNKNOWN",
] as const;
export const observationSources = [
  "claude_agents_json",
  "claude_hook",
  "codex_app_server",
  "codex_hook",
  "antigravity_brain",
  "antigravity_hook",
  "antigravity_cli",
  "plugin_acp",
  "grok_sessions",
  "grok_hook",
  "grok_active_sessions",
  "process_probe",
  "local_history",
  "zcode_db",
  "zcode_hook",
  "zcode_rollout",
] as const;
export const confidences = [
  "unknown",
  "inferred",
  "observed",
  "authoritative",
] as const;
export const waitingReasons = [
  "user_input",
  "permission",
  "sandbox",
  "worker",
  "dialog",
  "mcp_elicitation",
  "unknown",
] as const;

export type AgentType = CoreAgentType | (string & {});
export type Presence = (typeof presences)[number];
export type ActivityStatus = (typeof activityStatuses)[number];
export type ObservationSource = (typeof observationSources)[number];
export type Confidence = (typeof confidences)[number];
export type WaitingReason = (typeof waitingReasons)[number];

export interface DeviceHeartbeat {
  protocol_version: number;
  device_id: string;
  public_key: string;
  sent_at: string;
  signature: string;
  device_name?: string;
}

export const authStates = [
  "authenticated",
  "unauthenticated",
  "reauth_required",
  "unknown",
] as const;
export const credentialSources = [
  "official_cli",
  "app_server",
  "environment",
  "file",
  "keyring",
  "cache",
  "unknown",
] as const;
export const quotaStates = [
  "available",
  "exhausted",
  "unavailable",
  "unknown",
] as const;

export type AuthState = (typeof authStates)[number];
export type CredentialSource = (typeof credentialSources)[number];
export type QuotaState = (typeof quotaStates)[number];

export interface QuotaWindow {
  name: string;
  used_percent?: number;
  window_duration_minutes?: number;
  resets_at?: string;
}

export interface QuotaSnapshot {
  state: QuotaState;
  source: CredentialSource;
  refreshed_at: string;
  windows: QuotaWindow[];
  available_reset_credits?: number;
}

/** Safe-to-sync authentication view. It intentionally has no token fields. */
export interface AuthSnapshot {
  provider: AgentType;
  state: AuthState;
  source: CredentialSource;
  checked_at: string;
  account_hint?: string;
  plan_type?: string;
  auth_mode?: string;
  access_token_present: boolean;
  refresh_token_present: boolean;
  access_token_expires_at?: string;
  quota?: QuotaSnapshot;
  error_code?: string;
}

export interface AuthSyncBatch {
  protocol_version: number;
  device_id: string;
  public_key: string;
  checked_at: string;
  snapshots: AuthSnapshot[];
  signature: string;
}

export interface ProjectRef {
  display_name: string;
  fingerprint: string;
}

export interface SessionState {
  presence: Presence;
  activity_status: ActivityStatus;
  raw_status: string;
  raw_status_detail?: Record<string, string>;
  waiting_reason?: WaitingReason;
}

export interface SyncSessionPayload {
  project: ProjectRef;
  state: SessionState;
  started_at?: string;
  last_activity_at?: string;
}

export interface SyncEvent {
  sequence: number;
  event_id: string;
  schema_version: number;
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  event_type: "session_observed";
  observed_at: string;
  source: ObservationSource;
  confidence: Confidence;
  adapter_version: string;
  payload: SyncSessionPayload;
}

export interface SyncBatch {
  protocol_version: number;
  batch_id: string;
  device_id: string;
  public_key: string;
  created_at: string;
  first_sequence: number;
  last_sequence: number;
  events: SyncEvent[];
  signature: string;
}

export interface SyncAck {
  protocol_version: number;
  batch_id: string;
  device_id: string;
  accepted_through: number;
}

export interface DeviceRegistration {
  binding_code: string;
  device_id: string;
  public_key: string;
  signature: string;
  device_name?: string;
}
