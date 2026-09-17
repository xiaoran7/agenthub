import { FolderCheck, Radio, Plus } from "lucide-react";

interface SessionEmptyStateProps {
  onNewSession: () => void;
  onOpenHostControl: () => void;
  hostPhase?:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  workspaceCount?: number;
  sessionCount?: number;
  displayName?: string;
  activeProviderLabel?: string;
  activeProviderType?: string;
}

const phaseLabels = {
  loading: { label: "正在检查连接…", tone: "yellow" },
  starting: { label: "正在启动本机服务…", tone: "yellow" },
  stopped: { label: "本机服务未启动", tone: "gray" },
  local: { label: "本机会话 · 远程连接未开启", tone: "blue" },
  connecting: { label: "正在连接远程服务…", tone: "yellow" },
  connected: { label: "远程已连接", tone: "green" },
} as const;

export function SessionEmptyState({
  onNewSession,
  onOpenHostControl,
  hostPhase = "local",
  workspaceCount = 0,
  sessionCount = 0,
  activeProviderLabel = "OpenAI Codex",
}: SessionEmptyStateProps) {
  const currentPhase = phaseLabels[hostPhase];

  return (
    <div className="session-empty-container">
      <div className="session-empty-content">
        <header className="empty-header">
          <div className="empty-badge" role="status">
            <Radio
              size={12}
              className={`pulse-dot tone-${currentPhase.tone}`}
            />
            <span>{currentPhase.label}</span>
          </div>
          <h1>换个设备，继续会话</h1>
          <p className="empty-subtitle">
            在这台电脑上新建或恢复会话，开启远程连接后，即可从网页或手机继续。
          </p>
        </header>
        <div className="empty-actions">
          <button
            type="button"
            className="empty-cta-btn primary"
            onClick={onNewSession}
          >
            <Plus size={15} />
            <span>新建 {activeProviderLabel} 会话</span>
          </button>
          <button
            type="button"
            className="empty-cta-btn secondary"
            onClick={onOpenHostControl}
          >
            <FolderCheck size={15} />
            <span>管理工作目录与远程连接</span>
          </button>
        </div>
        <p className="empty-session-note">
          {sessionCount > 0
            ? `左侧有 ${sessionCount} 个会话可供选择；恢复成功后才能继续发送消息。`
            : "还没有会话，从新建会话开始。"}
        </p>
        <p className="empty-footer-tip">
          已授权 {workspaceCount}{" "}
          个工作目录。远程使用期间，请保持这台电脑开机并联网。
        </p>
      </div>
    </div>
  );
}
