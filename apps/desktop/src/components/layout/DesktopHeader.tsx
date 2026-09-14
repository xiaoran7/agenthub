import {
  Radio,
  RadioTower,
  Plus,
  Settings,
  CircleUserRound,
  RefreshCw,
  FolderTree,
} from "lucide-react";
import type { Snapshot } from "../../types";

interface DesktopHeaderProps {
  hostPhase?:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  onOpenHostControl: () => void;
  onOpenSettings: () => void;
  onNewSession: () => void;
  snapshot: Snapshot;
  onAccountToggle: () => void;
  refreshing?: boolean;
  onRefresh?: () => void;
}

const phaseInfo: Record<
  string,
  { label: string; tone: "green" | "yellow" | "gray" | "blue" }
> = {
  loading: { label: "探测中", tone: "yellow" },
  starting: { label: "Runtime 启动中", tone: "yellow" },
  stopped: { label: "Daemon 待命", tone: "gray" },
  local: { label: "本机就绪 (Remote 关)", tone: "blue" },
  connecting: { label: "连接 Gateway…", tone: "yellow" },
  connected: { label: "Remote 在线", tone: "green" },
};

export function DesktopHeader({
  hostPhase = "local",
  onOpenHostControl,
  onOpenSettings,
  onNewSession,
  snapshot,
  onAccountToggle,
  refreshing = false,
  onRefresh,
}: DesktopHeaderProps) {
  const current = phaseInfo[hostPhase] ?? { label: hostPhase, tone: "gray" };

  return (
    <header className="desktop-header">
      <div className="header-left">
        <div className="header-brand">
          <span className="brand-logo">AgentHub</span>
          <span className="brand-badge">Codex Remote</span>
        </div>

        {/* Native Host status pill */}
        <button
          type="button"
          className="host-status-trigger"
          onClick={onOpenHostControl}
          title="点击管理本机守护进程与 Remote 出站连接"
        >
          <span className={`status-indicator tone-${current.tone}`} />
          <span className="status-label">{current.label}</span>
          <span className="status-workspaces">
            {snapshot.local.sources.length} 个工作区
          </span>
        </button>
      </div>

      <div className="header-right">
        {onRefresh && (
          <button
            type="button"
            className="header-icon-btn"
            disabled={refreshing}
            onClick={onRefresh}
            title="刷新本机状态"
            aria-label="刷新本机状态"
          >
            <RefreshCw className={refreshing ? "spin" : ""} size={15} />
          </button>
        )}

        <button
          type="button"
          className="header-icon-btn"
          onClick={onOpenSettings}
          title="设置"
          aria-label="设置"
        >
          <Settings size={15} />
        </button>

        <button
          type="button"
          className="header-primary-btn"
          onClick={onNewSession}
          title="新建任务会话"
        >
          <Plus size={15} />
          <span>新建会话</span>
        </button>

        <button
          type="button"
          className="header-user-btn"
          onClick={onAccountToggle}
          title="账户菜单"
          aria-label="账户菜单"
        >
          <CircleUserRound size={17} />
          <span className="user-name">
            {snapshot.account.user.display_name ||
              snapshot.account.user.email.split("@")[0] ||
              "用户"}
          </span>
        </button>
      </div>
    </header>
  );
}
