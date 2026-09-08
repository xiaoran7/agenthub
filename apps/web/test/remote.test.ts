import assert from "node:assert/strict";
import test from "node:test";
import {
  conversationItems,
  pendingApprovals,
  type RemoteEvent,
} from "../src/lib/remote";

const event = (
  kind: string,
  params: Record<string, unknown>,
  approval_id?: string,
): RemoteEvent => ({
  cursor: 1,
  runner_id: "runner",
  payload: {
    type: "event",
    event: { kind, raw: { params }, ...(approval_id ? { approval_id } : {}) },
  },
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
      { kind: "tool", text: "/workspace" },
    ],
  );
});

test("resolved native approvals and completed turns remove stale controls", () => {
  const events = [
    event("approval_requested", { command: "pwd" }, "generation:native:7"),
  ];
  assert.equal(pendingApprovals(events).length, 1);
  assert.equal(
    pendingApprovals([
      ...events,
      event("serverRequest/resolved", { requestId: 7 }),
    ]).length,
    0,
  );
  assert.equal(
    pendingApprovals([...events, event("turn/completed", {})]).length,
    0,
  );
});
