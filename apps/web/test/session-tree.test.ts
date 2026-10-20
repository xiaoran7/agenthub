import assert from "node:assert/strict";
import test from "node:test";

import {
  buildSessionTree,
  getDeviceLabel,
  getProviderInfo,
  workspaceLabel,
  workspaceTooltip,
} from "../src/lib/session-tree";

/**
 * The tree is built from the wire shapes the console already holds
 * (`RemoteRunner` / `RemoteSession` / `DiscoveryCandidate`), which are
 * snake_case. These helpers keep the test bodies readable without weakening
 * the production types to accept both spellings.
 */
const active = (
  runnerId: string,
  providerSessionId: string,
  workspacePath: string,
  extra: Partial<{
    online: boolean;
    control_connected: boolean;
    execution_state: string;
    title: string;
    hub_session_id: string;
  }> = {},
) => ({
  runner_id: runnerId,
  hub_session_id: `hub-${providerSessionId}`,
  provider_session_id: providerSessionId,
  workspace_path: workspacePath,
  online: true,
  control_connected: true,
  execution_state: "idle",
  ...extra,
});

const host = (runnerId: string, online = true) => ({
  runner_id: runnerId,
  online,
  hello: null,
});

const candidate = (
  runnerId: string,
  providerSessionId: string,
  workspacePath: string,
  extra: {
    title?: string;
    provider?: string;
    resumable?: boolean;
  } = {},
) => ({
  runner_id: runnerId,
  provider_session_id: providerSessionId,
  workspace_path: workspacePath,
  ...extra,
});

test("a session is filed under exactly one device, agent and workspace", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "codex-1", "D:\\Work\\Web", { title: "侧边栏重构" }),
      active("dev_a", "codex-2", "D:\\Work\\Web", { title: "E2E 全绿" }),
    ],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(tree.length, 1);
  const device = tree[0]!;
  assert.equal(device.totalCount, 2);
  assert.equal(device.workspaces.length, 1, "one agent folder per device");

  const agent = device.workspaces[0]!;
  assert.equal(agent.provider, "codex");
  assert.equal(agent.totalCount, 2);
  assert.equal(agent.workspaces.length, 1, "one workspace folder per agent");

  const workspace = agent.workspaces[0]!;
  assert.equal(workspace.label, "Web", "shows the folder name, not the path");
  assert.equal(workspace.items.length, 2);
  assert.deepEqual(
    workspace.items.map((item) => item.title),
    ["侧边栏重构", "E2E 全绿"],
    "sessions sort by title so the order does not jitter between polls",
  );
});

test("the same agent is one folder per device, not one per session", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "codex-1", "D:\\Work\\One"),
      active("dev_a", "codex-2", "D:\\Work\\Two"),
    ],
    candidates: [],
    providerFilter: "all",
  });

  const agent = tree[0]!.workspaces[0]!;
  assert.equal(tree[0]!.workspaces.length, 1, "Codex appears once");
  assert.equal(agent.workspaces.length, 2, "with a folder per workspace");
  assert.deepEqual(
    agent.workspaces.map((workspace) => workspace.label),
    ["One", "Two"],
  );
});

test("different agents on one device are separate folders", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "codex-1", "D:\\Work"),
      active("dev_a", "claude_1", "D:\\Work"),
      active("dev_a", "sess_1", "D:\\Work"),
    ],
    candidates: [],
    providerFilter: "all",
  });

  assert.deepEqual(
    tree[0]!.workspaces.map((agent) => agent.provider),
    ["codex", "claude", "zcode"],
    "managed engines sort in a stable order",
  );
});

test("a discovered candidate with a live session is not listed twice", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [active("dev_a", "codex-1", "D:\\Work")],
    candidates: [
      candidate("dev_a", "codex-1", "D:\\Work", { title: "同一会话" }),
      candidate("dev_a", "codex-2", "D:\\Work", { title: "另一会话" }),
    ],
    providerFilter: "all",
  });

  const items = tree[0]!.workspaces[0]!.workspaces[0]!.items;
  assert.equal(items.length, 2, "the live session replaces its read-only stub");
  assert.equal(items.filter((item) => item.kind === "active").length, 1);
  assert.equal(items.filter((item) => item.kind === "candidate").length, 1);
});

test("an offline device still appears, and sorts after online ones", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_offline", false), host("dev_online", true)],
    activeSessions: [],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(tree.length, 2, "a device with no sessions is still listed");
  assert.equal(tree[0]!.online, true, "online devices sort first");
  assert.equal(tree[1]!.online, false);
});

test("filtering by agent hides the other agent folders", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "codex-1", "D:\\Work"),
      active("dev_a", "sess_1", "D:\\Work"),
    ],
    candidates: [],
    providerFilter: "zcode",
  });

  assert.equal(tree[0]!.workspaces.length, 1);
  assert.equal(tree[0]!.workspaces[0]!.provider, "zcode");
  assert.equal(tree[0]!.workspaces[0]!.totalCount, 1);
});

test("provider metadata from discovery wins over the id prefix", () => {
  const tree = buildSessionTree({
    discovered: new Map([["dev_a/sess_1", { provider: "claude_code" }]]),
    devices: [host("dev_a", true)],
    activeSessions: [active("dev_a", "sess_1", "D:\\Work")],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(
    tree[0]!.workspaces[0]!.provider,
    "claude",
    "a sess_ id is ZCode unless discovery says otherwise",
  );
});

test("a session for an unknown device creates that device", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [],
    activeSessions: [active("dev_new", "codex-1", "D:\\Work")],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(tree.length, 1);
  assert.equal(tree[0]!.runnerId, "dev_new");
  assert.equal(tree[0]!.online, false);
});

test("workspace labels read as folder names and keep the full path as a hint", () => {
  assert.equal(workspaceLabel("D:\\Work\\Web"), "Web");
  assert.equal(
    workspaceLabel("ws-claude-space-0123456789abcdef"),
    "claude-space",
  );
  assert.equal(
    workspaceTooltip("\\\\?\\D:\\Work\\Web"),
    "D:\\Work\\Web",
    "the extended-length prefix is not shown to the user",
  );
});

test("device labels show the advertised name and never the device id", () => {
  assert.equal(
    getDeviceLabel("dev_zzz", { device_name: "工作机 (work-pc)" }),
    "工作机 (work-pc)",
  );
  assert.equal(getDeviceLabel("dev_zzz", { device_name: "备用机" }), "备用机");
  assert.equal(
    getDeviceLabel("dev_abcdef123456", null),
    "电脑 · 123456",
    "a device that advertised no name falls back to the id's tail, so two of them stay apart",
  );
});

test("an active item exposes the hub session key the console selects by", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "sess-1", "D:\\Work", { hub_session_id: "hub-xyz" }),
    ],
    candidates: [],
    providerFilter: "all",
  });

  const item = tree[0]!.workspaces[0]!.workspaces[0]!.items[0]!;
  assert.equal(item.kind, "active");
  // `key` is the Provider's own id (shared with the discovery candidate);
  // `sessionKey` is the Hub's stable id the Web client actually selects.
  // Conflating the two silently breaks selection and the breadcrumb.
  assert.equal(item.key, "dev_a/sess-1");
  assert.equal(
    item.kind === "active" ? item.sessionKey : undefined,
    "dev_a/hub-xyz",
  );
});

test("provider detection covers each managed engine", () => {
  assert.equal(getProviderInfo("sess_1").type, "zcode");
  assert.equal(getProviderInfo("gemini_1").type, "zcode");
  assert.equal(getProviderInfo("any", "gemini_cli").name, "Gemini CLI");
  assert.equal(getProviderInfo("claude_1").type, "claude");
  assert.equal(getProviderInfo("antigravity_1").type, "antigravity");
  assert.equal(getProviderInfo("grok_1").type, "other");
  assert.equal(getProviderInfo("any", "cursor").name, "Cursor");
  assert.equal(getProviderInfo("any", "cline").name, "Cline");
  assert.equal(getProviderInfo("anything-else").type, "codex");
});

test("session tree items preserve token counts, model and activity timestamps", () => {
  const tree = buildSessionTree({
    discovered: new Map([
      [
        "dev_a/sess-1",
        {
          provider: "zcode",
          model: "gemini-2.5-flash",
          total_tokens: 14200,
          last_activity_at_ms: 1700000000000,
        },
      ],
    ]),
    devices: [host("dev_a", true)],
    activeSessions: [
      active("dev_a", "sess-1", "D:\\Work", { hub_session_id: "hub-1" }),
    ],
    candidates: [
      {
        runner_id: "dev_a",
        provider_session_id: "cand-2",
        workspace_path: "D:\\Work",
        title: "Discovered Session",
        model: "claude-3-7-sonnet",
        total_tokens: 5800,
        last_activity_at_ms: 1700000005000,
      },
    ],
    providerFilter: "all",
  });

  const allItems = tree[0]!.workspaces.flatMap((agent) =>
    agent.workspaces.flatMap((ws) => ws.items),
  );
  const activeItem = allItems.find((i) => i.kind === "active")!;
  assert.equal(activeItem.model, "gemini-2.5-flash");
  assert.equal(activeItem.totalTokens, 14200);
  assert.equal(activeItem.lastActivityAtMs, 1700000000000);

  const candItem = allItems.find((i) => i.kind === "candidate")!;
  assert.equal(candItem.model, "claude-3-7-sonnet");
  assert.equal(candItem.totalTokens, 5800);
  assert.equal(candItem.lastActivityAtMs, 1700000005000);
});
