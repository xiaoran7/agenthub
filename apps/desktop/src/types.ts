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
}

export interface Device {
  device_id: string;
  online: boolean;
  revoked: boolean;
  last_seen_at: string;
}

export interface Session {
  event_id: string;
  device_id: string;
  agent_type: "claude_code" | "codex";
  agent_session_id: string;
  observed_at: string;
  payload: {
    project: { display_name: string; fingerprint: string };
    state: { activity_status: ActivityStatus; presence: string };
    last_activity_at?: string;
  };
  sync_state?: "local" | "synced";
}

export interface LocalSource {
  agent_type: "claude_code" | "codex";
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
  memory: {
    claude_files: number;
    claude_bytes: number;
    codex_indexes: number;
    codex_bytes: number;
  };
  refreshed_at: string;
}

export interface Snapshot {
  account: Account;
  devices: Device[];
  sessions: Session[];
  device: { device_id: string; name: string };
  sync_running: boolean;
  server_url: string;
  local: LocalInventory;
}
