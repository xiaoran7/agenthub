/** Native Remote v3; separate from the incomplete legacy v2 envelopes. */
import { ContractError } from "./index.js";

export interface RemoteCommand {
  client_cmd_id: string;
  hub_session_id: string;
  host_generation: string | null;
  command:
    | {
        action: "connect";
        mode: "create" | "resume" | "attach";
        workspace_path: string;
        provider_session_id?: string;
        model?: string;
        sandbox?: "read-only" | "workspace-write" | "danger-full-access";
        approval_policy?: "untrusted" | "on-request" | "never";
      }
    | { action: "send_prompt"; text: string }
    | { action: "resolve_approval"; approval_id: string; approved: boolean }
    | { action: "interrupt" };
}

export interface RemotePendingApproval {
  approval_id: string;
  hub_session_id: string;
  host_generation: string;
  turn_id: string;
  can_approve: boolean;
  can_decline: boolean;
}

export interface RemoteBinding {
  hub_session_id: string;
  provider_session_id: string;
  workspace_path: string;
  host_generation: string;
  control_connected: boolean;
  execution_state: string;
  current_turn_id: string | null;
}

export function isRemoteWorkspaceId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^ws-[A-Za-z0-9_-]{1,24}-[0-9a-f]{16}$/u.test(value)
  );
}

export function parseRemoteCommand(value: unknown): RemoteCommand {
  const obj = object(value);
  only(obj, ["client_cmd_id", "hub_session_id", "host_generation", "command"]);
  text(obj.client_cmd_id, 128);
  text(obj.hub_session_id, 128);
  if (obj.host_generation !== null) text(obj.host_generation, 128);
  const command = object(obj.command);
  switch (command.action) {
    case "connect":
      only(command, [
        "action",
        "mode",
        "workspace_path",
        "provider_session_id",
        "model",
        "sandbox",
        "approval_policy",
      ]);
      member(command.mode, ["create", "resume", "attach"]);
      text(command.workspace_path, 256);
      if (!isRemoteWorkspaceId(command.workspace_path)) invalid();
      if (command.provider_session_id !== undefined)
        text(command.provider_session_id, 256);
      if (command.model !== undefined) text(command.model, 256);
      if (
        command.mode === "create" &&
        command.provider_session_id !== undefined
      )
        invalid();
      if (command.mode !== "create") text(command.provider_session_id, 256);
      if (command.sandbox !== undefined)
        member(command.sandbox, [
          "read-only",
          "workspace-write",
          "danger-full-access",
        ]);
      if (command.approval_policy !== undefined)
        member(command.approval_policy, ["untrusted", "on-request", "never"]);
      break;
    case "send_prompt":
      only(command, ["action", "text"]);
      text(command.text, 512 * 1024);
      break;
    case "resolve_approval":
      only(command, ["action", "approval_id", "approved"]);
      text(command.approval_id, 1024);
      if (typeof command.approved !== "boolean") invalid();
      break;
    case "interrupt":
      only(command, ["action"]);
      break;
    default:
      invalid();
  }
  if (obj.host_generation === null) invalid();
  return obj as unknown as RemoteCommand;
}

export function parseRemotePendingApproval(
  value: unknown,
): RemotePendingApproval {
  const approval = object(value);
  only(approval, [
    "approval_id",
    "hub_session_id",
    "host_generation",
    "turn_id",
    "can_approve",
    "can_decline",
  ]);
  text(approval.approval_id, 128);
  if (!/^approval-[0-9a-f]{64}$/u.test(approval.approval_id as string))
    invalid();
  text(approval.hub_session_id, 128);
  text(approval.host_generation, 128);
  text(approval.turn_id, 256);
  if (
    typeof approval.can_approve !== "boolean" ||
    typeof approval.can_decline !== "boolean"
  )
    invalid();
  return approval as unknown as RemotePendingApproval;
}

export function parseRemoteBinding(value: unknown): RemoteBinding {
  const binding = object(value);
  only(binding, [
    "hub_session_id",
    "provider_session_id",
    "workspace_path",
    "host_generation",
    "control_connected",
    "execution_state",
    "current_turn_id",
  ]);
  for (const key of [
    "hub_session_id",
    "provider_session_id",
    "host_generation",
  ])
    text(binding[key], 256);
  text(binding.workspace_path, 256);
  if (!isRemoteWorkspaceId(binding.workspace_path)) invalid();
  member(binding.execution_state, [
    "unknown",
    "idle",
    "running",
    "awaiting_permission",
    "awaiting_input",
    "error",
    "completed",
    "terminated",
  ]);
  if (typeof binding.control_connected !== "boolean") invalid();
  if (binding.current_turn_id !== null) text(binding.current_turn_id, 256);
  return binding as unknown as RemoteBinding;
}

function object(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    invalid();
  return value as Record<string, unknown>;
}
function only(obj: Record<string, unknown>, keys: string[]): void {
  if (Object.keys(obj).some((key) => !keys.includes(key))) invalid();
}
function text(value: unknown, max: number): void {
  if (
    typeof value !== "string" ||
    !value.trim() ||
    value.length > max ||
    value.includes("\0")
  )
    invalid();
}
function member(value: unknown, values: string[]): void {
  if (!values.includes(value as string)) invalid();
}
function invalid(): never {
  throw new ContractError("invalid_field", "invalid remote command or binding");
}
