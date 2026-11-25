import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { mergeMonotonicEvents } from "../src/lib/remote-session-state";
import {
  createRafBatcher,
  fetchSessionEventsSince,
  parseDirectSseBatch,
} from "../src/lib/remote-sse-sync";
import type { RemoteEvent } from "../src/lib/remote-types";

describe("parseDirectSseBatch", () => {
  it("decodes live message_delta SSE frames directly without triggering HTTP reconcile", () => {
    const frames = [
      {
        event: "remote",
        id: "42",
        data: JSON.stringify({
          cursor: 42,
          runner_id: "runner-1",
          payload: {
            type: "event",
            event: {
              hub_session_id: "hub-a",
              kind: "item/agentMessage/delta",
              raw: { params: { itemId: "m1", delta: "hello " } },
            },
          },
        }),
      },
      {
        event: "remote",
        id: "43",
        data: JSON.stringify({
          cursor: 43,
          runner_id: "runner-1",
          payload: {
            type: "event",
            event: {
              hub_session_id: "hub-other",
              kind: "item/agentMessage/delta",
              raw: { params: { itemId: "m2", delta: "ignored" } },
            },
          },
        }),
      },
    ];

    const result = parseDirectSseBatch(frames, "runner-1", "hub-a");
    assert.equal(result.relevant, true);
    assert.equal(result.needsDetailReconcile, false);
    assert.equal(result.events.length, 1);
    assert.equal(result.events[0]!.cursor, 42);
    assert.equal(result.latestBinding, null);
  });

  it("extracts binding transitions and marks receipt frames for reconcile", () => {
    const frames = [
      {
        event: "remote",
        id: "50",
        data: JSON.stringify({
          cursor: 50,
          runner_id: "runner-1",
          payload: {
            type: "event",
            event: {
              hub_session_id: "hub-a",
              kind: "turn/started",
              binding: {
                hub_session_id: "hub-a",
                provider_session_id: "prov-1",
                workspace_path: "ws-demo",
                host_generation: "gen-1",
                control_connected: true,
                execution_state: "running",
                current_turn_id: "turn-9",
              },
            },
          },
        }),
      },
      {
        event: "remote",
        id: "51",
        data: JSON.stringify({
          cursor: 51,
          runner_id: "runner-1",
          payload: {
            type: "receipt",
            receipt: { hub_session_id: "hub-a", status: "accepted" },
          },
        }),
      },
    ];

    const result = parseDirectSseBatch(frames, "runner-1", "hub-a");
    assert.equal(result.relevant, true);
    assert.equal(result.events.length, 2);
    assert.deepEqual(
      result.events.map((ev) => ev.cursor),
      [50, 51],
    );
    assert.equal(result.events[1]?.payload.type, "receipt");
    assert.equal(result.latestBinding?.execution_state, "running");
    assert.equal(result.latestBinding?.current_turn_id, "turn-9");
    assert.equal(result.needsDetailReconcile, true);
  });

  it("ignores frames from other runners completely", () => {
    const result = parseDirectSseBatch(
      [
        {
          event: "remote",
          id: "60",
          data: JSON.stringify({
            cursor: 60,
            runner_id: "runner-2",
            payload: { type: "event", event: { hub_session_id: "hub-a" } },
          }),
        },
      ],
      "runner-1",
      "hub-a",
    );
    assert.equal(result.relevant, false);
    assert.equal(result.events.length, 0);
  });

  it("includes ready and host_lost frames in events while requiring detail reconcile", () => {
    const result = parseDirectSseBatch(
      [
        {
          event: "remote",
          id: "70",
          data: JSON.stringify({
            cursor: 70,
            runner_id: "runner-1",
            payload: { type: "ready", pending_approvals: [] },
          }),
        },
        {
          event: "remote",
          id: "71",
          data: JSON.stringify({
            cursor: 71,
            runner_id: "runner-1",
            payload: { type: "host_lost" },
          }),
        },
      ],
      "runner-1",
      "hub-a",
    );
    assert.equal(result.relevant, true);
    assert.equal(result.needsDetailReconcile, true);
    assert.deepEqual(
      result.events.map((ev) => [ev.cursor, ev.payload.type]),
      [
        [70, "ready"],
        [71, "host_lost"],
      ],
    );
  });
});

function makeDeltaEvent(cursor: number, text = `m-${cursor}`): RemoteEvent {
  return {
    cursor,
    runner_id: "runner-1",
    payload: {
      type: "event",
      event: {
        kind: "item/agentMessage/delta",
        raw: { params: { itemId: "m1", delta: text } },
      },
    },
  };
}

describe("fetchSessionEventsSince (WEB-01 cursor gap & truncated recovery)", () => {
  it("fills lower-cursor gaps when optimistic SSE arrives ahead of an in-flight HTTP page", async () => {
    let accumulated: RemoteEvent[] = [makeDeltaEvent(100)];
    const requestedAfters: number[] = [];

    const result = await fetchSessionEventsSince({
      startCursor: 100,
      pageLimit: 2,
      getAccumulated: () => accumulated,
      setAccumulated: (next) => {
        accumulated = next;
      },
      requestPage: async (after) => {
        requestedAfters.push(after);
        if (after === 100) {
          // Simulate liveBatcher flushing optimistic SSE cursor 105 while HTTP page 1 is in flight.
          const sseMerged = mergeMonotonicEvents(
            accumulated,
            [makeDeltaEvent(105)],
            100,
          );
          accumulated = sseMerged.nextEvents;
          return { events: [makeDeltaEvent(101), makeDeltaEvent(102)] };
        }
        if (after === 102) {
          return { events: [makeDeltaEvent(103), makeDeltaEvent(104)] };
        }
        if (after === 104) {
          return { events: [makeDeltaEvent(105)] };
        }
        return { events: [] };
      },
    });

    assert.deepEqual(requestedAfters, [100, 102, 104]);
    assert.equal(result.nextHttpCursor, 105);
    assert.equal(result.changed, true);
    assert.deepEqual(
      accumulated.map((ev) => ev.cursor),
      [100, 101, 102, 103, 104, 105],
    );
  });

  it("resets accumulated events and re-seeds from detail.resume once when batch.truncated is true", async () => {
    let accumulated: RemoteEvent[] = [makeDeltaEvent(40), makeDeltaEvent(50)];
    const requestedAfters: number[] = [];
    let resetCount = 0;

    const result = await fetchSessionEventsSince({
      startCursor: 50,
      resume: { transcript: 500, approvals: 495 },
      getAccumulated: () => accumulated,
      setAccumulated: (next) => {
        accumulated = next;
      },
      onTruncatedReset: () => {
        resetCount += 1;
      },
      requestPage: async (after) => {
        requestedAfters.push(after);
        if (after === 50) {
          return { events: [], truncated: true };
        }
        assert.equal(after, 494);
        return { events: [makeDeltaEvent(495), makeDeltaEvent(500)] };
      },
    });

    assert.equal(resetCount, 1);
    assert.equal(result.truncatedReset, true);
    assert.equal(result.nextHttpCursor, 500);
    assert.deepEqual(requestedAfters, [50, 494]);
    assert.deepEqual(
      accumulated.map((ev) => ev.cursor),
      [495, 500],
    );
  });

  it("guards truncated recovery with a single-retry limit to prevent infinite loops", async () => {
    let accumulated: RemoteEvent[] = [makeDeltaEvent(10)];
    let calls = 0;

    const result = await fetchSessionEventsSince({
      startCursor: 10,
      resume: { transcript: 50, approvals: null },
      getAccumulated: () => accumulated,
      setAccumulated: (next) => {
        accumulated = next;
      },
      requestPage: async () => {
        calls += 1;
        return { events: [], truncated: true };
      },
    });

    assert.equal(calls, 2);
    assert.equal(result.truncatedReset, true);
    assert.deepEqual(accumulated, []);
  });
});

describe("createRafBatcher", () => {
  it("coalesces multiple rapid pushes into a single flushed batch", async () => {
    const flushes: number[][] = [];
    const batcher = createRafBatcher<number>((items) => flushes.push(items));
    batcher.push([1, 2]);
    batcher.push([3]);
    batcher.push([4, 5]);
    assert.equal(flushes.length, 0);
    await new Promise((resolve) => setTimeout(resolve, 35));
    assert.deepEqual(flushes, [[1, 2, 3, 4, 5]]);
    batcher.cancel();
  });
});
