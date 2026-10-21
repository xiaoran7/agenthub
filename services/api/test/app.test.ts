import assert from "node:assert/strict";
import { createPrivateKey, createPublicKey, sign } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  deviceIdForPublicKey,
  unsignedDeviceHeartbeatJson,
  unsignedAuthSyncBatchJson,
  parseSyncBatch,
  type SyncBatch,
  type SyncEvent,
  type AuthSyncBatch,
  type DeviceHeartbeat,
  unsignedBatchJson,
} from "@agent-pulse/contracts";

import { createGatewayApp, type GatewayAppOptions } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import type { PushOutcome } from "../src/push.js";
import {
  InMemoryGatewayStore,
  OneTimeBindingCodes,
  bindingCodeDigest,
} from "../src/store.js";

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

function createTestGatewayApp(options: GatewayAppOptions = {}) {
  return createGatewayApp({
    ...options,
    authenticator:
      options.authenticator ?? new DevelopmentIdentityAuthenticator(),
  });
}

function registrationPayload(bindingCode: string): Record<string, unknown> {
  const registration = {
    binding_code: bindingCode,
    device_id: fixture.device_id,
    public_key: fixture.public_key,
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

function heartbeatPayload(sentAt: string): DeviceHeartbeat {
  const heartbeat: DeviceHeartbeat = {
    protocol_version: 1,
    device_id: fixture.device_id,
    public_key: fixture.public_key,
    sent_at: sentAt,
    signature: "",
  };
  heartbeat.signature = sign(
    null,
    Buffer.from(unsignedDeviceHeartbeatJson(heartbeat), "utf8"),
    fixturePrivateKey,
  ).toString("hex");
  return heartbeat;
}

test("registration, signed ingestion, replay, and read queries form a loop", async () => {
  let now = new Date("2026-07-25T00:00:01Z");
  const store = new InMemoryGatewayStore(() => now);
  const app = createTestGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
    now: () => now,
  });

  const registration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
  });
  assert.equal(registration.statusCode, 201);

  const retryRegistration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
  });
  assert.equal(retryRegistration.statusCode, 200);

  const heartbeat = await app.inject({
    method: "POST",
    url: "/v1/devices/heartbeat",
    payload: heartbeatPayload("2026-07-25T00:00:01.000Z"),
  });
  assert.deepEqual(heartbeat.json(), {
    protocol_version: 1,
    device_id: fixture.device_id,
    accepted: true,
  });
  now = new Date("2026-07-25T00:00:10Z");
  const replayedHeartbeat = await app.inject({
    method: "POST",
    url: "/v1/devices/heartbeat",
    payload: heartbeatPayload("2026-07-25T00:00:01.000Z"),
  });
  assert.equal(replayedHeartbeat.statusCode, 200);
  assert.equal(replayedHeartbeat.json().accepted, false);
  const newerHeartbeat = await app.inject({
    method: "POST",
    url: "/v1/devices/heartbeat",
    payload: heartbeatPayload("2026-07-25T00:00:02.000Z"),
  });
  assert.equal(newerHeartbeat.statusCode, 200);
  assert.equal(newerHeartbeat.json().accepted, true);
  const staleHeartbeat = await app.inject({
    method: "POST",
    url: "/v1/devices/heartbeat",
    payload: heartbeatPayload("2026-07-24T00:00:00.000Z"),
  });
  assert.equal(staleHeartbeat.statusCode, 400);

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
    url: "/v1/sessions?activity_status=WAITING_PERMISSION&presence=LIVE",
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

  now = new Date("2026-07-25T00:01:00Z");
  const devices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(devices.json().devices[0].online, false);
  await app.close();
});

test("signed auth snapshots are stored without exposing token material", async () => {
  const store = new InMemoryGatewayStore();
  const app = createTestGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["auth-code", "auth-owner"]]),
    ),
  });
  const registration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("auth-code"),
  });
  assert.equal(registration.statusCode, 201);

  const authBatch = makeSignedAuthBatch();
  const ingestion = await app.inject({
    method: "POST",
    url: "/v1/device-auth",
    payload: authBatch,
  });
  assert.equal(ingestion.statusCode, 200);
  assert.deepEqual(ingestion.json(), {
    protocol_version: 1,
    device_id: fixture.device_id,
    accepted: 1,
  });

  const snapshots = await app.inject({
    method: "GET",
    url: "/v1/device-auth",
    headers: { "x-user-id": "auth-owner" },
  });
  assert.equal(snapshots.statusCode, 200);
  assert.equal(snapshots.json().snapshots.length, 1);
  assert.equal(snapshots.json().snapshots[0].access_token_present, true);
  assert.equal("access_token" in snapshots.json().snapshots[0], false);

  const foreign = await app.inject({
    method: "GET",
    url: "/v1/device-auth",
    headers: { "x-user-id": "another-owner" },
  });
  assert.deepEqual(foreign.json(), { snapshots: [] });
  const replay = await app.inject({
    method: "POST",
    url: "/v1/device-auth",
    payload: authBatch,
  });
  assert.equal(replay.json().accepted, 0);
  await app.close();
});

test("persistent binding codes expire and revoked devices keep history but cannot sync", async () => {
  let now = new Date("2026-07-28T10:00:00Z");
  const store = new InMemoryGatewayStore(() => now);
  const codes = [
    "generated-binding-code-a",
    "generated-binding-code-b",
    "generated-binding-code-c",
  ];
  const app = createGatewayApp({
    store,
    authenticator: new DevelopmentIdentityAuthenticator(),
    now: () => now,
    bindingCodeFactory: () => {
      const code = codes.shift();
      assert.ok(code);
      return code;
    },
  });

  const anonymousIssue = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    payload: {},
  });
  assert.equal(anonymousIssue.statusCode, 401);
  assert.equal(anonymousIssue.json().error, "authentication_required");

  const invalidTtl = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { "x-user-id": "user-1" },
    payload: { expires_in_seconds: 59 },
  });
  assert.equal(invalidTtl.statusCode, 400);
  assert.equal(invalidTtl.json().error, "invalid_binding_ttl");

  const unknownField = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { "x-user-id": "user-1" },
    payload: { unexpected: true },
  });
  assert.equal(unknownField.statusCode, 400);
  assert.equal(unknownField.json().error, "unknown_field");

  const issued = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { "x-user-id": "user-1" },
    payload: { expires_in_seconds: 60 },
  });
  assert.equal(issued.statusCode, 201);
  assert.deepEqual(issued.json(), {
    binding_code: "generated-binding-code-a",
    expires_at: "2026-07-28T10:01:00.000Z",
  });

  const registration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("generated-binding-code-a"),
  });
  assert.equal(registration.statusCode, 201);
  const retry = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("generated-binding-code-a"),
  });
  assert.equal(retry.statusCode, 200);

  assert.equal(
    (
      await app.inject({
        method: "POST",
        url: "/v1/sync/batches",
        payload: fixture,
      })
    ).statusCode,
    200,
  );
  const foreignRevoke = await app.inject({
    method: "DELETE",
    url: `/v1/devices/${fixture.device_id}`,
    headers: { "x-user-id": "user-2" },
  });
  assert.equal(foreignRevoke.statusCode, 204);
  assert.equal((await store.ingestBatch(fixture)).accepted, 0);

  const revoked = await app.inject({
    method: "DELETE",
    url: `/v1/devices/${fixture.device_id}`,
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(revoked.statusCode, 204);
  const devices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(devices.json().devices[0].revoked, true);
  assert.equal(devices.json().devices[0].online, false);
  assert.equal(
    (
      await app.inject({
        method: "GET",
        url: "/v1/sessions",
        headers: { "x-user-id": "user-1" },
      })
    ).json().sessions.length,
    1,
    "soft revocation must preserve monitoring history",
  );
  const rejectedBatch = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });
  assert.equal(rejectedBatch.statusCode, 403);
  assert.equal(rejectedBatch.json().error, "device_revoked");
  const rejectedRegistration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("generated-binding-code-a"),
  });
  assert.equal(rejectedRegistration.statusCode, 403);
  assert.equal(rejectedRegistration.json().error, "device_revoked");
  assert.equal(
    (
      await app.inject({
        method: "DELETE",
        url: `/v1/devices/${fixture.device_id}`,
        headers: { "x-user-id": "user-1" },
      })
    ).statusCode,
    204,
    "repeated revocation must stay idempotent",
  );

  const expiring = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { "x-user-id": "user-1" },
    payload: { expires_in_seconds: 60 },
  });
  assert.equal(expiring.statusCode, 201);
  now = new Date("2026-07-28T10:02:00Z");
  const expiredRegistration = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("generated-binding-code-b"),
  });
  assert.equal(expiredRegistration.statusCode, 403);
  assert.equal(expiredRegistration.json().error, "invalid_binding_code");

  // A revoked device can be reactivated with a fresh binding code
  const freshBinding = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { "x-user-id": "user-1" },
    payload: { expires_in_seconds: 60 },
  });
  assert.equal(freshBinding.statusCode, 201);
  const reactivated = await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("generated-binding-code-c"),
  });
  assert.equal(reactivated.statusCode, 200);
  assert.equal(reactivated.json().registered, true);

  const reactivatedDevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(reactivatedDevices.json().devices[0].revoked, false);
  assert.equal(reactivatedDevices.json().devices[0].online, true);

  await app.close();
});

test("one binding code cannot race-register two devices", async () => {
  const store = new InMemoryGatewayStore();
  const digest = bindingCodeDigest("concurrent-binding-code");
  await store.createBindingCode("user-1", digest, "2099-01-01T00:00:00.000Z");

  const results = await Promise.allSettled([
    store.registerDeviceWithBindingCode(
      digest,
      "dev_00000000000000000000000000000001",
      "1".repeat(64),
    ),
    store.registerDeviceWithBindingCode(
      digest,
      "dev_00000000000000000000000000000002",
      "2".repeat(64),
    ),
  ]);

  assert.equal(
    results.filter((result) => result.status === "fulfilled").length,
    1,
  );
  const rejected = results.find((result) => result.status === "rejected");
  assert.ok(rejected);
  assert.equal(
    (rejected.reason as { code?: unknown }).code,
    "binding_code_used",
  );
});

test("tampered and extra content is rejected before storage", async () => {
  const app = createTestGatewayApp({
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
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
  const app = createTestGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
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
  assert.equal((await store.listSessions("user-1", {})).length, 1);
  await app.close();
});

test("session event timeline is scoped to the owner and ordered by sequence", async () => {
  const app = createTestGatewayApp({
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
  });
  await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });
  const second = makeSignedBatch(2, (event) => {
    event.observed_at = "2026-07-25T00:00:05Z";
    event.payload.state = {
      presence: "LIVE",
      activity_status: "WORKING",
      raw_status: "active",
    };
  });
  const ingestion = await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: second,
  });
  assert.equal(ingestion.statusCode, 200);

  const timelineUrl =
    `/v1/sessions/events?device_id=${fixture.device_id}` +
    "&agent_type=codex&agent_session_id=contract-session";
  const timeline = await app.inject({
    method: "GET",
    url: timelineUrl,
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(timeline.statusCode, 200);
  const events = timeline.json().events;
  assert.equal(events.length, 2);
  assert.equal(events[0].sequence, 2);
  assert.equal(events[0].payload.state.activity_status, "WORKING");
  assert.equal(events[1].sequence, 1);
  assert.equal(events[1].payload.state.activity_status, "WAITING_PERMISSION");
  assert.equal(typeof events[0].received_at, "string");

  const limited = await app.inject({
    method: "GET",
    url: `${timelineUrl}&limit=1`,
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(limited.json().events.length, 1);
  assert.equal(limited.json().events[0].sequence, 2);

  const otherUser = await app.inject({
    method: "GET",
    url: timelineUrl,
    headers: { "x-user-id": "user-2" },
  });
  assert.equal(otherUser.statusCode, 200);
  assert.equal(otherUser.json().events.length, 0);

  const missingAgentType = await app.inject({
    method: "GET",
    url:
      `/v1/sessions/events?device_id=${fixture.device_id}` +
      "&agent_session_id=contract-session",
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(missingAgentType.statusCode, 400);
  assert.equal(missingAgentType.json().error, "invalid_query");

  const unknownField = await app.inject({
    method: "GET",
    url: `${timelineUrl}&transcript=1`,
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(unknownField.statusCode, 400);

  const invalidLimit = await app.inject({
    method: "GET",
    url: `${timelineUrl}&limit=0`,
    headers: { "x-user-id": "user-1" },
  });
  assert.equal(invalidLimit.statusCode, 400);
  await app.close();
});

test("waiting sessions notify the owner's subscriptions and prune dead ones", async () => {
  const store = new InMemoryGatewayStore();
  const sent: Array<{ endpoint: string; payload: string }> = [];
  let outcome: PushOutcome = "delivered";
  let delivered: () => void = () => {};
  const app = createTestGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
    push: {
      publicKey: "test-application-server-key",
      sender: {
        send: async (subscription, payload) => {
          sent.push({ endpoint: subscription.endpoint, payload });
          delivered();
          return outcome;
        },
      },
    },
  });
  const nextDelivery = (): Promise<void> =>
    new Promise((resolve) => {
      delivered = resolve;
    });

  const key = await app.inject({ method: "GET", url: "/v1/push/key" });
  assert.equal(key.statusCode, 200);
  assert.equal(key.json().public_key, "test-application-server-key");

  const subscription = {
    endpoint: "https://push.example/subscription/abc",
    expirationTime: null,
    keys: {
      p256dh: Buffer.concat([
        Buffer.from([0x04]),
        Buffer.alloc(64, 7),
      ]).toString("base64url"),
      auth: Buffer.alloc(16, 3).toString("base64url"),
    },
  };
  const subscribed = await app.inject({
    method: "POST",
    url: "/v1/push/subscriptions",
    headers: { "x-user-id": "user-1" },
    payload: subscription,
  });
  assert.equal(subscribed.statusCode, 204);
  assert.equal((await store.listPushSubscriptions("user-1")).length, 1);

  await app.inject({
    method: "POST",
    url: "/v1/devices/register",
    payload: registrationPayload("one-time-code"),
  });

  // The fixture session is already WAITING_PERMISSION, so storing it is the
  // transition that should reach the phone.
  const waiting = nextDelivery();
  await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: fixture,
  });
  await waiting;
  assert.equal(sent.length, 1);
  assert.deepEqual(JSON.parse(sent[0]?.payload ?? "{}"), {
    device_id: fixture.device_id,
    agent_type: "codex",
    agent_session_id: "contract-session",
    project: "contract-project",
    activity_status: "WAITING_PERMISSION",
    waiting_reason: "permission",
    observed_at: fixture.events[0]?.observed_at,
  });

  // Leaving the waiting state is not an interruption, so it must stay silent.
  await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: makeSignedBatch(2, (event) => {
      event.observed_at = "2026-07-25T00:00:05Z";
      event.payload.state = {
        presence: "LIVE",
        activity_status: "WORKING",
        raw_status: "active",
      };
    }),
  });
  assert.equal(sent.length, 1);

  outcome = "gone";
  const pruned = nextDelivery();
  await app.inject({
    method: "POST",
    url: "/v1/sync/batches",
    payload: makeSignedBatch(3, (event) => {
      event.observed_at = "2026-07-25T00:00:09Z";
      event.payload.state = {
        presence: "LIVE",
        activity_status: "WAITING_INPUT",
        raw_status: "blocked",
        waiting_reason: "user_input",
      };
    }),
  });
  await pruned;
  assert.equal(sent.length, 2);
  assert.equal(
    (await store.listPushSubscriptions("user-1")).length,
    0,
    "a subscription the push service reports gone must not be retried",
  );
  await app.close();
});

test("push subscriptions are validated and disabled without VAPID keys", async () => {
  const disabled = createTestGatewayApp();
  const disabledKey = await disabled.inject({
    method: "GET",
    url: "/v1/push/key",
  });
  assert.equal(disabledKey.statusCode, 404);
  assert.equal(disabledKey.json().error, "push_disabled");
  const disabledSubscribe = await disabled.inject({
    method: "POST",
    url: "/v1/push/subscriptions",
    headers: { "x-user-id": "user-1" },
    payload: {
      endpoint: "https://push.example/abc",
      keys: {
        p256dh: Buffer.concat([
          Buffer.from([0x04]),
          Buffer.alloc(64, 7),
        ]).toString("base64url"),
        auth: Buffer.alloc(16, 3).toString("base64url"),
      },
    },
  });
  assert.equal(disabledSubscribe.statusCode, 404);
  await disabled.close();

  const store = new InMemoryGatewayStore();
  const app = createTestGatewayApp({
    store,
    push: {
      publicKey: "test-application-server-key",
      sender: { send: async () => "delivered" },
    },
  });
  const validKeys = {
    p256dh: Buffer.concat([Buffer.from([0x04]), Buffer.alloc(64, 7)]).toString(
      "base64url",
    ),
    auth: Buffer.alloc(16, 3).toString("base64url"),
  };
  const rejected = async (payload: unknown): Promise<string> => {
    const response = await app.inject({
      method: "POST",
      url: "/v1/push/subscriptions",
      headers: { "x-user-id": "user-1" },
      payload: payload as never,
    });
    assert.equal(response.statusCode, 400);
    return response.json().error;
  };

  assert.equal(
    await rejected({ endpoint: "http://push.example/abc", keys: validKeys }),
    "invalid_endpoint",
  );
  assert.equal(
    await rejected({
      endpoint: "https://user:pass@push.example/abc",
      keys: validKeys,
    }),
    "invalid_endpoint",
  );
  assert.equal(
    await rejected({
      endpoint: "https://push.example/abc",
      keys: { ...validKeys, auth: Buffer.alloc(8, 3).toString("base64url") },
    }),
    "invalid_subscription",
  );
  assert.equal(
    await rejected({
      endpoint: "https://push.example/abc",
      keys: {
        ...validKeys,
        p256dh: Buffer.alloc(65, 7).toString("base64url"),
      },
    }),
    "invalid_subscription",
  );
  assert.equal(
    await rejected({
      endpoint: "https://push.example/abc",
      keys: validKeys,
      transcript: "private",
    }),
    "unknown_field",
  );
  assert.equal((await store.listPushSubscriptions("user-1")).length, 0);

  const anonymous = await app.inject({
    method: "POST",
    url: "/v1/push/subscriptions",
    payload: { endpoint: "https://push.example/abc", keys: validKeys },
  });
  assert.equal(anonymous.statusCode, 401);

  await app.inject({
    method: "POST",
    url: "/v1/push/subscriptions",
    headers: { "x-user-id": "user-1" },
    payload: { endpoint: "https://push.example/abc", keys: validKeys },
  });
  const foreignDelete = await app.inject({
    method: "DELETE",
    url: "/v1/push/subscriptions",
    headers: { "x-user-id": "user-2" },
    payload: { endpoint: "https://push.example/abc" },
  });
  assert.equal(foreignDelete.statusCode, 204);
  assert.equal(
    (await store.listPushSubscriptions("user-1")).length,
    1,
    "another user must not be able to unsubscribe this browser",
  );

  const removed = await app.inject({
    method: "DELETE",
    url: "/v1/push/subscriptions",
    headers: { "x-user-id": "user-1" },
    payload: { endpoint: "https://push.example/abc" },
  });
  assert.equal(removed.statusCode, 204);
  assert.equal((await store.listPushSubscriptions("user-1")).length, 0);
  await app.close();
});

test("the change stream is owner scoped, capped, and fires on accepted batches", async () => {
  const store = new InMemoryGatewayStore(
    () => new Date("2026-07-25T00:00:01Z"),
  );
  const app = createTestGatewayApp({
    store,
    bindingCodes: new OneTimeBindingCodes(
      new Map([["one-time-code", "user-1"]]),
    ),
  });
  await app.listen({ port: 0, host: "127.0.0.1" });
  const address = app.server.address();
  assert.ok(address !== null && typeof address === "object");
  const base = `http://127.0.0.1:${address.port}`;
  const streams: AbortController[] = [];
  const openStream = async (userId: string): Promise<Response> => {
    const controller = new AbortController();
    streams.push(controller);
    return fetch(`${base}/v1/stream`, {
      headers: { "x-user-id": userId },
      signal: controller.signal,
    });
  };

  try {
    const registration = await fetch(`${base}/v1/devices/register`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(registrationPayload("one-time-code")),
    });
    assert.equal(registration.status, 201);

    const owned = await openStream("user-1");
    assert.equal(owned.status, 200);
    assert.equal(
      owned.headers.get("content-type"),
      "text/event-stream",
      "clients need the SSE media type to parse frames",
    );
    assert.equal(owned.headers.get("cache-control"), "no-store");
    const ownedFrames = frameReader(owned);
    assert.equal(await ownedFrames.next(), `retry: 5000`);
    assert.equal(await ownedFrames.next(), "event: ready\ndata: {}");

    const stranger = await openStream("user-2");
    const strangerFrames = frameReader(stranger);
    assert.equal(await strangerFrames.next(), `retry: 5000`);
    assert.equal(await strangerFrames.next(), "event: ready\ndata: {}");

    const ingestion = await fetch(`${base}/v1/sync/batches`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(fixture),
    });
    assert.equal(ingestion.status, 200);

    const notification = await ownedFrames.next();
    assert.equal(
      notification,
      `event: sessions\ndata: ${JSON.stringify({
        device_id: fixture.device_id,
        accepted: fixture.events.length,
      })}`,
    );
    assert.equal(
      await strangerFrames.next(250),
      undefined,
      "another owner's device must never appear on this stream",
    );

    // A replayed batch stores nothing, so it must not wake up the panel.
    const replay = await fetch(`${base}/v1/sync/batches`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(fixture),
    });
    assert.equal(replay.status, 200);
    assert.equal(await ownedFrames.next(250), undefined);

    // One panel opens one stream; the cap keeps a stuck client from pinning
    // an unbounded number of connections and timers to a single owner.
    for (let index = 1; index < 8; index += 1) {
      assert.equal((await openStream("user-1")).status, 200);
    }
    const rejected = await openStream("user-1");
    assert.equal(rejected.status, 429);
    assert.equal(
      ((await rejected.json()) as { error: string }).error,
      "too_many_streams",
    );
  } finally {
    for (const controller of streams) {
      controller.abort();
    }
    await app.close();
  }
});

/**
 * Reads whole SSE frames. `next(timeoutMs)` resolves to undefined when no frame
 * arrives in time, which is how the test asserts a stream stayed silent.
 */
function frameReader(response: Response): {
  next: (timeoutMs?: number) => Promise<string | undefined>;
} {
  assert.ok(response.body !== null);
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffered = "";
  const readFrame = async (): Promise<string | undefined> => {
    for (;;) {
      const boundary = buffered.indexOf("\n\n");
      if (boundary >= 0) {
        const frame = buffered.slice(0, boundary);
        buffered = buffered.slice(boundary + 2);
        return frame;
      }
      const chunk = await reader.read();
      if (chunk.done) {
        return undefined;
      }
      buffered += decoder.decode(chunk.value, { stream: true });
    }
  };
  return {
    next: async (timeoutMs = 5_000) => {
      let timer: NodeJS.Timeout | undefined;
      const timeout = new Promise<undefined>((resolve) => {
        timer = setTimeout(() => resolve(undefined), timeoutMs);
      });
      try {
        return await Promise.race([readFrame(), timeout]);
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

function makeSignedBatch(
  sequence: number,
  mutate?: (event: SyncEvent) => void,
): SyncBatch {
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
  const event: SyncEvent = {
    ...structuredClone(originalEvent),
    sequence,
    // Distinct per sequence: reusing an event ID with different data is an
    // event_conflict, which would silently turn a batch into a 409.
    event_id: `223e4567-e89b-42d3-a456-4266141740${String(sequence).padStart(2, "0")}`,
    device_id: deviceId,
    observed_at: "2026-07-25T00:00:02Z",
  };
  mutate?.(event);
  const batch: SyncBatch = {
    protocol_version: 1,
    batch_id: "223e4567-e89b-42d3-a456-426614174000",
    device_id: deviceId,
    public_key: publicKey,
    created_at: "2026-07-25T00:00:02Z",
    first_sequence: sequence,
    last_sequence: sequence,
    events: [event],
    signature: "",
  };
  batch.signature = sign(
    null,
    Buffer.from(unsignedBatchJson(batch), "utf8"),
    privateKey,
  ).toString("hex");
  return batch;
}

function makeSignedAuthBatch(): AuthSyncBatch {
  const batch: AuthSyncBatch = {
    protocol_version: 1,
    device_id: fixture.device_id,
    public_key: fixture.public_key,
    checked_at: "2026-07-25T00:00:03Z",
    snapshots: [
      {
        provider: "codex",
        state: "authenticated",
        source: "app_server",
        checked_at: "2026-07-25T00:00:03Z",
        account_hint: "a***e@example.com",
        plan_type: "plus",
        auth_mode: "chatgpt",
        access_token_present: true,
        refresh_token_present: true,
        quota: {
          state: "available",
          source: "app_server",
          refreshed_at: "2026-07-25T00:00:03Z",
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
  return batch;
}
