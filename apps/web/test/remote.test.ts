import assert from "node:assert/strict";
import test from "node:test";
import type {
  RemoteBinding,
  RemotePendingApproval,
} from "@agent-pulse/contracts/remote";
import {
  conversationItems,
  groupConversationItems,
  isRedactedPathOnly,
  latestContextUsage,
  latestTurnFailure,
  pendingApprovals,
  readableRemoteText,
  type RemoteEvent,
} from "../src/lib/remote";

const event = (
  kind: string,
  params: Record<string, unknown>,
  options: {
    approval_id?: string;
    approval?: RemotePendingApproval;
    binding?: RemoteBinding;
  } = {},
): RemoteEvent => ({
  cursor: 1,
  runner_id: "runner",
  payload: {
    type: "event",
    event: {
      kind,
      raw: { params },
      ...options,
    },
  },
});

const approval = (
  fill: string,
  hubSessionId = "hub-one",
): RemotePendingApproval => ({
  approval_id: `approval-${fill.repeat(64)}`,
  hub_session_id: hubSessionId,
  host_generation: "generation-one",
  turn_id: "turn-one",
  can_approve: true,
  can_decline: true,
});

const ready = (
  approvals: RemotePendingApproval[],
  cursor: number,
): RemoteEvent => ({
  cursor,
  runner_id: "runner",
  payload: { type: "ready", pending_approvals: approvals },
});

test("native final messages replace deltas without duplicating the answer", () => {
  const items = conversationItems(
    [
      event("item/agentMessage/delta", { itemId: "a", delta: "Hel" }),
      event("item/agentMessage/delta", { itemId: "a", delta: "lo" }),
      event("item/completed", {
        item: { id: "a", type: "agentMessage", text: "Hello!" },
      }),
      event("item/completed", {
        item: {
          id: "tool",
          type: "commandExecution",
          command: "pwd",
          aggregatedOutput: "/workspace",
        },
      }),
    ],
    [],
  );
  assert.deepEqual(
    items.map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "assistant", text: "Hello!" },
      { kind: "tool", text: "工具执行已完成" },
    ],
  );
});

test("remote tool events merge once and consecutive calls group like Desktop", () => {
  const items = conversationItems(
    [
      event("item/agentMessage/delta", {
        itemId: "tool-intent",
        turnId: "turn-1",
        delta: '{"command":"pwd","description":"检查目录"}',
      }),
      event("item/started", {
        turnId: "turn-1",
        item: { id: "command-1", type: "commandExecution", command: "pwd" },
      }),
      event("item/completed", {
        turnId: "turn-1",
        item: {
          id: "command-1",
          type: "commandExecution",
          command: "pwd",
          aggregatedOutput: "/workspace",
        },
      }),
      event("item/completed", {
        turnId: "turn-1",
        item: { id: "command-2", type: "commandExecution", command: "ls" },
      }),
    ],
    [],
  );
  assert.equal(items.length, 2);
  assert.equal(items[0]?.title, "命令执行");
  assert.equal(items[0]?.status, "completed");
  assert.equal(items[0]?.output, "/workspace");
  const groups = groupConversationItems(items);
  assert.equal(groups[0]?.type, "tool_group");
  if (groups[0]?.type === "tool_group") assert.equal(groups[0].items.length, 2);
});

test("redacted local paths read as privacy notices", () => {
  assert.equal(isRedactedPathOnly("[local_path_redacted]"), true);
  assert.equal(
    readableRemoteText("已成功[local_path_redacted]"),
    "已成功（本机路径已隐藏）",
  );
});

test("context ring uses reported values and stays empty without usage", () => {
  assert.equal(latestContextUsage([]), null);
  assert.deepEqual(
    latestContextUsage([
      event("thread/tokenUsage/updated", {
        tokenUsage: {
          last: { inputTokens: 42_000 },
          modelContextWindow: 200_000,
        },
      }),
    ]),
    { used: 42_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([
      event("thread/tokenUsage/updated", {
        tokenUsage: {
          used: 68_000,
          window: 200_000,
        },
      }),
    ]),
    { used: 68_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([], {
      provider_session_id: "claude_abc",
      workspace_path: "ws-agent-0123456789abcdef",
      title: "课表爬虫",
      context_used_tokens: 54_000,
      context_window_tokens: 200_000,
    }),
    { used: 54_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([], {
      provider_session_id: "claude_legacy",
      workspace_path: "ws-agent-0123456789abcdef",
      title: "旧版Runner兼容",
      model: "claude-opus-4-6",
      prompt_tokens: 90_000,
      calls: 3,
    }),
    { used: 30_000, window: 200_000, source: "reported" },
  );
});

test("managed history snapshots replace stale replay and retain later live messages", () => {
  const first = event("item/completed", {
    item: { id: "old", type: "agentMessage", text: "stale" },
  });
  const history = {
    ...event("history/snapshot", {
      items: [
        { role: "user", kind: "message", text: "开始" },
        { role: "assistant", kind: "message", text: "已完成" },
        {
          role: "assistant",
          kind: "tool",
          label: "Read",
          text: "读取结果",
          status: "completed",
        },
      ],
    }),
    cursor: 2,
  };
  const live = {
    ...event("item/completed", {
      item: { id: "new", type: "agentMessage", text: "继续执行" },
    }),
    cursor: 3,
  };
  const items = conversationItems([first, history, live], []);
  assert.deepEqual(
    items.map(({ kind, text }) => [kind, text]),
    [
      ["user", "开始"],
      ["assistant", "已完成"],
      ["tool", "读取结果"],
      ["assistant", "继续执行"],
    ],
  );
  assert.equal(items[2]?.title, "Read");
});

test("ready snapshots restore, scope, and clear opaque approvals", () => {
  const target = approval("a");
  const other = approval("b", "hub-other");
  assert.deepEqual(pendingApprovals([ready([target, other], 1)], "hub-one"), [
    target,
  ]);
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  assert.deepEqual(pendingApprovals([requested, ready([], 2)], "hub-one"), []);
});

test("opaque resolution and completed turns remove exact approvals", () => {
  const target = approval("a");
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  assert.equal(pendingApprovals([requested], "hub-one").length, 1);
  assert.equal(
    pendingApprovals(
      [
        requested,
        event("approval_resolved", {}, { approval_id: target.approval_id }),
      ],
      "hub-one",
    ).length,
    0,
  );
  const binding: RemoteBinding = {
    hub_session_id: "hub-one",
    provider_session_id: "native-one",
    workspace_path: "ws-project-0123456789abcdef",
    host_generation: "generation-one",
    control_connected: true,
    execution_state: "idle",
    current_turn_id: null,
  };
  assert.equal(
    pendingApprovals(
      [requested, event("turn/completed", {}, { binding })],
      "hub-one",
    ).length,
    0,
  );
});

test("accepted approval receipts remove the exact opaque control", () => {
  const target = approval("b");
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  const receipt: RemoteEvent = {
    cursor: 2,
    runner_id: "runner",
    payload: {
      type: "receipt",
      receipt: {
        client_cmd_id: "command-1",
        status: "accepted",
        result: { approval_id: target.approval_id, response_sent: true },
      },
    },
  };
  const rejected: RemoteEvent = {
    ...receipt,
    payload: {
      ...receipt.payload,
      receipt: { ...receipt.payload.receipt!, status: "rejected" },
    },
  };

  assert.equal(pendingApprovals([requested, rejected], "hub-one").length, 1);
  assert.equal(pendingApprovals([requested, receipt], "hub-one").length, 0);
});

const errorEvent = (
  message: string,
  options: { willRetry?: boolean; turnId?: string; hubSessionId?: string } = {},
): RemoteEvent =>
  event("error", {
    turnId: options.turnId ?? "turn-one",
    willRetry: options.willRetry ?? false,
    error: { message },
  });

test("a failed turn reports the cause the runner forwarded", () => {
  const failure = latestTurnFailure(
    [
      errorEvent(
        "The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account.",
      ),
    ],
    "hub-one",
  );

  assert.ok(failure, "the console must be able to explain a failed turn");
  assert.equal(failure.retrying, false);
  assert.equal(failure.turnId, "turn-one");
  assert.match(failure.message, /gpt-6-luna/);
});

test("a retrying provider error is not the turn's final outcome", () => {
  const failure = latestTurnFailure(
    [errorEvent("stream stalled", { willRetry: true })],
    "hub-one",
  );

  assert.equal(failure?.retrying, true);
});

test("a new turn clears an earlier failure and a completed turn clears it too", () => {
  const failed = errorEvent("model unavailable");
  const restarted = event("turn/started", { turn: { id: "turn-two" } });
  assert.equal(latestTurnFailure([failed, restarted], "hub-one"), null);

  const completed = event("turn/completed", { turn: { id: "turn-two" } });
  assert.equal(latestTurnFailure([failed, completed], "hub-one"), null);
});

test("another session's turn must not clear this session's failure", () => {
  const failed = errorEvent("model unavailable");
  for (const kind of ["turn/started", "turn/completed"]) {
    const otherSession: RemoteEvent = {
      ...event(kind, { turn: { id: "turn-other" } }),
      payload: {
        type: "event",
        event: {
          kind,
          raw: { params: { turn: { id: "turn-other" } } },
          binding: {
            hub_session_id: "hub-other",
            provider_session_id: "native-one",
            workspace_path: "ws-app-0123456789abcdef",
            host_generation: "generation-one",
            control_connected: true,
            execution_state: "idle",
            current_turn_id: null,
          },
        },
      },
    };
    const failure = latestTurnFailure([failed, otherSession], "hub-one");
    assert.equal(
      failure?.message,
      "model unavailable",
      `${kind} for another session is not this session's outcome`,
    );
  }
});

test("an error the runner could not describe yields no invented cause", () => {
  for (const params of [
    {},
    { error: {} },
    { error: { message: "" } },
    { error: { message: 42 } },
  ]) {
    assert.equal(
      latestTurnFailure([event("error", params)], "hub-one"),
      null,
      `unrecognised error ${JSON.stringify(params)} must not fabricate text`,
    );
  }
});

test("an error belonging to another session is ignored", () => {
  const otherSession: RemoteEvent = {
    ...errorEvent("model unavailable"),
    payload: {
      type: "event",
      event: {
        kind: "error",
        raw: { params: { error: { message: "model unavailable" } } },
        binding: {
          hub_session_id: "hub-other",
          provider_session_id: "native-one",
          workspace_path: "ws-app-0123456789abcdef",
          host_generation: "generation-one",
          control_connected: true,
          execution_state: "error",
          current_turn_id: null,
        },
      },
    },
  };

  assert.equal(latestTurnFailure([otherSession], "hub-one"), null);
});
