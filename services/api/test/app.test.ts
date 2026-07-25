import assert from "node:assert/strict";
import { createPrivateKey, createPublicKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  deviceIdForPublicKey,
  parseSyncBatch,
  type SyncBatch,
  unsignedBatchJson,
} from "@agent-platform/contracts";

import { createGatewayApp } from "../src/app.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "../src/store.js";

const fixture = parseSyncBatch(
  JSON.parse(
    await readFile(
      new URL(
        "../../../packages/contracts/fixtures/sync-batch-v1.json",
        import.meta.url,
      ),
      "utf8",
    ),
  ),
);

test("registration, signed ingestion, replay, and read queries form a loop", async () => {
  let now = new Date("2026-07-25T00:00:01Z");
  const store = new InMemoryGatewayStore(() => now);
  const app = createGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });

  const registration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: {
      binding_code: "one-time-code",
      device_id: fixture.device_id,
      public_key: fixture.public_key,
    },
  });
  assert.equal(registration.statusCode, 201);

  const retryRegistration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: {
      binding_code: "one-time-code",
      device_id: fixture.device_id,
      public_key: fixture.public_key,
    },
  });
  assert.equal(retryRegistration.statusCode, 200);

  const ingestion = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });
  assert.equal(ingestion.statusCode, 200);
  assert.deepEqual(ingestion.json(), {
    protocol_version: 1,
    batch_id: fixture.batch_id,
    device_id: fixture.device_id,
    accepted_through: 1,
  });

  const replay = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });
  assert.equal(replay.statusCode, 200);

  const sessions = await app.inject({
    method: "GET",
    url: "/v1/sessions?activity_status=WORKING&presence=LIVE",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(sessions.statusCode, 200);
  assert.equal(sessions.json().sessions.length, 1);
  assert.equal(
    sessions.json().sessions[0].agent_session_id,
    "contract-session",
  );

  const otherUser = await app.inject({
    method: "GET",
    url: "/v1/sessions",
    headers: { "x-user-id": "user-2" },
  });
  assert.equal(otherUser.json().sessions.length, 0);

  now = new Date("2026-07-25T00:00:47Z");
  const devices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(devices.json().devices[0].online, false);
  await app.close();
});

test("tampered and extra content is rejected before storage", async () => {
  const app = createGatewayApp({
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: {
      binding_code: "one-time-code",
      device_id: fixture.device_id,
      public_key: fixture.public_key,
    },
  });

  const withPrompt = structuredClone(fixture) as SyncBatch & {
    events: Array<SyncBatch["events"][number] & { prompt?: string }>;
  };
  const firstEvent = withPrompt.events[0];
  assert.ok(firstEvent);
  firstEvent.prompt = "private prompt";
  const unknownField = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: withPrompt,
  });
  assert.equal(unknownField.statusCode, 400);
  assert.equal(unknownField.json().error, "unknown_field");

  const tampered = structuredClone(fixture);
  const tamperedEvent = tampered.events[0];
  assert.ok(tamperedEvent);
  tamperedEvent.payload.state.raw_status = "tampered";
  const invalidSignature = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: tampered,
  });
  assert.equal(invalidSignature.statusCode, 401);
  assert.equal(invalidSignature.json().error, "invalid_signature");
  await app.close();
});

test("a signed sequence gap is rejected atomically", async () => {
  const store = new InMemoryGatewayStore();
  const app = createGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: {
      binding_code: "one-time-code",
      device_id: fixture.device_id,
      public_key: fixture.public_key,
    },
  });
  await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });

  const gap = makeSignedBatch(3);
  const response = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: gap,
  });
  assert.equal(response.statusCode, 409);
  assert.equal(response.json().error, "sequence_gap");
  assert.equal(store.listSessions("user-1", {}).length, 1);
  await app.close();
});

function makeSignedBatch(sequence: number): SyncBatch {
  const seed = Buffer.from(
    "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    "hex",
  );
  const privateKey = createPrivateKey({
    key: Buffer.concat([
      Buffer.from("302e020100300506032b657004220420", "hex"),
      seed,
    ]),
    format: "der",
    type: "pkcs8",
  });
  const publicKey = createPublicKey(
    privateKey.export({ format: "pem", type: "pkcs8" }),
  )
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const deviceId = deviceIdForPublicKey(publicKey);
  const originalEvent = fixture.events[0];
  assert.ok(originalEvent);
  const batch: SyncBatch = {
    protocol_version: 1,
    batch_id: "223e4567-e89b-42d3-a456-426614174000",
    device_id: deviceId,
    public_key: publicKey,
    created_at: "2026-07-25T00:00:02Z",
    first_sequence: sequence,
    last_sequence: sequence,
    events: [
      {
        ...structuredClone(originalEvent),
        sequence,
        event_id: "223e4567-e89b-42d3-a456-426614174001",
        device_id: deviceId,
        observed_at: "2026-07-25T00:00:02Z",
      },
    ],
    signature: "",
  };
  batch.signature = sign(
    null,
    Buffer.from(unsignedBatchJson(batch), "utf8"),
    privateKey,
  ).toString("hex");
  return batch;
}
