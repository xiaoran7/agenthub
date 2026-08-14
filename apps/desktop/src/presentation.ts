import type { ActivityStatus, Session } from "./types";

export function activityTime(session: Session): string {
  return session.payload.last_activity_at ?? session.observed_at;
}

export function relativeTime(value: string, now = Date.now()): string {
  const seconds = Math.max(0, (now - Date.parse(value)) / 1_000);
  if (seconds < 60) return "刚刚";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)} 小时前`;
  return `${Math.floor(seconds / 86_400)} 天前`;
}

export function statusView(
  status: ActivityStatus,
  presence: string,
): { label: string; className: string } {
  if (status === "WORKING") return { label: "工作中", className: "working" };
  if (status === "WAITING_PERMISSION")
    return { label: "等待确认", className: "waiting" };
  if (status === "WAITING_INPUT")
    return { label: "等待输入", className: "waiting" };
  if (status === "IDLE") return { label: "空闲", className: "idle" };
  if (status === "FAILED") return { label: "失败", className: "failed" };
  if (presence === "PERSISTED")
    return { label: "历史会话", className: "history" };
  return { label: "未知", className: "history" };
}
