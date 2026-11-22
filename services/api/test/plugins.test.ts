import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { WebSocket } from "ws";
import { createGatewayApp } from "../src/app.js";
import { BearerTokenAuthenticator } from "../src/auth.js";
import { InMemoryGatewayStore } from "../src/store.js";
import { RemoteContentStore } from "../src/remote-store.js";
import type { RemotePluginsResponse } from "@agent-pulse/contracts";

test("Runner-authoritative plugins API: ready sync, intermediate downloading state, action relay, and toggle", async () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-plugins-"));
  const database = join(dir, "gateway.db");
  const devices = new InMemoryGatewayStore();
  const remoteContent = new RemoteContentStore(database);
  const token = "a".repeat(32);
  const owner = "test-owner-plugins";
  const runnerId = "dev_" + "c".repeat(32);
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  await devices.registerDevice(owner, runnerId, publicKey);

  const app = createGatewayApp({
    store: devices,
    authenticator: new BearerTokenAuthenticator(owner, token),
    remoteContent,
  });

  let ws: WebSocket | undefined;
  try {
    const address = await app.listen({ port: 0, host: "127.0.0.1" });
    const authHeaders = { authorization: `Bearer ${token}` };

    // 1. Before Runner connects: zero hardcoded plugins exist on Gateway.
    const initialRes = await app.inject({
      method: "GET",
      url: "/v3/remote/plugins",
      headers: authHeaders,
    });
    assert.equal(initialRes.statusCode, 200);
    const initialBody = JSON.parse(initialRes.body) as RemotePluginsResponse;
    assert.deepEqual(initialBody.plugins, []);
    assert.deepEqual(initialBody.runners, []);

    // 2. Offline Runner returns 503 runner_offline.
    const offlineAction = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runnerId}/plugins/action`,
      headers: authHeaders,
      payload: { action: "refresh_catalog" },
    });
    assert.equal(offlineAction.statusCode, 503);

    // 3. Connect Runner with plugins_v2 capability and send ready.plugins.
    ws = new WebSocket(address.replace("http:", "ws:") + "/v3/remote/ws");
    const frames: Record<string, unknown>[] = [];
    ws.on("message", (data) => {
      frames.push(JSON.parse(data.toString()) as Record<string, unknown>);
    });
    const nextFrame = async (
      type: string,
    ): Promise<Record<string, unknown>> => {
      for (let i = 0; i < 400; i++) {
        const idx = frames.findIndex((f) => f.type === type);
        if (idx >= 0) return frames.splice(idx, 1)[0]!;
        await new Promise((r) => setTimeout(r, 10));
      }
      assert.fail(`missing frame ${type}`);
    };

    const challenge = await nextFrame("challenge");
    const signed = Buffer.from(
      `agenthub.remote.v3\n${runnerId}\n${String(challenge.nonce)}\ngen-1\n0`,
    );
    ws.send(
      JSON.stringify({
        type: "hello",
        runner_id: runnerId,
        host_generation: "gen-1",
        received_command_seq: 0,
        capabilities: ["plugins", "plugins_v2"],
        signature: sign(null, signed, keys.privateKey).toString("hex"),
      }),
    );
    assert.equal((await nextFrame("hello_ack")).version, 3);
    // Gateway must never push legacy plugins_sync when Runner is authoritative.
    await new Promise((r) => setTimeout(r, 40));
    assert.equal(
      frames.some((f) => f.type === "plugins_sync"),
      false,
    );

    const zcodePlugin = {
      id: "zcode",
      name: "ZCode",
      displayName: "ZCode",
      category: "agent",
      description: "ACP ZCode adapter",
      version: "1.0.0",
      installedVersion: "1.0.0",
      installed: true,
      enabled: true,
      runtimeStatus: "ready",
      available: true,
      updatedAt: "2026-11-20T00:00:00Z",
    };
    const grokCatalog = {
      id: "grok_build",
      name: "Grok",
      displayName: "Grok",
      category: "agent",
      description: "ACP Grok adapter",
      version: "1.0.0",
      installed: false,
      enabled: false,
      runtimeStatus: "not_installed",
      updatedAt: "2026-11-20T00:00:00Z",
    };
    ws.send(
      JSON.stringify({
        type: "data",
        seq: 1,
        payload: {
          type: "ready",
          host_generation: "gen-1",
          workspaces: ["ws-demo-" + "a".repeat(16)],
          sessions: [],
          plugins: [zcodePlugin, grokCatalog],
        },
      }),
    );
    await nextFrame("ack");

    const listRes = await app.inject({
      method: "GET",
      url: `/v3/remote/plugins?runner=${runnerId}`,
      headers: authHeaders,
    });
    assert.equal(listRes.statusCode, 200);
    const listData = JSON.parse(listRes.body) as RemotePluginsResponse;
    assert.equal(listData.plugins.length, 2);
    assert.equal(
      listData.plugins.find((p) => p.id === "grok_build")?.runtimeStatus,
      "not_installed",
    );
    assert.equal(listData.runners.length, 1);
    assert.equal(listData.runners[0]!.runner, runnerId);

    // 4. Relay install action -> intermediate downloading frame -> terminal ready frame.
    let installSettled = false;
    const installPromise = app
      .inject({
        method: "POST",
        url: `/v3/remote/runners/${runnerId}/plugins/action`,
        headers: authHeaders,
        payload: { action: "install", plugin_id: "grok_build" },
      })
      .then((res) => {
        installSettled = true;
        return res;
      });

    const cmd1 = await nextFrame("plugin_command");
    assert.equal(cmd1.action, "install");
    assert.equal(cmd1.plugin_id, "grok_build");
    assert.equal(typeof cmd1.request_id, "string");

    ws.send(
      JSON.stringify({
        type: "plugin_state",
        request_id: cmd1.request_id,
        registry_url: "https://example.com/registry.json",
        plugins: [
          zcodePlugin,
          { ...grokCatalog, runtimeStatus: "downloading" },
        ],
      }),
    );
    await new Promise((r) => setTimeout(r, 50));
    assert.equal(
      installSettled,
      false,
      "intermediate downloading frame must not settle HTTP request",
    );

    ws.send(
      JSON.stringify({
        type: "plugin_state",
        request_id: cmd1.request_id,
        registry_url: "https://example.com/registry.json",
        ok: true,
        plugins: [
          zcodePlugin,
          {
            ...grokCatalog,
            installed: true,
            enabled: true,
            installedVersion: "1.0.0",
            runtimeStatus: "ready",
            available: true,
          },
        ],
      }),
    );

    const installRes = await installPromise;
    assert.equal(installRes.statusCode, 200);
    const installBody = JSON.parse(installRes.body) as {
      ok: boolean;
      plugins: Array<{ id: string; installed: boolean; enabled: boolean }>;
    };
    assert.equal(installBody.ok, true);
    assert.equal(
      installBody.plugins.find((p) => p.id === "grok_build")?.installed,
      true,
    );

    // 5. Legacy /v3/remote/plugins/:id/toggle relays via plugin_command.
    const togglePromise = app.inject({
      method: "POST",
      url: "/v3/remote/plugins/grok_build/toggle",
      headers: authHeaders,
      payload: { enabled: false },
    });
    const cmd2 = await nextFrame("plugin_command");
    assert.equal(cmd2.action, "toggle");
    assert.equal(cmd2.plugin_id, "grok_build");
    assert.equal(cmd2.enabled, false);
    ws.send(
      JSON.stringify({
        type: "plugin_state",
        request_id: cmd2.request_id,
        ok: true,
        plugins: [
          zcodePlugin,
          {
            ...grokCatalog,
            installed: true,
            enabled: false,
            runtimeStatus: "disabled",
          },
        ],
      }),
    );
    const toggleRes = await togglePromise;
    assert.equal(toggleRes.statusCode, 200);
    const toggleBody = JSON.parse(toggleRes.body) as {
      plugin: { id: string; enabled: boolean; runtimeStatus: string };
    };
    assert.equal(toggleBody.plugin.id, "grok_build");
    assert.equal(toggleBody.plugin.enabled, false);
    assert.equal(toggleBody.plugin.runtimeStatus, "disabled");

    // 6. Security validations on plugin actions & legacy toggle.
    const srcRes = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runnerId}/plugins/action`,
      headers: authHeaders,
      payload: {
        action: "install",
        plugin_id: "grok_build",
        source_url: "http://127.0.0.1:9000/evil",
      },
    });
    assert.equal(srcRes.statusCode, 400);
    assert.equal(
      (JSON.parse(srcRes.body) as { error: string }).error,
      "remote_source_url_unsupported",
    );

    const ssrfRes = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runnerId}/plugins/action`,
      headers: authHeaders,
      payload: {
        action: "set_registry",
        registry_url: "https://169.254.169.254/registry.json",
      },
    });
    assert.equal(ssrfRes.statusCode, 400);
    assert.equal(
      (JSON.parse(ssrfRes.body) as { error: string }).error,
      "invalid_plugin_registry_url",
    );

    const badToggle = await app.inject({
      method: "POST",
      url: "/v3/remote/plugins/BAD_ID!/toggle",
      headers: authHeaders,
      payload: { enabled: true },
    });
    assert.equal(badToggle.statusCode, 400);

    const foreignRunnerToggle = await app.inject({
      method: "POST",
      url: "/v3/remote/plugins/grok_build/toggle",
      headers: authHeaders,
      payload: { enabled: true, runner: "dev_nonexistent" },
    });
    assert.equal(foreignRunnerToggle.statusCode, 404);

    // 7. plugin_state pushes an `event: plugins` wakeup on /v3/remote/stream.
    const sseAbort = new AbortController();
    const sseRes = await fetch(`${address}/v3/remote/stream`, {
      headers: authHeaders,
      signal: sseAbort.signal,
    });
    assert.equal(sseRes.status, 200);
    const reader = sseRes.body!.getReader();
    const dec = new TextDecoder();
    let sseBuf = "";
    ws.send(
      JSON.stringify({
        type: "plugin_state",
        registry_url: "https://example.com/registry.json",
        plugins: [zcodePlugin],
      }),
    );
    const deadline = Date.now() + 2000;
    while (Date.now() < deadline && !sseBuf.includes("event: plugins")) {
      const chunk = await reader.read();
      if (chunk.done) break;
      sseBuf += dec.decode(chunk.value, { stream: true });
    }
    sseAbort.abort();
    assert.ok(
      sseBuf.includes("event: plugins") && sseBuf.includes(runnerId),
      `expected event: plugins for ${runnerId}, got: ${sseBuf}`,
    );

    // 8. Expanded / mapped IPv6 SSRF hosts must all be rejected with 400.
    for (const badHostUrl of [
      "https://[::ffff:127.0.0.1]/registry.json",
      "https://[::ffff:169.254.169.254]/registry.json",
      "https://[0:0:0:0:0:0:0:1]/registry.json",
      "https://[feb0::1]/registry.json",
      "https://[ff02::1]/registry.json",
    ]) {
      const badV6 = await app.inject({
        method: "POST",
        url: `/v3/remote/runners/${runnerId}/plugins/action`,
        headers: authHeaders,
        payload: { action: "set_registry", registry_url: badHostUrl },
      });
      assert.equal(badV6.statusCode, 400, `expected 400 for ${badHostUrl}`);
      assert.equal(
        (JSON.parse(badV6.body) as { error: string }).error,
        "invalid_plugin_registry_url",
      );
    }

    // 9. sessionPrefixes passes cleanly through plugin_state and GET /v3/remote/plugins.
    ws.send(
      JSON.stringify({
        type: "plugin_state",
        registry_url: "HTTPS://Example.COM/registry.json",
        plugins: [
          {
            ...zcodePlugin,
            sessionPrefix: "zcode_",
            sessionPrefixes: ["zcode_", "sess_"],
          },
        ],
      }),
    );
    let prefixBody: RemotePluginsResponse | undefined;
    const pollUntil = Date.now() + 2000;
    while (Date.now() < pollUntil) {
      const r = await app.inject({
        method: "GET",
        url: "/v3/remote/plugins",
        headers: authHeaders,
      });
      const parsed = JSON.parse(r.body) as RemotePluginsResponse;
      if (
        parsed.plugins
          .find((p) => p.id === "zcode")
          ?.sessionPrefixes?.includes("sess_")
      ) {
        prefixBody = parsed;
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    assert.deepEqual(
      prefixBody?.plugins.find((p) => p.id === "zcode")?.sessionPrefixes,
      ["zcode_", "sess_"],
    );

    // 10. Revoking a runner removes its plugin state and hides stale rows.
    const revokeRes = await app.inject({
      method: "DELETE",
      url: `/v1/devices/${runnerId}`,
      headers: authHeaders,
    });
    assert.equal(revokeRes.statusCode, 204);
    const afterRevoke = await app.inject({
      method: "GET",
      url: "/v3/remote/plugins",
      headers: authHeaders,
    });
    assert.equal(afterRevoke.statusCode, 200);
    assert.deepEqual(JSON.parse(afterRevoke.body), {
      plugins: [],
      runners: [],
    });

    // Even if a stale cache row reappears for the revoked runner, active runner filtering drops it.
    remoteContent.saveRunnerPlugins(owner, runnerId, undefined, [
      zcodePlugin as never,
    ]);
    const stillHidden = await app.inject({
      method: "GET",
      url: "/v3/remote/plugins",
      headers: authHeaders,
    });
    assert.deepEqual(JSON.parse(stillHidden.body), {
      plugins: [],
      runners: [],
    });
  } finally {
    ws?.close();
    await app.close();
    remoteContent.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
