import assert from "node:assert/strict";
import test from "node:test";

import { connectionStatus } from "../src/lib/connection-status";

test("offline state is immediate and keeps the last snapshot message", () => {
  assert.deepEqual(
    connectionStatus({
      configured: true,
      online: false,
      syncError: "旧的网关错误",
      lastSyncLabel: "20 秒前",
      streamConnected: true,
    }),
    {
      tone: "offline",
      label: "设备离线 · 保留上次数据",
    },
  );
});

test("gateway errors take precedence after the browser is online", () => {
  assert.deepEqual(
    connectionStatus({
      configured: true,
      online: true,
      syncError: "网关不可达",
      lastSyncLabel: "20 秒前",
      streamConnected: false,
    }),
    {
      tone: "error",
      label: "网关不可达",
    },
  );
});

test("connected and fallback polling labels stay distinct", () => {
  assert.deepEqual(
    connectionStatus({
      configured: true,
      online: true,
      syncError: undefined,
      lastSyncLabel: "刚刚",
      streamConnected: true,
    }),
    {
      tone: "live",
      label: "实时 · 已更新 刚刚",
    },
  );
  assert.deepEqual(
    connectionStatus({
      configured: true,
      online: true,
      syncError: undefined,
      lastSyncLabel: "5 秒前",
      streamConnected: false,
    }),
    {
      tone: "polling",
      label: "轮询 · 已更新 5 秒前",
    },
  );
});
