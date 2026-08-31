/**
 * AgentHub 3-Tier Slash Command Parser & Router (v2.0)
 *
 * Reference: docs/CAPABILITY_AND_LAUNCH_SPEC.md §5
 */

export const HUB_SLASH_COMMANDS = [
  "/cancel",
  "/stop",
  "/permission",
  "/clear",
  "/reconnect",
  "/switch",
] as const;

export const NATIVE_SLASH_COMMANDS = [
  "/compact",
  "/cost",
  "/review",
  "/doctor",
  "/undo",
  "/reset",
  "/init",
  "/approval",
] as const;

export type HubSlashCommand = (typeof HUB_SLASH_COMMANDS)[number];
export type NativeSlashCommand = (typeof NATIVE_SLASH_COMMANDS)[number];

export type SlashCommandClassification =
  | {
      kind: "hub";
      command: HubSlashCommand;
      args: string[];
      raw: string;
    }
  | {
      kind: "native";
      command: NativeSlashCommand;
      args: string[];
      raw: string;
      supported_providers: string[];
    }
  | {
      kind: "prompt_fallback";
      raw: string;
      command_candidate?: string;
    };

const NATIVE_PROVIDER_MAP: Record<NativeSlashCommand, string[]> = {
  "/compact": ["claude_code"],
  "/cost": ["claude_code"],
  "/doctor": ["claude_code"],
  "/init": ["claude_code"],
  "/undo": ["codex", "codex_app_server"],
  "/reset": ["codex", "codex_app_server"],
  "/approval": ["codex", "codex_app_server"],
  "/review": ["claude_code", "codex", "codex_app_server"],
};

export function parseSlashCommand(input: string): SlashCommandClassification {
  const trimmed = input.trim();
  if (!trimmed.startsWith("/")) {
    return { kind: "prompt_fallback", raw: input };
  }

  const parts = trimmed.split(/\s+/);
  const command = (parts[0] ?? "").toLowerCase();
  const args = parts.slice(1);

  if (HUB_SLASH_COMMANDS.includes(command as HubSlashCommand)) {
    return {
      kind: "hub",
      command: command as HubSlashCommand,
      args,
      raw: input,
    };
  }

  if (NATIVE_SLASH_COMMANDS.includes(command as NativeSlashCommand)) {
    const nativeCmd = command as NativeSlashCommand;
    return {
      kind: "native",
      command: nativeCmd,
      args,
      raw: input,
      supported_providers: NATIVE_PROVIDER_MAP[nativeCmd] ?? [],
    };
  }

  return {
    kind: "prompt_fallback",
    raw: input,
    command_candidate: command,
  };
}
