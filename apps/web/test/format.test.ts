import assert from "node:assert/strict";
import test from "node:test";

import { deviceShortId, formatRelative } from "../src/lib/format";

const now = Date.parse("2026-07-27T12:00:00Z");

test("relative times step through units", () => {
  assert.equal(formatRelative("2026-07-27T11:59:55Z", now), "刚刚");
  assert.equal(formatRelative("2026-07-27T11:59:10Z", now), "50 秒前");
  assert.equal(formatRelative("2026-07-27T11:15:00Z", now), "45 分钟前");
  assert.equal(formatRelative("2026-07-27T03:00:00Z", now), "9 小时前");
  assert.equal(formatRelative("2026-07-24T12:00:00Z", now), "3 天前");
  assert.equal(formatRelative("2026-05-01T00:00:00Z", now), "2026-05-01");
});

test("future timestamps from clock skew read as now", () => {
  assert.equal(formatRelative("2026-07-27T12:00:30Z", now), "刚刚");
});

test("invalid timestamps do not crash the panel", () => {
  assert.equal(formatRelative("not-a-date", now), "时间未知");
});

test("device IDs shorten but stay recognizable", () => {
  assert.equal(
    deviceShortId("dev_56475aa75463474c0285df5dbf2bcab7"),
    "dev_56475aa7…",
  );
  assert.equal(deviceShortId("dev_short"), "dev_short");
});
