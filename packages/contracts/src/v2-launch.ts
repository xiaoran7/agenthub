/**
 * AgentHub Launch Profile & Session-Scoped Permissions (v2.0)
 *
 * Reference: docs/CAPABILITY_AND_LAUNCH_SPEC.md
 */

import { ContractError } from "./index.js";

export const permissionModes = [
  "plan_only",
  "ask_all_edits",
  "accept_edits",
  "bypass_permissions",
] as const;

export type PermissionMode = (typeof permissionModes)[number];

export const toolRuleActions = ["allow", "ask", "deny"] as const;
export type ToolRuleAction = (typeof toolRuleActions)[number];

export interface ToolRule {
  tool_name: string;
  action: ToolRuleAction;
  whitelist_commands?: string[] | undefined;
}

export interface SessionPermissions {
  approval_mode: PermissionMode;
  allow_tool_execution?: boolean | undefined;
  tool_rules?: ToolRule[] | undefined;
}

export interface ReasoningConfig {
  enabled?: boolean | undefined;
  effort_level?: "low" | "medium" | "high" | undefined;
  budget_tokens?: number | undefined;
}

export interface ModelConfig {
  model_id: string;
  reasoning?: ReasoningConfig | undefined;
}

export interface EnvironmentConfig {
  custom_env_vars?: Record<string, string> | undefined;
  append_path?: string[] | undefined;
}

export interface LaunchProfile {
  workspace_path: string;
  workspace_display_name?: string | undefined;
  target_runner_id: string;
  provider: string;
  model: ModelConfig;
  permissions: SessionPermissions;
  environment?: EnvironmentConfig | undefined;
  session_metadata?:
    | {
        title?: string | undefined;
        tags?: string[] | undefined;
        created_by_client?: string | undefined;
      }
    | undefined;
  initial_prompt?: string | undefined;
}

export function validateLaunchProfile(raw: unknown): LaunchProfile {
  if (typeof raw !== "object" || raw === null) {
    throw new ContractError(
      "invalid_format",
      "launch_profile must be a non-null object",
    );
  }

  const obj = raw as Record<string, unknown>;

  if (
    typeof obj.workspace_path !== "string" ||
    obj.workspace_path.trim().length === 0
  ) {
    throw new ContractError(
      "invalid_field",
      "workspace_path must be a non-empty string",
    );
  }

  if (
    typeof obj.target_runner_id !== "string" ||
    obj.target_runner_id.trim().length === 0
  ) {
    throw new ContractError(
      "invalid_field",
      "target_runner_id must be a non-empty string",
    );
  }

  if (typeof obj.provider !== "string" || obj.provider.trim().length === 0) {
    throw new ContractError(
      "invalid_field",
      "provider must be a non-empty string",
    );
  }

  if (typeof obj.model !== "object" || obj.model === null) {
    throw new ContractError("invalid_field", "model must be an object");
  }

  const modelObj = obj.model as Record<string, unknown>;
  if (
    typeof modelObj.model_id !== "string" ||
    modelObj.model_id.trim().length === 0
  ) {
    throw new ContractError(
      "invalid_field",
      "model.model_id must be a non-empty string",
    );
  }

  if (typeof obj.permissions !== "object" || obj.permissions === null) {
    throw new ContractError("invalid_field", "permissions must be an object");
  }

  const permObj = obj.permissions as Record<string, unknown>;
  if (
    typeof permObj.approval_mode !== "string" ||
    !permissionModes.includes(permObj.approval_mode as PermissionMode)
  ) {
    throw new ContractError(
      "invalid_field",
      `approval_mode must be one of ${permissionModes.join(", ")}`,
    );
  }

  return raw as LaunchProfile;
}
