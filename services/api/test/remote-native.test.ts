import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { InMemoryGatewayStore } from "../src/store.js";
import { RemoteContentStore } from "../src/remote-store.js";
import type { RemoteCommand } from "@agent-pulse/contracts";

const runnerBinary = process.env.AGENTHUB_TEST_RUNNER_BINARY;
const codexBinary = process.env.AGENTHUB_TEST_CODEX_BINARY;

test(
  "native Codex Remote: signed Runner, two turns, Gateway restart and disk replay",
  {
    skip: !runnerBinary || !codexBinary,
    timeout: 240_000,
  },
  async () => {
    const dir = mkdtempSync(join(tmpdir(), "agenthub-native-"));
    const identityPath = join(dir, "identity.json");
    const info = JSON.parse(
      execFileSync(
        runnerBinary!,
        ["device", "init", "--identity", identityPath],
        { encoding: "utf8", windowsHide: true },
      ),
    ) as { device_id: string; public_key: string };
    const devices = new InMemoryGatewayStore();
    await devices.registerDevice(
      "native-test-owner",
      info.device_id,
      info.public_key,
    );
    const database = join(dir, "gateway.db");
    const makeApp = () =>
      createGatewayApp({
        store: devices,
        authenticator: new DevelopmentIdentityAuthenticator(),
        remoteContent: new RemoteContentStore(database),
      });
    let app = makeApp();
    const address = await app.listen({ port: 0, host: "127.0.0.1" });
    const port = Number(new URL(address).port);
    const child = spawn(
      runnerBinary!,
      [
        "--identity",
        identityPath,
        "remote-run",
        "--codex-binary",
        codexBinary!,
        "--workspace",
        dir,
        "--db",
        join(dir, "runner.db"),
        "--server",
        address.replace("http:", "ws:") + "/v3/remote/ws",
      ],
      {
        stdio: ["ignore", "ignore", "pipe"],
        windowsHide: true,
        env: {
          ...process.env,
          ...(process.env.AGENTHUB_TEST_PROXY
            ? {
                HTTP_PROXY: process.env.AGENTHUB_TEST_PROXY,
                HTTPS_PROXY: process.env.AGENTHUB_TEST_PROXY,
                ALL_PROXY: process.env.AGENTHUB_TEST_PROXY,
                http_proxy: process.env.AGENTHUB_TEST_PROXY,
                https_proxy: process.env.AGENTHUB_TEST_PROXY,
                all_proxy: process.env.AGENTHUB_TEST_PROXY,
              }
            : {}),
        },
      },
    );
    let diagnostics = "";
    child.stderr.on("data", (chunk) => {
      diagnostics = (diagnostics + String(chunk)).slice(-4000);
    });
    const headers = { "x-user-id": "native-test-owner" };
    const wait = async <T>(
      read: () => Promise<T>,
      accept: (value: T) => boolean,
    ): Promise<T> => {
      const deadline = Date.now() + 90_000;
      let last: T | undefined;
      while (Date.now() < deadline) {
        assert.equal(child.exitCode, null, `Runner stopped: ${diagnostics}`);
        const value = await read();
        last = value;
        if (accept(value)) return value;
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      const summary = Array.isArray(last)
        ? last.map((row) => ({
            kind: row.payload?.event?.kind,
            status: row.payload?.receipt?.status,
            code: row.payload?.receipt?.code,
            turn: row.payload?.event?.raw?.params?.turn?.status,
          }))
        : undefined;
      assert.fail(
        `native wait timed out: ${diagnostics}; ${JSON.stringify(summary)}`,
      );
    };
    const sessions = async () =>
      (await app.inject({ url: "/v3/remote/sessions", headers })).json();
    const submit = async (command: RemoteCommand) => {
      const response = await app.inject({
        method: "POST",
        url: `/v3/remote/runners/${info.device_id}/commands`,
        headers,
        payload: command,
      });
      assert.equal(response.statusCode, 202, response.body);
    };
    const events = async () =>
      (await app.inject({ url: "/v3/remote/events", headers })).json()
        .events as Array<{ payload: Record<string, any> }>;
    try {
      await wait(sessions, (value) =>
        value.runners.some(
          (runner: { online: boolean; hello: any }) =>
            runner.online && runner.hello?.workspaces?.length > 0,
        ),
      );
      const runnersList = (await sessions()).runners;
      const runner = runnersList.find(
        (r: { online: boolean; hello: any }) => r.online && r.hello,
      );
      const workspaceId = runner.hello.workspaces[0];
      assert.ok(
        typeof workspaceId === "string" && workspaceId.startsWith("ws-"),
        "workspace ID must be emitted in ready hello",
      );

      await submit({
        client_cmd_id: "native-create",
        hub_session_id: "native-hub",
        host_generation: runner.hello.host_generation,
        command: {
          action: "connect",
          mode: "create",
          workspace_path: workspaceId,
        },
      });
      const state = await wait(
        sessions,
        (value) => value.sessions.length === 1,
      );
      const binding = state.sessions[0].binding;
      assert.ok(binding.provider_session_id);
      const prompt: RemoteCommand = {
        client_cmd_id: "native-turn-1",
        hub_session_id: "native-hub",
        host_generation: binding.host_generation,
        command: {
          action: "send_prompt",
          text: "Protocol smoke test. Do not use tools or read files. Reply exactly AGENTHUB_REMOTE_OK.",
        },
      };
      await submit(prompt);
      await submit(prompt);
      await wait(events, (rows) =>
        rows.some((row) => row.payload.event?.kind === "turn/completed"),
      );
      let rows = await events();
      const first = rows.find(
        (row) => row.payload.event?.kind === "turn/completed",
      )!.payload.event;
      assert.equal(first.raw.params.turn.status, "completed");
      assert.equal(first.provider_session_id, binding.provider_session_id);
      await submit({
        ...prompt,
        client_cmd_id: "native-turn-2",
        command: {
          action: "send_prompt",
          text: "Without using tools, repeat the exact marker from the previous turn.",
        },
      });
      const secondRows = await wait(events, (rows) =>
        rows.some(
          (row) => row.payload.receipt?.client_cmd_id === "native-turn-2",
        ),
      );
      assert.equal(
        secondRows.find(
          (row) => row.payload.receipt?.client_cmd_id === "native-turn-2",
        )?.payload.receipt.status,
        "accepted",
      );
      await app.close();
      // The native Host remains alive with no server or viewers. Its output is journaled.
      await new Promise((resolve) => setTimeout(resolve, 6000));
      assert.equal(child.exitCode, null);
      app = makeApp();
      await app.listen({ port, host: "127.0.0.1" });
      rows = await wait(
        events,
        (rows) =>
          rows.filter((row) => row.payload.event?.kind === "turn/completed")
            .length === 2,
      );
      for (const row of rows.filter(
        (row) => row.payload.event?.kind === "turn/completed",
      )) {
        assert.equal(row.payload.event.raw.params.turn.status, "completed");
        assert.equal(
          row.payload.event.provider_session_id,
          binding.provider_session_id,
        );
      }
      const deltaText = rows
        .filter((row) => row.payload.event?.kind === "item/agentMessage/delta")
        .map((row) => row.payload.event.raw.params.delta)
        .join("");
      assert.match(deltaText, /AGENTHUB_REMOTE_OK/u);
      assert.equal(
        rows.filter((row) => row.payload.event?.kind === "turn/started").length,
        2,
      );
    } finally {
      child.kill();
      await new Promise<void>((resolve) => {
        if (child.exitCode !== null) resolve();
        else child.once("exit", () => resolve());
      });
      await app.close();
      rmSync(dir, { recursive: true, force: true });
    }
  },
);
