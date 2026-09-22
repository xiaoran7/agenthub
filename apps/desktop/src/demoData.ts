import type { ActivityStatus, Session, Snapshot } from "./types";

export function demoSession(
  agent: Session["agent_type"],
  project: string,
  activity: ActivityStatus,
  minutesAgo: number,
  presence = "LIVE",
): Session {
  const observedAt = new Date(Date.now() - minutesAgo * 60_000).toISOString();
  return {
    event_id: agent + "-" + project,
    device_id: "dev_demo",
    agent_type: agent,
    agent_session_id: agent + "-" + project,
    observed_at: observedAt,
    sync_state: minutesAgo < 3 ? "local" : "synced",
    payload: {
      project: { display_name: project, fingerprint: "demo" },
      state: { activity_status: activity, presence },
      last_activity_at: observedAt,
      runtime: {
        agent_type: agent,
        agent_session_id: agent + "-" + project,
        title:
          agent === "claude_code"
            ? project + "实施计划"
            : agent === "zcode"
              ? project + "全栈开发"
              : agent === "grok_build"
                ? project + "采集适配"
                : project + "接口与测试",
        model:
          agent === "claude_code"
            ? "claude-3-7-sonnet"
            : agent === "zcode"
              ? "gemini-3.8-flash-high"
              : agent === "grok_build"
                ? "grok-4.6"
                : "gpt-5.5-codex",
        reasoning_effort: agent === "codex" ? "medium" : "high",
        permission_mode:
          agent === "claude_code"
            ? "acceptEdits"
            : agent === "zcode"
              ? "auto-execute"
              : agent === "grok_build"
                ? "grok-build-plan"
                : undefined,
        approval_policy: agent === "codex" ? "on-request" : undefined,
        sandbox_mode:
          agent === "codex"
            ? "workspace-write"
            : agent === "grok_build"
              ? "off"
              : undefined,
        context_window_tokens:
          agent === "claude_code"
            ? 200_000
            : agent === "zcode"
              ? 1_000_000
              : agent === "grok_build"
                ? 256_000
                : 258_400,
        context_used_tokens:
          agent === "claude_code"
            ? 64_800
            : agent === "zcode"
              ? 85_600
              : agent === "grok_build"
                ? 48_200
                : 92_300,
        session_total_tokens:
          agent === "claude_code"
            ? 91_200
            : agent === "zcode"
              ? 128_000
              : agent === "grok_build"
                ? 72_400
                : 184_600,
      },
    },
  };
}

export const demoSnapshot: Snapshot = {
  account: {
    user: {
      user_id: "demo",
      email: "dev@example.com",
      display_name: "Developer",
    },
    subscription: { plan: "pro", status: "active" },
    entitlements: { max_devices: 5, history_days: 90 },
  },
  device: { device_id: "dev_demo", name: "DESKTOP-7K2A" },
  devices: [
    {
      device_id: "dev_demo",
      online: true,
      revoked: false,
      last_seen_at: new Date().toISOString(),
    },
  ],
  sync_running: true,
  realtime_connected: true,
  server_url: "https://platform.tannerlab.xyz",
  local: {
    pending_events: 2,
    session_count: 307,
    sources: [
      {
        agent_type: "claude_code",
        session_count: 95,
        detected: true,
        status: "healthy",
      },
      {
        agent_type: "codex",
        session_count: 212,
        detected: true,
        status: "healthy",
      },
      {
        agent_type: "antigravity",
        session_count: 7,
        detected: true,
        status: "healthy",
      },
      {
        agent_type: "grok_build",
        session_count: 4,
        detected: true,
        status: "healthy",
      },
    ],
    refreshed_at: new Date().toISOString(),
  },
  preferences: {
    refresh_interval_seconds: 4,
    claude_binary: "claude",
    codex_binary: "codex",
    grok_binary: "grok",
  },
  sessions: [
    demoSession("claude_code", "电商平台重构", "WORKING", 2),
    demoSession("claude_code", "支付服务优化", "WAITING_PERMISSION", 8),
    demoSession("claude_code", "用户增长分析", "IDLE", 25),
    demoSession("codex", "API 网关开发", "WORKING", 1),
    demoSession("codex", "数据导出工具", "UNKNOWN", 180, "PERSISTED"),
    demoSession("zcode", "AgentHub Remote 核心接入", "WORKING", 1),
    demoSession("antigravity", "搜索体验实验", "IDLE", 44),
    demoSession("grok_build", "AgentHub 采集", "WORKING", 3),
  ],
};
