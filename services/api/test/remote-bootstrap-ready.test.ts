import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { RemoteContentStore } from "../src/remote-store.js";

test("bootstrap ready frame updates host_generation without blanking prior discovery, models, or workspace_labels", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-bootstrap-ready-"));
  try {
    const store = new RemoteContentStore(join(dir, "remote.db"));
    const owner = "owner-1";
    const runner = "dev_runner_fastboot";

    // 1. Runner previously published a complete catalog + discovery snapshot.
    store.receive(
      owner,
      runner,
      1,
      {
        type: "ready",
        provider: "codex_app_server",
        host_generation: "gen-old",
        device_name: "xiaotang",
        workspaces: [
          "ws-Desktop-aaaa1111aaaa1111",
          "ws-agenthub-bbbb2222bbbb2222",
        ],
        workspace_labels: {
          "ws-Desktop-aaaa1111aaaa1111": "~/Desktop",
          "ws-agenthub-bbbb2222bbbb2222": "D: ClaudeSpace/Project/agenthub",
        },
        sessions: [],
        models: { data: [{ id: "gpt-5.4", displayName: "GPT-5.4" }] },
        discovery: {
          candidates: [
            {
              provider_session_id: "sess-1",
              workspace_path: "ws-agenthub-bbbb2222bbbb2222",
              title: "Existing session",
            },
          ],
        },
        plugins: [],
        pending_approvals: [],
        attach_running: false,
      },
      "gen-old",
    );

    // 2. Runner restarts and immediately sends a fast bootstrap ready frame
    //    before background CatalogWorker finishes scanning.
    store.receive(
      owner,
      runner,
      2,
      {
        type: "ready",
        provider: "codex_app_server",
        host_generation: "gen-new",
        device_name: "xiaotang",
        workspaces: ["ws-Desktop-aaaa1111aaaa1111"],
        workspace_labels: {
          "ws-Desktop-aaaa1111aaaa1111": "~/Desktop",
        },
        sessions: [],
        models: null,
        discovery: null,
        plugins: [],
        pending_approvals: [],
        attach_running: false,
      },
      "gen-new",
    );

    const duringBootstrap = store.runnerHello(owner, runner);
    assert.ok(duringBootstrap);
    assert.equal(duringBootstrap.host_generation, "gen-new");
    assert.deepEqual(duringBootstrap.models, {
      data: [{ id: "gpt-5.4", displayName: "GPT-5.4" }],
    });
    assert.deepEqual(duringBootstrap.discovery, {
      candidates: [
        {
          provider_session_id: "sess-1",
          workspace_path: "ws-agenthub-bbbb2222bbbb2222",
          title: "Existing session",
        },
      ],
    });
    assert.deepEqual(duringBootstrap.workspaces, [
      "ws-Desktop-aaaa1111aaaa1111",
      "ws-agenthub-bbbb2222bbbb2222",
    ]);
    assert.deepEqual(duringBootstrap.workspace_labels, {
      "ws-Desktop-aaaa1111aaaa1111": "~/Desktop",
      "ws-agenthub-bbbb2222bbbb2222": "D: ClaudeSpace/Project/agenthub",
    });

    // 3. Background CatalogWorker completes with an authoritative empty discovery;
    //    non-null discovery cleanly replaces the carried-over snapshot.
    store.receive(
      owner,
      runner,
      3,
      {
        type: "ready",
        provider: "codex_app_server",
        host_generation: "gen-new",
        device_name: "xiaotang",
        workspaces: ["ws-Desktop-aaaa1111aaaa1111"],
        workspace_labels: {
          "ws-Desktop-aaaa1111aaaa1111": "~/Desktop",
        },
        sessions: [],
        models: { data: [{ id: "claude-sonnet-4-6" }] },
        discovery: { candidates: [] },
        plugins: [],
        pending_approvals: [],
        attach_running: false,
      },
      "gen-new",
    );

    const afterScan = store.runnerHello(owner, runner);
    assert.ok(afterScan);
    assert.deepEqual(afterScan.models, { data: [{ id: "claude-sonnet-4-6" }] });
    assert.deepEqual(afterScan.discovery, { candidates: [] });
    assert.deepEqual(afterScan.workspaces, ["ws-Desktop-aaaa1111aaaa1111"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
