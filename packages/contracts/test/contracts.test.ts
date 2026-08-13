import assert from "node:assert/strict";
import { createPrivateKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  ContractError,
  deviceIdForPublicKey,
  parseAuthSyncBatch,
  parseCreateRemoteTask,
  parseDeviceRegistration,
  parseDeviceTaskClaim,
  parseDeviceTaskUpdate,
  parseSyncBatch,
  unsignedAuthSyncBatchJson,
  unsignedDeviceTaskClaimJson,
  unsignedDeviceTaskUpdateJson,
  verifyAuthSyncBatch,
  verifySyncBatch,
  type AuthSyncBatch,
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

test("version 1 cross-language vector validates", () => {
  const batch = parseSyncBatch(fixture);
  verifySyncBatch(batch);
  assert.equal(deviceIdForPublicKey(batch.public_key), batch.device_id);
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

test("remote task contracts reject paths and verify device signatures", () => {
  const task = parseCreateRemoteTask({
    target_device_id: parseSyncBatch(fixture).device_id,
    agent_type: "codex",
    prompt: "Run the focused test suite",
    project_fingerprint: "a".repeat(64),
    idempotency_key: "request-0001",
    execution_policy: "local_confirmation",
    expires_in_seconds: 3600,
  });
  assert.equal(task.execution_policy, "local_confirmation");
  assert.throws(
    () => parseCreateRemoteTask({ ...task, project_path: "C:\\private" }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "unknown_field",
  );

  const claim = {
    protocol_version: 1,
    device_id: parseSyncBatch(fixture).device_id,
    public_key: parseSyncBatch(fixture).public_key,
    requested_at: "2026-08-13T12:00:00.000Z",
    signature: "",
  };
  claim.signature = sign(
    null,
    Buffer.from(unsignedDeviceTaskClaimJson(claim), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  assert.deepEqual(parseDeviceTaskClaim(claim), claim);

  const update = {
    protocol_version: 1,
    task_id: "11111111-1111-4111-8111-111111111111",
    device_id: claim.device_id,
    public_key: claim.public_key,
    expected_version: 2,
    status: "accepted" as const,
    updated_at: "2026-08-13T12:00:00.000Z",
    signature: "",
  };
  update.signature = sign(
    null,
    Buffer.from(unsignedDeviceTaskUpdateJson(update), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  assert.deepEqual(parseDeviceTaskUpdate(update), update);
  assert.throws(
    () => parseDeviceTaskUpdate({ ...update, status: "queued" }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "invalid_field",
  );
});
