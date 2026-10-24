import assert from "node:assert/strict";
import { createPrivateKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  ContractError,
  deviceIdForPublicKey,
  parseAuthSyncBatch,
  parseDeviceHeartbeat,
  parseDeviceRegistration,
  parseRemoteBinding,
  parseRemoteCommand,
  parseRemotePendingApproval,
  parseRemoteReady,
  parseSyncBatch,
  unsignedAuthSyncBatchJson,
  unsignedDeviceHeartbeatJson,
  verifyAuthSyncBatch,
  verifySyncBatch,
  type AuthSyncBatch,
  type DeviceHeartbeat,
} from "../src/index.js";

const fixture = JSON.parse(
  await readFile(
    new URL("../fixtures/sync-batch-v1.json", import.meta.url),
    "utf8",
  ),
) as unknown;

const fixturePrivateKey = createPrivateKey({
  key: Buffer.concat([
    Buffer.from("302e020100300506032b657004220420", "hex"),
    Buffer.from(
      "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
      "hex",
    ),
  ]),
  format: "der",
  type: "pkcs8",
});

function signedRegistration(bindingCode: string): Record<string, unknown> {
  const batch = parseSyncBatch(fixture);
  const registration = {
    binding_code: bindingCode,
    device_id: batch.device_id,
    public_key: batch.public_key,
  };
  return {
    ...registration,
    signature: sign(
      null,
      Buffer.from(JSON.stringify(registration), "utf8"),
      fixturePrivateKey,
    ).toString("hex"),
  };
}

function signedHeartbeat(): DeviceHeartbeat {
  const batch = parseSyncBatch(fixture);
  const heartbeat: DeviceHeartbeat = {
    protocol_version: 1,
    device_id: batch.device_id,
    public_key: batch.public_key,
    sent_at: "2026-07-25T00:00:01.000Z",
    signature: "",
  };
  heartbeat.signature = sign(
    null,
    Buffer.from(unsignedDeviceHeartbeatJson(heartbeat), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  return heartbeat;
}

test("remote workspaces accept only daemon-issued identifiers", () => {
  const workspace = "ws-project-0123456789abcdef";
  const command = {
    client_cmd_id: "command-one",
    hub_session_id: "hub-one",
    host_generation: "generation-one",
    command: { action: "connect", mode: "create", workspace_path: workspace },
  };
  const binding = {
    hub_session_id: "hub-one",
    provider_session_id: "native-one",
    workspace_path: workspace,
    host_generation: "generation-one",
    control_connected: true,
    execution_state: "idle",
    current_turn_id: null,
  };
  assert.deepEqual(parseRemoteCommand(command), command);
  assert.deepEqual(parseRemoteBinding(binding), binding);
  for (const unsafe of [
    { ...command, host_generation: null },
    {
      ...command,
      command: { ...command.command, workspace_path: "D:/private" },
    },
    { ...binding, workspace_path: "/home/alice/private" },
    { ...binding, provider_response: { token: "secret" } },
  ]) {
    assert.throws(
      () =>
        "command" in unsafe
          ? parseRemoteCommand(unsafe)
          : parseRemoteBinding(unsafe),
      (error: unknown) =>
        error instanceof ContractError && error.code === "invalid_field",
    );
  }
});

test("remote pending approvals expose only opaque actionable descriptors", () => {
  const approval = {
    approval_id: `approval-${"a".repeat(64)}`,
    hub_session_id: "hub-one",
    host_generation: "generation-one",
    turn_id: "turn-one",
    can_approve: true,
    can_decline: false,
    diff: "@@ -1 +1 @@\n-old\n+new",
  };
  assert.deepEqual(parseRemotePendingApproval(approval), approval);
  for (const unsafe of [
    { ...approval, request_id: 7 },
    { ...approval, raw: { command: "secret" } },
    { ...approval, command: "secret" },
    { ...approval, secret_token: "secret" },
    { ...approval, approval_id: "approval-provider-request" },
    { ...approval, can_approve: "true" },
    { ...approval, diff: "x".repeat(65537) },
  ]) {
    assert.throws(
      () => parseRemotePendingApproval(unsafe),
      (error: unknown) =>
        error instanceof ContractError && error.code === "invalid_field",
    );
  }
});

test("version 1 cross-language vector validates", () => {
  const batch = parseSyncBatch(fixture);
  verifySyncBatch(batch);
  assert.equal(deviceIdForPublicKey(batch.public_key), batch.device_id);
});

test("device heartbeats validate signatures and reject tampering", () => {
  const batch = parseSyncBatch(fixture);
  const heartbeat: DeviceHeartbeat = {
    protocol_version: 1,
    device_id: batch.device_id,
    public_key: batch.public_key,
    sent_at: "2026-07-25T00:00:01.000Z",
    signature: "",
  };
  heartbeat.signature = sign(
    null,
    Buffer.from(unsignedDeviceHeartbeatJson(heartbeat), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  assert.deepEqual(parseDeviceHeartbeat(heartbeat), heartbeat);

  const tampered = { ...heartbeat, sent_at: "2026-07-25T00:00:02.000Z" };
  assert.throws(
    () => parseDeviceHeartbeat(tampered),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_signature",
  );
  assert.throws(
    () => parseDeviceHeartbeat({ ...heartbeat, extra: true }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "unknown_field",
  );
});

test("redacted auth sync batch validates and rejects token fields", () => {
  const batch: AuthSyncBatch = {
    protocol_version: 1,
    device_id: parseSyncBatch(fixture).device_id,
    public_key: parseSyncBatch(fixture).public_key,
    checked_at: "2026-07-25T00:00:00Z",
    snapshots: [
      {
        provider: "codex",
        state: "authenticated",
        source: "app_server",
        checked_at: "2026-07-25T00:00:00Z",
        account_hint: "a***e@example.com",
        plan_type: "plus",
        auth_mode: "chatgpt",
        access_token_present: true,
        refresh_token_present: true,
        quota: {
          state: "available",
          source: "app_server",
          refreshed_at: "2026-07-25T00:00:00Z",
          windows: [
            {
              name: "primary",
              used_percent: 25,
              window_duration_minutes: 300,
              resets_at: "2026-07-25T05:00:00Z",
            },
          ],
          available_reset_credits: 2,
        },
      },
    ],
    signature: "",
  };
  batch.signature = sign(
    null,
    Buffer.from(unsignedAuthSyncBatchJson(batch), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  assert.deepEqual(parseAuthSyncBatch(batch), batch);
  verifyAuthSyncBatch(batch);

  const withToken = structuredClone(batch) as unknown as Record<
    string,
    unknown
  >;
  const snapshots = withToken.snapshots as Array<Record<string, unknown>>;
  snapshots[0]!.access_token = "must-never-sync";
  assert.throws(
    () => parseAuthSyncBatch(withToken),
    (error: unknown) =>
      error instanceof ContractError && error.code === "unknown_field",
  );
});

test("auth sync batches reject duplicate providers", () => {
  const batch = {
    protocol_version: 1,
    device_id: parseSyncBatch(fixture).device_id,
    public_key: parseSyncBatch(fixture).public_key,
    checked_at: "2026-07-25T00:00:00Z",
    snapshots: [
      {
        provider: "codex",
        state: "unknown",
        source: "unknown",
        checked_at: "2026-07-25T00:00:00Z",
        access_token_present: false,
        refresh_token_present: false,
      },
      {
        provider: "codex",
        state: "unknown",
        source: "unknown",
        checked_at: "2026-07-25T00:00:00Z",
        access_token_present: false,
        refresh_token_present: false,
      },
    ],
    signature: "0".repeat(128),
  };
  assert.throws(
    () => parseAuthSyncBatch(batch),
    (error: unknown) =>
      error instanceof ContractError &&
      error.code === "duplicate_auth_provider",
  );
});

test("unknown nested fields are rejected", () => {
  const value = structuredClone(fixture) as {
    events: Array<{ payload: Record<string, unknown> }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.payload.prompt = "must never be accepted";

  assert.throws(
    () => parseSyncBatch(value),
    (error: unknown) =>
      error instanceof ContractError && error.code === "unknown_field",
  );
});

test("tampering is detected after structural validation", () => {
  const value = structuredClone(fixture) as {
    events: Array<{ payload: { state: { raw_status: string } } }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.payload.state.raw_status = "tampered";
  const batch = parseSyncBatch(value);

  assert.throws(
    () => verifySyncBatch(batch),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_signature",
  );
});

test("uppercase UUIDs are rejected to keep replay idempotent", () => {
  const value = structuredClone(fixture) as {
    events: Array<{ event_id: string }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.event_id = firstEvent.event_id.toUpperCase();

  assert.throws(
    () => parseSyncBatch(value),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_field",
  );
});

test("lone surrogate strings are rejected", () => {
  const value = structuredClone(fixture) as {
    events: Array<{ payload: { state: { raw_status: string } } }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.payload.state.raw_status = "\ud800";

  assert.throws(
    () => parseSyncBatch(value),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_field",
  );
});

test("empty raw_status_detail objects are rejected", () => {
  const value = structuredClone(fixture) as {
    events: Array<{
      payload: { state: { raw_status_detail?: Record<string, string> } };
    }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.payload.state.raw_status_detail = {};

  assert.throws(
    () => parseSyncBatch(value),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_field",
  );
});

test("raw_status_detail keys outside the snake_case charset are rejected", () => {
  const value = structuredClone(fixture) as {
    events: Array<{
      payload: { state: { raw_status_detail?: Record<string, string> } };
    }>;
  };
  const firstEvent = value.events[0];
  assert.ok(firstEvent);
  firstEvent.payload.state.raw_status_detail = { Method: "shell" };

  assert.throws(
    () => parseSyncBatch(value),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_field",
  );
});

test("device registration binds the ID to the public key", () => {
  const batch = parseSyncBatch(fixture);
  assert.throws(
    () =>
      parseDeviceRegistration({
        binding_code: "code",
        device_id: `dev_${"0".repeat(32)}`,
        public_key: batch.public_key,
        signature: "0".repeat(128),
      }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "device_id_mismatch",
  );
});

test("device registration requires proof of private key possession", () => {
  const registration = signedRegistration("code");
  assert.equal(
    parseDeviceRegistration(registration).device_id,
    parseSyncBatch(fixture).device_id,
  );

  assert.throws(
    () => parseDeviceRegistration({ ...registration, binding_code: "other" }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_signature",
  );
});

test("device registration and heartbeat accept optional device_name", () => {
  const registration = signedRegistration("code");
  const parsed = parseDeviceRegistration({
    ...registration,
    device_name: "xiaotang",
  });
  assert.equal(parsed.device_name, "xiaotang");

  const heartbeat = signedHeartbeat();
  const parsedHeartbeat = parseDeviceHeartbeat({
    ...heartbeat,
    device_name: "xiaotang",
  });
  assert.equal(parsedHeartbeat.device_name, "xiaotang");
});

test("remote send_prompt accepts a bounded per-turn reasoning effort", () => {
  const command = {
    client_cmd_id: "command-one",
    hub_session_id: "hub-one",
    host_generation: "generation-one",
    command: { action: "send_prompt", text: "hello", effort: "high" },
  };
  assert.deepEqual(parseRemoteCommand(command), command);
  // Omitting effort keeps the thread's own setting.
  assert.deepEqual(
    parseRemoteCommand({
      ...command,
      command: { action: "send_prompt", text: "hello" },
    }),
    { ...command, command: { action: "send_prompt", text: "hello" } },
  );
  for (const unsafe of [
    { ...command, command: { ...command.command, effort: 5 } },
    { ...command, command: { ...command.command, effort: "" } },
    { ...command, command: { ...command.command, effort: "x".repeat(65) } },
  ]) {
    assert.throws(
      () => parseRemoteCommand(unsafe),
      (error: unknown) =>
        error instanceof ContractError && error.code === "invalid_field",
    );
  }
});

test("the ready frame accepts a named device and rejects anything outside the contract", () => {
  const workspace = "ws-project-0123456789abcdef";
  const ready = {
    type: "ready",
    provider: "codex_app_server",
    host_generation: "generation-one",
    device_name: "书房主机",
    workspaces: [workspace],
    sessions: [
      {
        hub_session_id: "hub-one",
        provider_session_id: "native-one",
        workspace_path: workspace,
        host_generation: "generation-one",
        control_connected: true,
        execution_state: "idle",
        current_turn_id: null,
      },
    ],
    pending_approvals: [],
    pending_questions: [],
    attach_running: false,
  };
  assert.deepEqual(parseRemoteReady(ready), ready);
  for (const unsafe of [
    // An unexpected key would let a Runner smuggle data past the contract.
    { ...ready, private_path: "D:/private" },
    // A blank label would render as an empty device name in the console.
    { ...ready, device_name: "   " },
    { ...ready, device_name: 42 },
    // A physical path is not a workspace identifier.
    { ...ready, workspaces: ["D:/private"] },
    { ...ready, workspaces: [workspace, workspace] },
    // A session from another generation cannot be projected.
    {
      ...ready,
      sessions: [
        {
          ...ready.sessions[0],
          host_generation: "generation-two",
        },
      ],
    },
  ]) {
    assert.throws(
      () => parseRemoteReady(unsafe),
      (error: unknown) =>
        error instanceof ContractError && error.code === "invalid_field",
    );
  }
});

test("send_prompt accepts per-turn model override and validated attachments", () => {
  const cmd = {
    client_cmd_id: "cmd-attach-1",
    hub_session_id: "hub-one",
    host_generation: "generation-one",
    command: {
      action: "send_prompt" as const,
      text: "请结合附件分析",
      effort: "high",
      model: "claude-opus-4-6",
      attachments: [
        {
          name: "schedule.png",
          media_type: "image/png",
          data: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
          encoding: "base64" as const,
        },
      ],
    },
  };
  assert.deepEqual(parseRemoteCommand(cmd), cmd);
});
