import assert from "node:assert/strict";
import { generateKeyPairSync, sign } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { WebSocket } from "ws";
import { RemoteContentStore } from "../src/remote-store.js";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { InMemoryGatewayStore } from "../src/store.js";
import type { RemoteCommand } from "@agent-pulse/contracts";

const runner = "dev_" + "a".repeat(32);

test("retired v2 prototype routes are absent from the default Gateway", async () => {
  const app = createGatewayApp();
  try {
    for (const url of ["/v2/sessions", "/v2/capabilities", "/v2/ws"])
      assert.equal((await app.inject({ method: "GET", url })).statusCode, 404);
    assert.equal(
      (
        await app.inject({
          method: "POST",
          url: "/v2/capabilities",
          payload: {},
        })
      ).statusCode,
      404,
    );
  } finally {
    await app.close();
  }
});
const binding = {
  hub_session_id: "hub-one",
  provider_session_id: "native-one",
  workspace_path: "D:/example",
  host_generation: "host-one",
  control_connected: true,
  execution_state: "idle",
  current_turn_id: null,
};
const command: RemoteCommand = {
  client_cmd_id: "cmd-one",
  hub_session_id: "hub-one",
  host_generation: null,
  command: { action: "connect", mode: "create", workspace_path: "D:/example" },
};
const receipt = {
  client_cmd_id: "cmd-one",
  hub_session_id: "hub-one",
  status: "accepted",
  code: null,
  result: { binding },
};

test("remote content, exact duplicate detection, and commands persist across Gateway restart", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-remote-"));
  const path = join(dir, "remote.db");
  try {
    let store = new RemoteContentStore(path);
    store.register("owner", runner);
    assert.equal(store.submit("owner", runner, command).seq, 1);
    assert.equal(store.submit("owner", runner, command).seq, 1);
    assert.throws(
      () =>
        store.submit("owner", runner, {
          ...command,
          hub_session_id: "conflict",
        }),
      /command_id_conflict/u,
    );
    store.receive("owner", runner, 1, { type: "receipt", receipt });
    store.receive("owner", runner, 1, { type: "receipt", receipt });
    assert.throws(
      () => store.receive("owner", runner, 1, { type: "host_lost" }),
      /sequence_conflict/u,
    );
    assert.throws(
      () => store.receive("owner", runner, 3, { type: "host_lost" }),
      /sequence_gap/u,
    );
    assert.equal(store.events("owner", 0).length, 1);
    assert.equal(store.sessions("other").length, 0);
    store.close();
    store = new RemoteContentStore(path);
    assert.equal(
      store.sessions("owner")[0]?.binding.provider_session_id,
      "native-one",
    );
    assert.equal(store.watermarks(runner).received, 1);
    assert.deepEqual(
      store.commandHistory("owner", runner, "hub-one")[0]?.receipt,
      receipt,
    );
    store.close();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("device challenge, owner scoping, command receipts, and revocation over real WS", async (t) => {
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const devices = new InMemoryGatewayStore();
  await devices.registerDevice("owner", runner, publicKey);
  const content = new RemoteContentStore(":memory:");
  const app = createGatewayApp({
    store: devices,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteContent: content,
  });
  const address = await app.listen({ port: 0, host: "127.0.0.1" });
  const ws = new WebSocket(address.replace("http:", "ws:") + "/v3/remote/ws");
  t.after(async () => {
    ws.terminate();
    await app.close();
  });
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
  const url = `/v3/remote/runners/${runner}/commands`;
  assert.equal(
    (
      await app.inject({
        method: "POST",
        url,
        headers: { "x-user-id": "other" },
        payload: command,
      })
    ).statusCode,
    404,
  );
  assert.equal(
    (
      await app.inject({
        method: "POST",
        url,
        headers: { "x-user-id": "owner" },
        payload: command,
      })
    ).statusCode,
    202,
  );
  assert.deepEqual((await next("command")).command, command);
  ws.send(
    JSON.stringify({
      type: "data",
      seq: 1,
      payload: { type: "receipt", receipt },
    }),
  );
  assert.equal((await next("ack")).seq, 1);
  const largeOutput = "x".repeat(2 * 1024 * 1024);
  ws.send(
    JSON.stringify({
      type: "data",
      seq: 2,
      payload: {
        type: "event",
        event: {
          hub_session_id: "hub-one",
          kind: "item/completed",
          raw: {
            params: {
              item: {
                id: "large-tool",
                type: "commandExecution",
                aggregatedOutput: largeOutput,
              },
            },
          },
        },
      },
    }),
  );
  assert.equal((await next("ack")).seq, 2);
  assert.equal(
    JSON.stringify(
      (
        await app.inject({
          url: "/v3/remote/events",
          headers: { "x-user-id": "owner" },
        })
      ).json(),
    ).includes(largeOutput),
    true,
  );
  const own = await app.inject({
    url: "/v3/remote/sessions",
    headers: { "x-user-id": "owner" },
  });
  assert.equal(
    own.json().sessions[0].binding.provider_session_id,
    "native-one",
  );
  const other = await app.inject({
    url: "/v3/remote/events",
    headers: { "x-user-id": "other" },
  });
  assert.equal(other.json().events.length, 0);
  assert.equal(
    (await app.inject({ url: "/v3/remote/sessions" })).statusCode,
    401,
  );
  await devices.revokeDevice("owner", runner);
  ws.send(JSON.stringify({ type: "heartbeat" }));
  assert.equal((await next("error")).code, "device_not_found");
});

test("native identity cannot silently change on the same Hub binding", () => {
  const store = new RemoteContentStore(":memory:");
  try {
    store.submit("owner", runner, command);
    store.receive("owner", runner, 1, { type: "receipt", receipt });
    assert.throws(
      () =>
        store.receive("owner", runner, 2, {
          type: "event",
          event: {
            hub_session_id: "hub-one",
            binding: { ...binding, provider_session_id: "different" },
          },
        }),
      /native_identity_changed/u,
    );
    assert.equal(store.watermarks(runner).received, 1);
  } finally {
    store.close();
  }
});

test("an accepted command retry is acknowledged while Runner is offline, without adding work", async () => {
  const devices = new InMemoryGatewayStore();
  await devices.registerDevice("owner", runner, "a".repeat(64));
  const content = new RemoteContentStore(":memory:");
  content.register("owner", runner);
  content.submit("owner", runner, command);
  const app = createGatewayApp({
    store: devices,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteContent: content,
  });
  try {
    const response = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: command,
    });
    assert.equal(response.statusCode, 202);
    assert.equal(response.json().seq, 1);
    assert.equal(content.watermarks(runner).command_seq, 1);
    const next = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: { ...command, client_cmd_id: "new" },
    });
    assert.equal(next.json().error, "runner_offline");
  } finally {
    await app.close();
  }
});
