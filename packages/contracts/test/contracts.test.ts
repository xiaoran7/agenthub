import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  ContractError,
  deviceIdForPublicKey,
  parseDeviceRegistration,
  parseSyncBatch,
  verifySyncBatch,
} from "../src/index.js";

const fixture = JSON.parse(
  await readFile(
    new URL("../fixtures/sync-batch-v1.json", import.meta.url),
    "utf8",
  ),
) as unknown;

test("version 1 cross-language vector validates", () => {
  const batch = parseSyncBatch(fixture);
  verifySyncBatch(batch);
  assert.equal(deviceIdForPublicKey(batch.public_key), batch.device_id);
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

test("device registration binds the ID to the public key", () => {
  const batch = parseSyncBatch(fixture);
  assert.throws(
    () =>
      parseDeviceRegistration({
        binding_code: "code",
        device_id: `dev_${"0".repeat(32)}`,
        public_key: batch.public_key,
      }),
    (error: unknown) =>
      error instanceof ContractError && error.code === "device_id_mismatch",
  );
});
