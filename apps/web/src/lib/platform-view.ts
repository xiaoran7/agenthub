import type { ActivityStatus, AgentType, AuthRecord, AuthState } from "./types";
import type { DeviceRecord, SessionRecord } from "./types";

export type SectionKey =
  | "overview"
  | "sessions"
  | "devices"
  | "activity"
  | "diagnostics"
  | "help";

export type SessionFilter =
  "all" | "running" | "waiting" | "completed" | "failed";

export type ProviderFilter = "all" | AgentType;
export type RangeFilter = "24h" | "7d" | "all";

export const NAV_ITEMS: ReadonlyArray<{
  key: SectionKey;
  label: string;
  icon:
    | "overview"
    | "sessions"
    | "devices"
    | "activity"
    | "diagnostics"
    | "help";
}> = [
  { key: "overview", label: "总览", icon: "overview" },
  { key: "sessions", label: "会话", icon: "sessions" },
  { key: "devices", label: "设备", icon: "devices" },
  { key: "activity", label: "动态", icon: "activity" },
  { key: "diagnostics", label: "诊断", icon: "diagnostics" },
];

export const MOBILE_NAV_ITEMS = [
  ...NAV_ITEMS,
  { key: "help", label: "帮助", icon: "help" },
] as const;

export const SECTION_LABELS: Record<SectionKey, string> = {
  overview: "总览",
  sessions: "会话",
  devices: "设备",
  activity: "动态",
  diagnostics: "诊断",
  help: "帮助",
};

export const SESSION_FILTERS: ReadonlyArray<{
  key: SessionFilter;
  label: string;
}> = [
  { key: "all", label: "全部" },
  { key: "running", label: "运行中" },
  { key: "waiting", label: "等待中" },
  { key: "completed", label: "已完成" },
  { key: "failed", label: "失败" },
];

export const AGENT_LABELS: Record<AgentType, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  antigravity: "Antigravity (Google)",
  grok_build: "Grok Build",
};

export interface AuthStatusPresentation {
  label: string;
  tone: "live" | "waiting" | "error" | "idle" | "unknown";
}

export function authStatus(snapshot: AuthRecord): AuthStatusPresentation {
  const labels: Record<AuthState, AuthStatusPresentation> = {
    authenticated: { label: "已认证", tone: "live" },
    unauthenticated: { label: "未认证", tone: "idle" },
    reauth_required: { label: "需重新认证", tone: "waiting" },
    unknown: { label: "未知", tone: "unknown" },
  };
  return labels[snapshot.state];
}

export function credentialSourceLabel(source: AuthRecord["source"]): string {
  const labels: Record<AuthRecord["source"], string> = {
    official_cli: "官方 CLI",
    app_server: "App Server",
    environment: "环境变量",
    file: "本地凭据文件",
    keyring: "系统 Keychain",
    cache: "加密缓存",
    unknown: "未知来源",
  };
  return labels[source];
}

export function quotaLabel(snapshot: AuthRecord): string {
  const quota = snapshot.quota;
  if (quota === undefined || quota.state === "unavailable") return "未提供配额";
  if (quota.state === "exhausted") return "额度已耗尽";
  const window = quota.windows.find(
    (entry) => entry.used_percent !== undefined,
  );
  if (window?.used_percent !== undefined) {
    return `${window.name} 已用 ${window.used_percent}%`;
  }
  return quota.state === "available" ? "配额可用" : "配额未知";
}

export interface StatusPresentation {
  label: string;
  tone: "live" | "waiting" | "completed" | "error" | "idle" | "unknown";
}

export function statusPresentation(
  status: ActivityStatus,
  presence: string,
): StatusPresentation {
  switch (status) {
    case "WORKING":
      return { label: "运行中", tone: "live" };
    case "WAITING_INPUT":
    case "WAITING_PERMISSION":
      return { label: "等待中", tone: "waiting" };
    case "COMPLETED":
      return { label: "已完成", tone: "completed" };
    case "FAILED":
      return { label: "失败", tone: "error" };
    case "STOPPED":
      return { label: "已停止", tone: "idle" };
    case "IDLE":
      return { label: presence === "LIVE" ? "空闲" : "历史", tone: "idle" };
    default:
      return { label: "未知", tone: "unknown" };
  }
}

export function sessionTask(session: SessionRecord): string {
  const details = session.payload.state.raw_status_detail;
  const task = details?.task ?? details?.summary ?? details?.message;
  if (task !== undefined && task.trim() !== "") {
    return task;
  }
  if (session.payload.state.activity_status === "WAITING_INPUT") {
    return "等待用户输入";
  }
  if (session.payload.state.activity_status === "WAITING_PERMISSION") {
    return "等待权限批准";
  }
  if (session.payload.state.activity_status === "FAILED") {
    return "工具调用失败";
  }
  if (session.payload.state.activity_status === "COMPLETED") {
    return "会话已完成";
  }
  return "正在监控会话状态";
}

export function sessionTool(session: SessionRecord): string | undefined {
  return session.payload.state.raw_status_detail?.tool;
}

export function sessionStatus(session: SessionRecord): StatusPresentation {
  return statusPresentation(
    session.payload.state.activity_status,
    session.payload.state.presence,
  );
}

export function matchesSessionFilter(
  session: SessionRecord,
  filter: SessionFilter,
): boolean {
  switch (filter) {
    case "all":
      return true;
    case "running":
      return session.payload.state.activity_status === "WORKING";
    case "waiting":
      return (
        session.payload.state.activity_status === "WAITING_INPUT" ||
        session.payload.state.activity_status === "WAITING_PERMISSION"
      );
    case "completed":
      return ["COMPLETED", "STOPPED", "IDLE"].includes(
        session.payload.state.activity_status,
      );
    case "failed":
      return session.payload.state.activity_status === "FAILED";
  }
}

export function sessionSortRank(session: SessionRecord): number {
  const status = session.payload.state.activity_status;
  if (status === "FAILED") return 0;
  if (status === "WAITING_INPUT" || status === "WAITING_PERMISSION") return 1;
  if (status === "WORKING") return 2;
  if (status === "IDLE") return 3;
  if (status === "UNKNOWN") return 4;
  return 5;
}

export function sortPlatformSessions(
  sessions: SessionRecord[],
): SessionRecord[] {
  return [...sessions].sort((left, right) => {
    const rank = sessionSortRank(left) - sessionSortRank(right);
    if (rank !== 0) return rank;
    return Date.parse(right.observed_at) - Date.parse(left.observed_at);
  });
}

export function formatRelativeCompact(iso: string, nowMs: number): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return "时间未知";
  const seconds = Math.max(0, Math.floor((nowMs - time) / 1000));
  if (seconds < 10) return "刚刚";
  if (seconds < 60) return `${seconds}秒前`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}小时前`;
  return `${Math.floor(hours / 24)}天前`;
}

export function formatClockCompact(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return "时间未知";
  return new Date(time).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });
}

export function formatDateTimeCompact(iso: string): string {
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return "时间未知";
  return new Date(time).toLocaleString("zh-CN", {
    dateStyle: "medium",
    timeStyle: "medium",
    hour12: false,
  });
}

export function formatDuration(
  startedAt: string | undefined,
  nowMs: number,
): string {
  if (startedAt === undefined) return "—";
  const start = Date.parse(startedAt);
  if (Number.isNaN(start)) return "—";
  const seconds = Math.max(0, Math.floor((nowMs - start) / 1000));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const remainder = seconds % 60;
  return [hours, minutes, remainder]
    .map((value) => String(value).padStart(2, "0"))
    .join(":");
}

export function deviceLabel(
  device: DeviceRecord | undefined,
  deviceId: string,
): string {
  if (device?.display_name !== undefined) return device.display_name;
  return `设备 ${deviceId.replace(/^dev_/, "").slice(0, 6).toUpperCase()}`;
}

export function devicePlatform(device: DeviceRecord | undefined): string {
  return device?.os ?? "Runner 信息不可用";
}

export function deviceVersion(device: DeviceRecord | undefined): string {
  return device?.runner_version ?? "版本未上报";
}

export function deviceSessionCount(
  sessions: SessionRecord[],
  deviceId: string,
): number {
  return sessions.filter((session) => session.device_id === deviceId).length;
}

export function eventLabel(session: SessionRecord): string {
  const status = sessionStatus(session);
  if (status.tone === "waiting") return "会话等待";
  if (status.tone === "error") return "会话错误";
  if (status.tone === "completed") return "会话完成";
  if (sessionTool(session) !== undefined) return "工具调用";
  return "会话更新";
}

export function eventDescription(session: SessionRecord): string {
  const status = sessionStatus(session);
  if (status.tone === "waiting") return "会话正在等待你的输入";
  if (status.tone === "error") return "Agent 报告了一次失败操作";
  if (status.tone === "completed") return "会话已成功完成";
  if (sessionTool(session) !== undefined) {
    return `${AGENT_LABELS[session.agent_type]} 调用了 ${sessionTool(session)}`;
  }
  return `${AGENT_LABELS[session.agent_type]} 会话已更新`;
}

export function errorCount(sessions: SessionRecord[]): number {
  return sessions.filter(
    (session) => session.payload.state.activity_status === "FAILED",
  ).length;
}
