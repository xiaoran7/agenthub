import {
  FolderCheck,
  Radio,
  Plus,
  Terminal,
  ShieldCheck,
  Cpu,
  Layers,
  ArrowRight,
} from "lucide-react";

interface SessionEmptyStateProps {
  onNewSession: () => void;
  onOpenHostControl: () => void;
  hostPhase?:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  workspaceCount?: number;
  sessionCount?: number;
  displayName?: string;
}

const phaseLabels: Record<
  string,
  { label: string; tone: "green" | "yellow" | "gray" | "blue" }
> = {
  loading: { label: "检测中…", tone: "yellow" },
  starting: { label: "Runtime 启动中", tone: "yellow" },
  stopped: { label: "Daemon 待命", tone: "gray" },
  local: { label: "本机就绪 (Remote 未开)", tone: "blue" },
  connecting: { label: "正在连接 Gateway", tone: "yellow" },
  connected: { label: "Remote 已连接", tone: "green" },
};

export function SessionEmptyState({
  onNewSession,
  onOpenHostControl,
  hostPhase = "local",
  workspaceCount = 0,
  sessionCount = 0,
  displayName = "Developer",
}: SessionEmptyStateProps) {
  const currentPhase = phaseLabels[hostPhase] ?? {
    label: hostPhase,
    tone: "gray",
  };

  return (
    <div className="session-empty-container">
      <div className="session-empty-card">
        <header className="empty-header">
          <div className="empty-badge">
            <Radio
              size={12}
              className={`pulse-dot tone-${currentPhase.tone}`}
            />
            <span>Codex Remote Hub</span>
          </div>
          <h1>官方 Agent 远程继续与控制</h1>
          <p className="empty-subtitle">
            AgentHub 是官方 Agent
            的远程接续舱与守护中心。关闭此窗口后，会话仍将在后台由{" "}
            <code>agenthubd</code> 稳定守护。
          </p>
        </header>

        <div className="empty-grid">
          {/* Card 1: 会话接管 */}
          <section className="empty-feature-card">
            <div className="feature-icon-wrapper">
              <Layers size={20} />
            </div>
            <div className="feature-content">
              <h3>工作区与会话</h3>
              <p>
                从左侧边栏选择已有的 Codex 会话进行交互接管。实时流式观察 Agent
                思考、审批高风险工具、或直接发送后续指令。
              </p>
              <div className="feature-metrics">
                <span>
                  当前管理 <strong>{sessionCount}</strong> 个本机会话
                </span>
              </div>
              <button
                type="button"
                className="empty-cta-btn primary"
                onClick={onNewSession}
              >
                <Plus size={15} />
                <span>新建 Codex 会话</span>
              </button>
            </div>
          </section>

          {/* Card 2: 宿主与暴露 */}
          <section className="empty-feature-card">
            <div className="feature-icon-wrapper">
              <Cpu size={20} />
            </div>
            <div className="feature-content">
              <h3>本机守护与 Remote</h3>
              <p>
                控制当前电脑的 Gateway
                出站连接开关，管理工作区目录白名单。开启后可在网页端与移动端漫游续聊。
              </p>
              <div className="feature-metrics">
                <span className={`status-pill tone-${currentPhase.tone}`}>
                  {currentPhase.label}
                </span>
                <span>
                  已授权 <strong>{workspaceCount}</strong> 个工作目录
                </span>
              </div>
              <button
                type="button"
                className="empty-cta-btn secondary"
                onClick={onOpenHostControl}
              >
                <FolderCheck size={15} />
                <span>管理工作区与 Remote</span>
              </button>
            </div>
          </section>
        </div>

        <footer className="empty-footer-tip">
          <div className="tip-badge">
            <Terminal size={13} />
            <span>无缝接管</span>
          </div>
          <p>
            在已授权的任意项目目录下运行官方 <code>codex</code>
            ，会话将自动同步到此列表，随时接续。
          </p>
        </footer>
      </div>
    </div>
  );
}
