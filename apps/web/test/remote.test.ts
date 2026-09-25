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
