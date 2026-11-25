import type {
  RemoteAttachment,
  RemoteBinding,
  RemoteCommand,
  RemotePendingApproval,
  RemotePendingQuestion,
  RemotePermissionMode,
} from "@agent-pulse/contracts/remote";

export type { RemoteAttachment, RemotePermissionMode };

export type RemoteRequest = <T>(
  path: string,
  body?: unknown,
  timeoutMs?: number,
  userId?: string,
  signal?: AbortSignal,
) => Promise<T>;

export interface DiscoverySubagent {
  id: string;
  agent_type?: string | null | undefined;
  description?: string | null | undefined;
  model?: string | null | undefined;
  total_tokens?: number | null | undefined;
  last_activity_at_ms?: number | null | undefined;
}

export interface DiscoveryCandidate {
  provider_session_id: string;
  workspace_path: string;
  title: string | null;
  provider?: string;
  resumable?: boolean;
  model?: string | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
  cache_read_tokens?: number | null;
  context_used_tokens?: number | null;
  context_window_tokens?: number | null;
  calls?: number | null;
  request_count?: number | null;
  last_activity_at_ms?: number | null;
  execution_state?: string | null;
  subagents?: DiscoverySubagent[] | undefined;
}

export interface RemoteRunnerModels {
  data: Array<{
    id: string;
    model?: string;
    displayName?: string;
    display_name?: string;
    name?: string;
  }>;
  agents?: Record<string, Array<{ id: string; display_name?: string }>>;
}

export type RemoteRunnerHello = null | {
  host_generation: string;
  device_name?: string;
  pending_approvals?: RemotePendingApproval[];
  workspaces: string[];
  workspace_labels?: Record<string, string>;
  models?: RemoteRunnerModels;
  discovery?: {
    candidates: DiscoveryCandidate[];
  };
};

export interface RemoteRunnerHelloResponse {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: RemoteRunnerHello;
  codex_session_control?: boolean;
}

export interface RemoteRunner {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: RemoteRunnerHello;
  codex_session_control?: boolean;
}

export interface RemoteSession {
  runner_id: string;
  binding: RemoteBinding;
  online: boolean;
  provider?: string;
}

export interface RemoteSnapshot {
  cursor: number;
  runners: RemoteRunner[];
  sessions: RemoteSession[];
}

export interface RemoteEvent {
  cursor: number;
  runner_id: string;
  payload: {
    type: string;
    pending_approvals?: RemotePendingApproval[];
    pending_questions?: RemotePendingQuestion[];
    event?: {
      kind: string;
      raw?: { params?: Record<string, unknown> };
      approval_id?: string;
      approval?: RemotePendingApproval;
      question_id?: string;
      question?: RemotePendingQuestion;
      binding?: RemoteBinding;
    };
    receipt?: {
      client_cmd_id: string;
      status: string;
      code?: string;
      result?: {
        turn?: { id: string };
        approval_id?: string;
        question_id?: string;
        response_sent?: boolean;
      };
    };
  };
}

export interface RemoteCommandRecord {
  command: RemoteCommand;
  receipt: {
    status: string;
    code?: string | null;
    result?: { turn?: { id: string } };
  } | null;
}

export interface RemoteSessionDetail {
  binding: RemoteBinding | null;
  cursor: number;
  resume?: { transcript: number | null; approvals: number | null } | null;
  commands: RemoteCommandRecord[];
}

export interface ConversationItem {
  id: string;
  kind: "user" | "assistant" | "tool";
  text: string;
  title?: string | undefined;
  command?: string | undefined;
  output?: string | undefined;
  description?: string | undefined;
  status?: "running" | "completed" | "failed" | undefined;
  turnId?: string | undefined;
}

export interface TurnFailure {
  turnId: string | null;
  message: string;
  retrying: boolean;
}

export type ConversationGroup =
  | { type: "single"; item: ConversationItem }
  | { type: "tool_group"; items: ConversationItem[] };
