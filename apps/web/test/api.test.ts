import assert from "node:assert/strict";
import test from "node:test";

import {
  ApiError,
  describeApiError,
  devicesUrl,
  sessionEventsUrl,
  sessionsUrl,
} from "../src/lib/api";

test("read URLs work same-origin and against an explicit base", () => {
  assert.equal(devicesUrl(""), "/v1/devices");
  assert.equal(
    sessionsUrl("http://127.0.0.1:3001"),
    "http://127.0.0.1:3001/v1/sessions",
  );
});

test("timeline URLs escape session identifiers", () => {
  const url = sessionEventsUrl(
    "",
    {
      device_id: "dev_56475aa75463474c0285df5dbf2bcab7",
      agent_type: "codex",
      agent_session_id: "thread #1&2",
    },
    100,
  );
  assert.equal(
    url,
    "/v1/sessions/events?device_id=dev_56475aa75463474c0285df5dbf2bcab7" +
      "&agent_type=codex&agent_session_id=thread+%231%262&limit=100",
  );
});

test("errors map to operator-readable messages", () => {
  assert.equal(
    describeApiError(new ApiError(0, "network_error")),
    "无法连接网关",
  );
  assert.equal(
    describeApiError(new ApiError(401, "authentication_required")),
    "网关访问令牌缺失或无效，请检查服务端配置",
  );
  assert.equal(
    describeApiError(new ApiError(403, "forbidden")),
    "网关拒绝了当前身份",
  );
  assert.equal(
    describeApiError(new ApiError(500, "internal_error")),
    "网关内部错误",
  );
  assert.equal(
    describeApiError(new ApiError(400, "invalid_query")),
    "网关返回错误（400 invalid_query）",
  );
  assert.equal(describeApiError(new Error("boom")), "未知错误");
});
