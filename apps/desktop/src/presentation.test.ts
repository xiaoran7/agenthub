import { describe, expect, it } from "vitest";

import {
  activityTime,
  adjacentItem,
  agentLabel,
  agentShortLabel,
  groupSessionsByProvider,
  groupSessionsByWorkspace,
  hasActiveTurn,
  isAttentionSession,
  permissionFieldLabel,
  relativeTime,
  statusView,
} from "./presentation";
import type { Session } from "./types";

describe("desktop presentation", () => {
  it("keeps persisted sessions visibly distinct from unknown live sessions", () => {
    expect(statusView("UNKNOWN", "PERSISTED")).toEqual({
      label: "历史会话",
      className: "history",
    });
    expect(statusView("UNKNOWN", "LIVE")).toEqual({
      label: "未知",
      className: "history",
    });
  });

  it("uses the last activity timestamp when available", () => {
    const session = {
      observed_at: "2026-08-14T10:00:00.000Z",
      payload: { last_activity_at: "2026-08-14T09:59:00.000Z" },
    } as Session;

    expect(activityTime(session)).toBe("2026-08-14T09:59:00.000Z");
  });

  it("formats bounded relative times", () => {
    const now = Date.parse("2026-08-14T10:00:00.000Z");
    expect(relativeTime("2026-08-14T09:59:30.000Z", now)).toBe("刚刚");
    expect(relativeTime("2026-08-14T09:52:00.000Z", now)).toBe("8 分钟前");
    expect(relativeTime("2026-08-12T10:00:00.000Z", now)).toBe("2 天前");
    expect(relativeTime("not-a-date", now)).toBe("时间未知");
  });

  it("names Grok Build after Claude Code and Codex", () => {
    expect(agentLabel("claude_code")).toBe("Claude Code");
    expect(agentLabel("codex")).toBe("Codex");
    expect(agentLabel("grok_build")).toBe("Grok Build");
    expect(agentShortLabel("grok_build")).toBe("Grok");
    expect(permissionFieldLabel("grok_build")).toBe("Agent 配置");
  });

  it("moves within the current ordered result and stops at both boundaries", () => {
    const items = ["newest", "middle", "oldest"];
    expect(adjacentItem(items, 1, -1)).toBe("newest");
    expect(adjacentItem(items, 1, 1)).toBe("oldest");
    expect(adjacentItem(items, 0, -1)).toBeUndefined();
    expect(adjacentItem(items, 2, 1)).toBeUndefined();
    expect(adjacentItem(items, -1, 1)).toBeUndefined();
  });

  it("groups sessions by provider preserving order", () => {
    const s1 = {
      agent_type: "claude_code",
      payload: { state: { activity_status: "WORKING" } },
    } as Session;
    const s2 = {
      agent_type: "codex",
      payload: { state: { activity_status: "WAITING_PERMISSION" } },
    } as Session;
    const s3 = {
      agent_type: "claude_code",
      payload: { state: { activity_status: "IDLE" } },
    } as Session;

    const groups = groupSessionsByProvider([s1, s2, s3]);
    expect(groups).toHaveLength(2);
    expect(groups[0].provider).toBe("codex");
    expect(groups[0].sessions).toHaveLength(1);
    expect(groups[1].provider).toBe("claude_code");
    expect(groups[1].sessions).toHaveLength(2);
  });

  it("groups the selected agent sessions by workspace preserving recency order", () => {
    const sessions = [
      { payload: { project: { display_name: "ClaudeSpace" } } },
      { payload: { project: { display_name: "Solum" } } },
      { payload: { project: { display_name: "ClaudeSpace" } } },
    ] as Session[];

    expect(groupSessionsByWorkspace(sessions)).toEqual([
      { name: "ClaudeSpace", sessions: [sessions[0], sessions[2]] },
      { name: "Solum", sessions: [sessions[1]] },
    ]);
  });

  it("restores active turn state from daemon activity status", () => {
    expect(hasActiveTurn("WORKING")).toBe(true);
    expect(hasActiveTurn("WAITING_PERMISSION")).toBe(true);
    expect(hasActiveTurn("WAITING_INPUT")).toBe(true);
    expect(hasActiveTurn("IDLE")).toBe(false);
    expect(hasActiveTurn("COMPLETED")).toBe(false);
    expect(hasActiveTurn("FAILED")).toBe(false);
    expect(hasActiveTurn("UNKNOWN")).toBe(false);
  });

  it("identifies sessions requiring attention", () => {
    expect(
      isAttentionSession({
        payload: { state: { activity_status: "WAITING_PERMISSION" } },
      } as Session),
    ).toBe(true);
    expect(
      isAttentionSession({
        payload: { state: { activity_status: "WAITING_INPUT" } },
      } as Session),
    ).toBe(true);
    expect(
      isAttentionSession({
        payload: { state: { activity_status: "WORKING" } },
      } as Session),
    ).toBe(false);
  });
});
