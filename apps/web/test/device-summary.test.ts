import assert from "node:assert/strict";
import test from "node:test";

import { summarizeDevices } from "../src/lib/device-summary";
import type { DeviceRecord, SessionRecord } from "../src/lib/types";

const devices: DeviceRecord[] = [
  {
    device_id: "dev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    created_at: "2026-07-28T00:00:00.000Z",
    last_seen_at: "2026-07-28T00:01:00.000Z",
    online: true,
    revoked: false,
  },
  {
    device_id: "dev_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    created_at: "2026-07-28T00:00:00.000Z",
    last_seen_at: "2026-07-28T00:00:00.000Z",
    online: false,
    revoked: true,
  },
];

function session(
  deviceId: string,
  agentType: SessionRecord["agent_type"],
  presence: SessionRecord["payload"]["state"]["presence"],
  activity: SessionRecord["payload"]["state"]["activity_status"],
  sequence: number,
): SessionRecord {
  return {
    schema_version: 1,
    event_id: `00000000-0000-4000-8000-${String(sequence).padStart(12, "0")}`,
    device_id: deviceId,
    sequence,
    agent_type: agentType,
    agent_session_id: `session-${sequence}`,
    event_type: "session_observed",
    observed_at: "2026-07-28T00:01:00.000Z",
    source: "codex_app_server",
    confidence: "observed",
    adapter_version: "0.1.0",
    payload: {
      project: {
        display_name: "project",
        fingerprint: "a".repeat(64),
      },
      state: {
        presence,
        activity_status: activity,
        raw_status: activity,
      },
    },
    received_at: "2026-07-28T00:01:00.000Z",
  };
}

test("device summaries derive agents and overlapping session counts", () => {
  const summaries = summarizeDevices(devices, [
    session(devices[0]!.device_id, "codex", "LIVE", "WORKING", 1),
    session(
      devices[0]!.device_id,
      "claude_code",
      "LIVE",
      "WAITING_PERMISSION",
      2,
    ),
    session(devices[0]!.device_id, "codex", "PERSISTED", "FAILED", 3),
  ]);

  assert.deepEqual(summaries[0], {
    device: devices[0],
    agents: ["claude_code", "codex"],
    totalSessions: 3,
    liveSessions: 2,
    waitingSessions: 1,
  });
  assert.deepEqual(summaries[1], {
    device: devices[1],
    agents: [],
    totalSessions: 0,
    liveSessions: 0,
    waitingSessions: 0,
  });
});
