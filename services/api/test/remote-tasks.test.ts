import assert from "node:assert/strict";
import { generateKeyPairSync, sign, type KeyObject } from "node:crypto";
import test from "node:test";

import {
  deviceIdForPublicKey,
  unsignedDeviceTaskClaimJson,
  unsignedDeviceTaskUpdateJson,
  type DeviceTaskClaim,
  type DeviceTaskUpdate,
  type RemoteTask,
} from "@agent-pulse/contracts";

import { createGatewayApp } from "../src/app.js";
import { InMemoryAccountStore, SessionAuthenticator } from "../src/accounts.js";
import { InMemoryGatewayStore } from "../src/store.js";

interface DeviceFixture {
  deviceId: string;
  publicKey: string;
  privateKey: KeyObject;
}

function sessionCookie(response: { headers: Record<string, unknown> }): string {
  const value = response.headers["set-cookie"];
  if (typeof value !== "string") assert.fail("session cookie missing");
  return value.split(";", 1)[0] ?? "";
}

function deviceFixture(): DeviceFixture {
  const keys = generateKeyPairSync("ed25519");
  const publicDer = keys.publicKey.export({ format: "der", type: "spki" });
  const publicKey = publicDer.subarray(publicDer.length - 32).toString("hex");
  return {
    deviceId: deviceIdForPublicKey(publicKey),
    publicKey,
    privateKey: keys.privateKey,
  };
}

function signedClaim(
  device: DeviceFixture,
  requestedAt: string,
): DeviceTaskClaim {
  const claim: DeviceTaskClaim = {
    protocol_version: 1,
    device_id: device.deviceId,
    public_key: device.publicKey,
    requested_at: requestedAt,
    signature: "",
  };
  claim.signature = sign(
    null,
    Buffer.from(unsignedDeviceTaskClaimJson(claim), "utf8"),
    device.privateKey,
  ).toString("hex");
  return claim;
}

function signedUpdate(
  device: DeviceFixture,
  task: RemoteTask,
  status: DeviceTaskUpdate["status"],
  updatedAt: string,
): DeviceTaskUpdate {
  const update: DeviceTaskUpdate = {
    protocol_version: 1,
    task_id: task.task_id,
    device_id: device.deviceId,
    public_key: device.publicKey,
    expected_version: task.version,
    status,
    updated_at: updatedAt,
    signature: "",
  };
  update.signature = sign(
    null,
    Buffer.from(unsignedDeviceTaskUpdateJson(update), "utf8"),
    device.privateKey,
  ).toString("hex");
  return update;
}

async function register(
  app: ReturnType<typeof createGatewayApp>,
  email: string,
): Promise<{ cookie: string; userId: string }> {
  const response = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: { email, password: "correct horse battery staple" },
  });
  assert.equal(response.statusCode, 201);
  return {
    cookie: sessionCookie(response),
    userId: response.json().user.user_id as string,
  };
}

function taskPayload(deviceId: string, key = "task-request-0001") {
  return {
    target_device_id: deviceId,
    agent_type: "codex",
    prompt: "检查测试失败并给出最小修复。",
    idempotency_key: key,
    execution_policy: "local_confirmation",
    expires_in_seconds: 3_600,
  };
}

test("free accounts cannot create remote tasks and unavailable policies fail closed", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
    now: () => new Date("2026-08-13T12:00:00.000Z"),
  });
  const owner = await register(app, "free@example.com");
  const device = deviceFixture();
  await store.registerDevice(owner.userId, device.deviceId, device.publicKey);
  const rejected = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: taskPayload(device.deviceId),
  });
  assert.equal(rejected.statusCode, 403);
  assert.equal(rejected.json().error, "remote_tasks_unavailable");

  accounts.setSubscription(owner.userId, { plan: "pro", status: "active" });
  const trusted = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: {
      ...taskPayload(device.deviceId),
      execution_policy: "trusted_execution",
    },
  });
  assert.equal(trusted.statusCode, 403);
  assert.equal(trusted.json().error, "execution_policy_unavailable");
  await app.close();
});

test("remote task relay is tenant scoped, idempotent, signed, and transition safe", async () => {
  let current = new Date("2026-08-13T12:00:00.000Z");
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore(() => current);
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
    now: () => current,
  });
  const owner = await register(app, "owner@example.com");
  const other = await register(app, "other@example.com");
  accounts.setSubscription(owner.userId, { plan: "pro", status: "active" });
  accounts.setSubscription(other.userId, { plan: "pro", status: "active" });
  const device = deviceFixture();
  await store.registerDevice(owner.userId, device.deviceId, device.publicKey);

  const created = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: taskPayload(device.deviceId),
  });
  assert.equal(created.statusCode, 201);
  const queued = created.json() as RemoteTask;
  assert.equal(queued.status, "queued");
  assert.equal(queued.version, 1);

  current = new Date("2026-08-13T12:00:01.000Z");
  const repeated = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: taskPayload(device.deviceId),
  });
  assert.equal(repeated.statusCode, 200);
  assert.equal(repeated.json().task_id, queued.task_id);

  const isolated = await app.inject({
    method: "GET",
    url: "/v1/tasks",
    headers: { cookie: other.cookie },
  });
  assert.deepEqual(isolated.json(), { tasks: [] });
  const hiddenCancel = await app.inject({
    method: "POST",
    url: `/v1/tasks/${queued.task_id}/cancel`,
    headers: { cookie: other.cookie },
  });
  assert.equal(hiddenCancel.statusCode, 404);

  const invalidClaim = signedClaim(device, current.toISOString());
  invalidClaim.signature = "0".repeat(128);
  const rejectedClaim = await app.inject({
    method: "POST",
    url: "/v1/device-tasks/claim",
    payload: invalidClaim,
  });
  assert.equal(rejectedClaim.statusCode, 401);

  const claim = signedClaim(device, current.toISOString());
  const deliveredResponse = await app.inject({
    method: "POST",
    url: "/v1/device-tasks/claim",
    payload: claim,
  });
  assert.equal(deliveredResponse.statusCode, 200);
  const delivered = deliveredResponse.json() as RemoteTask;
  assert.equal(delivered.status, "delivered");
  assert.equal(delivered.version, 2);
  const retriedClaim = await app.inject({
    method: "POST",
    url: "/v1/device-tasks/claim",
    payload: claim,
  });
  assert.equal(retriedClaim.json().task_id, delivered.task_id);
  assert.equal(retriedClaim.json().version, 2);

  const illegal = signedUpdate(
    device,
    delivered,
    "succeeded",
    current.toISOString(),
  );
  const illegalResponse = await app.inject({
    method: "POST",
    url: `/v1/device-tasks/${delivered.task_id}/status`,
    payload: illegal,
  });
  assert.equal(illegalResponse.statusCode, 409);
  assert.equal(illegalResponse.json().error, "invalid_task_transition");

  const acceptedUpdate = signedUpdate(
    device,
    delivered,
    "accepted",
    current.toISOString(),
  );
  const acceptedResponse = await app.inject({
    method: "POST",
    url: `/v1/device-tasks/${delivered.task_id}/status`,
    payload: acceptedUpdate,
  });
  assert.equal(acceptedResponse.statusCode, 200);
  const accepted = acceptedResponse.json() as RemoteTask;
  assert.equal(accepted.version, 3);
  const replay = await app.inject({
    method: "POST",
    url: `/v1/device-tasks/${delivered.task_id}/status`,
    payload: acceptedUpdate,
  });
  assert.equal(replay.statusCode, 200);
  assert.equal(replay.json().version, 3);

  const runningUpdate = signedUpdate(
    device,
    accepted,
    "running",
    current.toISOString(),
  );
  const runningResponse = await app.inject({
    method: "POST",
    url: `/v1/device-tasks/${delivered.task_id}/status`,
    payload: runningUpdate,
  });
  const running = runningResponse.json() as RemoteTask;
  assert.equal(running.status, "running");
  const canceled = await app.inject({
    method: "POST",
    url: `/v1/tasks/${running.task_id}/cancel`,
    headers: { cookie: owner.cookie },
  });
  assert.equal(canceled.statusCode, 200);
  assert.equal(canceled.json().status, "canceled");

  const noTask = await app.inject({
    method: "POST",
    url: "/v1/device-tasks/claim",
    payload: signedClaim(device, current.toISOString()),
  });
  assert.equal(noTask.statusCode, 204);
  await app.close();
});

test("subscription concurrency and task expiry are enforced by the store", async () => {
  let current = new Date("2026-08-13T12:00:00.000Z");
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore(() => current);
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
    now: () => current,
  });
  const owner = await register(app, "limit@example.com");
  accounts.setSubscription(owner.userId, { plan: "pro", status: "active" });
  const device = deviceFixture();
  await store.registerDevice(owner.userId, device.deviceId, device.publicKey);
  for (let index = 0; index < 3; index += 1) {
    const response = await app.inject({
      method: "POST",
      url: "/v1/tasks",
      headers: { cookie: owner.cookie },
      payload: taskPayload(device.deviceId, `task-limit-000${index}`),
    });
    assert.equal(response.statusCode, 201);
  }
  const limited = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: taskPayload(device.deviceId, "task-limit-0004"),
  });
  assert.equal(limited.statusCode, 403);
  assert.equal(limited.json().error, "task_limit_reached");

  current = new Date("2026-08-13T13:00:01.000Z");
  const tasks = await app.inject({
    method: "GET",
    url: "/v1/tasks?status=expired",
    headers: { cookie: owner.cookie },
  });
  assert.equal(tasks.json().tasks.length, 3);
  const replacement = await app.inject({
    method: "POST",
    url: "/v1/tasks",
    headers: { cookie: owner.cookie },
    payload: taskPayload(device.deviceId, "task-limit-0004"),
  });
  assert.equal(replacement.statusCode, 201);
  await app.close();
});
