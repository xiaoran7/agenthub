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
