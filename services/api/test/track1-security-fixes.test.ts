import assert from "node:assert/strict";
import {
  createECDH,
  generateKeyPairSync,
  randomUUID,
  sign,
  type KeyObject,
} from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { get as httpGet } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { WebSocket } from "ws";

import {
  deviceIdForPublicKey,
  unsignedAuthSyncBatchJson,
  unsignedBatchJson,
  type AuthSyncBatch,
  type SyncBatch,
} from "@agent-pulse/contracts";

import {
  AccountError,
  InMemoryAccountStore,
  SessionAuthenticator,
  tokenDigest,
} from "../src/accounts.js";
import { createGatewayApp } from "../src/app.js";
import {
  BearerTokenAuthenticator,
  DevelopmentIdentityAuthenticator,
} from "../src/auth.js";
import {
  HttpEmailVerificationSender,
  ResendEmailSender,
} from "../src/email.js";
import { PostgresAccountAuthRepo } from "../src/postgres/pg-account-auth.js";
import { PostgresAccountLifecycleRepo } from "../src/postgres/pg-account-lifecycle.js";
import { PostgresBillingAuditPushRepo } from "../src/postgres/pg-billing-audit-push.js";
import { PostgresSyncReadRepo } from "../src/postgres/pg-sync-read.js";
import { PostgresWorkspacesRepo } from "../src/postgres/pg-workspaces.js";
import { WebPushSender, type PushSubscription } from "../src/push.js";
import {
  isForbiddenHost,
  isSafeHttpsRegistryUrl,
} from "../src/remote-plugin-ssrf.js";
import type { RemoteControlHooks } from "../src/remote-routes.js";
import { RemoteContentStore } from "../src/remote-store.js";
import { SqliteAccountStore } from "../src/sqlite-account-store.js";
import { SqliteGatewayStore } from "../src/sqlite-store.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "../src/store.js";

function makeDevice() {
  const pair = generateKeyPairSync("ed25519");
  const spki = pair.publicKey.export({ format: "der", type: "spki" });
  const publicKey = Buffer.from(spki.subarray(-32)).toString("hex");
  const deviceId = deviceIdForPublicKey(publicKey);
  return { deviceId, publicKey, privateKey: pair.privateKey };
}

function signHello(
  privateKey: KeyObject,
  runnerId: string,
  nonce: string,
  generation: string,
  receivedCommandSeq = 0,
): string {
  return sign(
    null,
    Buffer.from(
      `agenthub.remote.v3\n${runnerId}\n${nonce}\n${generation}\n${receivedCommandSeq}`,
    ),
    privateKey,
  ).toString("hex");
}

function makePushSubscription(suffix: string): PushSubscription {
  const receiver = createECDH("prime256v1");
  receiver.generateKeys();
  return {
    endpoint: `https://fcm.googleapis.com/fcm/send/sub-${suffix}`,
    p256dh: receiver.getPublicKey().toString("base64url"),
    auth: Buffer.alloc(16, 5).toString("base64url"),
  };
}

function makeVapidKeys() {
  const key = createECDH("prime256v1");
  let priv: Buffer;
  do {
    key.generateKeys();
    priv = key.getPrivateKey();
  } while (priv.length !== 32);
  return {
    publicKey: key.getPublicKey().toString("base64url"),
    privateKey: priv.toString("base64url"),
    subject: "mailto:ops@example.com",
  };
}

async function connectRunnerWs(
  wsUrl: string,
  dev: ReturnType<typeof makeDevice>,
  generation = "gen-1",
  capabilities: string[] = ["plugins", "plugins_v2"],
) {
  const ws = new WebSocket(wsUrl);
  const frames: Record<string, unknown>[] = [];
  ws.on("message", (raw) => {
    frames.push(JSON.parse(raw.toString()) as Record<string, unknown>);
  });
  const nextFrame = async (type: string): Promise<Record<string, unknown>> => {
    for (let i = 0; i < 400; i += 1) {
      const idx = frames.findIndex((f) => f.type === type);
      if (idx >= 0) return frames.splice(idx, 1)[0]!;
      await new Promise((r) => setTimeout(r, 10));
    }
    assert.fail(`timed out waiting for WS frame ${type}`);
  };
  const challenge = await nextFrame("challenge");
  ws.send(
    JSON.stringify({
      type: "hello",
      runner_id: dev.deviceId,
      host_generation: generation,
      received_command_seq: 0,
      capabilities,
      signature: signHello(
        dev.privateKey,
        dev.deviceId,
        String(challenge.nonce),
        generation,
      ),
    }),
  );
  await nextFrame("hello_ack");
  return { ws, frames, nextFrame };
}

test("T1-P0-01: cross-tenant DELETE /v1/devices/:deviceId and revokeRunner cannot kick victim WS or wipe plugins", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const victim = makeDevice();
  await store.registerDevice("owner-victim", victim.deviceId, victim.publicKey);
  let hooks: RemoteControlHooks | undefined;
  const app = createGatewayApp({
    store,
    remoteContent: content,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteRoutes: {
      onControlHooks: (h) => {
        hooks = h;
      },
    },
  });
  let ws: WebSocket | undefined;
  try {
    const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
    const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";
    const conn = await connectRunnerWs(wsUrl, victim);
    ws = conn.ws;

    let wsClosed = false;
    ws.once("close", () => {
      wsClosed = true;
    });

    // Victim runner reports plugin state
    ws.send(
      JSON.stringify({
        type: "plugin_state",
        plugins: [
          {
            id: "zcode",
            name: "ZCode",
            displayName: "ZCode",
            installed: true,
            enabled: true,
            runtimeStatus: "ready",
          },
        ],
      }),
    );
    await new Promise((r) => setTimeout(r, 50));
    assert.equal(
      content.listRunnerPlugins("owner-victim", victim.deviceId).plugins.length,
      1,
    );

    // Attacker attempts DELETE /v1/devices/:victimDeviceId
    const attackerDelete = await app.inject({
      method: "DELETE",
      url: `/v1/devices/${victim.deviceId}`,
      headers: { "x-user-id": "owner-attacker" },
    });
    assert.equal(attackerDelete.statusCode, 204);
    await new Promise((r) => setTimeout(r, 40));

    // Victim WS MUST remain open and plugins MUST remain intact!
    assert.equal(wsClosed, false);
    assert.equal(ws.readyState, WebSocket.OPEN);
    assert.equal(
      content.listRunnerPlugins("owner-victim", victim.deviceId).plugins.length,
      1,
    );

    // Direct hook call with wrong expectedOwner MUST also be a no-op
    hooks?.revokeRunner(victim.deviceId, "owner-attacker");
    await new Promise((r) => setTimeout(r, 40));
    assert.equal(wsClosed, false);
    assert.equal(
      content.listRunnerPlugins("owner-victim", victim.deviceId).plugins.length,
      1,
    );

    // Victim owner revoking their own device kicks WS and wipes plugins
    const closedPromise = new Promise<number>((resolve) => {
      ws!.once("close", (code) => resolve(code));
    });
    const ownerDelete = await app.inject({
      method: "DELETE",
      url: `/v1/devices/${victim.deviceId}`,
      headers: { "x-user-id": "owner-victim" },
    });
    assert.equal(ownerDelete.statusCode, 204);
    assert.equal(await closedPromise, 1008);
    assert.equal(
      content.listRunnerPlugins("owner-victim", victim.deviceId).plugins.length,
      0,
    );
  } finally {
    ws?.terminate();
    await app.close();
    content.close();
  }
});

test("T1-P1-01: OneTimeBindingCodes does not release already-claimed code on failed replay", async () => {
  const store = new InMemoryGatewayStore();
  const bindingCodes = new OneTimeBindingCodes(
    new Map([["code-one-time-12345678", "owner-1"]]),
  );
  const app = createGatewayApp({
    store,
    bindingCodes,
    authenticator: new DevelopmentIdentityAuthenticator(),
  });
  try {
    const devA = makeDevice();
    const devB = makeDevice();
    const makeReg = (device: ReturnType<typeof makeDevice>, code: string) => {
      const payload = {
        binding_code: code,
        device_id: device.deviceId,
        public_key: device.publicKey,
      };
      const signature = sign(
        null,
        Buffer.from(
          JSON.stringify({
            binding_code: payload.binding_code,
            device_id: payload.device_id,
            public_key: payload.public_key,
          }),
          "utf8",
        ),
        device.privateKey,
      ).toString("hex");
      return { ...payload, signature };
    };

    // First registration claims and consumes the one-time binding code
    const first = await app.inject({
      method: "POST",
      url: "/v1/devices/register",
      payload: makeReg(devA, "code-one-time-12345678"),
    });
    assert.equal(first.statusCode, 201);

    // Revoke devA
    await store.revokeDevice("owner-1", devA.deviceId);

    // Replaying devA with the old binding code fails with device_revoked (isFresh === false)
    const replay = await app.inject({
      method: "POST",
      url: "/v1/devices/register",
      payload: makeReg(devA, "code-one-time-12345678"),
    });
    assert.equal(replay.statusCode, 403);
    assert.equal(replay.json().error, "device_revoked");

    // The binding code MUST NOT have been released for another device (or fresh reactivation)!
    const hijack = await app.inject({
      method: "POST",
      url: "/v1/devices/register",
      payload: makeReg(devB, "code-one-time-12345678"),
    });
    assert.equal(hijack.statusCode, 409);
    assert.equal(hijack.json().error, "binding_code_used");
  } finally {
    await app.close();
  }
});

test("T1-P1-02: /v3/remote/ws enforces per-IP unauth cap (8) and frees slot on auth or close", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const dev = makeDevice();
  await store.registerDevice("owner-ip", dev.deviceId, dev.publicKey);
  const app = createGatewayApp({
    store,
    remoteContent: content,
    trustProxy: true,
    authenticator: new DevelopmentIdentityAuthenticator(),
  });
  const sockets: WebSocket[] = [];
  try {
    const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
    const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";

    // Open 8 unauthenticated sockets from IP A
    await Promise.all(
      Array.from({ length: 8 }, () => {
        const s = new WebSocket(wsUrl, {
          headers: { "x-forwarded-for": "203.0.113.10" },
        });
        sockets.push(s);
        return new Promise<void>((resolve, reject) => {
          s.once("open", () => resolve());
          s.once("error", reject);
        });
      }),
    );

    // 9th unauthenticated socket from IP A is rejected with 1013
    const ninth = new WebSocket(wsUrl, {
      headers: { "x-forwarded-for": "203.0.113.10" },
    });
    sockets.push(ninth);
    const ninthCode = await new Promise<number>((resolve, reject) => {
      ninth.once("close", (code) => resolve(code));
      ninth.once("error", reject);
    });
    assert.equal(ninthCode, 1013);

    // Different IP B is still allowed to connect
    const otherIp = new WebSocket(wsUrl, {
      headers: { "x-forwarded-for": "203.0.113.11" },
    });
    sockets.push(otherIp);
    const challengeMsg = await new Promise<Record<string, unknown>>(
      (resolve, reject) => {
        otherIp.once("message", (d) =>
          resolve(JSON.parse(d.toString()) as Record<string, unknown>),
        );
        otherIp.once("error", reject);
      },
    );
    assert.equal(challengeMsg.type, "challenge");

    // Closing one socket from IP A frees its unauth slot
    const closedOne = new Promise<void>((resolve) =>
      sockets[0]!.once("close", () => resolve()),
    );
    sockets[0]!.close();
    await closedOne;
    await new Promise((r) => setTimeout(r, 20));

    const replacement = new WebSocket(wsUrl, {
      headers: { "x-forwarded-for": "203.0.113.10" },
    });
    sockets.push(replacement);
    const replMsg = await new Promise<Record<string, unknown>>(
      (resolve, reject) => {
        replacement.once("message", (d) =>
          resolve(JSON.parse(d.toString()) as Record<string, unknown>),
        );
        replacement.once("error", reject);
      },
    );
    assert.equal(replMsg.type, "challenge");
  } finally {
    for (const s of sockets) s.terminate();
    content.close();
    await app.close();
  }
});

test("T1-P1-03: /v3/remote/ws rejects sequence_conflict with 1008 while acking pruned sequences", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:", 2000);
  const dev = makeDevice();
  await store.registerDevice("owner-seq", dev.deviceId, dev.publicKey);
  const app = createGatewayApp({
    store,
    remoteContent: content,
    authenticator: new DevelopmentIdentityAuthenticator(),
  });
  let ws1: WebSocket | undefined;
  let ws2: WebSocket | undefined;
  try {
    const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
    const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";

    // 1. Conflicting payload on an existing sequence closes WS with 1008 sequence_conflict
    const c1 = await connectRunnerWs(wsUrl, dev, "gen-1");
    ws1 = c1.ws;
    const readyA = {
      type: "ready",
      host_generation: "gen-1",
      device_name: "Host A",
      workspaces: [],
      sessions: [],
    };
    ws1.send(JSON.stringify({ type: "data", seq: 1, payload: readyA }));
    assert.equal((await c1.nextFrame("ack")).seq, 1);

    const closed1 = new Promise<{ code: number; reason: string }>((resolve) => {
      ws1!.once("close", (code, reason) =>
        resolve({ code, reason: reason.toString("utf8") }),
      );
    });
    ws1.send(
      JSON.stringify({
        type: "data",
        seq: 1,
        payload: { ...readyA, device_name: "Tampered Host" },
      }),
    );
    const errFrame = await c1.nextFrame("error");
    assert.equal(errFrame.code, "sequence_conflict");
    const closeInfo = await closed1;
    assert.equal(closeInfo.code, 1008);
    assert.equal(closeInfo.reason, "sequence_conflict");

    // 2. Pruned sequence throws sequence_pruned at store layer, but WS cleanly acks received watermark
    for (let seq = 2; seq <= 2010; seq += 1) {
      content.receive("owner-seq", dev.deviceId, seq, readyA, "gen-1");
    }
    assert.throws(
      () => content.receive("owner-seq", dev.deviceId, 1, readyA, "gen-1"),
      /sequence_pruned/u,
    );

    const c2 = await connectRunnerWs(wsUrl, dev, "gen-1");
    ws2 = c2.ws;
    ws2.send(JSON.stringify({ type: "data", seq: 1, payload: readyA }));
    const ackAfterPrune = await c2.nextFrame("ack");
    assert.equal(ackAfterPrune.seq, 2010);
    assert.equal(ws2.readyState, WebSocket.OPEN);
  } finally {
    ws1?.terminate();
    ws2?.terminate();
    content.close();
    await app.close();
  }
});

test("T1-P1-04: SSRF guard blocks trailing dots, single-label hosts, and reserved IPv4/IPv6 ranges", () => {
  const blockedUrls = [
    "https://localhost./registry.json",
    "https://example.com./registry.json",
    "https://127.0.0.1./registry.json",
    "https://metadata/registry.json",
    "https://redis/registry.json",
    "https://192.0.0.1/registry.json",
    "https://198.18.0.1/registry.json",
    "https://198.19.255.254/registry.json",
    "https://[64:ff9b::127.0.0.1]/registry.json",
    "https://[64:ff9b:1::1]/registry.json",
    "https://[::ffff:0:127.0.0.1]/registry.json",
    "https://[fec0::1]/registry.json",
    "https://[100::1]/registry.json",
    "https://[::ffff:198.18.0.1]/registry.json",
  ];
  for (const url of blockedUrls) {
    assert.equal(
      isSafeHttpsRegistryUrl(url),
      false,
      `expected SSRF guard to reject ${url}`,
    );
  }
  assert.equal(isForbiddenHost("metadata"), true);
  assert.equal(isForbiddenHost("example.com."), true);
  assert.equal(isForbiddenHost("registry.example.com"), false);
  assert.equal(
    isSafeHttpsRegistryUrl("https://registry.example.com/plugins.json"),
    true,
  );
  assert.equal(
    isSafeHttpsRegistryUrl("https://[2606:4700:4700::1111]/plugins.json"),
    true,
  );
});

test("T1-P1-05: push subscriptions track subscriber_user_id, isolate quotas/deletes, and cascade on member removal", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const vapid = makeVapidKeys();
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts, false),
    singleAdmin: false,
    push: {
      publicKey: vapid.publicKey,
      sender: { send: async () => "delivered" },
    },
  });
  try {
    const ownerReg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "push-owner@example.com",
        password: "correct horse battery staple",
      },
    });
    const ownerCookie = String(ownerReg.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;
    const ownerId = ownerReg.json().user.user_id as string;
    accounts.setSubscription(ownerId, { plan: "team", status: "active" });

    const memberReg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "push-member@example.com",
        password: "correct horse battery staple",
      },
    });
    const memberCookie = String(memberReg.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;
    const memberId = memberReg.json().user.user_id as string;

    const inv = await app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie: ownerCookie },
      payload: { email: "push-member@example.com", role: "member" },
    });
    const tok = decodeURIComponent(
      String(inv.json().invitation_url).split("#token=")[1]!,
    );
    await app.inject({
      method: "POST",
      url: "/v1/team/invitations/accept",
      headers: { cookie: memberCookie },
      payload: { token: tok },
    });
    await app.inject({
      method: "POST",
      url: "/v1/team/workspace",
      headers: { cookie: memberCookie },
      payload: { workspace_owner_id: ownerId },
    });

    // Owner subscribes endpoint 0
    const ownerSub = makePushSubscription("owner-0");
    const ownerSubRes = await app.inject({
      method: "POST",
      url: "/v1/push/subscriptions",
      headers: { cookie: ownerCookie },
      payload: {
        endpoint: ownerSub.endpoint,
        keys: { p256dh: ownerSub.p256dh, auth: ownerSub.auth },
      },
    });
    assert.equal(ownerSubRes.statusCode, 204);

    // Member cannot overwrite owner's endpoint
    const overwriteAttempt = await app.inject({
      method: "POST",
      url: "/v1/push/subscriptions",
      headers: { cookie: memberCookie },
      payload: {
        endpoint: ownerSub.endpoint,
        keys: { p256dh: ownerSub.p256dh, auth: ownerSub.auth },
      },
    });
    assert.equal(overwriteAttempt.statusCode, 403);
    assert.equal(overwriteAttempt.json().error, "workspace_permission_denied");

    // Member fills their own 16-subscription quota
    for (let i = 0; i < 16; i += 1) {
      const sub = makePushSubscription(`member-${i}`);
      const r = await app.inject({
        method: "POST",
        url: "/v1/push/subscriptions",
        headers: { cookie: memberCookie },
        payload: {
          endpoint: sub.endpoint,
          keys: { p256dh: sub.p256dh, auth: sub.auth },
        },
      });
      assert.equal(r.statusCode, 204);
    }
    // 17th member subscription hits quota
    const member17 = makePushSubscription("member-17");
    const limitRes = await app.inject({
      method: "POST",
      url: "/v1/push/subscriptions",
      headers: { cookie: memberCookie },
      payload: {
        endpoint: member17.endpoint,
        keys: { p256dh: member17.p256dh, auth: member17.auth },
      },
    });
    assert.equal(limitRes.statusCode, 400);
    assert.equal(limitRes.json().error, "push_subscription_limit");

    // Owner's own quota is unaffected by member's 16 subscriptions!
    const ownerSub2 = makePushSubscription("owner-1");
    const ownerSub2Res = await app.inject({
      method: "POST",
      url: "/v1/push/subscriptions",
      headers: { cookie: ownerCookie },
      payload: {
        endpoint: ownerSub2.endpoint,
        keys: { p256dh: ownerSub2.p256dh, auth: ownerSub2.auth },
      },
    });
    assert.equal(ownerSub2Res.statusCode, 204);

    // Member cannot delete owner's subscription
    const memberDeleteOwner = await app.inject({
      method: "DELETE",
      url: "/v1/push/subscriptions",
      headers: { cookie: memberCookie },
      payload: { endpoint: ownerSub.endpoint },
    });
    assert.equal(memberDeleteOwner.statusCode, 204);
    assert.equal(
      (await store.listPushSubscriptions(ownerId, ownerId)).length,
      2,
    );

    // Removing member cascades deletion of ALL member's push subscriptions while keeping owner's
    const removeRes = await app.inject({
      method: "DELETE",
      url: `/v1/team/members/${memberId}`,
      headers: { cookie: ownerCookie },
    });
    assert.equal(removeRes.statusCode, 204);
    assert.equal(
      (await store.listPushSubscriptions(ownerId, memberId)).length,
      0,
    );
    assert.equal(
      (await store.listPushSubscriptions(ownerId, ownerId)).length,
      2,
    );
  } finally {
    await app.close();
  }

  // SQLite push_subscriptions migration & subscriber cleanup
  const dir = mkdtempSync(join(tmpdir(), "agenthub-sqlite-push-"));
  const dbPath = join(dir, "gateway.db");
  try {
    // Seed a legacy push_subscriptions table without subscriber_user_id
    const legacy = new DatabaseSync(dbPath);
    legacy.exec(`
      CREATE TABLE push_subscriptions (
        endpoint TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        payload TEXT NOT NULL
      );
    `);
    legacy.close();

    const sqlite = new SqliteGatewayStore(dbPath);
    try {
      const subA = makePushSubscription("sql-a");
      const subB = makePushSubscription("sql-b");
      await sqlite.savePushSubscription("ws-1", subA, "user-a");
      await sqlite.savePushSubscription("ws-1", subB, "user-b");
      await sqlite.deletePushSubscription("ws-1", subA.endpoint, "user-b");
      assert.equal((await sqlite.listPushSubscriptions("ws-1")).length, 2);
      await sqlite.deletePushSubscriptionsForSubscriber("ws-1", "user-b");
      const remaining = await sqlite.listPushSubscriptions("ws-1");
      assert.equal(remaining.length, 1);
      assert.equal(remaining[0]?.endpoint, subA.endpoint);
      assert.equal(remaining[0]?.subscriber_user_id, "user-a");
    } finally {
      sqlite.close();
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("T1-P1-06: password reset rollback error is caught without unhandledRejection", async () => {
  const accounts = new InMemoryAccountStore();
  accounts.deletePendingPasswordReset = async () => {
    throw new Error("db unavailable during rollback");
  };
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts, false),
    singleAdmin: false,
    emailVerification: {
      baseUrl: "https://platform.example.com",
      sender: {
        sendVerification: async () => {},
        sendPasswordReset: async () => {
          throw new Error("smtp timeout");
        },
      },
    },
  });
  let unhandled: unknown = undefined;
  const onUnhandled = (reason: unknown) => {
    unhandled = reason;
  };
  process.on("unhandledRejection", onUnhandled);
  try {
    await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "reset-rollback@example.com",
        password: "correct horse battery staple",
      },
    });
    const res = await app.inject({
      method: "POST",
      url: "/v1/auth/password-resets",
      payload: { email: "reset-rollback@example.com" },
    });
    assert.equal(res.statusCode, 202);
    await new Promise((r) => setTimeout(r, 50));
    assert.equal(unhandled, undefined);
  } finally {
    process.off("unhandledRejection", onUnhandled);
    await app.close();
  }
});

test("T1-P2-02: SqliteGatewayStore normalizes observed_at & checked_at to UTC for ordering while preserving raw JSON for replay", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-sqlite-utc-"));
  const dbPath = join(dir, "gateway.db");
  const sqlite = new SqliteGatewayStore(dbPath);
  try {
    const dev = makeDevice();
    await sqlite.registerDevice("owner-utc", dev.deviceId, dev.publicKey);

    // Event 1: 2026-11-25T10:00:00+08:00 == 2026-11-25T02:00:00Z (EARLIER in UTC!)
    // Event 2: 2026-11-25T03:00:00Z (LATER in UTC, though string "2026-11-25T03..." < "2026-11-25T10..."!)
    const unsigned: SyncBatch = {
      protocol_version: 1,
      batch_id: randomUUID(),
      device_id: dev.deviceId,
      public_key: dev.publicKey,
      created_at: "2026-11-25T03:00:01Z",
      first_sequence: 1,
      last_sequence: 2,
      events: [
        {
          event_id: randomUUID(),
          schema_version: 1,
          sequence: 1,
          device_id: dev.deviceId,
          agent_type: "codex",
          agent_session_id: "sess-earlier",
          event_type: "session_observed",
          observed_at: "2026-11-25T10:00:00+08:00",
          source: "codex_app_server",
          confidence: "observed",
          adapter_version: "0.1.0",
          payload: {
            project: { display_name: "earlier", fingerprint: "a".repeat(64) },
            state: {
              presence: "LIVE",
              activity_status: "WORKING",
              raw_status: "active",
            },
          },
        },
        {
          event_id: randomUUID(),
          schema_version: 1,
          sequence: 2,
          device_id: dev.deviceId,
          agent_type: "claude_code",
          agent_session_id: "sess-later",
          event_type: "session_observed",
          observed_at: "2026-11-25T03:00:00Z",
          source: "claude_hook",
          confidence: "observed",
          adapter_version: "0.1.0",
          payload: {
            project: { display_name: "later", fingerprint: "b".repeat(64) },
            state: {
              presence: "LIVE",
              activity_status: "WORKING",
              raw_status: "active",
            },
          },
        },
      ],
      signature: "0".repeat(128),
    };
    const batch: SyncBatch = {
      ...unsigned,
      signature: sign(
        null,
        Buffer.from(unsignedBatchJson(unsigned), "utf8"),
        dev.privateKey,
      ).toString("hex"),
    };
    assert.equal((await sqlite.ingestBatch(batch)).accepted, 2);

    // Chronological DESC order must place 03:00Z BEFORE 10:00+08:00 (02:00Z)!
    const sessions = await sqlite.listSessions("owner-utc", {});
    assert.equal(sessions[0]?.agent_session_id, "sess-later");
    assert.equal(sessions[1]?.agent_session_id, "sess-earlier");
    // Raw event JSON preserves original +08:00 offset so idempotent replay succeeds!
    assert.equal(sessions[1]?.observed_at, "2026-11-25T10:00:00+08:00");
    assert.equal((await sqlite.ingestBatch(batch)).accepted, 0);

    // Same for auth_snapshots checked_at ordering + replay equality
    const unsignedAuth: AuthSyncBatch = {
      protocol_version: 1,
      device_id: dev.deviceId,
      public_key: dev.publicKey,
      checked_at: "2026-11-25T03:00:01Z",
      snapshots: [
        {
          provider: "codex",
          state: "authenticated",
          source: "official_cli",
          checked_at: "2026-11-25T10:00:00+08:00",
          access_token_present: true,
          refresh_token_present: true,
        },
        {
          provider: "claude_code",
          state: "authenticated",
          source: "official_cli",
          checked_at: "2026-11-25T03:00:00Z",
          access_token_present: true,
          refresh_token_present: true,
        },
      ],
      signature: "0".repeat(128),
    };
    const authBatch: AuthSyncBatch = {
      ...unsignedAuth,
      signature: sign(
        null,
        Buffer.from(unsignedAuthSyncBatchJson(unsignedAuth), "utf8"),
        dev.privateKey,
      ).toString("hex"),
    };
    assert.equal((await sqlite.ingestAuthBatch(authBatch)).accepted, 2);
    const snaps = await sqlite.listAuth("owner-utc", {});
    assert.equal(snaps[0]?.provider, "claude_code");
    assert.equal(snaps[1]?.provider, "codex");
    assert.equal(snaps[1]?.checked_at, "2026-11-25T10:00:00+08:00");
    assert.equal((await sqlite.ingestAuthBatch(authBatch)).accepted, 0);
  } finally {
    sqlite.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("T1-P2-03 & T1-P2-04: plugin_status notifies SSE plugins event and /v3/remote/sessions eviction rejects pending plugin requests", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const dev = makeDevice();
  const token = "t".repeat(32);
  await store.registerDevice("owner-p23", dev.deviceId, dev.publicKey);
  const app = createGatewayApp({
    store,
    remoteContent: content,
    authenticator: new BearerTokenAuthenticator("owner-p23", token),
  });
  let ws: WebSocket | undefined;
  try {
    const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
    const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";
    const authHeaders = { authorization: `Bearer ${token}` };
    const conn = await connectRunnerWs(wsUrl, dev, "gen-1");
    ws = conn.ws;

    // Seed plugin via ready frame
    ws.send(
      JSON.stringify({
        type: "data",
        seq: 1,
        payload: {
          type: "ready",
          host_generation: "gen-1",
          workspaces: [],
          sessions: [],
          plugins: [
            {
              id: "zcode",
              name: "ZCode",
              displayName: "ZCode",
              installed: true,
              enabled: true,
              runtimeStatus: "idle",
            },
          ],
        },
      }),
    );
    await conn.nextFrame("ack");

    // Open SSE stream with agent: false (avoids undici keep-alive pre-warm socket)
    let sseBuf = "";
    const sseReq = httpGet(`${baseUrl}/v3/remote/stream`, {
      headers: authHeaders,
      agent: false,
    });
    const sseReady = new Promise<void>((resolve, reject) => {
      sseReq.on("error", reject);
      sseReq.on("response", (res) => {
        if (res.statusCode !== 200) {
          reject(new Error(`unexpected SSE status ${res.statusCode}`));
          return;
        }
        res.setEncoding("utf8");
        res.on("data", (chunk: string) => {
          sseBuf += chunk;
        });
        resolve();
      });
    });
    await sseReady;
    ws.send(
      JSON.stringify({
        type: "data",
        seq: 2,
        payload: {
          type: "plugin_status",
          plugin_id: "zcode",
          runtime_status: "running",
        },
      }),
    );
    await conn.nextFrame("ack");
    const deadline = Date.now() + 2000;
    while (Date.now() < deadline && !sseBuf.includes("event: plugins")) {
      await new Promise((r) => setTimeout(r, 15));
    }
    sseReq.destroy();
    assert.ok(
      sseBuf.includes("event: plugins") && sseBuf.includes(dev.deviceId),
      `expected event: plugins on plugin_status, got: ${sseBuf}`,
    );

    // T1-P2-04: Start an in-flight plugin action, revoke device directly in store (bypassing DELETE hook),
    // then hit GET /v3/remote/sessions -> pending plugin action MUST fail immediately with 503 runner_offline!
    const actionPromise = app.inject({
      method: "POST",
      url: `/v3/remote/runners/${dev.deviceId}/plugins/action`,
      headers: authHeaders,
      payload: { action: "refresh_catalog" },
    });
    await conn.nextFrame("plugin_command");
    await store.revokeDevice("owner-p23", dev.deviceId);
    const sessionsRes = await app.inject({
      method: "GET",
      url: "/v3/remote/sessions",
      headers: authHeaders,
    });
    assert.equal(sessionsRes.statusCode, 200);
    const actionRes = await actionPromise;
    assert.equal(actionRes.statusCode, 503);
    assert.equal(actionRes.json().error, "runner_offline");
  } finally {
    ws?.terminate();
    await app.close();
    content.close();
  }
});

test("T1-P2-05: team invitations block member email enumeration and rate limit invite/accept/verify-confirm endpoints", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts, false),
    singleAdmin: false,
  });
  try {
    const ownerReg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "enum-owner@example.com",
        password: "correct horse battery staple",
      },
    });
    const ownerCookie = String(ownerReg.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;
    const ownerId = ownerReg.json().user.user_id as string;
    accounts.setSubscription(ownerId, { plan: "team", status: "active" });

    const memberReg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "enum-member@example.com",
        password: "correct horse battery staple",
      },
    });
    const memberCookie = String(memberReg.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;

    const inv = await app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie: ownerCookie },
      payload: { email: "enum-member@example.com", role: "member" },
    });
    const tok = decodeURIComponent(
      String(inv.json().invitation_url).split("#token=")[1]!,
    );
    await app.inject({
      method: "POST",
      url: "/v1/team/invitations/accept",
      headers: { cookie: memberCookie },
      payload: { token: tok },
    });
    await app.inject({
      method: "POST",
      url: "/v1/team/workspace",
      headers: { cookie: memberCookie },
      payload: { workspace_owner_id: ownerId },
    });

    // Member probing nonexistent email MUST get 403 team_permission_denied (NOT 404)!
    const probeUnknown = await app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie: memberCookie },
      payload: { email: "nobody-here@example.com", role: "member" },
    });
    assert.equal(probeUnknown.statusCode, 403);
    assert.equal(probeUnknown.json().error, "team_permission_denied");

    // Rate limit on POST /v1/auth/email-verifications/confirm after 20 attempts
    for (let i = 0; i < 20; i += 1) {
      const r = await app.inject({
        method: "POST",
        url: "/v1/auth/email-verifications/confirm",
        payload: { token: "a".repeat(43) },
      });
      assert.equal(r.statusCode, 400);
    }
    const verifyLimited = await app.inject({
      method: "POST",
      url: "/v1/auth/email-verifications/confirm",
      payload: { token: "a".repeat(43) },
    });
    assert.equal(verifyLimited.statusCode, 429);
    assert.equal(verifyLimited.json().error, "login_rate_limited");

    // Rate limit on POST /v1/team/invitations/accept after 20 attempts (1 already used above)
    for (let i = 0; i < 19; i += 1) {
      const r = await app.inject({
        method: "POST",
        url: "/v1/team/invitations/accept",
        headers: { cookie: memberCookie },
        payload: { token: "b".repeat(43) },
      });
      assert.equal(r.statusCode, 400);
    }
    const acceptLimited = await app.inject({
      method: "POST",
      url: "/v1/team/invitations/accept",
      headers: { cookie: memberCookie },
      payload: { token: "b".repeat(43) },
    });
    assert.equal(acceptLimited.statusCode, 429);

    // Rate limit on POST /v1/team/invitations after 20 attempts (2 already used above)
    for (let i = 0; i < 18; i += 1) {
      const r = await app.inject({
        method: "POST",
        url: "/v1/team/invitations",
        headers: { cookie: memberCookie },
        payload: { email: `probe-${i}@example.com`, role: "member" },
      });
      assert.equal(r.statusCode, 403);
    }
    const inviteLimited = await app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie: memberCookie },
      payload: { email: "probe-over@example.com", role: "member" },
    });
    assert.equal(inviteLimited.statusCode, 429);
  } finally {
    await app.close();
  }
});

test("T1-P2-06: Postgres repos guard UUIDs, validate actorUserId, prune login limits at 24h, lock active user on verify, sanitize OAuth & retry 23505, and cascade push on member removal", async () => {
  type RecordedQuery = { text: string; values: unknown[] };
  function makeMockSql(
    handler: (
      text: string,
      values: unknown[],
    ) => unknown[] | Promise<unknown[]>,
  ) {
    const queries: RecordedQuery[] = [];
    const fn = ((strings: TemplateStringsArray, ...values: unknown[]) => {
      const text = strings.join("?").replace(/\s+/g, " ").trim();
      queries.push({ text, values });
      return Promise.resolve(handler(text, values));
    }) as unknown as {
      (strings: TemplateStringsArray, ...values: unknown[]): Promise<unknown[]>;
      begin<T>(cb: (sql: unknown) => Promise<T>): Promise<T>;
      json(v: unknown): unknown;
    };
    fn.json = (v: unknown) => v;
    fn.begin = async <T>(cb: (sql: unknown) => Promise<T>): Promise<T> =>
      cb(fn);
    return { sql: fn as never, queries };
  }

  // 1. recordAuditEvent rejects non-UUID and nonexistent actorUserId, uses ::uuid not ::text
  const validOwner = randomUUID();
  const missingActor = randomUUID();
  const auditMock = makeMockSql((text, values) => {
    assert.equal(text.includes("user_id::text"), false);
    if (text.includes("SELECT 1 FROM agent_pulse_users")) {
      return values[0] === validOwner ? [{ "?column?": 1 }] : [];
    }
    return [];
  });
  const auditRepo = new PostgresBillingAuditPushRepo(auditMock.sql);
  assert.equal(
    await auditRepo.recordAuditEvent({
      scope: "workspace",
      workspaceOwnerId: "not-a-uuid",
      action: "device.revoked",
      metadata: {},
    }),
    false,
  );
  assert.equal(auditMock.queries.length, 0);
  assert.equal(
    await auditRepo.recordAuditEvent({
      scope: "workspace",
      workspaceOwnerId: validOwner,
      actorUserId: "bad-actor",
      action: "device.revoked",
      metadata: {},
    }),
    false,
  );
  assert.equal(
    await auditRepo.recordAuditEvent({
      scope: "workspace",
      workspaceOwnerId: validOwner,
      actorUserId: missingActor,
      action: "device.revoked",
      metadata: {},
    }),
    false,
  );

  // 2. pruneExpiredData uses 24h cutoff for agent_pulse_login_limits (not 90d)
  const pruneMock = makeMockSql(() => []);
  const syncRead = new PostgresSyncReadRepo(pruneMock.sql);
  await syncRead.pruneExpiredData("2026-11-25T12:00:00.000Z", 90);
  const loginPrune = pruneMock.queries.find((q) =>
    q.text.includes("DELETE FROM agent_pulse_login_limits"),
  );
  assert.ok(loginPrune);
  assert.equal(loginPrune.values[1], "2026-11-24T12:00:00.000Z");

  // 3. consumeEmailVerification locks active user first and never consumes token when user is inactive
  const verifyMock = makeMockSql((text) => {
    if (text.includes("FOR UPDATE OF users")) return [];
    return [];
  });
  const lifecycle = new PostgresAccountLifecycleRepo(verifyMock.sql);
  assert.equal(
    await lifecycle.consumeEmailVerification(
      tokenDigest("a".repeat(43)),
      "2026-11-25T12:00:00.000Z",
    ),
    undefined,
  );
  assert.equal(
    verifyMock.queries.some((q) =>
      q.text.includes("UPDATE agent_pulse_email_verifications"),
    ),
    false,
  );

  // 4. upsertOAuthAccount normalizes long/control-char displayName and retries on 23505
  let attemptCount = 0;
  const existingUserId = randomUUID();
  const oauthMock = makeMockSql((text, values) => {
    if (text.includes("WHERE oauth_provider = ? AND oauth_sub = ?")) {
      attemptCount += 1;
      if (attemptCount === 1) return [];
      return [
        {
          user_id: existingUserId,
          email: "oauth@example.com",
          display_name: "A".repeat(80),
          password_hash: "x",
          status: "active",
          email_verified_at: "2026-11-25T12:00:00.000Z",
          oauth_provider: "github",
          oauth_sub: "sub-1",
          created_at: "2026-11-25T12:00:00.000Z",
        },
      ];
    }
    if (text.includes("INSERT INTO agent_pulse_users")) {
      const insertedName = String(values[3]);
      assert.ok(
        insertedName.length <= 80 &&
          !/[\u0000-\u001f\u007f]/.test(insertedName),
      );
      throw { code: "23505", constraint_name: "agent_pulse_users_oauth_idx" };
    }
    if (
      text.includes(
        "FROM agent_pulse_users AS users INNER JOIN agent_pulse_subscriptions",
      )
    ) {
      return [
        {
          user_id: existingUserId,
          email: "oauth@example.com",
          display_name: "A".repeat(80),
          password_hash: "x",
          status: "active",
          email_verified_at: "2026-11-25T12:00:00.000Z",
          created_at: "2026-11-25T12:00:00.000Z",
          plan: "free",
          subscription_status: "active",
        },
      ];
    }
    return [];
  });
  const authRepo = new PostgresAccountAuthRepo(oauthMock.sql);
  const oauthView = await authRepo.upsertOAuthAccount({
    provider: "github",
    sub: "sub-1",
    email: "oauth@example.com",
    displayName: "\x00" + "A".repeat(120) + "\n",
    emailVerified: true,
    now: "2026-11-25T12:00:00.000Z",
  });
  assert.equal(oauthView.user.user_id, existingUserId);
  assert.equal(attemptCount, 2);

  // 5. PostgresWorkspacesRepo.updateWorkspaceMember deletes removed member's push subscriptions
  const wsOwner = randomUUID();
  const wsMember = randomUUID();
  const wsMock = makeMockSql((text) => {
    if (text.includes("FOR UPDATE OF targets")) {
      return [{ actor_role: "owner", target_role: "member" }];
    }
    return [];
  });
  const wsRepo = new PostgresWorkspacesRepo(wsMock.sql);
  assert.equal(
    await wsRepo.updateWorkspaceMember({
      actorUserId: wsOwner,
      workspaceOwnerId: wsOwner,
      memberUserId: wsMember,
      remove: true,
      updatedAt: "2026-11-25T12:00:00.000Z",
    }),
    true,
  );
  const pushDelete = wsMock.queries.find((q) =>
    q.text.includes("DELETE FROM agent_platform_push_subscriptions"),
  );
  assert.ok(pushDelete);
  assert.deepEqual(pushDelete.values, [wsOwner, wsMember]);
});

test("T1-P2-07: item/started validates field types and length bounds", () => {
  const store = new RemoteContentStore(":memory:");
  const runner = "dev_" + "e".repeat(32);
  try {
    store.register("owner-val", runner);
    const makeStarted = (item: Record<string, unknown>) => ({
      type: "event",
      event: {
        event_id: `evt-${randomUUID()}`,
        host_generation: "gen-1",
        hub_session_id: "hub-1",
        provider_session_id: "native-1",
        kind: "item/started",
        raw: { params: { item } },
      },
    });

    // Reject non-string command, oversized path, empty id, oversized text
    for (const badItem of [
      { id: "", type: "commandExecution" },
      { id: "x".repeat(257), type: "commandExecution" },
      { id: "it-1", type: "commandExecution", command: { nested: true } },
      { id: "it-2", type: "commandExecution", command: "a".repeat(4097) },
      { id: "it-3", type: "fileChange", path: "p".repeat(1025) },
      { id: "it-4", type: "thinking", text: 12345 },
      { id: "it-5", type: "thinking", text: "t".repeat(65537) },
    ]) {
      assert.throws(
        () => store.receive("owner-val", runner, 1, makeStarted(badItem)),
        /unsafe_provider_event/u,
      );
    }

    // Valid bounded item/started succeeds
    assert.equal(
      store.receive(
        "owner-val",
        runner,
        1,
        makeStarted({
          id: "it-ok",
          type: "commandExecution",
          command: "echo hi",
          status: "running",
        }),
      ),
      1,
    );
  } finally {
    store.close();
  }
});

test("T1-P2-08: SqliteAccountStore enforces workspaceView ownership/memberLimit, NOCASE email uniqueness, and 60s last_seen_at throttle", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-sqlite-acc-"));
  const dbPath = join(dir, "accounts.db");
  const accounts = new SqliteAccountStore(dbPath);
  try {
    const ownerId = randomUUID();
    const otherId = randomUUID();
    await accounts.createAccount({
      userId: ownerId,
      email: "Admin@Example.com",
      displayName: "Admin",
      passwordHash: "hash-1",
      singleAdmin: false,
    });

    // Case-insensitive unique email constraint rejects mixed-case duplicate
    await assert.rejects(
      accounts.createAccount({
        userId: otherId,
        email: "admin@example.com",
        displayName: "Admin 2",
        passwordHash: "hash-2",
        singleAdmin: false,
      }),
      (err: unknown) =>
        err instanceof AccountError && err.code === "email_already_registered",
    );

    // workspaceView verifies actorUserId === workspaceOwnerId and respects memberLimit
    assert.equal(
      await accounts.workspaceView(otherId, ownerId, 5, "2026-11-25T12:00:00Z"),
      undefined,
    );
    const view = await accounts.workspaceView(
      ownerId,
      ownerId,
      5,
      "2026-11-25T12:00:00Z",
    );
    assert.ok(view);
    assert.equal(view.member_limit, 5);

    // findWebSession throttles last_seen_at UPDATE to 60s
    const digest = tokenDigest("s".repeat(43));
    const t0 = "2026-11-25T12:00:00.000Z";
    const t30s = "2026-11-25T12:00:30.000Z";
    const t61s = "2026-11-25T12:01:01.000Z";
    const exp = "2026-11-26T12:00:00.000Z";
    await accounts.createWebSession({
      sessionId: randomUUID(),
      digest,
      userId: ownerId,
      createdAt: t0,
      expiresAt: exp,
    });

    assert.ok(await accounts.findWebSession(digest, t30s));
    const sessionsAt30 = await accounts.listWebSessions(ownerId, digest, t30s);
    assert.equal(sessionsAt30[0]?.last_seen_at, t0);

    assert.ok(await accounts.findWebSession(digest, t61s));
    const sessionsAt61 = await accounts.listWebSessions(ownerId, digest, t61s);
    assert.equal(sessionsAt61[0]?.last_seen_at, t61s);
  } finally {
    accounts.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("T1-P3-01 & T1-P3-02: WebPushSender returns failed (not gone) on payload_too_large and email senders set redirect: error", async () => {
  const vapid = makeVapidKeys();
  const validSub = makePushSubscription("oversize");
  const sender = new WebPushSender(
    vapid,
    async () => new Response(null, { status: 201 }),
  );

  // Oversized payload (>3000 bytes) must return "failed" so valid subscription is NOT deleted!
  const oversizeOutcome = await sender.send(validSub, "x".repeat(3100));
  assert.equal(oversizeOutcome, "failed");

  // Invalid curve point (65 bytes starting with 0x04, not on P-256) returns "gone"
  const badPoint = Buffer.concat([
    Buffer.from([0x04]),
    Buffer.alloc(64, 0),
  ]).toString("base64url");
  assert.equal(
    await sender.send({ ...validSub, p256dh: badPoint }, "{}"),
    "gone",
  );

  // ResendEmailSender sets redirect: "error"
  let resendRedirect: RequestRedirect | undefined;
  const resend = new ResendEmailSender(
    "re_test_redirect_check_key",
    "noreply@example.com",
    async (_url, init) => {
      resendRedirect = init?.redirect;
      return new Response("{}", { status: 200 });
    },
  );
  await resend.sendVerification({
    user_id: "u1",
    email: "u1@example.com",
    display_name: "U1",
    verification_url: "https://platform.example.com/verify#token=x",
    expires_at: "2026-11-26T00:00:00Z",
  });
  assert.equal(resendRedirect, "error");

  // HttpEmailVerificationSender sets redirect: "error"
  const origFetch = globalThis.fetch;
  let httpRedirect: RequestRedirect | undefined;
  globalThis.fetch = (async (_url: unknown, init?: RequestInit) => {
    httpRedirect = init?.redirect;
    return new Response("{}", { status: 200 });
  }) as typeof fetch;
  try {
    const httpSender = new HttpEmailVerificationSender(
      "https://mail.example.com/send",
      "k".repeat(40),
    );
    await httpSender.sendVerification({
      user_id: "u1",
      email: "u1@example.com",
      display_name: "U1",
      verification_url: "https://platform.example.com/verify#token=x",
      expires_at: "2026-11-26T00:00:00Z",
    });
    assert.equal(httpRedirect, "error");
  } finally {
    globalThis.fetch = origFetch;
  }
});
