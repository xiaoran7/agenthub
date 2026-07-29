export type ConnectionStatusTone = "polling" | "live" | "offline" | "error";

export interface ConnectionStatus {
  tone: ConnectionStatusTone;
  label: string;
}

interface ConnectionStatusInput {
  configured: boolean;
  online: boolean;
  syncError: string | undefined;
  lastSyncLabel: string | undefined;
  streamConnected: boolean;
}

export function connectionStatus({
  configured,
  online,
  syncError,
  lastSyncLabel,
  streamConnected,
}: ConnectionStatusInput): ConnectionStatus {
  if (!configured) {
    return { tone: "polling", label: "初始化…" };
  }
  if (!online) {
    return { tone: "offline", label: "设备离线 · 保留上次数据" };
  }
  if (syncError !== undefined) {
    return { tone: "error", label: syncError };
  }
  if (lastSyncLabel === undefined) {
    return { tone: "polling", label: "连接网关中…" };
  }
  return {
    tone: streamConnected ? "live" : "polling",
    label: `${streamConnected ? "实时" : "轮询"} · 已更新 ${lastSyncLabel}`,
  };
}
