import assert from "node:assert/strict";
import test from "node:test";

import {
  filterSessions,
  matchesTab,
  sortSessions,
  tabCounts,
} from "../src/lib/classify";
import type { ActivityStatus, Presence, SessionRecord } from "../src/lib/types";

function makeSession(overrides: {
  presence?: Presence;
  activity?: ActivityStatus;
  device_id?: string;
  agent_session_id?: string;
  observed_at?: string;
  sequence?: number;
}): SessionRecord {
  return {
    sequence: overrides.sequence ?? 1,
    event_id: "123e4567-e89b-42d3-a456-426614174001",
    schema_version: 1,
    device_id: overrides.device_id ?? "dev_a",
    agent_type: "claude_code",
    agent_session_id: overrides.agent_session_id ?? "session-1",
    event_type: "session_observed",
    observed_at: overrides.observed_at ?? "2026-07-27T00:00:00Z",
    source: "claude_agents_json",
    confidence: "authoritative",
    adapter_version: "0.1.0",
    payload: {
      project: { display_name: "demo", fingerprint: "ab".repeat(32) },
      state: {
        presence: overrides.presence ?? "LIVE",
        activity_status: overrides.activity ?? "WORKING",
        raw_status: "working",
      },
    },
    received_at: "2026-07-27T00:00:01Z",
  };
}

test("tabs overlap by design: waiting is also live, finished can be historical", () => {
  const waiting = makeSession({
    presence: "LIVE",
    activity: "WAITING_PERMISSION",
  });
  assert.equal(matchesTab(waiting, "waiting"), true);
  assert.equal(matchesTab(waiting, "live"), true);
  assert.equal(matchesTab(waiting, "finished"), false);

  const historicalFailure = makeSession({
    presence: "PERSISTED",
    activity: "FAILED",
  });
  assert.equal(matchesTab(historicalFailure, "finished"), true);
  assert.equal(matchesTab(historicalFailure, "historical"), true);
  assert.equal(matchesTab(historicalFailure, "live"), false);

  const unknown = makeSession({ presence: "UNKNOWN", activity: "UNKNOWN" });
  assert.equal(matchesTab(unknown, "all"), true);
  assert.equal(matchesTab(unknown, "live"), false);
  assert.equal(matchesTab(unknown, "waiting"), false);
  assert.equal(matchesTab(unknown, "finished"), false);
  assert.equal(matchesTab(unknown, "historical"), false);
});

test("counts and filters respect the device filter", () => {
  const sessions = [
    makeSession({ device_id: "dev_a", activity: "WORKING" }),
    makeSession({
      device_id: "dev_b",
      agent_session_id: "session-2",
      activity: "WAITING_INPUT",
    }),
  ];
  const counts = tabCounts(sessions, undefined);
  assert.equal(counts.all, 2);
  assert.equal(counts.waiting, 1);
  const scoped = tabCounts(sessions, "dev_a");
  assert.equal(scoped.all, 1);
  assert.equal(scoped.waiting, 0);
  assert.equal(filterSessions(sessions, "waiting", "dev_b").length, 1);
  assert.equal(filterSessions(sessions, "waiting", "dev_a").length, 0);
});

test("waiting sessions sort first, then most recent observations", () => {
  const older = makeSession({
    agent_session_id: "older",
    observed_at: "2026-07-27T00:00:00Z",
    sequence: 1,
  });
  const newer = makeSession({
    agent_session_id: "newer",
    observed_at: "2026-07-27T00:05:00Z",
    sequence: 2,
  });
  const waiting = makeSession({
    agent_session_id: "waiting",
    activity: "WAITING_PERMISSION",
    observed_at: "2026-07-27T00:01:00Z",
    sequence: 3,
  });
  const sorted = sortSessions([older, newer, waiting]);
  assert.deepEqual(
    sorted.map((session) => session.agent_session_id),
    ["waiting", "newer", "older"],
  );
});

test("sequence breaks observation-time ties", () => {
  const first = makeSession({ agent_session_id: "first", sequence: 1 });
  const second = makeSession({ agent_session_id: "second", sequence: 2 });
  const sorted = sortSessions([first, second]);
  assert.deepEqual(
    sorted.map((session) => session.agent_session_id),
    ["second", "first"],
  );
});
