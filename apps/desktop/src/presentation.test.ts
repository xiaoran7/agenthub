import { describe, expect, it } from "vitest";

import {
  activityTime,
  adjacentItem,
  agentLabel,
  agentShortLabel,
  formatDetailedTime,
  groupSessionsByProvider,
  groupSessionsByWorkspace,
  hasActiveTurn,
  isAttentionSession,
  permissionFieldLabel,
  relativeTime,
  rememberTitleOverride,
  sessionDisplayTitle,
  sessionTooltip,
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
    expect(agentLabel("codex")).toBe("ChatGPT (Codex)");
    expect(agentLabel("zcode")).toBe("ZCode Agent");
    expect(agentLabel("grok_build")).toBe("Grok Build");
    expect(agentShortLabel("codex")).toBe("ChatGPT");
    expect(agentShortLabel("zcode")).toBe("ZCode");
    expect(agentShortLabel("grok_build")).toBe("Grok");
    expect(permissionFieldLabel("zcode")).toBe("执行策略");
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
    expect(groups[0].provider).toBe("claude_code");
    expect(groups[0].sessions).toHaveLength(2);
    expect(groups[1].provider).toBe("codex");
    expect(groups[1].sessions).toHaveLength(1);
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

  it("does not fall back to workspace name when title is missing or identical to workspace", () => {
    const untitledSession = {
      device_id: "dev-1",
      agent_type: "codex",
      agent_session_id: "019d1469-804d-78c2-bb58-a92c3a5ef599",
      payload: {
        project: { display_name: "ClaudeSpace" },
        runtime: { title: "ClaudeSpace" },
      },
    } as unknown as Session;

    expect(sessionDisplayTitle(untitledSession)).toBe("未命名会话 #019d1469");

    const noTitleSession = {
      device_id: "dev-1",
      agent_type: "codex",
      agent_session_id: "019d1469-804d-78c2-bb58-a92c3a5ef599",
      payload: {
        project: { display_name: "ClaudeSpace" },
      },
    } as unknown as Session;

    expect(sessionDisplayTitle(noTitleSession)).toBe("未命名会话 #019d1469");
  });

  it("prioritizes explicit valid runtime titles", () => {
    const session = {
      device_id: "dev-1",
      agent_type: "claude_code",
      agent_session_id: "sess-abc",
      payload: {
        project: { display_name: "ClaudeSpace" },
        runtime: { title: "实现会话重命名功能" },
      },
    } as unknown as Session;

    expect(sessionDisplayTitle(session)).toBe("实现会话重命名功能");
  });

  it("prioritizes user custom title overrides and indexes cross-keys", () => {
    const session = {
      device_id: "dev-1",
      agent_type: "codex",
      agent_session_id: "custom-sess-1234",
      payload: {
        project: { display_name: "ClaudeSpace" },
        runtime: { title: "原始标题" },
      },
    } as unknown as Session;

    rememberTitleOverride("dev-1:codex:custom-sess-1234", "重命名后的会话名称");
    expect(sessionDisplayTitle(session)).toBe("重命名后的会话名称");
  });

  it("resolves stripped prefixes for claude sessions between daemon and disk formats", () => {
    // Daemon assigns "claude_77813f44-uuid" while transcript uses "77813f44-uuid"
    rememberTitleOverride("claude_code:77813f44-0000-0000-0000-000000000000", "阅读全局skill并重构");

    const sessionWithPrefix = {
      device_id: "local",
      agent_type: "claude_code",
      agent_session_id: "claude_77813f44-0000-0000-0000-000000000000",
      payload: {
        project: { display_name: "ClaudeSpace" },
      },
    } as unknown as Session;

    expect(sessionDisplayTitle(sessionWithPrefix)).toBe("阅读全局skill并重构");

    const sessionBareUuid = {
      device_id: "local",
      agent_type: "claude_code",
      agent_session_id: "77813f44-0000-0000-0000-000000000000",
      payload: {
        project: { display_name: "ClaudeSpace" },
      },
    } as unknown as Session;

    expect(sessionDisplayTitle(sessionBareUuid)).toBe("阅读全局skill并重构");
  });

  it("formats detailed timestamp and generates comprehensive tooltip", () => {
    expect(formatDetailedTime("2026-03-05T08:30:15.000Z")).toContain(
      "2026-03-05",
    );
    const session = {
      device_id: "dev-1",
      agent_type: "codex",
      agent_session_id: "019d1469-804d-78c2-bb58-a92c3a5ef599",
      observed_at: "2026-03-05T08:30:15.000Z",
      payload: {
        project: { display_name: "ClaudeSpace" },
        runtime: { model: "o3-mini", permission_mode: "acceptEdits" },
      },
    } as unknown as Session;

    const tooltip = sessionTooltip(session, "会话 #019d1469");
    expect(tooltip).toContain("会话 #019d1469");
    expect(tooltip).toContain("019d1469-804d-78c2-bb58-a92c3a5ef599");
    expect(tooltip).toContain("工作区: ClaudeSpace");
    expect(tooltip).toContain("模型: o3-mini");
    expect(tooltip).toContain("模式: acceptEdits");
  });
});
