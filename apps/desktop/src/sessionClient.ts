import { Channel, invoke } from "@tauri-apps/api/core";

import type { Session, SessionTurnEvent, SessionTurnResult } from "./types";

export const SESSION_COMMANDS = {
  launch: "launch_new_session",
  send: "send_session_message",
  cancel: "cancel_session_turn",
  resolveApproval: "resolve_session_approval",
} as const;

interface SessionMessageInput {
  agent_type: Session["agent_type"];
  agent_session_id: string;
  prompt: string;
  model?: string;
  permission_mode?: string;
  reasoning_effort?: string;
  attachments: Array<{ name: string; path: string }>;
}

interface SessionRef {
  agent_type: Session["agent_type"];
  agent_session_id: string;
}

export function sendSessionMessage(
  input: SessionMessageInput,
  onEvent: Channel<SessionTurnEvent>,
): Promise<SessionTurnResult> {
  return invoke<SessionTurnResult>(SESSION_COMMANDS.send, { input, onEvent });
}

export function cancelSessionTurn(input: SessionRef): Promise<unknown> {
  return invoke(SESSION_COMMANDS.cancel, { input });
}

export function resolveSessionApproval(
  input: SessionRef & { approval_id: string; approved: boolean },
): Promise<unknown> {
  return invoke(SESSION_COMMANDS.resolveApproval, { input });
}
