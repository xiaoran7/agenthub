import assert from "node:assert/strict";
import { createPrivateKey, sign } from "node:crypto";
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
