/** Native Remote v3; separate from the incomplete legacy v2 envelopes. */
import { ContractError } from "./index.js";

export interface RemoteAttachment {
  name: string;
  media_type: string;
  data: string;
  encoding?: "base64" | "utf8";
}

export type RemotePermissionMode =
  "default" | "acceptEdits" | "plan" | "bypassPermissions";

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
        agent_type?: string;
        sandbox?: "read-only" | "workspace-write" | "danger-full-access";
        approval_policy?: "untrusted" | "on-request" | "never";
        permission_mode?: RemotePermissionMode;
      }
    | {
        action: "send_prompt";
        text: string;
        effort?: string;
        model?: string;
        permission_mode?: RemotePermissionMode;
        attachments?: RemoteAttachment[];
      }
    | { action: "resolve_approval"; approval_id: string; approved: boolean }
    | {
        action: "answer_question";
        question_id: string;
        answer: string | string[];
      }
    | { action: "interrupt" };
}

export interface RemotePendingApproval {
  approval_id: string;
  hub_session_id: string;
  host_generation: string;
  turn_id: string;
  can_approve: boolean;
  can_decline: boolean;
  tool_name?: string;
  command_preview?: string;
  path?: string;
  diff?: string;
  description?: string;
  risk_level?: "low" | "medium" | "high";
}

export interface RemoteQuestionOption {
  label: string;
  description?: string;
  preview?: string;
}

export interface RemotePendingQuestion {
  question_id: string;
  hub_session_id: string;
  host_generation: string;
  turn_id: string;
  prompt: string;
  header?: string;
  options?: RemoteQuestionOption[];
  multi_select?: boolean;
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

export interface RemoteReady {
  type: "ready";
  provider?: string;
  host_generation: string;
  /** Human-facing name for the computer; the device id stays a backend key. */
  device_name?: string;
  workspaces: string[];
  sessions: RemoteBinding[];
  models?: unknown;
  discovery?: unknown;
  pending_approvals?: RemotePendingApproval[];
  pending_questions?: RemotePendingQuestion[];
  attach_running?: boolean;
}

export function isRemoteWorkspaceId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^ws-[A-Za-z0-9_-]{1,24}-[0-9a-f]{16}$/u.test(value)
  );
}

/**
 * Validates the `ready` frame the Runner publishes.
 *
 * The frame is the only channel that carries discovered sessions, so the
 * allowlist is the cross-package contract: the Runner builds it
 * (`crates/runner/src/remote_network.rs`) and the Gateway enforces it here.
 */
export function parseRemoteReady(value: unknown): RemoteReady {
  const ready = object(value);
  only(ready, [
    "type",
    "provider",
    "host_generation",
    "device_name",
    "workspaces",
    "sessions",
    "models",
    "discovery",
    "pending_approvals",
    "pending_questions",
    "attach_running",
  ]);
  if (ready.type !== "ready") invalid();
  if (ready.provider !== undefined) text(ready.provider, 64);
  text(ready.host_generation, 128);
  if (ready.device_name !== undefined) {
    text(ready.device_name, 64);
    // A blank label would render as an empty device name in the console.
    if ((ready.device_name as string).trim() === "") invalid();
  }
  if (!Array.isArray(ready.workspaces)) invalid();
  for (const workspace of ready.workspaces)
    if (!isRemoteWorkspaceId(workspace)) invalid();
  if (new Set(ready.workspaces).size !== ready.workspaces.length) invalid();
  if (!Array.isArray(ready.sessions)) invalid();
  const sessions = ready.sessions.map((item) => parseRemoteBinding(item));
  for (const session of sessions)
    if (session.host_generation !== ready.host_generation) invalid();
  if (ready.pending_approvals !== undefined) {
    if (!Array.isArray(ready.pending_approvals)) invalid();
    const approvals = ready.pending_approvals.map((item) =>
      parseRemotePendingApproval(item),
    );
    if (
      new Set(approvals.map((approval) => approval.approval_id)).size !==
      approvals.length
    )
      invalid();
    for (const approval of approvals)
      if (approval.host_generation !== ready.host_generation) invalid();
  }
  if (ready.pending_questions !== undefined) {
    if (!Array.isArray(ready.pending_questions)) invalid();
    for (const question of ready.pending_questions)
      parseRemotePendingQuestion(question);
  }
  if (
    ready.attach_running !== undefined &&
    typeof ready.attach_running !== "boolean"
  )
    invalid();
  return ready as unknown as RemoteReady;
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
        "agent_type",
        "sandbox",
        "approval_policy",
        "permission_mode",
      ]);
      member(command.mode, ["create", "resume", "attach"]);
      text(command.workspace_path, 256);
      if (!isRemoteWorkspaceId(command.workspace_path)) invalid();
      if (command.provider_session_id !== undefined)
        text(command.provider_session_id, 256);
      if (command.model !== undefined) text(command.model, 256);
      if (command.agent_type !== undefined) text(command.agent_type, 64);
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
      if (command.permission_mode !== undefined)
        member(command.permission_mode, [
          "default",
          "acceptEdits",
          "plan",
          "bypassPermissions",
        ]);
      break;
    case "send_prompt":
      only(command, [
        "action",
        "text",
        "effort",
        "model",
        "permission_mode",
        "attachments",
      ]);
      // Per-turn reasoning effort, model override, and permission mode.
      // Desktop already sends effort; the Host accepts all three for every
      // provider and providers without the concept ignore them.
      if (command.effort !== undefined) text(command.effort, 64);
      if (command.model !== undefined) text(command.model, 256);
      if (command.permission_mode !== undefined)
        member(command.permission_mode, [
          "default",
          "acceptEdits",
          "plan",
          "bypassPermissions",
        ]);
      if (command.attachments !== undefined) {
        if (
          !Array.isArray(command.attachments) ||
          command.attachments.length === 0 ||
          command.attachments.length > 16
        )
          invalid();
        for (const item of command.attachments) {
          const att = object(item);
          only(att, ["name", "media_type", "data", "encoding"]);
          text(att.name, 256);
          text(att.media_type, 128);
          text(att.data, 8 * 1024 * 1024);
          if (att.encoding !== undefined)
            member(att.encoding, ["base64", "utf8"]);
        }
        if (
          typeof command.text !== "string" ||
          command.text.length > 512 * 1024 ||
          command.text.includes("\0")
        )
          invalid();
      } else {
        text(command.text, 512 * 1024);
      }
      break;
    case "resolve_approval":
      only(command, ["action", "approval_id", "approved"]);
      text(command.approval_id, 1024);
      if (typeof command.approved !== "boolean") invalid();
      break;
    case "answer_question":
      only(command, ["action", "question_id", "answer"]);
      text(command.question_id, 1024);
      if (typeof command.answer === "string") {
        text(command.answer, 512 * 1024);
      } else if (Array.isArray(command.answer)) {
        if (command.answer.length === 0) invalid();
        for (const item of command.answer) {
          text(item, 1024);
        }
      } else {
        invalid();
      }
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
    "tool_name",
    "command_preview",
    "path",
    "diff",
    "description",
    "risk_level",
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
  if (approval.tool_name !== undefined) text(approval.tool_name, 128);
  if (approval.command_preview !== undefined)
    text(approval.command_preview, 32768);
  if (approval.path !== undefined) text(approval.path, 1024);
  if (approval.diff !== undefined) text(approval.diff, 65536);
  if (approval.description !== undefined) text(approval.description, 4096);
  if (approval.risk_level !== undefined)
    member(approval.risk_level, ["low", "medium", "high"]);
  return approval as unknown as RemotePendingApproval;
}

export function parseRemotePendingQuestion(
  value: unknown,
): RemotePendingQuestion {
  const q = object(value);
  only(q, [
    "question_id",
    "hub_session_id",
    "host_generation",
    "turn_id",
    "prompt",
    "header",
    "options",
    "multi_select",
  ]);
  text(q.question_id, 128);
  text(q.hub_session_id, 128);
  text(q.host_generation, 128);
  text(q.turn_id, 256);
  text(q.prompt, 32768);
  if (q.header !== undefined) text(q.header, 128);
  if (q.multi_select !== undefined && typeof q.multi_select !== "boolean")
    invalid();
  if (q.options !== undefined) {
    if (!Array.isArray(q.options)) invalid();
    for (const opt of q.options) {
      const optObj = object(opt);
      only(optObj, ["label", "description", "preview"]);
      text(optObj.label, 256);
      if (optObj.description !== undefined) text(optObj.description, 2048);
      if (optObj.preview !== undefined) text(optObj.preview, 8192);
    }
  }
  return q as unknown as RemotePendingQuestion;
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
