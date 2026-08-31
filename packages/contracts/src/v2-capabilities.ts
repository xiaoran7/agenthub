/**
 * AgentHub Dynamic Capability Discovery Contracts (v2.0)
 *
 * Reference: docs/CAPABILITY_AND_LAUNCH_SPEC.md
 */

import { ContractError } from "./index.js";

export interface SupportedModel {
  id: string;
  display_name: string;
  supports_thinking?: boolean;
  max_thinking_budget?: number;
  default_thinking_budget?: number;
  supports_reasoning_effort?: boolean;
  supported_efforts?: string[];
  default_effort?: string;
}

export interface ProviderCapability {
  provider_id: string;
  display_name: string;
  cli_version: string;
  executable_path?: string;
  available: boolean;
  supported_models: SupportedModel[];
  supported_permission_modes: string[];
  supported_slash_commands: string[];
  supports_stream_cancellation: boolean;
}

export interface CapabilityReport {
  runner_id: string;
  hostname: string;
  os: string;
  architecture: string;
  providers: ProviderCapability[];
  reported_at: number;
}

export function validateCapabilityReport(raw: unknown): CapabilityReport {
  if (typeof raw !== "object" || raw === null) {
    throw new ContractError(
      "invalid_format",
      "capability_report must be a non-null object",
    );
  }

  const obj = raw as Record<string, unknown>;

  if (typeof obj.runner_id !== "string" || obj.runner_id.trim().length === 0) {
    throw new ContractError(
      "invalid_field",
      "runner_id must be a non-empty string",
    );
  }

  if (
    typeof obj.hostname !== "string" ||
    typeof obj.os !== "string" ||
    typeof obj.architecture !== "string"
  ) {
    throw new ContractError(
      "invalid_field",
      "hostname, os, and architecture must be strings",
    );
  }

  if (!Array.isArray(obj.providers)) {
    throw new ContractError("invalid_field", "providers must be an array");
  }

  return raw as CapabilityReport;
}
