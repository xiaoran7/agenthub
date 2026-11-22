import assert from "node:assert/strict";
import test from "node:test";

import {
  attentionEntries,
  buildSessionTree,
  getDeviceLabel,
  getItemActivityMs,
  getProviderInfo,
  getWorkspaceActivityMs,
  groupPendingApprovals,
  isWaitingOnPerson,
  sanitizeSessionTitle,
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
    provider: string;
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
      active("dev_a", "sess_1", "D:\\Work", { provider: "zcode" }),
    ],
    candidates: [],
    providerFilter: "all",
  });

  assert.deepEqual(
    tree[0]!.workspaces.map((agent) => agent.provider),
    ["codex", "claude", "zcode"],
    "managed engines sort before dynamic plugins",
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
      active("dev_a", "sess_1", "D:\\Work", { provider: "zcode" }),
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
    discovered: new Map([["dev_a/antigravity_1", { provider: "claude_code" }]]),
    devices: [host("dev_a", true)],
    activeSessions: [active("dev_a", "antigravity_1", "D:\\Work")],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(
    tree[0]!.workspaces[0]!.provider,
    "claude",
    "authoritative discovery provider overrides id prefix",
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

test("provider detection covers native engines and dynamic plugins", () => {
  const plugins = [
    { id: "zcode", displayName: "ZCode", sessionPrefix: "sess_" },
    { id: "gemini_cli", displayName: "Gemini CLI", sessionPrefix: "gemini_" },
    { id: "grok_build", displayName: "Grok", sessionPrefix: "grok_" },
  ];
  assert.equal(getProviderInfo("sess_1", undefined, plugins).type, "zcode");
  assert.equal(
    getProviderInfo("gemini_1", undefined, plugins).type,
    "gemini_cli",
  );
  assert.equal(
    getProviderInfo("any", "gemini_cli", plugins).name,
    "Gemini CLI",
  );
  assert.equal(getProviderInfo("claude_1").type, "claude");
  assert.equal(getProviderInfo("antigravity_1").type, "antigravity");
  assert.equal(
    getProviderInfo("any", "deepseek_harness").type,
    "deepseek_harness",
  );
  assert.equal(
    getProviderInfo("grok_1", undefined, plugins).type,
    "grok_build",
  );
  assert.equal(getProviderInfo("any", "my_acp_bot").type, "my_acp_bot");
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

test("untitled sessions fall back to a plain name with no id fragment", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [host("dev_a", true)],
    activeSessions: [active("dev_a", "sess_971bd511-xxxx", "D:\\Work")],
    candidates: [candidate("dev_a", "sess_a82fe102-yyyy", "D:\\Work")],
    providerFilter: "all",
  });
  const items = tree[0]!.workspaces[0]!.workspaces[0]!.items;
  assert.equal(items[0]!.title, "新会话");
  assert.equal(items[1]!.title, "新会话");
});

test("getItemActivityMs tiers active connected sessions above candidates regardless of timestamps", () => {
  const activeWithTime = {
    kind: "active" as const,
    key: "dev_a/sess-1",
    runnerId: "dev_a",
    providerSessionId: "sess-1",
    workspaceId: "D:\\Work",
    title: "Active With Time",
    provider: getProviderInfo("sess-1"),
    sessionKey: "dev_a/hub-1",
    session: { online: true, control_connected: true, execution_state: "idle" },
    lastActivityAtMs: 1700000000000,
  };
  const activeWithoutTime = {
    ...activeWithTime,
    providerSessionId: "sess-2",
    title: "Active Without Time",
    lastActivityAtMs: undefined,
  };
  const activeOnlineOnly = {
    ...activeWithTime,
    providerSessionId: "sess-3",
    title: "Active Online Only",
    session: {
      online: true,
      control_connected: false,
      execution_state: "idle",
    },
    lastActivityAtMs: 1700000050000,
  };
  const candidateNewer = {
    kind: "candidate" as const,
    key: "dev_a/cand-1",
    runnerId: "dev_a",
    providerSessionId: "cand-1",
    workspaceId: "D:\\Work",
    title: "Candidate Newer",
    provider: getProviderInfo("cand-1"),
    candidate: { resumable: true },
    lastActivityAtMs: 1700000090000,
  };

  const list = [
    candidateNewer,
    activeWithoutTime,
    activeOnlineOnly,
    activeWithTime,
  ];
  list.sort((a, b) => getItemActivityMs(b) - getItemActivityMs(a));

  assert.deepEqual(
    list.map((i) => i.title),
    [
      "Active With Time",
      "Active Without Time",
      "Active Online Only",
      "Candidate Newer",
    ],
  );
});

test("getWorkspaceActivityMs calculates max activity without array allocations", () => {
  assert.equal(getWorkspaceActivityMs({ items: [] }), 0);

  const older = {
    kind: "candidate" as const,
    key: "dev_a/c1",
    runnerId: "dev_a",
    providerSessionId: "c1",
    workspaceId: "D:\\Work",
    title: "Older",
    provider: getProviderInfo("c1"),
    candidate: { resumable: true },
    lastActivityAtMs: 1700000010000,
  };
  const newer = {
    kind: "candidate" as const,
    key: "dev_a/c2",
    runnerId: "dev_a",
    providerSessionId: "c2",
    workspaceId: "D:\\Work",
    title: "Newer",
    provider: getProviderInfo("c2"),
    candidate: { resumable: true },
    lastActivityAtMs: 1700000090000,
  };

  assert.equal(
    getWorkspaceActivityMs({ items: [older, newer] }),
    1700000090000,
  );
});

test("buildSessionTree sorts workspaces by most recent activity descending", () => {
  const tree = buildSessionTree({
    discovered: new Map([
      ["dev_a/cand-old", { last_activity_at_ms: 1700000001000 }],
      ["dev_a/cand-new", { last_activity_at_ms: 1700000099000 }],
    ]),
    devices: [host("dev_a", true)],
    activeSessions: [],
    candidates: [
      candidate("dev_a", "cand-old", "D:\\Work\\AlphabeticallyFirst", {
        title: "Old Work",
      }),
      candidate("dev_a", "cand-new", "D:\\Work\\AlphabeticallySecond", {
        title: "Recent Work",
      }),
    ],
    providerFilter: "all",
  });

  const workspaces = tree[0]!.workspaces[0]!.workspaces;
  assert.deepEqual(
    workspaces.map((ws) => ws.label),
    ["AlphabeticallySecond", "AlphabeticallyFirst"],
    "more recently active workspace sorts first regardless of alphabetical label",
  );
});

/* ================= 跨机 "要你处理" 收集 ================= */

const treeWithWaiting = () =>
  buildSessionTree({
    discovered: new Map(),
    devices: [
      { runner_id: "dev_a", online: true, hello: { device_name: "工作室" } },
      { runner_id: "dev_b", online: true, hello: { device_name: "笔记本" } },
    ],
    activeSessions: [
      active("dev_a", "claude_a1", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "awaiting_permission",
        title: "等审批的会话",
      }),
      active("dev_a", "claude_a2", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "running",
        title: "正在跑",
      }),
      active("dev_b", "claude_b1", "ws-bbbbbbbbbbbbbbb1-0123456789abcdef", {
        execution_state: "awaiting_input",
        title: "等回答的会话",
      }),
      active("dev_b", "claude_b2", "ws-bbbbbbbbbbbbbbb1-0123456789abcdef", {
        execution_state: "idle",
        title: "空闲",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });

test("attentionEntries lists waiting sessions from every device", () => {
  const entries = attentionEntries(treeWithWaiting());
  // Compared as sets: which device sorts first is `buildSessionTree`'s
  // business, and this test is about *whether* both devices are represented.
  assert.deepEqual(
    new Set(entries.map((e) => e.title)),
    new Set(["等审批的会话", "等回答的会话"]),
  );
  assert.deepEqual(
    new Set(entries.map((e) => e.deviceLabel)),
    new Set(["工作室", "笔记本"]),
  );
});

test("attentionEntries skips sessions that are not waiting on a person", () => {
  const entries = attentionEntries(treeWithWaiting());
  for (const entry of entries) {
    assert.ok(
      isWaitingOnPerson(entry.executionState),
      `${entry.title} should be waiting, was ${entry.executionState}`,
    );
  }
});

test("attentionEntries ignores a session whose Host is not controlling it", () => {
  // The approval strip offers approve/decline inline. A session no Host is
  // driving cannot answer either command, so listing it would offer buttons
  // that always fail.
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [{ runner_id: "dev_a", online: true, hello: null }],
    activeSessions: [
      active("dev_a", "claude_a1", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        control_connected: false,
        execution_state: "awaiting_permission",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });
  assert.deepEqual(attentionEntries(tree), []);
});

test("attentionEntries includes a session the Host reports approvals for", () => {
  // The two sources publish on different clocks: a session can be holding an
  // open approval before its execution_state flips. Taking either signal makes
  // the row appear as soon as either source knows.
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [{ runner_id: "dev_a", online: true, hello: null }],
    activeSessions: [
      active("dev_a", "claude_a1", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "running",
        hub_session_id: "hub-a1",
        title: "刚弹审批",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });
  const pending = new Map<string, unknown[]>([["dev_a/hub-a1", [{}]]]);
  const entries = attentionEntries(tree, pending);
  assert.equal(entries.length, 1);
  assert.equal(entries[0]!.pendingApprovals, 1);
});

test("attentionEntries sorts newest activity first", () => {
  const now = Date.now();
  const tree = buildSessionTree({
    discovered: new Map([
      [
        "dev_a/claude_old",
        { last_activity_at_ms: now - 60 * 60 * 1000, provider: "claude" },
      ],
      ["dev_a/claude_new", { last_activity_at_ms: now, provider: "claude" }],
    ]),
    devices: [{ runner_id: "dev_a", online: true, hello: null }],
    activeSessions: [
      active("dev_a", "claude_old", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "awaiting_permission",
        title: "一小时前就等你",
      }),
      active("dev_a", "claude_new", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "awaiting_permission",
        title: "刚刚等你",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });
  assert.deepEqual(
    attentionEntries(tree).map((e) => e.title),
    ["刚刚等你", "一小时前就等你"],
  );
});

test("attentionEntries splits the session key on the runner boundary", () => {
  // `sessionKey` is `${runnerId}/${hubSessionId}`. Slicing by the runner id's
  // own length keeps a `/` inside either half from cutting the wrong place.
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [{ runner_id: "dev_a", online: true, hello: null }],
    activeSessions: [
      active("dev_a", "claude_a1", "ws-aaaaaaaaaaaaaaa1-0123456789abcdef", {
        execution_state: "awaiting_input",
        hub_session_id: "hub/with/slashes",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });
  const entries = attentionEntries(tree);
  assert.equal(entries.length, 1);
  assert.equal(entries[0]!.runnerId, "dev_a");
  assert.equal(entries[0]!.hubSessionId, "hub/with/slashes");
  assert.equal(entries[0]!.sessionKey, "dev_a/hub/with/slashes");
});

test("groupPendingApprovals buckets approvals by runner and session", () => {
  const runners = [
    {
      runner_id: "dev_a",
      hello: {
        pending_approvals: [
          { approval_id: "a1", hub_session_id: "hub-a" },
          { approval_id: "a2", hub_session_id: "hub-a" },
        ],
      },
    },
    {
      runner_id: "dev_b",
      hello: {
        pending_approvals: [{ approval_id: "b1", hub_session_id: "hub-b" }],
      },
    },
    // A device that has published nothing yet must not throw or invent a key.
    { runner_id: "dev_c", hello: null },
  ];
  const grouped = groupPendingApprovals(runners);
  assert.equal(grouped.get("dev_a/hub-a")?.length, 2);
  assert.equal(grouped.get("dev_b/hub-b")?.length, 1);
  assert.equal(grouped.size, 2);
});

test("sanitizeSessionTitle strips XML command tags, markdown links and empty wrappers", () => {
  assert.equal(
    sanitizeSessionTitle(
      "<command-name>/compact</command-name><command-message>compact</command-message><command-args></command-args>",
    ),
    "/compact",
  );
  assert.equal(
    sanitizeSessionTitle(
      "<USER_REQUEST>审查 [README.md](file:///d:/README.md) 的排版</USER_REQUEST>",
    ),
    "审查 README.md 的排版",
  );
  assert.equal(sanitizeSessionTitle("<conversation_history>"), "新会话");
  assert.equal(workspaceLabel("D___ClaudeSpace"), "ClaudeSpace");
  assert.equal(workspaceLabel("C___Users_tanner_Project_agenthub"), "agenthub");
  assert.equal(workspaceLabel("ws-__-4a2f89c011223344"), "本地目录 · 4a2f");
  assert.equal(workspaceLabel("____"), "本地目录");
});
test("buildSessionTree respects active session provider even when discovered map is empty", () => {
  const tree = buildSessionTree({
    discovered: new Map(),
    devices: [
      { runner_id: "dev_a", online: true, hello: { device_name: "xiaotang" } },
    ],
    activeSessions: [
      active("dev_a", "claude_desktop_session_1", "ws-demo-0123456789abcdef", {
        provider: "claude_desktop",
      }),
      active("dev_a", "claude_code_session_1", "ws-demo-0123456789abcdef", {
        provider: "claude_code",
      }),
    ],
    candidates: [],
    providerFilter: "all",
  });

  assert.equal(tree.length, 1);
  const agents = tree[0]!.workspaces.map((a) => ({
    provider: a.provider,
    count: a.totalCount,
  }));
  assert.deepEqual(agents, [
    { provider: "claude", count: 1 },
    { provider: "claude_desktop", count: 1 },
  ]);
});

test("getProviderInfo matches legacy sess_* sessions to zcode via sessionPrefixes", () => {
  const info = getProviderInfo("sess_legacy_99", undefined, [
    {
      id: "zcode",
      displayName: "ZCode",
      sessionPrefix: "zcode_",
      sessionPrefixes: ["zcode_", "sess_"],
    },
  ]);
  assert.equal(info.type, "zcode");
  assert.equal(info.name, "ZCode");
  assert.equal(info.badgeClass, "plugin");
});
