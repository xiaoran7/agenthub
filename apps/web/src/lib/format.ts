import type {
  ActivityStatus,
  AgentType,
  Confidence,
  ObservationSource,
  Presence,
  WaitingReason,
} from "./types";

// Record<Enum, string> keeps every label map exhaustive at compile time
// without importing runtime values from the contracts package.

export const ACTIVITY_LABELS: Record<ActivityStatus, string> = {
  WORKING: "工作中",
  WAITING_INPUT: "等待输入",
  WAITING_PERMISSION: "等待权限",
  IDLE: "空闲",
  COMPLETED: "已完成",
  FAILED: "失败",
  STOPPED: "已停止",
  UNKNOWN: "未知",
};

export const PRESENCE_LABELS: Record<Presence, string> = {
  LIVE: "实时",
  PERSISTED: "历史",
  UNKNOWN: "未知",
};

export const AGENT_LABELS: Record<AgentType, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  antigravity: "Antigravity (Google)",
  grok_build: "Grok Build",
  zcode: "ZCode",
};

export const WAITING_REASON_LABELS: Record<WaitingReason, string> = {
  user_input: "等待用户输入",
  permission: "等待权限批准",
  sandbox: "等待沙箱批准",
  worker: "等待后台任务",
  dialog: "等待对话框",
  mcp_elicitation: "等待 MCP 提问",
  unknown: "等待原因未知",
};

export const CONFIDENCE_LABELS: Record<Confidence, string> = {
  authoritative: "权威",
  observed: "观察",
  inferred: "推断",
  unknown: "未知",
};

export const SOURCE_LABELS: Record<ObservationSource, string> = {
  claude_agents_json: "Claude CLI",
  claude_hook: "Claude Hook",
  codex_app_server: "Codex app-server",
  codex_hook: "Codex Hook",
  antigravity_brain: "Antigravity Brain",
  antigravity_cli: "Antigravity CLI",
  antigravity_hook: "Antigravity Hook",
  grok_sessions: "Grok Build 会话",
  grok_hook: "Grok Build Hook",
  grok_active_sessions: "Grok Build 活动会话",
  zcode_db: "ZCode 本地数据库",
  zcode_hook: "ZCode Hook",
  zcode_rollout: "ZCode Rollout",
  process_probe: "进程探测",
  local_history: "本地历史",
};

export function formatRelative(iso: string, nowMs: number): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return "时间未知";
  }
  const seconds = Math.floor(Math.max(0, nowMs - time) / 1000);
  if (seconds < 10) {
    return "刚刚";
  }
  if (seconds < 60) {
    return `${seconds} 秒前`;
  }
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) {
    return `${minutes} 分钟前`;
  }
  const hours = Math.floor(minutes / 60);
  if (hours < 24) {
    return `${hours} 小时前`;
  }
  const days = Math.floor(hours / 24);
  if (days < 30) {
    return `${days} 天前`;
  }
  return new Date(time).toISOString().slice(0, 10);
}

export function formatClock(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) {
    return "时间未知";
  }
  return new Date(time).toLocaleString("zh-CN", { hour12: false });
}

/** Shortens `dev_<32 hex>` for display without losing the recognizable head. */
export function deviceShortId(deviceId: string): string {
  return deviceId.length <= 14 ? deviceId : `${deviceId.slice(0, 12)}…`;
}
