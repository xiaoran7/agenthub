import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import test from "node:test";
import { WebSocket } from "ws";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { InMemoryGatewayStore } from "../src/store.js";
import { RemoteContentStore } from "../src/remote-store.js";

const runner = "dev_" + "b".repeat(32);

/**
 * A `ready` frame carrying everything a Runner can publish.
 *
 * The catalog and the candidates are the two fields the session list no longer
 * carries, so they have to be present here for the tests below to mean
 * anything: a projection asserted against an empty `hello` passes whatever the
 * code does.
 */
const fullReady = {
  type: "ready",
  host_generation: "host-one",
  device_name: "工作本",
  workspaces: ["ws-example-0123456789abcdef"],
  models: {
    data: [{ id: "gpt-5-codex" }],
    agents: {
      codex: [{ id: "gpt-5-codex" }],
      claude_code: [{ id: "claude-sonnet-5-5" }],
      claude_desktop: [{ id: "claude-sonnet-5-5" }],
    },
  },
  discovery: {
    candidates: [
      {
        provider_session_id: "sess-old",
        workspace_path: "ws-example-0123456789abcdef",
        title: "旧会话",
        provider: "codex",
        resumable: true,
      },
    ],
  },
  sessions: [],
  pending_approvals: [],
};

/**
 * Boots a Gateway with one registered device and one connected Runner.
 *
 * The Runner is connected over a real socket rather than through
 * `RemoteContentStore.receive`, because the route under test reads presence
 * (`online`) and the store alone does not make a device look connected.
 */
async function connectedGateway(): Promise<{
  app: Awaited<ReturnType<typeof createGatewayApp>>;
  ws: WebSocket;
  next: (type: string) => Promise<Record<string, unknown>>;
  close: () => Promise<void>;
}> {
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const devices = new InMemoryGatewayStore();
  await devices.registerDevice("owner", runner, publicKey);
  const app = createGatewayApp({
    store: devices,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteContent: new RemoteContentStore(":memory:"),
  });
  const address = await app.listen({ port: 0, host: "127.0.0.1" });
  const ws = new WebSocket(address.replace("http:", "ws:") + "/v3/remote/ws");
  const frames: Record<string, unknown>[] = [];
  ws.on("message", (data) =>
    frames.push(JSON.parse(data.toString()) as Record<string, unknown>),
  );
  const next = async (type: string): Promise<Record<string, unknown>> => {
    for (let attempt = 0; attempt < 500; attempt++) {
      const index = frames.findIndex((frame) => frame.type === type);
      if (index >= 0) return frames.splice(index, 1)[0]!;
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
    assert.fail(`missing ${type}`);
  };
  const challenge = await next("challenge");
  const signed = Buffer.from(
    `agenthub.remote.v3\n${runner}\n${String(challenge.nonce)}\nhost-one\n0`,
  );
  ws.send(
    JSON.stringify({
      type: "hello",
      runner_id: runner,
      host_generation: "host-one",
      received_command_seq: 0,
      signature: sign(null, signed, keys.privateKey).toString("hex"),
    }),
  );
  assert.equal((await next("hello_ack")).version, 3);
  ws.send(JSON.stringify({ type: "data", seq: 1, payload: fullReady }));
  assert.equal((await next("ack")).seq, 1);
  return {
    app,
    ws,
    next,
    close: async () => {
      ws.terminate();
      await app.close();
    },
  };
}

const ownerHeaders = { "x-user-id": "owner" };

test("the session list carries the fields it renders and not the catalog or the candidates", async (t) => {
  const gateway = await connectedGateway();
  t.after(() => gateway.close());
  try {
    const response = await gateway.app.inject({
      url: "/v3/remote/sessions",
      headers: ownerHeaders,
    });
    assert.equal(response.statusCode, 200);
    const runner0 = response.json().runners[0];

    // Present: what the list itself shows.
    assert.equal(runner0.runner_id, runner);
    assert.equal(runner0.online, true);
    assert.equal(runner0.hello.device_name, "工作本");
    assert.deepEqual(runner0.hello.workspaces, ["ws-example-0123456789abcdef"]);
    // `host_generation` is not decoration: every command the console sends is
    // tagged with it, and a Host that no longer matches rejects the command.
    assert.equal(runner0.hello.host_generation, "host-one");
    // `pending_approvals` is read per sidebar row, so it has to survive.
    assert.ok(Array.isArray(runner0.hello.pending_approvals));

    // Absent: the two fields that made this response half a megabyte.
    assert.equal(runner0.hello.models, undefined);
    assert.equal(runner0.hello.discovery, undefined);
  } finally {
    await gateway.close();
  }
});

test("the per-device route returns the full catalog and candidates the list dropped", async (t) => {
  const gateway = await connectedGateway();
  t.after(() => gateway.close());
  try {
    const response = await gateway.app.inject({
      url: `/v3/remote/runners/${runner}/hello`,
      headers: ownerHeaders,
    });
    assert.equal(response.statusCode, 200);
    const body = response.json();

    assert.equal(body.runner_id, runner);
    assert.equal(body.online, true);
    // Both halves are back, byte for byte as the Runner published them.
    assert.deepEqual(body.hello.models, fullReady.models);
    assert.deepEqual(body.hello.discovery, fullReady.discovery);
    // And the fields the list already carried are still here, so a caller can
    // read one response instead of merging two.
    assert.equal(body.hello.device_name, "工作本");
    assert.equal(body.hello.host_generation, "host-one");
  } finally {
    await gateway.close();
  }
});

test("the per-device route refuses another owner and an unknown device", async (t) => {
  const gateway = await connectedGateway();
  t.after(() => gateway.close());
  try {
    // Another account's scope: a runner id is not a capability.
    assert.equal(
      (
        await gateway.app.inject({
          url: `/v3/remote/runners/${runner}/hello`,
          headers: { "x-user-id": "other" },
        })
      ).statusCode,
      404,
    );
    // A device that does not exist at all.
    assert.equal(
      (
        await gateway.app.inject({
          url: "/v3/remote/runners/dev_" + "c".repeat(32) + "/hello",
          headers: ownerHeaders,
        })
      ).statusCode,
      404,
    );
    // No identity at all.
    assert.equal(
      (
        await gateway.app.inject({
          url: `/v3/remote/runners/${runner}/hello`,
        })
      ).statusCode,
      401,
    );
  } finally {
    await gateway.close();
  }
});

test("a registered device that has published nothing answers not-ready rather than an empty catalog", async (t) => {
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const devices = new InMemoryGatewayStore();
  // Registered but never connected, so there is no `hello` row at all. This is
  // the device the console shows with an empty workspace list, and asking for
  // its catalog has to be distinguishable from asking about a stranger's.
  await devices.registerDevice("owner", runner, publicKey);
  const app = createGatewayApp({
    store: devices,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteContent: new RemoteContentStore(":memory:"),
  });
  t.after(() => app.close());
  try {
    const response = await app.inject({
      url: `/v3/remote/runners/${runner}/hello`,
      headers: ownerHeaders,
    });
    // Not a 404: the device is this owner's. Not a 200 with an empty `hello`
    // either, because an empty object would read as "this Host has no models"
    // when the truth is "this Host has not said anything yet".
    assert.notEqual(response.statusCode, 404);
    assert.notEqual(response.statusCode, 200);
  } finally {
    await app.close();
  }
});
test("the session list tags active sessions with their authoritative discovery provider while keeping hello thin", async (t) => {
  const gateway = await connectedGateway();
  t.after(() => gateway.close());
  try {
    gateway.ws.send(
      JSON.stringify({
        type: "data",
        seq: 2,
        payload: {
          ...fullReady,
          sessions: [
            {
              hub_session_id: "hub-desktop-1",
              provider_session_id: "claude_desktop_abc",
              workspace_path: "ws-example-0123456789abcdef",
              host_generation: "host-one",
              control_connected: true,
              execution_state: "idle",
              current_turn_id: null,
            },
          ],
          discovery: {
            candidates: [
              {
                provider_session_id: "claude_desktop_abc",
                workspace_path: "ws-example-0123456789abcdef",
                title: "Desktop session",
                provider: "claude_desktop",
                resumable: true,
              },
            ],
          },
        },
      }),
    );
    assert.equal((await gateway.next("ack")).seq, 2);

    const response = await gateway.app.inject({
      url: "/v3/remote/sessions",
      headers: ownerHeaders,
    });
    assert.equal(response.statusCode, 200);
    const body = response.json();
    assert.equal(body.runners[0].hello.discovery, undefined);
    assert.equal(body.runners[0].hello.models, undefined);
    assert.equal(body.sessions.length, 1);
    assert.equal(body.sessions[0].provider, "claude_desktop");
  } finally {
    await gateway.close();
  }
});
