import assert from "node:assert/strict";
import {
  createHash,
  generateKeyPairSync,
  randomUUID,
  sign,
  type KeyObject,
} from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { WebSocket } from "ws";

import {
  deviceIdForPublicKey,
  parseRemoteCommand,
  unsignedBatchJson,
  unsignedDeviceHeartbeatJson,
  type DeviceHeartbeat,
} from "@agent-pulse/contracts";

import {
  AccountError,
  InMemoryAccountStore,
  SessionAuthenticator,
  hashPassword,
} from "../src/accounts.js";
import { createGatewayApp } from "../src/app.js";
import {
  BearerTokenAuthenticator,
  DevelopmentIdentityAuthenticator,
} from "../src/auth.js";
import type { BillingProvider, PaidSubscriptionPlan } from "../src/billing.js";
import { UUID_RE } from "../src/postgres/pg-shared.js";
import { RemoteContentStore } from "../src/remote-store.js";
import {
  commandDigest,
  isWorkspaceAuthorizedInHello,
} from "../src/remote-store-validation.js";
import { resolveTrustProxyConfig } from "../src/security-guards.js";
import { SqliteAccountStore } from "../src/sqlite-account-store.js";
import { SqliteGatewayStore } from "../src/sqlite-store.js";
import { InMemoryGatewayStore, bindingCodeDigest } from "../src/store.js";

function makeDevice() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const spki = publicKey.export({ format: "der", type: "spki" });
  const rawPub = spki.subarray(spki.length - 32).toString("hex");
  return {
    deviceId: deviceIdForPublicKey(rawPub),
    publicKey: rawPub,
    privateKey,
  };
}

function signHello(
  privateKey: KeyObject,
  runnerId: string,
  nonce: string,
  generation: string,
  receivedCommandSeq = 0,
): string {
  const payload = Buffer.from(
    `agenthub.remote.v3\n${runnerId}\n${nonce}\n${generation}\n${String(receivedCommandSeq)}`,
  );
  return sign(null, payload, privateKey).toString("hex");
}

function signedHeartbeat(
  dev: ReturnType<typeof makeDevice>,
  sentAt: string,
  deviceName?: string,
): DeviceHeartbeat {
  const hb: DeviceHeartbeat = {
    protocol_version: 1,
    device_id: dev.deviceId,
    public_key: dev.publicKey,
    sent_at: sentAt,
    signature: "",
  };
  hb.signature = sign(
    null,
    Buffer.from(unsignedDeviceHeartbeatJson(hb), "utf8"),
    dev.privateKey,
  ).toString("hex");
  return deviceName === undefined ? hb : { ...hb, device_name: deviceName };
}

test("API-H2: active login lockout survives window rollover and resets cleanly after expiry", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-lockout-"));
  const sqlite = new SqliteAccountStore(join(dir, "accounts.db"));
  const memory = new InMemoryAccountStore();
  try {
    for (const store of [memory, sqlite]) {
      const scope = createHash("sha256").update("scope-1").digest("hex");
      const baseMs = Date.parse("2026-11-25T10:00:00.000Z");
      const at = (offsetSec: number) =>
        new Date(baseMs + offsetSec * 1_000).toISOString();

      // 1 attempt at T=0, 4 more at T=850s -> allowed (5 total)
      assert.equal(
        (
          await store.consumeLoginAttempt({
            scopeDigest: scope,
            now: at(0),
            windowSeconds: 900,
            maximumAttempts: 5,
            blockSeconds: 900,
          })
        ).allowed,
        true,
      );
      for (let i = 0; i < 4; i += 1) {
        assert.equal(
          (
            await store.consumeLoginAttempt({
              scopeDigest: scope,
              now: at(850),
              windowSeconds: 900,
              maximumAttempts: 5,
              blockSeconds: 900,
            })
          ).allowed,
          true,
        );
      }
      // 6th attempt at T=850s triggers lockout until T=1750s
      const blocked = await store.consumeLoginAttempt({
        scopeDigest: scope,
        now: at(850),
        windowSeconds: 900,
        maximumAttempts: 5,
        blockSeconds: 900,
      });
      assert.deepEqual(blocked, { allowed: false, retry_after_seconds: 900 });

      // At T=901s (window rolled over, lockout still active): MUST stay blocked
      const stillBlocked = await store.consumeLoginAttempt({
        scopeDigest: scope,
        now: at(901),
        windowSeconds: 900,
        maximumAttempts: 5,
        blockSeconds: 900,
      });
      assert.equal(stillBlocked.allowed, false);
      assert.equal(stillBlocked.retry_after_seconds, 849);

      // At T=1751s (lockout expired): allowed AND window resets (next attempt also allowed)
      const firstAfter = await store.consumeLoginAttempt({
        scopeDigest: scope,
        now: at(1751),
        windowSeconds: 900,
        maximumAttempts: 5,
        blockSeconds: 900,
      });
      assert.deepEqual(firstAfter, { allowed: true, retry_after_seconds: 0 });
      const secondAfter = await store.consumeLoginAttempt({
        scopeDigest: scope,
        now: at(1752),
        windowSeconds: 900,
        maximumAttempts: 5,
        blockSeconds: 900,
      });
      assert.deepEqual(secondAfter, { allowed: true, retry_after_seconds: 0 });
    }
  } finally {
    sqlite.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("API-H2: login IP scope caps cross-email spraying and password change is rate limited", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    singleAdmin: false,
  });
  try {
    const reg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "victim-rate@example.com",
        password: "correct horse battery staple",
      },
    });
    assert.equal(reg.statusCode, 201);
    const cookie = String(reg.headers["set-cookie"]).split(";", 1)[0]!;

    for (let i = 0; i < 10; i += 1) {
      const res = await app.inject({
        method: "POST",
        url: "/v1/auth/password",
        headers: { cookie },
        payload: {
          current_password: "wrong password value",
          new_password: "another strong password",
        },
      });
      assert.equal(res.statusCode, 401);
    }
    const pwdLimited = await app.inject({
      method: "POST",
      url: "/v1/auth/password",
      headers: { cookie },
      payload: {
        current_password: "wrong password value",
        new_password: "another strong password",
      },
    });
    assert.equal(pwdLimited.statusCode, 429);
    assert.equal(pwdLimited.json().error, "login_rate_limited");

    for (let i = 0; i < 60; i += 1) {
      const res = await app.inject({
        method: "POST",
        url: "/v1/auth/login",
        payload: {
          email: `spray-${i}@example.com`,
          password: "wrong password value",
        },
      });
      assert.equal(res.statusCode, 401);
    }
    const ipLimited = await app.inject({
      method: "POST",
      url: "/v1/auth/login",
      payload: {
        email: "spray-61@example.com",
        password: "wrong password value",
      },
    });
    assert.equal(ipLimited.statusCode, 429);
    assert.equal(ipLimited.json().error, "login_rate_limited");
  } finally {
    await app.close();
  }
});

test("API-H3: Fastify honors numeric trustProxy hop count from resolveTrustProxyConfig", async () => {
  const hops = resolveTrustProxyConfig("0.0.0.0", "1");
  assert.equal(hops, 1);
  const app = createGatewayApp({ trustProxy: hops });
  app.get("/_test/ip", async (request) => ({ ip: request.ip }));
  try {
    const res = await app.inject({
      method: "GET",
      url: "/_test/ip",
      headers: { "x-forwarded-for": "198.51.100.10, 203.0.113.7" },
    });
    assert.equal(res.statusCode, 200);
    assert.equal(res.json().ip, "203.0.113.7");
  } finally {
    await app.close();
  }
});

test("API-H4: idle /v3/remote/stream closes immediately on logout without leaking events", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const app = createGatewayApp({
    accounts,
    store,
    remoteContent: content,
    authenticator: new SessionAuthenticator(accounts),
    remoteRoutes: { streamAuthRevalidateMs: 60_000 },
  });
  const address = await app.listen({ host: "127.0.0.1", port: 0 });
  const abort = new AbortController();
  try {
    const reg = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "sse-revoke@example.com",
        password: "correct horse battery staple",
      },
    });
    assert.equal(reg.statusCode, 201);
    const cookie = String(reg.headers["set-cookie"]).split(";", 1)[0]!;
    const userId = reg.json().user.user_id as string;

    const streamRes = await fetch(`${address}/v3/remote/stream`, {
      headers: { cookie, accept: "text/event-stream" },
      signal: abort.signal,
    });
    assert.equal(streamRes.status, 200);
    const reader = streamRes.body!.getReader();
    const initial = await reader.read();
    assert.equal(initial.done, false);

    const logout = await app.inject({
      method: "POST",
      url: "/v1/auth/logout",
      headers: { cookie },
    });
    assert.equal(logout.statusCode, 204);

    // Trigger plugin update after logout; stream must close rather than emit plugins.
    content.saveRunnerPlugins(userId, "dev_" + "a".repeat(32), undefined, []);
    const decoder = new TextDecoder();
    let tail = "";
    const deadline = Date.now() + 2_000;
    let closed = false;
    while (Date.now() < deadline) {
      const nextChunk = await Promise.race([
        reader.read(),
        new Promise<{ done: false; value: undefined }>((resolve) =>
          setTimeout(() => resolve({ done: false, value: undefined }), 100),
        ),
      ]);
      if (nextChunk.done) {
        closed = true;
        break;
      }
      if (nextChunk.value) {
        tail += decoder.decode(nextChunk.value, { stream: true });
      }
    }
    assert.equal(closed, true, "idle SSE stream must close promptly on logout");
    assert.equal(tail.includes("event: plugins"), false);
  } finally {
    abort.abort();
    content.close();
    await app.close();
  }
});

test("API-H5: WS pipelined frames short-circuit after error and unauth sockets are capped at 64", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const dev = makeDevice();
  await store.registerDevice("owner-ws", dev.deviceId, dev.publicKey);
  const app = createGatewayApp({
    store,
    remoteContent: content,
    trustProxy: true,
    authenticator: new DevelopmentIdentityAuthenticator(),
  });
  const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
  const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";
  const openSockets: WebSocket[] = [];
  try {
    // 1. Pipelined frame after an invalid frame must not be applied.
    const ws = new WebSocket(wsUrl);
    openSockets.push(ws);
    await new Promise<void>((resolve, reject) => {
      ws.once("error", reject);
      ws.on("message", (raw) => {
        const msg = JSON.parse(raw.toString()) as {
          type: string;
          nonce?: string;
        };
        if (msg.type === "challenge" && msg.nonce) {
          ws.send(
            JSON.stringify({
              type: "hello",
              runner_id: dev.deviceId,
              host_generation: "gen-1",
              received_command_seq: 0,
              signature: signHello(
                dev.privateKey,
                dev.deviceId,
                msg.nonce,
                "gen-1",
              ),
            }),
          );
        } else if (msg.type === "hello_ack") {
          resolve();
        }
      });
    });
    const closeCode = new Promise<number>((resolve) => {
      ws.once("close", (code) => resolve(code));
    });
    ws.send(JSON.stringify({ type: "bogus_frame" }));
    ws.send(
      JSON.stringify({
        type: "data",
        seq: 1,
        payload: {
          type: "ready",
          host_generation: "gen-1",
          workspaces: [],
          sessions: [],
        },
      }),
    );
    assert.equal(await closeCode, 1008);
    assert.equal(content.watermarks(dev.deviceId).received, 0);

    // 2. Cap unauthenticated sockets at 64 globally across distinct IPs and reject 65th with 1013.
    const opened = await Promise.all(
      Array.from({ length: 64 }, (_, index) => {
        const ip = `198.51.100.${Math.floor(index / 8) + 1}`;
        const sock = new WebSocket(wsUrl, {
          headers: { "x-forwarded-for": ip },
        });
        openSockets.push(sock);
        return new Promise<void>((resolve, reject) => {
          sock.once("open", () => resolve());
          sock.once("error", reject);
        });
      }),
    );
    assert.equal(opened.length, 64);

    const overflow = new WebSocket(wsUrl, {
      headers: { "x-forwarded-for": "198.51.100.9" },
    });
    openSockets.push(overflow);
    const overflowCode = await new Promise<number>((resolve, reject) => {
      overflow.once("close", (code) => resolve(code));
      overflow.once("error", reject);
    });
    assert.equal(overflowCode, 1013);
  } finally {
    for (const sock of openSockets) sock.terminate();
    content.close();
    await app.close();
  }
});

test("API-H6: single-owner Bearer mode works with accounts store and UUID_RE guards non-UUIDs", async () => {
  assert.equal(UUID_RE.test("default-owner"), false);
  assert.equal(UUID_RE.test(randomUUID()), true);

  const dir = mkdtempSync(join(tmpdir(), "agenthub-bearer-"));
  const accounts = new SqliteAccountStore(join(dir, "accounts.db"));
  const token = "b".repeat(40);
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new BearerTokenAuthenticator("default-owner", token),
  });
  try {
    const authHeaders = { authorization: `Bearer ${token}` };
    const me = await app.inject({
      method: "GET",
      url: "/v1/auth/me",
      headers: authHeaders,
    });
    assert.equal(me.statusCode, 200);
    assert.equal(me.json().workspace.workspace_owner_id, "default-owner");
    assert.equal(me.json().workspaces[0].workspace_owner_id, "default-owner");

    const bind = await app.inject({
      method: "POST",
      url: "/v1/device-bindings",
      headers: authHeaders,
      payload: {},
    });
    assert.equal(bind.statusCode, 201);
    assert.ok(bind.json().binding_code);
  } finally {
    accounts.close();
    await app.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("API-H7: SqliteAccountStore invalidates superseded tokens, enforces active status, and prunes unblocked attempts", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-sqlite-h7-"));
  const dbPath = join(dir, "accounts.db");
  const store = new SqliteAccountStore(dbPath);
  try {
    const hash1 = await hashPassword("first strong password");
    const hash2 = await hashPassword("second strong password");
    const userId = randomUUID();
    await store.createAccount({
      userId,
      email: "sqlite-h7@example.com",
      displayName: "Admin",
      passwordHash: hash1,
    });

    const reset1 = createHash("sha256").update("r1").digest("hex");
    const reset2 = createHash("sha256").update("r2").digest("hex");
    assert.equal(
      (
        await store.issuePasswordReset({
          digest: reset1,
          userId,
          createdAt: "2026-11-25T10:00:00.000Z",
          expiresAt: "2026-11-25T11:00:00.000Z",
          minimumIntervalSeconds: 60,
        })
      ).allowed,
      true,
    );
    assert.equal(
      (
        await store.issuePasswordReset({
          digest: reset2,
          userId,
          createdAt: "2026-11-25T10:02:00.000Z",
          expiresAt: "2026-11-25T11:02:00.000Z",
          minimumIntervalSeconds: 60,
        })
      ).allowed,
      true,
    );
    // Superseded reset1 must be invalidated.
    assert.equal(
      (
        await store.consumePasswordReset({
          digest: reset1,
          passwordHash: hash2,
          now: "2026-11-25T10:03:00.000Z",
        })
      ).updated,
      false,
    );
    // Changing password invalidates pending reset2.
    assert.equal(
      (
        await store.changePassword({
          userId,
          expectedPasswordHash: hash1,
          passwordHash: hash2,
          currentSessionDigest: createHash("sha256").update("s").digest("hex"),
          changedAt: "2026-11-25T10:04:00.000Z",
        })
      ).updated,
      true,
    );
    assert.equal(
      (
        await store.consumePasswordReset({
          digest: reset2,
          passwordHash: hash1,
          now: "2026-11-25T10:05:00.000Z",
        })
      ).updated,
      false,
    );

    // Unblocked expired login attempt is pruned.
    const oldScope = createHash("sha256").update("old-scope").digest("hex");
    await store.consumeLoginAttempt({
      scopeDigest: oldScope,
      now: "2026-11-20T00:00:00.000Z",
      windowSeconds: 900,
      maximumAttempts: 5,
      blockSeconds: 900,
    });
    await store.pruneExpiredData("2026-11-25T00:00:00.000Z");
    const checkDb = new DatabaseSync(dbPath);
    try {
      const remaining = checkDb
        .prepare("SELECT COUNT(*) AS cnt FROM agent_pulse_login_attempts")
        .get() as { cnt: number };
      assert.equal(Number(remaining.cnt), 0);
    } finally {
      checkDb.close();
    }

    // Deleted account returns undefined from accountView and cannot consume reset.
    const reset3 = createHash("sha256").update("r3").digest("hex");
    await store.issuePasswordReset({
      digest: reset3,
      userId,
      createdAt: "2026-11-25T10:10:00.000Z",
      expiresAt: "2026-11-25T11:10:00.000Z",
      minimumIntervalSeconds: 60,
    });
    await store.deleteAccount(userId);
    assert.equal(await store.accountView(userId), undefined);
    assert.equal(
      (
        await store.consumePasswordReset({
          digest: reset3,
          passwordHash: hash1,
          now: "2026-11-25T10:11:00.000Z",
        })
      ).updated,
      false,
    );
  } finally {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("M2/M4/M6/M7/M9/M10/M11/M12/M13 regression coverage", async () => {
  // M2: OAuth normalizes email & blocks cross-provider hijack
  const memAccounts = new InMemoryAccountStore();
  await memAccounts.upsertOAuthAccount({
    provider: "github",
    sub: "gh-1",
    email: "OAuth.User@Example.COM",
    displayName: "OAuth User",
    emailVerified: true,
  });
  await assert.rejects(
    () =>
      memAccounts.upsertOAuthAccount({
        provider: "google",
        sub: "g-2",
        email: "oauth.user@example.com",
        displayName: "Hijacker",
        emailVerified: true,
      }),
    (err: unknown) =>
      err instanceof AccountError &&
      err.code === "oauth_account_conflict" &&
      err.statusCode === 409,
  );

  // M4: canceled subscription allows re-subscribe via checkout; active blocks with billing_portal_required
  const billingProvider: BillingProvider = {
    name: "test",
    webhookPath: "/v1/billing/test/events",
    webhookSignatureHeader: "x-sig",
    async createCheckout(_input: {
      userId: string;
      email: string;
      plan: PaidSubscriptionPlan;
      customerId?: string;
    }) {
      return "https://checkout.stripe.com/resub";
    },
    async createPortal() {
      return "https://billing.stripe.com/portal";
    },
    parseWebhook() {
      return undefined;
    },
  };
  const billingAccounts = new InMemoryAccountStore();
  const billingApp = createGatewayApp({
    accounts: billingAccounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(billingAccounts),
    billingProvider,
  });
  try {
    const reg = await billingApp.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "resub@example.com",
        password: "correct horse battery staple",
      },
    });
    const cookie = String(reg.headers["set-cookie"]).split(";", 1)[0]!;
    const userId = reg.json().user.user_id as string;
    await billingAccounts.applySubscriptionEvent({
      event_id: randomUUID(),
      user_id: userId,
      provider: "test",
      provider_customer_id: "cus_1",
      provider_subscription_id: "sub_1",
      plan: "pro",
      status: "active",
      occurred_at: "2026-11-25T10:00:00.000Z",
    });
    const blockedCheckout = await billingApp.inject({
      method: "POST",
      url: "/v1/billing/checkout",
      headers: { cookie },
      payload: { plan: "team" },
    });
    assert.equal(blockedCheckout.statusCode, 409);
    assert.equal(blockedCheckout.json().error, "billing_portal_required");

    await billingAccounts.applySubscriptionEvent({
      event_id: randomUUID(),
      user_id: userId,
      provider: "test",
      provider_customer_id: "cus_1",
      provider_subscription_id: "sub_1",
      plan: "pro",
      status: "canceled",
      occurred_at: "2026-11-25T11:00:00.000Z",
    });
    const allowedCheckout = await billingApp.inject({
      method: "POST",
      url: "/v1/billing/checkout",
      headers: { cookie },
      payload: { plan: "pro" },
    });
    assert.equal(allowedCheckout.statusCode, 200);
  } finally {
    await billingApp.close();
  }

  // M6 + M11 + M12: recordDeviceName does not create malformed hello; receipt extra keys rejected; resumePoint parses JSON; commandDigest canonicalizes key order
  const content = new RemoteContentStore(":memory:");
  try {
    const dev = makeDevice();
    content.recordDeviceName("owner-1", dev.deviceId, "Workstation");
    assert.equal(
      content.deviceNames("owner-1").get(dev.deviceId),
      "Workstation",
    );
    assert.equal(content.runnerHello("owner-1", dev.deviceId), null);
    assert.equal(
      content.workspaceStatus(
        "owner-1",
        dev.deviceId,
        "gen-1",
        "ws-demo-" + "a".repeat(16),
      ),
      "runner_not_ready",
    );

    const cmdA = parseRemoteCommand({
      client_cmd_id: "cmd-canon",
      hub_session_id: "hub-1",
      host_generation: "gen-1",
      command: { action: "send_prompt", text: "hi", effort: "high" },
    });
    const cmdB = parseRemoteCommand({
      host_generation: "gen-1",
      hub_session_id: "hub-1",
      client_cmd_id: "cmd-canon",
      command: { effort: "high", text: "hi", action: "send_prompt" },
    });
    assert.equal(commandDigest(cmdA), commandDigest(cmdB));

    assert.equal(
      isWorkspaceAuthorizedInHello(
        {
          discovery: {
            candidates: [null, { workspace_path: "ws-demo-0123456789abcdef" }],
          },
        },
        "ws-demo-0123456789abcdef",
      ),
      true,
    );

    content.submit("owner-1", dev.deviceId, cmdA);
    assert.throws(
      () =>
        content.receive("owner-1", dev.deviceId, 1, {
          type: "receipt",
          extra: "smuggled",
          receipt: {
            client_cmd_id: "cmd-canon",
            hub_session_id: "hub-1",
            status: "accepted",
            code: null,
            result: null,
          },
        }),
      /invalid_receipt/u,
    );
    content.receive("owner-1", dev.deviceId, 1, {
      type: "receipt",
      receipt: {
        client_cmd_id: "cmd-canon",
        hub_session_id: "hub-1",
        status: "accepted",
        code: null,
        result: null,
      },
    });
    assert.equal(content.commands(dev.deviceId, 0).length, 0);

    // Chat text containing literal substring must NOT fool resumePoint
    content.receive("owner-1", dev.deviceId, 2, {
      type: "event",
      event: {
        event_id: "ev-chat",
        host_generation: "gen-1",
        hub_session_id: "hub-1",
        provider_session_id: "native-1",
        kind: "item/agentMessage/delta",
        raw: {
          params: {
            itemId: "m1",
            delta: 'fake {"kind":"history/snapshot"} text',
          },
        },
      },
    });
    assert.equal(
      content.resumePoint("owner-1", dev.deviceId, "hub-1").transcript,
      null,
    );
  } finally {
    content.close();
  }

  // M7 + M13: timezone-aware heartbeat comparison & SqliteGatewayStore event_conflict + atomic binding rollback
  const dir = mkdtempSync(join(tmpdir(), "agenthub-m7-m13-"));
  const gStore = new SqliteGatewayStore(join(dir, "gw.db"), {
    now: () => new Date("2026-11-25T10:00:00.000Z"),
  });
  const memStore = new InMemoryGatewayStore(
    () => new Date("2026-11-25T10:00:00.000Z"),
  );
  try {
    const dev = makeDevice();
    for (const s of [memStore, gStore]) {
      await s.registerDevice("owner-tz", dev.deviceId, dev.publicKey);
      const first = await s.recordHeartbeat(
        signedHeartbeat(dev, "2026-11-25T02:00:05.000Z"),
        "2026-11-25T02:00:05.000Z",
      );
      assert.equal(first.accepted, true);
      // 10:00:01+08:00 === 02:00:01Z, which is EARLIER than 02:00:05Z!
      const olderOffset = await s.recordHeartbeat(
        signedHeartbeat(dev, "2026-11-25T10:00:01+08:00"),
        "2026-11-25T02:00:06.000Z",
      );
      assert.equal(olderOffset.accepted, false);
    }

    // M13: event_conflict code on SQLite
    const ev1 = {
      protocol_version: 1 as const,
      batch_id: randomUUID(),
      device_id: dev.deviceId,
      public_key: dev.publicKey,
      created_at: "2026-11-25T10:00:00.000Z",
      first_sequence: 1,
      last_sequence: 1,
      events: [
        {
          event_id: "55555555-5555-4555-8555-555555555555",
          schema_version: 1,
          device_id: dev.deviceId,
          sequence: 1,
          observed_at: "2026-11-25T10:00:00.000Z",
          agent_type: "codex" as const,
          agent_session_id: "s-1",
          event_type: "session_observed" as const,
          source: "codex_app_server" as const,
          confidence: "authoritative" as const,
          adapter_version: "1.0.0",
          payload: {
            project: { display_name: "p", fingerprint: "a".repeat(64) },
            state: {
              presence: "LIVE" as const,
              activity_status: "WORKING" as const,
              raw_status: "a",
            },
          },
        },
      ],
      signature: "",
    };
    ev1.signature = sign(
      null,
      Buffer.from(unsignedBatchJson(ev1)),
      dev.privateKey,
    ).toString("hex");
    await gStore.ingestBatch(ev1);
    const conflictBatch = structuredClone(ev1);
    conflictBatch.events[0]!.payload.state.raw_status = "different";
    await assert.rejects(() => gStore.ingestBatch(conflictBatch), {
      code: "event_conflict",
    });

    // M13: failed registerDeviceWithBindingCode rolls back claimed_device_id atomically
    const codeHash = bindingCodeDigest("bind-rollback");
    await gStore.createBindingCode(
      "other-owner",
      codeHash,
      "2026-11-25T11:00:00.000Z",
    );
    await assert.rejects(
      () =>
        gStore.registerDeviceWithBindingCode(
          codeHash,
          dev.deviceId,
          dev.publicKey,
        ),
      { code: "device_conflict" },
    );
    const freshDev = makeDevice();
    const ok = await gStore.registerDeviceWithBindingCode(
      codeHash,
      freshDev.deviceId,
      freshDev.publicKey,
    );
    assert.equal(ok.created, true);
  } finally {
    gStore.close();
    rmSync(dir, { recursive: true, force: true });
  }

  // M9 + M10: member denied approval_policy=never; revoked runner denied /hello with 404
  const accounts = new InMemoryAccountStore();
  const devices = new InMemoryGatewayStore();
  const remoteContent = new RemoteContentStore(":memory:");
  const app = createGatewayApp({
    accounts,
    store: devices,
    remoteContent,
    authenticator: new SessionAuthenticator(accounts),
    singleAdmin: false,
  });
  try {
    const ownerRes = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "m9-owner@example.com",
        password: "correct horse battery staple",
      },
    });
    const ownerCookie = String(ownerRes.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;
    const ownerId = ownerRes.json().user.user_id as string;
    accounts.setSubscription(ownerId, { plan: "team", status: "active" });

    const memberRes = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "m9-member@example.com",
        password: "correct horse battery staple",
      },
    });
    const memberCookie = String(memberRes.headers["set-cookie"]).split(
      ";",
      1,
    )[0]!;
    const inv = await app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie: ownerCookie },
      payload: { email: "m9-member@example.com", role: "member" },
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

    const dev = makeDevice();
    await devices.registerDevice(ownerId, dev.deviceId, dev.publicKey);
    remoteContent.receive(ownerId, dev.deviceId, 1, {
      type: "ready",
      host_generation: "gen-1",
      workspaces: ["ws-demo-" + "a".repeat(16)],
      sessions: [],
    });

    const deniedNever = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${dev.deviceId}/commands`,
      headers: { cookie: memberCookie },
      payload: {
        client_cmd_id: "cmd-never",
        hub_session_id: "hub-1",
        host_generation: "gen-1",
        command: {
          action: "connect",
          mode: "create",
          workspace_path: "ws-demo-" + "a".repeat(16),
          approval_policy: "never",
        },
      },
    });
    assert.equal(deniedNever.statusCode, 403);
    assert.equal(deniedNever.json().error, "workspace_permission_denied");

    // M10: CSRF rejection preserves 403 csrf_rejected inside remoteApp
    const csrfBlocked = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${dev.deviceId}/commands`,
      headers: {
        cookie: ownerCookie,
        host: "platform.example.com",
        "sec-fetch-site": "cross-site",
      },
      payload: {},
    });
    assert.equal(csrfBlocked.statusCode, 403);
    assert.equal(csrfBlocked.json().error, "csrf_rejected");

    // M10: revoked device returns 404 device_not_found on /hello even though content exists
    await devices.revokeDevice(ownerId, dev.deviceId);
    const revokedHello = await app.inject({
      method: "GET",
      url: `/v3/remote/runners/${dev.deviceId}/hello`,
      headers: { cookie: ownerCookie },
    });
    assert.equal(revokedHello.statusCode, 404);
    assert.equal(revokedHello.json().error, "device_not_found");
  } finally {
    remoteContent.close();
    await app.close();
  }
});
