import assert from "node:assert/strict";
import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  type KeyObject,
} from "node:crypto";
import test from "node:test";

import {
  deviceIdForPublicKey,
  type SyncBatch,
  type SyncEvent,
  unsignedBatchJson,
} from "@agent-platform/contracts";

import { createGatewayApp } from "../src/app.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "../src/store.js";

const DEVICE_COUNT = 12;
const SESSIONS_PER_DEVICE = 250;

interface TestDevice {
  index: number;
  ownerId: string;
  bindingCode: string;
  privateKey: KeyObject;
  publicKey: string;
  deviceId: string;
}

test("M4 multi-device load keeps 3,000 sessions isolated and replay-safe", async () => {
  const devices = Array.from({ length: DEVICE_COUNT }, (_, index) =>
    makeDevice(index, index < 8 ? "load-owner-a" : "load-owner-b"),
  );
  const store = new InMemoryGatewayStore(
    () => new Date("2026-07-28T00:00:00Z"),
  );
  const app = createGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map(
        devices.map((device) => [device.bindingCode, device.ownerId] as const),
      ),
    ),
  });
  const batches = new Map<number, SyncBatch>();

  for (const device of devices) {
    const registration = await app.inject({
      method: "POST",
      url: "/v1/devices/register",
      payload: registrationPayload(device),
    });
    assert.equal(registration.statusCode, 201);

    const batch = makeBatch(device, SESSIONS_PER_DEVICE);
    batches.set(device.index, batch);
    const ingestion = await app.inject({
      method: "POST",
      url: "/v1/sync/batches",
      payload: batch,
    });
    assert.equal(ingestion.statusCode, 200);
  }

  const ownerADevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "load-owner-a" },
  });
  const ownerBDevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "load-owner-b" },
  });
  assert.equal(ownerADevices.json().devices.length, 8);
  assert.equal(ownerBDevices.json().devices.length, 4);

  const ownerASessions = await app.inject({
    method: "GET",
    url: "/v1/sessions",
    headers: { "x-user-id": "load-owner-a" },
  });
  const ownerBSessions = await app.inject({
    method: "GET",
    url: "/v1/sessions",
    headers: { "x-user-id": "load-owner-b" },
  });
  assert.equal(ownerASessions.json().sessions.length, 2_000);
  assert.equal(ownerBSessions.json().sessions.length, 1_000);

  const target = devices[0];
  assert.ok(target);
  const filtered = await app.inject({
    method: "GET",
    url: `/v1/sessions?device_id=${target.deviceId}`,
    headers: { "x-user-id": "load-owner-a" },
  });
  assert.equal(filtered.json().sessions.length, SESSIONS_PER_DEVICE);
  assert.ok(
    filtered
      .json()
      .sessions.every(
        (session: { device_id: string }) =>
          session.device_id === target.deviceId,
      ),
  );

  // Replaying a fully accepted batch is an ACK recovery path, not a duplicate.
  const replayBatch = batches.get(target.index);
  assert.ok(replayBatch);
  const replay = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: replayBatch,
  });
  assert.equal(replay.statusCode, 200);
  assert.equal(
    (await store.listSessions("load-owner-a", { device_id: target.deviceId }))
      .length,
    SESSIONS_PER_DEVICE,
  );

  const sample = filtered.json().sessions[0] as Record<string, unknown>;
  assert.deepEqual(Object.keys(sample).sort(), [
    "adapter_version",
    "agent_session_id",
    "agent_type",
    "confidence",
    "device_id",
    "event_id",
    "event_type",
    "observed_at",
    "payload",
    "received_at",
    "schema_version",
    "sequence",
    "source",
  ]);
  assert.ok(!("owner_id" in sample));
  assert.ok(!("public_key" in sample));
  assert.ok(!("signature" in sample));

  await app.close();
});

function makeDevice(index: number, ownerId: string): TestDevice {
  const seed = createHash("sha256").update(`m4-device-${index}`).digest();
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
  return {
    index,
    ownerId,
    bindingCode: `m4-binding-${index}`,
    privateKey,
    publicKey,
    deviceId: deviceIdForPublicKey(publicKey),
  };
}

function registrationPayload(device: TestDevice): Record<string, unknown> {
  const registration = {
    binding_code: device.bindingCode,
    device_id: device.deviceId,
    public_key: device.publicKey,
  };
  return {
    ...registration,
    signature: sign(
      null,
      Buffer.from(JSON.stringify(registration), "utf8"),
      device.privateKey,
    ).toString("hex"),
  };
}

function makeBatch(device: TestDevice, count: number): SyncBatch {
  const observedAt = new Date(
    Date.UTC(2026, 6, 28, 0, 0, device.index),
  ).toISOString();
  const events = Array.from({ length: count }, (_, index): SyncEvent => {
    const sequence = index + 1;
    const waiting = index % 10 === 0;
    const state: SyncEvent["payload"]["state"] = {
      presence: "LIVE",
      activity_status: waiting ? "WAITING_INPUT" : "WORKING",
      raw_status: waiting ? "blocked" : "active",
    };
    if (waiting) {
      state.waiting_reason = "user_input";
    }
    return {
      sequence,
      event_id: uuidFor(1, device.index * SESSIONS_PER_DEVICE + sequence),
      schema_version: 1,
      device_id: device.deviceId,
      agent_type: index % 2 === 0 ? "claude_code" : "codex",
      agent_session_id: `load-session-${device.index}-${index}`,
      event_type: "session_observed",
      observed_at: observedAt,
      source: index % 2 === 0 ? "claude_agents_json" : "codex_app_server",
      confidence: "authoritative",
      adapter_version: "m4-load-fixture",
      payload: {
        project: {
          display_name: `project-${index % 25}`,
          fingerprint: createHash("sha256")
            .update(`project-${device.index}-${index % 25}`)
            .digest("hex"),
        },
        state,
      },
    };
  });
  const batch: SyncBatch = {
    protocol_version: 1,
    batch_id: uuidFor(2, device.index + 1),
    device_id: device.deviceId,
    public_key: device.publicKey,
    created_at: observedAt,
    first_sequence: 1,
    last_sequence: count,
    events,
    signature: "",
  };
  batch.signature = sign(
    null,
    Buffer.from(unsignedBatchJson(batch), "utf8"),
    device.privateKey,
  ).toString("hex");
  return batch;
}

function uuidFor(namespace: number, value: number): string {
  return `${namespace.toString(16).padStart(8, "0")}-0000-4000-8000-${value
    .toString(16)
    .padStart(12, "0")}`;
}
