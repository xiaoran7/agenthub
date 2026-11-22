import { ContractError } from "./contract-error.js";

export const AGENT_TYPE_PATTERN = /^[a-z][a-z0-9_-]{1,63}$/u;

export function isValidAgentType(value: unknown): value is string {
  return typeof value === "string" && AGENT_TYPE_PATTERN.test(value);
}

export function expectAgentType(value: unknown, field: string): string {
  if (!isValidAgentType(value)) {
    throw new ContractError(
      "invalid_field",
      `${field} must be a lowercase agent identifier`,
    );
  }
  return value;
}

export interface PluginCapabilities {
  discover: boolean;
  launch: boolean;
  resume: boolean;
  models: boolean;
  history: boolean;
  approvals: boolean;
}

export type PluginRuntimeStatus =
  | "not_installed"
  | "idle"
  | "ready"
  | "running"
  | "downloading"
  | "error"
  | "disabled";

export interface AgentPlugin {
  id: string;
  name: string;
  displayName?: string;
  category: string;
  description: string;
  version: string;
  installedVersion?: string;
  installed?: boolean;
  enabled: boolean;
  runtimeStatus: PluginRuntimeStatus;
  error?: string | undefined;
  sessionPrefix?: string;
  sessionPrefixes?: string[];
  aliases?: string[];
  icon?: string;
  capabilities?: PluginCapabilities;
  available?: boolean;
  binaryPath?: string;
  sourceUrl?: string;
  updatedAt: string;
  runner?: string;
}

export type PluginCommandAction =
  | "list"
  | "catalog"
  | "refresh_catalog"
  | "install"
  | "uninstall"
  | "toggle"
  | "enable"
  | "disable"
  | "set_registry";

export interface PluginActionRequest {
  action: PluginCommandAction;
  plugin_id?: string;
  enabled?: boolean;
  source_url?: string;
  registry_url?: string;
}

export interface PluginCommandPayload extends PluginActionRequest {
  type: "plugin_command";
  request_id?: string;
}

export interface PluginStatePayload {
  type: "plugin_state";
  request_id?: string;
  runner?: string;
  registry_url?: string;
  plugins: AgentPlugin[];
  ok?: boolean;
  error?: string;
}

export interface RemotePluginsResponse {
  plugins: AgentPlugin[];
  runners: Array<{
    runner: string;
    registry_url?: string;
    updated_at: string;
    plugins: AgentPlugin[];
  }>;
}

export interface PluginToggleRequest {
  enabled?: boolean;
  runner?: string;
}

export interface PluginStatusReport {
  plugin_id: string;
  runtime_status: PluginRuntimeStatus;
  error?: string | undefined;
}
