import assert from "node:assert/strict";
import test from "node:test";
import type {
  RemoteBinding,
  RemotePendingApproval,
} from "@agent-pulse/contracts/remote";
import {
  conversationItems,
  pendingApprovals,
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
