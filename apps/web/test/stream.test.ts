import assert from "node:assert/strict";
import test from "node:test";

import {
  SseDecoder,
  backoffMs,
  streamBatchAffectsSession,
  streamBatchAffectsSnapshot,
} from "../src/lib/stream";

test("frames are only emitted once the blank-line boundary arrives", () => {
  const decoder = new SseDecoder();
  assert.deepEqual(decoder.push("event: sessi"), []);
  assert.deepEqual(decoder.push('ons\ndata: {"accepted":1}'), []);
  assert.deepEqual(decoder.push("\n\n"), [
    { event: "sessions", data: '{"accepted":1}' },
  ]);
});

test("comments, retry hints and multi-line data follow the SSE grammar", () => {
  const decoder = new SseDecoder();
  const frames = decoder.push(
    ": keepalive\n\nretry: 5000\n\nevent: ready\ndata: {}\n\ndata: one\ndata: two\n\n",
  );
  assert.deepEqual(frames, [
    // A comment-only frame carries no field and must not reach the panel.
    { event: "ready", data: "{}" },
    { event: "message", data: "one\ntwo" },
  ]);
});

test("a chunk boundary inside CRLF does not split a line", () => {
  const decoder = new SseDecoder();
  assert.deepEqual(decoder.push("event: sessions\r"), []);
  assert.deepEqual(decoder.push("\ndata: {}\r\n\r\n"), [
    { event: "sessions", data: "{}" },
  ]);
});

test("values keep everything after the first colon", () => {
  const decoder = new SseDecoder();
  assert.deepEqual(decoder.push('data: {"device_id":"dev_00"}\n\n'), [
    { event: "message", data: '{"device_id":"dev_00"}' },
  ]);
});

test("reconnect backoff grows and then stops at the cap", () => {
  assert.equal(backoffMs(1), 1_000);
  assert.equal(backoffMs(2), 2_000);
  assert.equal(backoffMs(4), 8_000);
  assert.equal(backoffMs(9), 15_000);
  assert.equal(backoffMs(50), 15_000);
});

test("an id line is carried as the resume cursor and a retry line is not a frame", () => {
  const decoder = new SseDecoder();
  const frames = decoder.push(
    'id: 42\nevent: remote\ndata: {"cursor":42}\n\nretry: 1000\n\n: keepalive\n\n',
  );
  assert.equal(frames.length, 1);
  assert.equal(frames[0]?.event, "remote");
  assert.equal(frames[0]?.id, "42");
  assert.equal(frames[0]?.data, '{"cursor":42}');
});

test("streamBatchAffectsSnapshot skips pure streaming text deltas without binding changes", () => {
  const deltaFrame = {
    event: "remote",
    id: "10",
    data: JSON.stringify({
      cursor: 10,
      runner_id: "dev_1",
      payload: {
        type: "event",
        event: {
          kind: "item/agentMessage/delta",
          hub_session_id: "hub-1",
          binding: null,
        },
      },
    }),
  };
  assert.equal(streamBatchAffectsSnapshot([deltaFrame]), false);

  const turnStartedFrame = {
    event: "remote",
    id: "11",
    data: JSON.stringify({
      cursor: 11,
      runner_id: "dev_1",
      payload: {
        type: "event",
        event: {
          kind: "turn/started",
          hub_session_id: "hub-1",
          binding: null,
        },
      },
    }),
  };
  assert.equal(
    streamBatchAffectsSnapshot([deltaFrame, turnStartedFrame]),
    true,
  );

  const readyFrame = {
    event: "remote",
    id: "12",
    data: JSON.stringify({
      cursor: 12,
      runner_id: "dev_1",
      payload: { type: "ready" },
    }),
  };
  assert.equal(streamBatchAffectsSnapshot([readyFrame]), true);
});

test("streamBatchAffectsSession isolates open session polling from other runners and hubs", () => {
  const otherRunner = {
    event: "remote",
    id: "20",
    data: JSON.stringify({
      cursor: 20,
      runner_id: "dev_other",
      payload: { type: "ready" },
    }),
  };
  const otherHub = {
    event: "remote",
    id: "21",
    data: JSON.stringify({
      cursor: 21,
      runner_id: "dev_target",
      payload: {
        type: "event",
        event: { kind: "item/agentMessage/delta", hub_session_id: "hub-other" },
      },
    }),
  };
  assert.equal(
    streamBatchAffectsSession(
      [otherRunner, otherHub],
      "dev_target",
      "hub-open",
    ),
    false,
  );

  const sameHub = {
    event: "remote",
    id: "22",
    data: JSON.stringify({
      cursor: 22,
      runner_id: "dev_target",
      payload: {
        type: "event",
        event: { kind: "item/agentMessage/delta", hub_session_id: "hub-open" },
      },
    }),
  };
  assert.equal(
    streamBatchAffectsSession([otherHub, sameHub], "dev_target", "hub-open"),
    true,
  );

  const runnerBroadcast = {
    event: "remote",
    id: "23",
    data: JSON.stringify({
      cursor: 23,
      runner_id: "dev_target",
      payload: { type: "ready" },
    }),
  };
  assert.equal(
    streamBatchAffectsSession([runnerBroadcast], "dev_target", "hub-open"),
    true,
  );
});
