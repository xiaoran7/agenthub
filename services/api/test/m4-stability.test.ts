import assert from "node:assert/strict";
import test from "node:test";

import type { SyncBatch } from "@agent-platform/contracts";

import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "../src/store.js";
import {
  M4_DEVICE_COUNT as DEVICE_COUNT,
  M4_SESSIONS_PER_DEVICE as SESSIONS_PER_DEVICE,
  makeM4Batch as makeBatch,
  makeM4Device as makeDevice,
  m4RegistrationPayload as registrationPayload,
} from "./m4-fixtures.js";

test("M4 multi-device load keeps 3,000 sessions isolated and replay-safe", async () => {
  const devices = Array.from({ length: DEVICE_COUNT }, (_, index) =>
    makeDevice(index, index < 8 ? "load-owner-a" : "load-owner-b"),
  );
  const store = new InMemoryGatewayStore(
    () => new Date("2026-07-28T00:00:00Z"),
  );
  const app = createGatewayApp({
    authenticator: new DevelopmentIdentityAuthenticator(),
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
