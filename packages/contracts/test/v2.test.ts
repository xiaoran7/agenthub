import assert from "node:assert/strict";
import test from "node:test";

import {
  ContractError,
  createFrameEnvelope,
  validateFrameEnvelope,
  validateLaunchProfile,
  validateCapabilityReport,
  validateCommandPayload,
  validateEventPayload,
  parseSlashCommand,
  type LaunchProfile,
} from "../src/index.js";

test("v2 FrameEnvelope creation and validation", () => {
  const frame = createFrameEnvelope("handshake", 1, 0, {
    runner_id: "run_test_01",
    client_version: "2.0.0",
    min_supported_protocol: 1,
    max_supported_protocol: 2,
    last_seen_server_seq: 0,
    capabilities: {
      providers: ["claude_code", "codex"],
    },
  });

  assert.equal(frame.ver, 2);
  assert.equal(frame.type, "handshake");
  assert.equal(frame.seq, 1);
  assert.equal(frame.ack_seq, 0);

  const validated = validateFrameEnvelope(frame);
  assert.equal(validated.type, "handshake");

  assert.throws(
    () => validateFrameEnvelope({ ...frame, ver: 1 }),
    (err: unknown) =>
      err instanceof ContractError && err.code === "invalid_version",
  );

  assert.throws(
    () => validateFrameEnvelope({ ...frame, seq: 0 }),
    (err: unknown) =>
      err instanceof ContractError && err.code === "invalid_field",
  );
});

test("v2 LaunchProfile validation", () => {
  const validProfile: LaunchProfile = {
    workspace_path: "d:/ClaudeSpace/Project/solum",
    workspace_display_name: "Solum",
    target_runner_id: "run_test_01",
    provider: "claude_code",
    model: {
      model_id: "claude-3-7-sonnet-20250219",
      reasoning: { effort_level: "high", budget_tokens: 16000 },
    },
    permissions: {
      approval_mode: "ask_all_edits",
      allow_tool_execution: true,
    },
  };

  const validated = validateLaunchProfile(validProfile);
  assert.equal(validated.provider, "claude_code");
  assert.equal(validated.model.model_id, "claude-3-7-sonnet-20250219");

  assert.throws(
    () => validateLaunchProfile({ ...validProfile, workspace_path: "" }),
    (err: unknown) => err instanceof ContractError,
  );

  assert.throws(
    () =>
      validateLaunchProfile({
        ...validProfile,
        permissions: {
          approval_mode: "invalid_mode",
        } as unknown as LaunchProfile["permissions"],
      }),
    (err: unknown) => err instanceof ContractError,
  );
});

test("v2 CapabilityReport validation", () => {
  const report = {
    runner_id: "run_01",
    hostname: "workstation",
    os: "windows",
    architecture: "x86_64",
    providers: [
      {
        provider_id: "claude_code",
        display_name: "Claude Code",
        cli_version: "0.2.29",
        available: true,
        supported_models: [
          {
            id: "claude-3-7-sonnet-20250219",
            display_name: "Claude 3.7 Sonnet",
            supports_thinking: true,
          },
        ],
        supported_permission_modes: ["default", "accept_edits"],
        supported_slash_commands: ["/compact", "/cost"],
        supports_stream_cancellation: true,
      },
    ],
    reported_at: Date.now(),
  };

  const validated = validateCapabilityReport(report);
  assert.equal(validated.providers.length, 1);
  assert.equal(validated.providers[0]?.provider_id, "claude_code");
});

test("v2 Command and Event Payload validation", () => {
  const cmd = validateCommandPayload({
    action: "send_prompt",
    text: "Refactor WebSocket code",
  });
  assert.equal(cmd.action, "send_prompt");

  const event = validateEventPayload({
    event_type: "stream_delta",
    turn_id: "turn_1",
    block_index: 0,
    delta_type: "text",
    delta_content: "Optimizing...",
  });
  assert.equal(event.event_type, "stream_delta");
});

test("v2 3-tier Slash Command parsing and routing", () => {
  // 1. Hub Commands
  const cancelCmd = parseSlashCommand("/cancel");
  assert.equal(cancelCmd.kind, "hub");
  if (cancelCmd.kind === "hub") {
    assert.equal(cancelCmd.command, "/cancel");
  }

  const permCmd = parseSlashCommand("/permission accept_edits");
  assert.equal(permCmd.kind, "hub");
  if (permCmd.kind === "hub") {
    assert.equal(permCmd.command, "/permission");
    assert.deepEqual(permCmd.args, ["accept_edits"]);
  }

  // 2. Native Commands
  const compactCmd = parseSlashCommand("/compact");
  assert.equal(compactCmd.kind, "native");
  if (compactCmd.kind === "native") {
    assert.equal(compactCmd.command, "/compact");
    assert.ok(compactCmd.supported_providers.includes("claude_code"));
  }

  const undoCmd = parseSlashCommand("/undo");
  assert.equal(undoCmd.kind, "native");
  if (undoCmd.kind === "native") {
    assert.equal(undoCmd.command, "/undo");
    assert.ok(undoCmd.supported_providers.includes("codex"));
  }

  // 3. Fallback
  const prompt = parseSlashCommand("Hello Agent");
  assert.equal(prompt.kind, "prompt_fallback");
  assert.equal(prompt.raw, "Hello Agent");

  const customCmd = parseSlashCommand("/unknown_custom_cmd foo bar");
  assert.equal(customCmd.kind, "prompt_fallback");
  if (customCmd.kind === "prompt_fallback") {
    assert.equal(customCmd.command_candidate, "/unknown_custom_cmd");
  }
});
