/**
 * AgentHub Command Payloads & Validation (v2.0)
 *
 * Reference: docs/SYNC_PROTOCOL.md
 */

import { ContractError } from "./index.js";
import { type LaunchProfile, validateLaunchProfile } from "./v2-launch.js";

export type CommandAction =
  | "launch_session"
  | "send_prompt"
  | "resolve_approval"
  | "interrupt_turn"
  | "import_native_session";

export interface LaunchSessionCommand {
  action: "launch_session";
  launch_profile: LaunchProfile;
}

export interface SendPromptCommand {
  action: "send_prompt";
  text: string;
  client_turn_id?: string | undefined;
}

export interface ResolveApprovalCommand {
  action: "resolve_approval";
  approval_id: string;
  decision: "approved" | "rejected";
  custom_feedback?: string | undefined;
}

export interface InterruptTurnCommand {
  action: "interrupt_turn";
  reason?: string | undefined;
}

export interface ImportNativeSessionCommand {
  action: "import_native_session";
  provider: string;
  native_session_id: string;
  workspace_path: string;
}

export type CommandPayload =
  | LaunchSessionCommand
  | SendPromptCommand
  | ResolveApprovalCommand
  | InterruptTurnCommand
  | ImportNativeSessionCommand;

export function validateCommandPayload(raw: unknown): CommandPayload {
  if (typeof raw !== "object" || raw === null) {
    throw new ContractError(
      "invalid_format",
      "command payload must be an object",
    );
  }

  const obj = raw as Record<string, unknown>;
  const action = obj.action;

  switch (action) {
    case "launch_session":
      validateLaunchProfile(obj.launch_profile);
      return obj as unknown as LaunchSessionCommand;

    case "send_prompt":
      if (typeof obj.text !== "string" || obj.text.trim().length === 0) {
        throw new ContractError(
          "invalid_field",
          "send_prompt requires non-empty text",
        );
      }
      return obj as unknown as SendPromptCommand;

    case "resolve_approval":
      if (
        typeof obj.approval_id !== "string" ||
        obj.approval_id.trim().length === 0
      ) {
        throw new ContractError("invalid_field", "approval_id is required");
      }
      if (obj.decision !== "approved" && obj.decision !== "rejected") {
        throw new ContractError(
          "invalid_field",
          "decision must be approved or rejected",
        );
      }
      return obj as unknown as ResolveApprovalCommand;

    case "interrupt_turn":
      return obj as unknown as InterruptTurnCommand;

    case "import_native_session":
      if (
        typeof obj.provider !== "string" ||
        typeof obj.native_session_id !== "string" ||
        typeof obj.workspace_path !== "string"
      ) {
        throw new ContractError(
          "invalid_field",
          "import_native_session requires provider, native_session_id, and workspace_path",
        );
      }
      return obj as unknown as ImportNativeSessionCommand;

    default:
      throw new ContractError(
        "invalid_field",
        `unrecognized command action: ${String(action)}`,
      );
  }
}
