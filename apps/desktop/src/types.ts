export type ActivityStatus =
  | "WORKING"
  | "WAITING_INPUT"
  | "WAITING_PERMISSION"
  | "IDLE"
  | "COMPLETED"
  | "FAILED"
  | "STOPPED"
  | "UNKNOWN";

export interface Account {
  user: {
    user_id: string;
    email: string;
    display_name: string;
  };
  subscription: { plan: string; status: string };
  entitlements?: {
    max_devices?: number;
    history_days?: number;
  };
  offline?: boolean;
}

export interface Device {
  device_id: string;
  online: boolean;
  revoked: boolean;
  last_seen_at: string;
}

export type AgentType =
  "claude_code" | "codex" | "antigravity" | "grok_build" | "zcode";

export function normalizeAgentType(type: unknown): AgentType {
  if (type === "z_code" || type === "zcode") return "zcode";
  if (type === "claude_code" || type === "claudecode") return "claude_code";
  if (type === "codex") return "codex";
  if (type === "antigravity") return "antigravity";
  if (type === "grok_build") return "grok_build";
  return (type as AgentType) || "codex";
}

export interface SessionRuntimeContext {
  agent_type: AgentType;
  agent_session_id: string;
  title?: string;
  model?: string;
  reasoning_effort?: string;
  permission_mode?: string;
  approval_policy?: string;
  sandbox_mode?: string;
  context_window_tokens?: number;
  context_used_tokens?: number;
  session_total_tokens?: number;
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  reasoning_tokens?: number;
}

export interface PendingApproval {
  approval_id: string;
  hub_session_id: string;
  host_generation: string;
  turn_id: string;
  can_approve: boolean;
  can_decline: boolean;
}

export interface Session {
  event_id: string;
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  observed_at: string;
  payload: {
    project: { display_name: string; fingerprint: string };
    state: { activity_status: ActivityStatus; presence: string };
    last_activity_at?: string;
    runtime?: SessionRuntimeContext | null;
    pending_approval?: PendingApproval | null;
  };
  sync_state?: "local" | "synced";
  archived?: boolean;
}

export interface SessionTimelineEvent {
  event_id: string;
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  sequence: number;
  observed_at: string;
  received_at: string;
  payload: {
    project: { display_name: string; fingerprint: string };
    state: { activity_status: ActivityStatus; presence: string };
    last_activity_at?: string;
  };
}

export interface DesktopPreferences {
  refresh_interval_seconds: number;
  claude_binary: string;
  codex_binary: string;
  grok_binary: string;
  default_cwd?: string;
  selected_models?: Record<string, string>;
  app_language?: string;
  theme?: string;
}

export interface SessionTurnResult {
  response: string;
  completed_at: string;
}

export type SessionTurnEvent =
  | { type: "started" }
  | { type: "assistant_delta"; text: string }
  | { type: "assistant_message"; text: string }
  | { type: "activity"; label: string }
  | { type: "approval_requested"; approval: PendingApproval };

export interface DashboardRealtimeEvent {
  connected: boolean;
  changed: boolean;
}

export interface LocalSource {
  agent_type: "claude_code" | "codex" | "antigravity" | "grok_build" | "zcode";
  session_count: number;
  detected: boolean;
  status: "healthy" | "backoff" | "scanning";
  last_success_at?: string;
  error_code?: string;
}

export interface LocalInventory {
  pending_events: number;
  session_count: number;
  sources: LocalSource[];
  refreshed_at: string;
}

export interface Snapshot {
  account: Account;
  devices: Device[];
  sessions: Session[];
  device: { device_id: string; name: string };
  sync_running: boolean;
  realtime_connected: boolean;
  server_url: string;
  local: LocalInventory;
  preferences: DesktopPreferences;
}

export interface TgDeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string;
  expires_in: number;
  interval: number;
}

export interface TgDevicePollResponse {
  status: "pending" | "success";
  slow_down?: boolean;
  snapshot?: Snapshot;
}
