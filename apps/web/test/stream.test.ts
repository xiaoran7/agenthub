import assert from "node:assert/strict";
import test from "node:test";

import { SseDecoder, backoffMs } from "../src/lib/stream";

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
