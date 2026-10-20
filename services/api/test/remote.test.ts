import assert from "node:assert/strict";
import { generateKeyPairSync, sign, type KeyObject } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { WebSocket } from "ws";
import { RemoteContentStore } from "../src/remote-store.js";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { InMemoryAccountStore, SessionAuthenticator } from "../src/accounts.js";
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
  workspace_path: "ws-example-0123456789abcdef",
  host_generation: "host-one",
  control_connected: true,
  execution_state: "idle",
  current_turn_id: null,
};
const approval = {
  approval_id: `approval-${"a".repeat(64)}`,
  hub_session_id: "hub-one",
  host_generation: "host-one",
  turn_id: "turn-one",
  can_approve: true,
  can_decline: true,
};
const ready = (pending_approvals: unknown[]) => ({
  type: "ready",
  host_generation: "host-one",
  workspaces: [binding.workspace_path],
  sessions: [binding],
  pending_approvals,
});

const hostEvent = (kind: string, fields: Record<string, unknown> = {}) => ({
  event_id: `event-${kind.replaceAll("/", "-")}`,
  host_generation: "host-one",
  hub_session_id: "hub-one",
  provider_session_id: "native-one",
  kind,
  binding: null,
  approval_id: null,
  ...fields,
});

const command: RemoteCommand = {
  client_cmd_id: "cmd-one",
  hub_session_id: "hub-one",
  host_generation: "host-one",
  command: {
    action: "connect",
    mode: "create",
    workspace_path: "ws-example-0123456789abcdef",
  },
};
const receipt = {
  client_cmd_id: "cmd-one",
  hub_session_id: "hub-one",
  status: "accepted",
  code: null,
  result: { binding },
};

async function connectRunner(
  address: string,
  privateKey: KeyObject,
  hostGeneration: string,
): Promise<{
  ws: WebSocket;
  next: (type: string) => Promise<Record<string, unknown>>;
}> {
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
    `agenthub.remote.v3\n${runner}\n${String(challenge.nonce)}\n${hostGeneration}\n0`,
  );
  ws.send(
    JSON.stringify({
      type: "hello",
      runner_id: runner,
      host_generation: hostGeneration,
      received_command_seq: 0,
      signature: sign(null, signed, privateKey).toString("hex"),
    }),
  );
  assert.equal((await next("hello_ack")).version, 3);
  return { ws, next };
}

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

test("unsafe provider receipt fields are rejected before persistence", () => {
  const store = new RemoteContentStore(":memory:");
  try {
    const unsafeCommand = { ...command, client_cmd_id: "cmd-unsafe" };
    const unsafeReceipt = {
      ...receipt,
      client_cmd_id: unsafeCommand.client_cmd_id,
      result: {
        ...receipt.result,
        provider_response: {
          thread: { id: "native-one", cwd: "C:/Users/alice/private" },
          access_token: "usable-secret",
        },
      },
    };
    store.submit("owner", runner, unsafeCommand);
    assert.throws(
      () =>
        store.receive("owner", runner, 1, {
          type: "receipt",
          receipt: unsafeReceipt,
        }),
      /invalid_receipt/u,
    );
    assert.equal(store.watermarks(runner).received, 0);
    assert.equal(
      store.commandHistory("owner", runner, "hub-one")[0]?.receipt,
      null,
    );
  } finally {
    store.close();
  }
});

test("approval snapshots replace state and unsafe approval events never persist", () => {
  const store = new RemoteContentStore(":memory:");
  try {
    store.register("owner", runner);
    assert.throws(
      () => store.receive("owner", runner, 1, { type: "receipt", receipt }),
      /unknown_command_receipt/u,
    );

    store.receive("owner", runner, 1, ready([approval]));
    assert.deepEqual(store.runners("owner")[0]?.hello?.pending_approvals, [
      approval,
    ]);
    assert.deepEqual(
      store
        .events("owner", 0, runner, "hub-one")
        .map((item) => item.payload.type),
      ["ready"],
    );

    store.receive("owner", runner, 2, {
      type: "event",
      event: hostEvent("approval_requested", {
        event_id: "event-one",
        approval_id: approval.approval_id,
        approval,
        binding,
      }),
    });
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: hostEvent("approval_requested", {
            approval_id: approval.approval_id,
            approval,
            raw: { id: 7, params: { command: "private" } },
          }),
        }),
      /unsafe_approval_event/u,
    );
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: hostEvent("provider_request", {
            raw: { id: "provider-secret", params: {} },
          }),
        }),
      /unsafe_provider_event/u,
    );
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: hostEvent("item/completed", {
            raw: {
              params: {
                item: {
                  id: "tool-one",
                  type: "commandExecution",
                  command: "cat C:/Users/alice/.env",
                  aggregatedOutput: "usable-secret",
                },
              },
            },
          }),
        }),
      /unsafe_provider_event/u,
    );
    // A started tool row may carry the command it is about to run, but only
    // the render fields the console understands.
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: hostEvent("item/started", {
            raw: {
              params: {
                item: {
                  id: "tool-two",
                  type: "commandExecution",
                  input: { secret: "usable-secret" },
                },
              },
            },
          }),
        }),
      /unsafe_provider_event/u,
    );
    // Context capacity is two integers; provider bookkeeping around them is
    // not something the console renders, so it never reaches storage.
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: hostEvent("thread/tokenUsage/updated", {
            raw: {
              params: {
                tokenUsage: {
                  used: 42000,
                  window: 200000,
                  internalBreakdown: "usable-secret",
                },
              },
            },
          }),
        }),
      /unsafe_provider_event/u,
    );
    for (const extra of [
      { command: "private command" },
      { diff: "private diff" },
      { request_id: 7 },
    ]) {
      assert.throws(
        () =>
          store.receive("owner", runner, 3, {
            type: "event",
            event: {
              event_id: "event-unsafe",
              host_generation: "host-one",
              hub_session_id: "hub-one",
              provider_session_id: "native-one",
              kind: "approval_requested",
              approval_id: approval.approval_id,
              approval,
              binding: null,
              ...extra,
            },
          }),
        /invalid_event/u,
      );
    }
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: {
            event_id: "event-mismatch",
            host_generation: "host-other",
            hub_session_id: "hub-one",
            provider_session_id: "native-one",
            kind: "approval_requested",
            approval_id: approval.approval_id,
            approval,
            binding: null,
          },
        }),
      /invalid_approval_event/u,
    );
    assert.throws(
      () =>
        store.receive("owner", runner, 3, {
          type: "event",
          event: {
            event_id: "event-envelope",
            host_generation: "host-one",
            hub_session_id: "hub-one",
            provider_session_id: "native-one",
            kind: "approval_requested",
            approval_id: approval.approval_id,
            approval,
            binding: null,
          },
          provider_response: { token: "secret" },
        }),
      /invalid_event/u,
    );
    assert.equal(store.watermarks(runner).received, 2);

    store.receive("owner", runner, 3, ready([]));
    assert.deepEqual(store.runners("owner")[0]?.hello?.pending_approvals, []);
    assert.deepEqual(
      store
        .events("owner", 0, runner, "hub-one")
        .map((item) => item.payload.type),
      ["ready", "event", "ready"],
    );
  } finally {
    store.close();
  }
});

test("ready rejects malformed or duplicate workspace identifiers atomically", () => {
  const store = new RemoteContentStore(":memory:");
  try {
    for (const workspaces of [
      [7],
      ["D:/private"],
      ["ws-example-short"],
      [binding.workspace_path, binding.workspace_path],
    ]) {
      assert.throws(
        () => store.receive("owner", runner, 1, { ...ready([]), workspaces }),
        /invalid_ready/u,
      );
      assert.equal(store.watermarks(runner).received, 0);
    }
    store.receive("owner", runner, 1, ready([]));
    assert.equal(store.watermarks(runner).received, 1);
  } finally {
    store.close();
  }
});

test("ready snapshots replace current bindings and stale generations cannot overwrite them", () => {
  const store = new RemoteContentStore(":memory:");
  const second = {
    ...binding,
    hub_session_id: "hub-two",
    provider_session_id: "native-two",
  };
  try {
    store.receive(
      "owner",
      runner,
      1,
      { ...ready([]), sessions: [binding, second] },
      "host-one",
    );
    store.receive(
      "owner",
      runner,
      2,
      { ...ready([]), sessions: [second] },
      "host-one",
    );
    assert.deepEqual(store.session("owner", runner, "hub-one"), {
      ...binding,
      control_connected: false,
      execution_state: "unknown",
      current_turn_id: null,
    });

    store.receive(
      "owner",
      runner,
      3,
      {
        type: "event",
        event: hostEvent("turn/started", {
          host_generation: "host-old",
          hub_session_id: "hub-two",
          provider_session_id: "native-two",
          binding: {
            ...second,
            host_generation: "host-old",
            execution_state: "running",
            current_turn_id: "turn-stale",
          },
        }),
      },
      "host-one",
    );
    assert.deepEqual(store.session("owner", runner, "hub-two"), second);
    assert.equal(store.watermarks(runner).received, 3);
    store.receive(
      "owner",
      runner,
      4,
      { ...ready([]), host_generation: "host-old", sessions: [] },
      "host-one",
    );
    assert.equal(store.watermarks(runner).received, 4);
    assert.equal(store.runners("owner")[0]?.hello?.host_generation, "host-one");
    assert.deepEqual(store.session("owner", runner, "hub-two"), second);
    store.receive("owner", runner, 5, ready([]), "host-one");
    assert.equal(store.watermarks(runner).received, 5);
  } finally {
    store.close();
  }
});

test("device challenge, owner scope, command receipts, and revocation over real WebSocket", async (t) => {
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
  ws.send(JSON.stringify({ type: "data", seq: 1, payload: ready([]) }));
  assert.equal((await next("ack")).seq, 1);
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
      seq: 2,
      payload: { type: "receipt", receipt },
    }),
  );
  assert.equal((await next("ack")).seq, 2);
  ws.send(
    JSON.stringify({
      type: "data",
      seq: 3,
      payload: {
        type: "event",
        event: hostEvent("item/completed", {
          raw: {
            params: {
              item: {
                id: "large-tool",
                type: "commandExecution",
              },
            },
          },
        }),
      },
    }),
  );
  assert.equal((await next("ack")).seq, 3);
  const events = (
    await app.inject({
      url: "/v3/remote/events",
      headers: { "x-user-id": "owner" },
    })
  ).json().events;
  const completedEvent = events.find(
    (entry: { payload?: { event?: { kind?: string } } }) =>
      entry.payload?.event?.kind === "item/completed",
  );
  assert.deepEqual(completedEvent?.payload.event.raw.params.item, {
    id: "large-tool",
    type: "commandExecution",
  });
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
          event: hostEvent("turn/started", {
            provider_session_id: "different",
            binding: { ...binding, provider_session_id: "different" },
          }),
        }),
      /native_identity_changed/u,
    );
    assert.equal(store.watermarks(runner).received, 1);
  } finally {
    store.close();
  }
});

test("a durably queued command stays 202 through immediate backpressure and idempotent retry", async () => {
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const devices = new InMemoryGatewayStore();
  await devices.registerDevice("owner", runner, publicKey);
  const content = new RemoteContentStore(":memory:");
  let backpressured = false;
  const app = createGatewayApp({
    store: devices,
    authenticator: new DevelopmentIdentityAuthenticator(),
    remoteContent: content,
    remoteRoutes: { isBackpressured: () => backpressured },
  });
  const address = await app.listen({ port: 0, host: "127.0.0.1" });
  const { ws, next } = await connectRunner(
    address,
    keys.privateKey,
    "host-one",
  );
  try {
    ws.send(JSON.stringify({ type: "data", seq: 1, payload: ready([]) }));
    assert.equal((await next("ack")).seq, 1);
    backpressured = true;
    const first = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: command,
    });
    assert.equal(first.statusCode, 202);
    assert.equal(first.json().seq, 1);
    assert.equal(content.watermarks(runner).command_seq, 1);

    const retry = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: command,
    });
    assert.equal(retry.statusCode, 202);
    assert.equal(retry.json().seq, 1);
    assert.equal(content.watermarks(runner).command_seq, 1);

    backpressured = false;
    ws.send(JSON.stringify({ type: "heartbeat" }));
    assert.equal((await next("heartbeat_ack")).type, "heartbeat_ack");
    assert.deepEqual((await next("command")).command, command);
  } finally {
    ws.terminate();
    await app.close();
  }
});

test("a new Host generation replaces the old socket without stale close unregistering it", async () => {
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
  const oldConnection = await connectRunner(
    address,
    keys.privateKey,
    "host-one",
  );
  const oldClosed = new Promise<void>((resolve) =>
    oldConnection.ws.once("close", () => resolve()),
  );
  const currentConnection = await connectRunner(
    address,
    keys.privateKey,
    "host-two",
  );
  try {
    await oldClosed;
    currentConnection.ws.send(
      JSON.stringify({
        type: "data",
        seq: 1,
        payload: ready([]),
      }),
    );
    assert.equal((await currentConnection.next("ack")).seq, 1);
    assert.equal(content.runners("owner")[0]?.hello, null);
    const currentBinding = { ...binding, host_generation: "host-two" };
    currentConnection.ws.send(
      JSON.stringify({
        type: "data",
        seq: 2,
        payload: {
          ...ready([]),
          host_generation: "host-two",
          sessions: [currentBinding],
        },
      }),
    );
    assert.equal((await currentConnection.next("ack")).seq, 2);
    currentConnection.ws.send(JSON.stringify({ type: "heartbeat" }));
    assert.equal(
      (await currentConnection.next("heartbeat_ack")).type,
      "heartbeat_ack",
    );
    const sessions = await app.inject({
      url: "/v3/remote/sessions",
      headers: { "x-user-id": "owner" },
    });
    assert.equal(sessions.json().runners[0].online, true);
    assert.equal(sessions.json().sessions[0].binding.control_connected, true);
    assert.equal(
      sessions.json().sessions[0].binding.host_generation,
      "host-two",
    );
  } finally {
    oldConnection.ws.terminate();
    currentConnection.ws.terminate();
    await app.close();
  }
});

test("connect commands require the current ready generation and workspace authorization", async () => {
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
  const { ws, next } = await connectRunner(
    address,
    keys.privateKey,
    "host-one",
  );
  try {
    const create = {
      ...command,
      client_cmd_id: "connect-guarded",
      host_generation: "host-one",
      command: {
        action: "connect",
        mode: "create",
        workspace_path: binding.workspace_path,
      },
    } satisfies RemoteCommand;
    const beforeReady = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: create,
    });
    assert.equal(beforeReady.json().error, "runner_not_ready");
    ws.send(JSON.stringify({ type: "data", seq: 1, payload: ready([]) }));
    assert.equal((await next("ack")).seq, 1);
    const unauthorized = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: {
        ...create,
        client_cmd_id: "connect-unauthorized",
        command: {
          ...create.command,
          workspace_path: "ws-other-0123456789abcdef",
        },
      },
    });
    assert.equal(unauthorized.json().error, "workspace_not_authorized");
    const accepted = await app.inject({
      method: "POST",
      url: `/v3/remote/runners/${runner}/commands`,
      headers: { "x-user-id": "owner" },
      payload: create,
    });
    assert.equal(accepted.statusCode, 202);
  } finally {
    ws.terminate();
    await app.close();
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

test("command history filters by the projected session id and upgrades old databases", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-hub-"));
  const path = join(dir, "remote.sqlite");
  const scoped = (hub: string, n: number): RemoteCommand => ({
    ...command,
    client_cmd_id: `cmd-${hub}-${n}`,
    hub_session_id: hub,
  });
  try {
    // A database with the pre-upgrade shape: no `hub` column, no index on it.
    const legacy = new DatabaseSync(path);
    legacy.exec(`CREATE TABLE remote_runners (runner TEXT PRIMARY KEY, owner TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, command_seq INTEGER NOT NULL DEFAULT 0, command_ack INTEGER NOT NULL DEFAULT 0, hello TEXT);
      CREATE TABLE remote_commands (runner TEXT NOT NULL, cmd_id TEXT NOT NULL, seq INTEGER NOT NULL, body TEXT NOT NULL, receipt TEXT, PRIMARY KEY(runner,cmd_id), UNIQUE(runner,seq));`);
    const insert = legacy.prepare(
      "INSERT INTO remote_commands(runner,cmd_id,seq,body) VALUES (?,?,?,?)",
    );
    for (const [n, item] of [
      scoped("hub-one", 1),
      scoped("hub-two", 2),
      scoped("hub-one", 3),
    ].entries())
      insert.run(runner, item.client_cmd_id, n + 1, JSON.stringify(item));
    legacy
      .prepare(
        "INSERT INTO remote_runners(runner,owner,command_seq) VALUES (?,?,?)",
      )
      .run(runner, "owner", 3);
    legacy.close();

    // Opening it migrates the rows in place: history stays exact per session.
    const store = new RemoteContentStore(path);
    try {
      assert.deepEqual(
        store
          .commandHistory("owner", runner, "hub-one")
          .map((row) => row.command.client_cmd_id),
        ["cmd-hub-one-1", "cmd-hub-one-3"],
      );
      assert.deepEqual(
        store
          .commandHistory("owner", runner, "hub-two")
          .map((row) => row.command.client_cmd_id),
        ["cmd-hub-two-2"],
      );
      assert.equal(
        store.commandHistory("owner", runner, "hub-missing").length,
        0,
      );

      // Newly submitted commands project the same column and are filtered by it.
      store.submit("owner", runner, scoped("hub-two", 4));
      assert.equal(store.commandHistory("owner", runner, "hub-two").length, 2);
      assert.equal(store.commandHistory("owner", runner, "hub-one").length, 2);
    } finally {
      store.close();
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the remote API resolves a session cookie to its owner and ignores the development header", async () => {
  const keys = generateKeyPairSync("ed25519");
  const publicKey = keys.publicKey
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  const accounts = new InMemoryAccountStore();
  const devices = new InMemoryGatewayStore();
  const content = new RemoteContentStore(":memory:");
  const app = createGatewayApp({
    store: devices,
    accounts,
    authenticator: new SessionAuthenticator(accounts),
    remoteContent: content,
  });
  try {
    const registered = await app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email: "owner@example.com",
        password: "correct horse battery staple",
        display_name: "Owner",
      },
    });
    assert.equal(registered.statusCode, 201);
    const cookie = String(registered.headers["set-cookie"]).split(";", 1)[0]!;
    const userId = registered.json().user.user_id as string;

    // The device belongs to the signed-in account's workspace.
    await devices.registerDevice(userId, runner, publicKey);
    const address = await app.listen({ port: 0, host: "127.0.0.1" });
    const { ws, next } = await connectRunner(
      address,
      keys.privateKey,
      "host-one",
    );
    try {
      ws.send(JSON.stringify({ type: "data", seq: 1, payload: ready([]) }));
      assert.equal((await next("ack")).seq, 1);

      // The session cookie is the console's only identity, so the snapshot
      // must be scoped to the cookie's owner.
      const viaCookie = await app.inject({
        method: "GET",
        url: "/v3/remote/sessions",
        headers: { cookie },
      });
      assert.equal(viaCookie.statusCode, 200);
      const body = viaCookie.json();
      assert.equal(body.runners.length, 1);
      assert.equal(body.runners[0].runner_id, runner);
      assert.equal(body.sessions.length, 1);

      // A stale development header must not re-scope the request away from
      // the signed-in owner.
      const viaStaleHeader = await app.inject({
        method: "GET",
        url: "/v3/remote/sessions",
        headers: { cookie, "x-user-id": "someone-else" },
      });
      assert.equal(viaStaleHeader.statusCode, 200);
      assert.equal(viaStaleHeader.json().runners.length, 1);
      assert.equal(viaStaleHeader.json().sessions.length, 1);
    } finally {
      ws.terminate();
    }
  } finally {
    await app.close();
  }
});
