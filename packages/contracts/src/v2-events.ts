/**
 * AgentHub Event Payloads & Validation (v2.0)
 *
 * Reference: docs/SYNC_PROTOCOL.md
 */

import { ContractError } from "./index.js";
import { type LaunchProfile } from "./v2-launch.js";

export type EventType =
  | "session_launched"
  | "stream_delta"
  | "session_state_changed"
  | "tool_approval_requested"
  | "tool_execution_started"
  | "tool_execution_finished"
  | "turn_started"
  | "turn_completed"
  | "reconcile_check"
  | "reconcile_sync";

export interface SessionLaunchedEvent {
  event_type: "session_launched";
  hub_session_id: string;
  provider_session_id: string;
  launch_profile: LaunchProfile;
}

export interface StreamDeltaEvent {
  event_type: "stream_delta";
  turn_id: string;
  block_index: number;
  delta_type: "text" | "thinking" | "tool_input" | "tool_output";
  delta_content: string;
}

export interface SessionStateChangedEvent {
  event_type: "session_state_changed";
  previous_state: string;
  current_state: string;
  reason?:
    | {
        waiting_for?: string | undefined;
        approval_id?: string | undefined;
        tool_name?: string | undefined;
        tool_params?: Record<string, unknown> | undefined;
        error_message?: string | undefined;
      }
    | undefined;
}

export interface ToolApprovalRequestedEvent {
  event_type: "tool_approval_requested";
  approval_id: string;
  tool_name: string;
  tool_params: Record<string, unknown>;
  diff_preview?: string | undefined;
  danger_level?: "low" | "medium" | "high" | undefined;
}

export interface ToolExecutionStartedEvent {
  event_type: "tool_execution_started";
  execution_id: string;
  tool_name: string;
  tool_params?: Record<string, unknown> | undefined;
}

export interface ToolExecutionFinishedEvent {
  event_type: "tool_execution_finished";
  execution_id: string;
  tool_name: string;
  exit_code?: number | undefined;
  duration_ms?: number | undefined;
  output_summary?: string | undefined;
  error?: string | undefined;
}

export interface TurnStartedEvent {
  event_type: "turn_started";
  turn_id: string;
  user_prompt: string;
}

export interface TurnCompletedEvent {
  event_type: "turn_completed";
  turn_id: string;
  token_usage?:
    | {
        input_tokens?: number | undefined;
        output_tokens?: number | undefined;
        thinking_tokens?: number | undefined;
      }
    | undefined;
  duration_ms?: number | undefined;
}

export interface ReconcileCheckEvent {
  event_type: "reconcile_check";
  runner_id: string;
  sessions_digest: Array<{
    hub_session_id: string;
    execution_state: string;
    current_turn: number;
    state_fingerprint: string;
  }>;
}

export interface ReconcileSyncEvent {
  event_type: "reconcile_sync";
  hub_session_id: string;
  full_snapshot: unknown;
}

export type EventPayload =
  | SessionLaunchedEvent
  | StreamDeltaEvent
  | SessionStateChangedEvent
  | ToolApprovalRequestedEvent
  | ToolExecutionStartedEvent
  | ToolExecutionFinishedEvent
  | TurnStartedEvent
  | TurnCompletedEvent
  | ReconcileCheckEvent
  | ReconcileSyncEvent;

export function validateEventPayload(raw: unknown): EventPayload {
  if (typeof raw !== "object" || raw === null) {
    throw new ContractError(
      "invalid_format",
      "event payload must be an object",
    );
  }

  const obj = raw as Record<string, unknown>;
  const event_type = obj.event_type;

  if (typeof event_type !== "string") {
    throw new ContractError("invalid_field", "event_type must be a string");
  }

  return raw as EventPayload;
}
