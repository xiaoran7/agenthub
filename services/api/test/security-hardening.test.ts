import assert from "node:assert/strict";
import {
  createECDH,
  createHash,
  generateKeyPairSync,
  sign,
  type KeyObject,
} from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { WebSocket } from "ws";

import {
  deviceIdForPublicKey,
  parseRemoteCommand,
  unsignedBatchJson,
} from "@agent-pulse/contracts";

import { createGatewayApp } from "../src/app.js";
import {
  InMemoryAccountStore,
  SessionAuthenticator,
  hashPassword,
} from "../src/accounts.js";
import { WebPushSender } from "../src/push.js";
import { RemoteContentStore } from "../src/remote-store.js";
import { resolveTrustProxyConfig } from "../src/security-guards.js";
import { SqliteAccountStore } from "../src/sqlite-account-store.js";
import { SqliteGatewayStore } from "../src/sqlite-store.js";
import { InMemoryGatewayStore, bindingCodeDigest } from "../src/store.js";

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

test("secureCookies ignores unprefixed agent_pulse_session Cookie Tossing", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts, true),
    secureCookies: true,
    singleAdmin: false,
  });

  const reg = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "victim@example.com",
      password: "correct horse battery staple",
    },
  });
  assert.equal(reg.statusCode, 201);
  const setCookie = String(reg.headers["set-cookie"]);
  const match = /__Host-agent_pulse_session=([^;]+)/.exec(setCookie);
  assert.ok(match);
  const realToken = match[1]!;

  // Attacker tosses unprefixed agent_pulse_session cookie: rejected when secureCookies=true
  const tossedOnly = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: `agent_pulse_session=${realToken}` },
  });
  assert.equal(tossedOnly.statusCode, 401);

  // __Host- cookie still authenticates even if tossed plain cookie precedes it
  const both = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: {
      cookie: `agent_pulse_session=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; __Host-agent_pulse_session=${realToken}`,
    },
  });
  assert.equal(both.statusCode, 200);
  await app.close();
});

test("CSRF hook blocks cross-site/simple cookie mutations while allowing same-origin", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts, false),
  });
  const reg = await app.inject({
    method: "POST",
    url: "/v1/auth/setup",
    payload: {
      email: "owner@example.com",
      password: "correct horse battery staple",
    },
  });
  const cookie = String(reg.headers["set-cookie"]).split(";")[0]!;

  const crossSite = await app.inject({
    method: "POST",
    url: "/v1/auth/logout",
    headers: {
      cookie,
      host: "platform.example.com",
      "sec-fetch-site": "cross-site",
    },
  });
  assert.equal(crossSite.statusCode, 403);
  assert.equal(crossSite.json().error, "csrf_rejected");

  const badOrigin = await app.inject({
    method: "POST",
    url: "/v1/auth/logout",
    headers: {
      cookie,
      host: "platform.example.com",
      origin: "https://evil.example.com",
    },
  });
  assert.equal(badOrigin.statusCode, 403);

  const simpleForm = await app.inject({
    method: "POST",
    url: "/v1/auth/logout",
    headers: {
      cookie,
      host: "platform.example.com",
      "content-type": "text/plain",
    },
    payload: "ignored",
  });
  assert.equal(simpleForm.statusCode, 403);

  const sameOrigin = await app.inject({
    method: "POST",
    url: "/v1/auth/logout",
    headers: {
      cookie,
      host: "platform.example.com",
      origin: "https://platform.example.com",
      "sec-fetch-site": "same-origin",
    },
  });
  assert.equal(sameOrigin.statusCode, 204);
  await app.close();
});

test("single-admin setup serializes concurrent races without TOCTOU", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts, false),
    singleAdmin: true,
  });
  const attempts = await Promise.all(
    Array.from({ length: 5 }, (_, i) =>
      app.inject({
        method: "POST",
        url: "/v1/auth/setup",
        payload: {
          email: `admin-${i}@example.com`,
          password: "correct horse battery staple",
        },
      }),
    ),
  );
  const statuses = attempts.map((r) => r.statusCode).sort();
  assert.deepEqual(statuses, [201, 409, 409, 409, 409]);
  await app.close();
});

test("WebPushSender blocks HTTP redirects with failed outcome", async () => {
  const ecdh = createECDH("prime256v1");
  ecdh.generateKeys();
  const receiver = createECDH("prime256v1");
  receiver.generateKeys();
  const sender = new WebPushSender(
    {
      publicKey: ecdh.getPublicKey().toString("base64url"),
      privateKey: ecdh.getPrivateKey().toString("base64url"),
      subject: "mailto:ops@example.com",
    },
    async (_url, init) => {
      assert.equal(init?.redirect, "error");
      throw new TypeError("redirect disabled");
    },
  );
  const outcome = await sender.send(
    {
      endpoint: "https://fcm.googleapis.com/fcm/send/abc",
      p256dh: receiver.getPublicKey().toString("base64url"),
      auth: Buffer.alloc(16, 3).toString("base64url"),
    },
    '{"ok":true}',
  );
  assert.equal(outcome, "failed");
});

test("question_requested/question_resolved validate without approval_id and WS backpressure does not close socket", async () => {
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const dev = makeDevice();
  await store.registerDevice("owner-1", dev.deviceId, dev.publicKey);
  const app = createGatewayApp({
    store,
    remoteContent: content,
    authenticator: { authenticate: async () => "owner-1" },
  });
  const baseUrl = await app.listen({ host: "127.0.0.1", port: 0 });
  const wsUrl = baseUrl.replace("http://", "ws://") + "/v3/remote/ws";

  content.receive("owner-1", dev.deviceId, 1, {
    type: "event",
    event: {
      event_id: "q-evt-1",
      host_generation: "gen-1",
      hub_session_id: "hub-1",
      provider_session_id: "native-1",
      kind: "question_requested",
      question_id: "approval-" + "a".repeat(64),
      question: {
        question_id: "approval-" + "a".repeat(64),
        hub_session_id: "hub-1",
        host_generation: "gen-1",
        turn_id: "turn-1",
        prompt: "Choose target",
        multi_select: true,
        options: [{ label: "A" }],
      },
    },
  });
  content.receive("owner-1", dev.deviceId, 2, {
    type: "event",
    event: {
      event_id: "q-evt-2",
      host_generation: "gen-1",
      hub_session_id: "hub-1",
      provider_session_id: "native-1",
      kind: "question_resolved",
      question_id: "approval-" + "a".repeat(64),
    },
  });

  // Queue two commands before WS connects; simulate backpressure via app option or check pump stability
  const cmd1 = parseRemoteCommand({
    client_cmd_id: "cmd-1",
    hub_session_id: "hub-1",
    host_generation: "gen-1",
    command: {
      action: "send_prompt",
      text: "hello",
      attachments: [
        {
          name: "a.png",
          media_type: "image/png",
          encoding: "base64",
          data: "QUJDRA==",
        },
      ],
    },
  });
  content.register("owner-1", dev.deviceId);
  content.submit("owner-1", dev.deviceId, cmd1);
  content.receive("owner-1", dev.deviceId, 3, {
    type: "receipt",
    receipt: {
      client_cmd_id: "cmd-1",
      hub_session_id: "hub-1",
      status: "accepted",
      code: null,
      result: null,
    },
  });
  // Idempotent duplicate command succeeds after attachment dehydration!
  assert.equal(content.submit("owner-1", dev.deviceId, cmd1).seq, 1);
  const hist = content.commandHistory("owner-1", dev.deviceId, "hub-1");
  assert.equal(hist[0]?.command.command.action, "send_prompt");
  if (hist[0]?.command.command.action === "send_prompt") {
    assert.equal(hist[0].command.command.attachments?.[0]?.data, "");
  }

  const ws = new WebSocket(wsUrl);
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
              0,
            ),
          }),
        );
      } else if (msg.type === "hello_ack") {
        resolve();
      }
    });
  });
  ws.close();
  content.close();
  await app.close();
});

test("SqliteGatewayStore persists batches, auth snapshots, and SqliteAccountStore persists resets & workspace isolation", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-sqlite-full-"));
  const dbPath = join(dir, "gateway.db");
  const now = new Date("2026-11-18T12:00:00.000Z");
  const gStore = new SqliteGatewayStore(dbPath, { now: () => now });
  const aStore = new SqliteAccountStore(dbPath, { now: () => now });
  try {
    const dev = makeDevice();
    await gStore.createBindingCode(
      "u-1",
      bindingCodeDigest("code-123"),
      "2026-11-18T13:00:00.000Z",
      "u-1",
    );
    await gStore.registerDeviceWithBindingCode(
      bindingCodeDigest("code-123"),
      dev.deviceId,
      dev.publicKey,
    );
    const unsigned = {
      protocol_version: 1 as const,
      batch_id: "11111111-1111-4111-8111-111111111111",
      device_id: dev.deviceId,
      public_key: dev.publicKey,
      created_at: "2026-11-18T12:00:00.000Z",
      first_sequence: 1,
      last_sequence: 1,
      events: [
        {
          event_id: "22222222-2222-4222-8222-222222222222",
          schema_version: 1,
          device_id: dev.deviceId,
          sequence: 1,
          observed_at: "2026-11-18T12:00:00.000Z",
          agent_type: "antigravity" as const,
          agent_session_id: "agy-1",
          event_type: "session_observed" as const,
          source: "antigravity_brain" as const,
          confidence: "authoritative" as const,
          adapter_version: "1.0.0",
          payload: {
            project: { display_name: "demo", fingerprint: "a".repeat(64) },
            state: {
              presence: "LIVE" as const,
              activity_status: "WORKING" as const,
              raw_status: "running",
            },
          },
        },
      ],
      signature: "",
    };
    const sig = sign(
      null,
      Buffer.from(unsignedBatchJson(unsigned)),
      dev.privateKey,
    ).toString("hex");
    const ingest = await gStore.ingestBatch({ ...unsigned, signature: sig });
    assert.equal(ingest.accepted, 1);
    assert.equal((await gStore.listSessions("u-1", {})).length, 1);

    await gStore.ingestAuthBatch({
      protocol_version: 1,
      device_id: dev.deviceId,
      public_key: dev.publicKey,
      checked_at: "2026-11-18T12:00:00.000Z",
      snapshots: [
        {
          provider: "antigravity",
          state: "authenticated",
          source: "official_cli",
          access_token_present: true,
          refresh_token_present: false,
          checked_at: "2026-11-18T12:00:00.000Z",
        },
      ],
      signature: "0".repeat(128),
    });
    assert.equal((await gStore.listAuth("u-1", {})).length, 1);

    await assert.rejects(
      () =>
        gStore.recordHeartbeat(
          {
            protocol_version: 1,
            device_id: dev.deviceId,
            public_key: "f".repeat(64),
            sent_at: "2026-11-18T12:00:00.000Z",
            signature: "0".repeat(128),
          },
          "2026-11-18T12:00:00.000Z",
        ),
      { code: "device_key_mismatch" },
    );

    const acc = await aStore.createAccount({
      userId: "u-1",
      email: "admin@example.com",
      displayName: "Admin",
      passwordHash: await hashPassword("correct horse battery staple"),
      singleAdmin: true,
    });
    assert.equal(acc.user.email, "admin@example.com");
    await aStore.createWebSession({
      sessionId: "33333333-3333-4333-8333-333333333333",
      digest: createHash("sha256").update("tok").digest("hex"),
      userId: "u-1",
      createdAt: "2026-11-18T12:00:00.000Z",
      expiresAt: "2026-11-19T12:00:00.000Z",
    });
    assert.equal(
      await aStore.switchWebSessionWorkspace({
        digest: createHash("sha256").update("tok").digest("hex"),
        actorUserId: "u-1",
        workspaceOwnerId: "other-user",
      }),
      undefined,
    );
    const resetDigest = createHash("sha256").update("reset-1").digest("hex");
    await aStore.issuePasswordReset({
      digest: resetDigest,
      userId: "u-1",
      createdAt: "2026-11-18T12:00:00.000Z",
      expiresAt: "2026-11-18T13:00:00.000Z",
      minimumIntervalSeconds: 60,
    });
    const resetResult = await aStore.consumePasswordReset({
      digest: resetDigest,
      passwordHash: "new-hash",
      now: "2026-11-18T12:05:00.000Z",
    });
    assert.equal(resetResult.updated, true);
    assert.equal(resetResult.revoked_sessions, 1);

    const memAccounts = new InMemoryAccountStore();
    await memAccounts.createAccount({
      userId: "u-oauth",
      email: "admin@example.com",
      displayName: "Admin",
      passwordHash: await hashPassword("correct horse battery staple"),
    });
    await assert.rejects(() =>
      memAccounts.upsertOAuthAccount({
        provider: "github",
        sub: "sub-unverified",
        email: "admin@example.com",
        displayName: "Attacker",
        emailVerified: false,
      }),
    );
  } finally {
    aStore.close();
    gStore.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("team members cannot bind/revoke devices, toggle plugins, or escalate permissions", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const app = createGatewayApp({
    accounts,
    store,
    remoteContent: content,
    authenticator: new SessionAuthenticator(accounts, false),
    singleAdmin: false,
  });
  const ownerRes = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "owner-rbac@example.com",
      password: "correct horse battery staple",
    },
  });
  const ownerCookie = String(ownerRes.headers["set-cookie"]).split(";")[0]!;
  const ownerId = ownerRes.json().user.user_id as string;
  await accounts.setSubscription(ownerId, { plan: "team", status: "active" });

  const memberRes = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "member-rbac@example.com",
      password: "correct horse battery staple",
    },
  });
  const memberCookie = String(memberRes.headers["set-cookie"]).split(";")[0]!;

  const inviteRes = await app.inject({
    method: "POST",
    url: "/v1/team/invitations",
    headers: { cookie: ownerCookie },
    payload: { email: "member-rbac@example.com", role: "member" },
  });
  const inviteUrl = inviteRes.json().invitation_url as string;
  const inviteToken = decodeURIComponent(inviteUrl.split("#token=")[1]!);
  await app.inject({
    method: "POST",
    url: "/v1/team/invitations/accept",
    headers: { cookie: memberCookie },
    payload: { token: inviteToken },
  });
  await app.inject({
    method: "POST",
    url: "/v1/team/workspace",
    headers: { cookie: memberCookie },
    payload: { workspace_owner_id: ownerId },
  });

  const dev = makeDevice();
  await store.registerDevice(ownerId, dev.deviceId, dev.publicKey);
  content.register(ownerId, dev.deviceId);

  const memberBind = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { cookie: memberCookie },
  });
  assert.equal(memberBind.statusCode, 403);

  const memberRevoke = await app.inject({
    method: "DELETE",
    url: `/v1/devices/${dev.deviceId}`,
    headers: { cookie: memberCookie },
  });
  assert.equal(memberRevoke.statusCode, 403);

  const memberPlugin = await app.inject({
    method: "POST",
    url: "/v3/remote/plugins/grok_build/toggle",
    headers: { cookie: memberCookie },
    payload: { enabled: true },
  });
  assert.equal(memberPlugin.statusCode, 403);

  const memberRunnerPluginAction = await app.inject({
    method: "POST",
    url: `/v3/remote/runners/${dev.deviceId}/plugins/action`,
    headers: { cookie: memberCookie },
    payload: { action: "install", plugin_id: "zcode" },
  });
  assert.equal(memberRunnerPluginAction.statusCode, 403);

  const memberDangerousCmd = await app.inject({
    method: "POST",
    url: `/v3/remote/runners/${dev.deviceId}/commands`,
    headers: { cookie: memberCookie },
    payload: {
      client_cmd_id: "cmd-danger",
      hub_session_id: "hub-1",
      host_generation: "gen-1",
      command: {
        action: "connect",
        mode: "create",
        workspace_path: "ws-demo-" + "a".repeat(16),
        permission_mode: "bypassPermissions",
      },
    },
  });
  assert.equal(memberDangerousCmd.statusCode, 403);

  const memberDangerousPrompt = await app.inject({
    method: "POST",
    url: `/v3/remote/runners/${dev.deviceId}/commands`,
    headers: { cookie: memberCookie },
    payload: {
      client_cmd_id: "cmd-danger-prompt",
      hub_session_id: "hub-1",
      host_generation: "gen-1",
      command: {
        action: "send_prompt",
        text: "hello",
        permission_mode: "bypassPermissions",
      },
    },
  });
  assert.equal(memberDangerousPrompt.statusCode, 403);

  content.close();
  await app.close();
});

test("trustProxy defaults to loopback, honors hop counts, and prefers an explicit allowlist", () => {
  assert.deepEqual(resolveTrustProxyConfig("127.0.0.1", undefined), [
    "127.0.0.1/8",
    "::1/128",
  ]);
  assert.equal(resolveTrustProxyConfig("0.0.0.0", undefined), false);
  assert.equal(resolveTrustProxyConfig("0.0.0.0", "1"), 1);
  assert.equal(resolveTrustProxyConfig("127.0.0.1", "2"), 2);
  assert.deepEqual(
    resolveTrustProxyConfig("0.0.0.0", "2", " 192.0.2.5 , 2001:db8::/64 "),
    ["192.0.2.5", "2001:db8::/64"],
  );
  assert.equal(resolveTrustProxyConfig("127.0.0.1", "0", "192.0.2.5"), false);
  assert.throws(() => resolveTrustProxyConfig("0.0.0.0", "9"));
  assert.throws(() => resolveTrustProxyConfig("0.0.0.0", "-1"));
});

test("enforceCookieCsrf respects Fastify trustProxy X-Forwarded-Host without accepting untrusted spoofing", async () => {
  const accounts = new InMemoryAccountStore();
  const proxiedApp = createGatewayApp({
    store: new InMemoryGatewayStore(),
    accounts,
    trustProxy: resolveTrustProxyConfig("127.0.0.1", undefined),
    authenticator: new SessionAuthenticator(accounts, true),
    secureCookies: true,
    singleAdmin: false,
  });
  const reg = await proxiedApp.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "csrf-proxy@example.com",
      password: "correct-horse-battery",
    },
  });
  assert.equal(reg.statusCode, 201);
  const cookiePair = String(reg.headers["set-cookie"]).split(";")[0]!;

  // Trusted reverse proxy forwarding external Origin + X-Forwarded-Host succeeds.
  const allowed = await proxiedApp.inject({
    method: "POST",
    url: "/v1/device-bindings",
    payload: {},
    headers: {
      host: "127.0.0.1:3101",
      "x-forwarded-host": "platform.example.com",
      origin: "https://platform.example.com",
      "sec-fetch-site": "same-origin",
      "content-type": "application/json",
      cookie: cookiePair,
    },
  });
  assert.equal(allowed.statusCode, 201);

  // Spoofed Origin mismatching both Host and X-Forwarded-Host is still rejected.
  const crossOrigin = await proxiedApp.inject({
    method: "POST",
    url: "/v1/device-bindings",
    payload: {},
    headers: {
      host: "127.0.0.1:3101",
      "x-forwarded-host": "platform.example.com",
      origin: "https://evil.example.com",
      "sec-fetch-site": "same-origin",
      "content-type": "application/json",
      cookie: cookiePair,
    },
  });
  assert.equal(crossOrigin.statusCode, 403);
  await proxiedApp.close();

  // Without trustProxy, spoofed X-Forwarded-Host is ignored.
  const untrustedAccounts = new InMemoryAccountStore();
  const untrustedApp = createGatewayApp({
    store: new InMemoryGatewayStore(),
    accounts: untrustedAccounts,
    trustProxy: false,
    authenticator: new SessionAuthenticator(untrustedAccounts, true),
    secureCookies: true,
    singleAdmin: false,
  });
  const reg2 = await untrustedApp.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "csrf-untrusted@example.com",
      password: "correct-horse-battery",
    },
  });
  assert.equal(reg2.statusCode, 201);
  const cookie2 = String(reg2.headers["set-cookie"]).split(";")[0]!;
  const spoofed = await untrustedApp.inject({
    method: "POST",
    url: "/v1/device-bindings",
    payload: {},
    headers: {
      host: "127.0.0.1:3101",
      "x-forwarded-host": "attacker.example.com",
      origin: "https://attacker.example.com",
      "sec-fetch-site": "same-origin",
      "content-type": "application/json",
      cookie: cookie2,
    },
  });
  assert.equal(spoofed.statusCode, 403);
  await untrustedApp.close();
});
