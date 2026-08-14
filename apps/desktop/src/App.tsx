import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  ChevronRight,
  CircleUserRound,
  Cloud,
  Code2,
  Eye,
  EyeOff,
  LogOut,
  Monitor,
  Pause,
  Play,
  RefreshCw,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";

import { activityTime, relativeTime, statusView } from "./presentation";
import type { ActivityStatus, Session, Snapshot } from "./types";

type AuthMode = "login" | "register";

export function App() {
  const demo =
    import.meta.env.DEV &&
    new URLSearchParams(window.location.search).has("demo");
  const [snapshot, setSnapshot] = useState<Snapshot | undefined>(() =>
    demo ? demoSnapshot : undefined,
  );
  const [restoring, setRestoring] = useState(!demo);
  const [error, setError] = useState<string>();

  const restore = useCallback(async () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      setRestoring(false);
      return;
    }
    try {
      const restored = await invoke<Snapshot | null>("restore_session");
      if (restored !== null) setSnapshot(restored);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setRestoring(false);
    }
  }, []);

  useEffect(() => {
    if (!demo) void restore();
  }, [demo, restore]);

  if (restoring) return <LoadingScreen />;
  if (snapshot === undefined) {
    return (
      <AuthScreen
        initialError={error}
        onAuthenticated={(next) => {
          setError(undefined);
          setSnapshot(next);
        }}
      />
    );
  }
  return (
    <Dashboard
      initialSnapshot={snapshot}
      onLogout={() => setSnapshot(undefined)}
    />
  );
}

function LoadingScreen() {
  return (
    <main className="loading-screen">
      <PulseMark />
      <span>正在连接本机 Agent…</span>
    </main>
  );
}

function AuthScreen({
  initialError,
  onAuthenticated,
}: {
  initialError?: string;
  onAuthenticated: (snapshot: Snapshot) => void;
}) {
  const [mode, setMode] = useState<AuthMode>("login");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(initialError);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const next = await invoke<Snapshot>("authenticate", {
        input: {
          mode,
          email,
          password,
          display_name: mode === "register" ? displayName : null,
        },
      });
      onAuthenticated(next);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="auth-shell">
      <section className="auth-story">
        <Brand />
        <div className="auth-story-copy">
          <PulseMark large />
          <h1>
            本地会话，自动发现，
            <br />
            安全同步。
          </h1>
          <p>
            安装后只需登录一次。Claude Code 与 Codex
            会话会自动出现在你的全部设备上。
          </p>
        </div>
        <LocalCloudDiagram />
      </section>
      <section className="auth-panel">
        <form onSubmit={submit} className="auth-form">
          <header>
            <h2>
              {mode === "login" ? "登录 AgentPulse" : "创建 AgentPulse 账号"}
            </h2>
            <p>
              {mode === "login"
                ? "继续管理这台电脑上的 Agent 会话。"
                : "一个账号即可连接你的所有设备。"}
            </p>
          </header>
          <div className="mode-switch" aria-label="账号操作">
            <button
              type="button"
              aria-current={mode === "login"}
              onClick={() => setMode("login")}
            >
              登录
            </button>
            <button
              type="button"
              aria-current={mode === "register"}
              onClick={() => setMode("register")}
            >
              注册
            </button>
          </div>
          {mode === "register" && (
            <label>
              <span>显示名称</span>
              <input
                autoComplete="name"
                value={displayName}
                onChange={(event) => setDisplayName(event.target.value)}
                required
              />
            </label>
          )}
          <label>
            <span>邮箱</span>
            <input
              autoFocus
              autoComplete="email"
              type="email"
              placeholder="name@example.com"
              value={email}
              onChange={(event) => setEmail(event.target.value)}
              required
            />
          </label>
          <label>
            <span>密码</span>
            <div className="password-field">
              <input
                autoComplete={
                  mode === "login" ? "current-password" : "new-password"
                }
                minLength={10}
                type={showPassword ? "text" : "password"}
                placeholder="至少 10 个字符"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
              <button
                type="button"
                aria-label={showPassword ? "隐藏密码" : "显示密码"}
                onClick={() => setShowPassword((value) => !value)}
              >
                {showPassword ? <EyeOff /> : <Eye />}
              </button>
            </div>
          </label>
          {error !== undefined && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button className="primary-button" disabled={busy} type="submit">
            {busy ? (
              <>
                <RefreshCw className="spin" /> 正在连接
              </>
            ) : mode === "login" ? (
              "登录并连接这台电脑"
            ) : (
              "注册并连接这台电脑"
            )}
          </button>
          <p className="privacy-note">
            <ShieldCheck /> 仅同步会话状态，不上传对话内容或登录凭据
          </p>
        </form>
      </section>
    </main>
  );
}

function Dashboard({
  initialSnapshot,
  onLogout,
}: {
  initialSnapshot: Snapshot;
  onLogout: () => void;
}) {
  const [snapshot, setSnapshot] = useState(initialSnapshot);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [notice, setNotice] = useState(true);
  const sessions = useMemo(
    () =>
      [...snapshot.sessions].sort((a, b) =>
        activityTime(b).localeCompare(activityTime(a)),
      ),
    [snapshot.sessions],
  );
  const claudeCount = sessions.filter(
    (session) => session.agent_type === "claude_code",
  ).length;
  const codexCount = sessions.filter(
    (session) => session.agent_type === "codex",
  ).length;

  const refresh = useCallback(async () => {
    try {
      const next = await invoke<Snapshot>("refresh_dashboard");
      setSnapshot(next);
      setError(undefined);
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => void refresh(), 5_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const toggleSync = async () => {
    setBusy(true);
    try {
      await invoke("set_sync_paused", { paused: snapshot.sync_running });
      setSnapshot((current) => ({
        ...current,
        sync_running: !current.sync_running,
      }));
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    setBusy(true);
    try {
      await invoke("logout");
      onLogout();
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <PulseMark />
        <nav aria-label="主导航">
          <button aria-current="page">
            <Activity />
            <span>会话</span>
          </button>
          <button>
            <Monitor />
            <span>设备</span>
          </button>
          <button>
            <Settings />
            <span>设置</span>
          </button>
        </nav>
        <button className="sidebar-account" title={snapshot.account.user.email}>
          <CircleUserRound />
        </button>
      </aside>
      <section className="workspace">
        <header className="workspace-header">
          <div>
            <span className="section-label">
              <Monitor /> 本机设备
            </span>
            <h1>这台电脑</h1>
            <p>
              {snapshot.device.name} <i />{" "}
              {snapshot.sync_running ? "同步正常" : "同步已暂停"}
            </p>
          </div>
          <div className="header-actions">
            <button
              className="outline-button"
              disabled={busy}
              onClick={toggleSync}
            >
              {snapshot.sync_running ? <Pause /> : <Play />}
              {snapshot.sync_running ? "暂停同步" : "继续同步"}
            </button>
            <button
              className="icon-button"
              aria-label="刷新"
              onClick={() => void refresh()}
            >
              <RefreshCw />
            </button>
            <button
              className="icon-button"
              aria-label="退出登录"
              onClick={() => void signOut()}
            >
              <LogOut />
            </button>
          </div>
        </header>

        <section className="source-list" aria-label="本地 Agent 来源">
          <SourceRow
            icon={<Code2 />}
            name="Claude Code"
            count={claudeCount}
            connected={snapshot.sync_running}
          />
          <SourceRow
            icon={<span className="codex-cube">◇</span>}
            name="Codex"
            count={codexCount}
            connected={snapshot.sync_running}
          />
          <div className="pulse-connector" aria-hidden="true">
            <span />
          </div>
        </section>

        <section className="session-section">
          <div className="session-heading">
            <div>
              <h2>会话</h2>
              <p>{sessions.length} 个会话已安全同步</p>
            </div>
            <button>
              <SlidersHorizontal /> 筛选
            </button>
          </div>
          <div className="session-table" role="table" aria-label="会话列表">
            <div className="session-table-head" role="row">
              <span>项目</span>
              <span>Agent</span>
              <span>状态</span>
              <span>最近活动</span>
              <span />
            </div>
            {sessions.length === 0 ? (
              <div className="empty-state">
                <Activity />
                <h3>等待第一个会话</h3>
                <p>打开 Claude Code 或 Codex 后，会话会自动出现在这里。</p>
              </div>
            ) : (
              sessions.map((session) => (
                <SessionRow
                  key={`${session.device_id}:${session.agent_type}:${session.agent_session_id}`}
                  session={session}
                />
              ))
            )}
          </div>
        </section>

        {notice && (
          <div className="detection-notice" role="status">
            <span>
              <ShieldCheck />
            </span>
            <div>
              <strong>已自动检测到 Claude Code 和 Codex 本地来源</strong>
              <p>所有会话状态将自动同步到你的账号。</p>
            </div>
            <button aria-label="关闭提示" onClick={() => setNotice(false)}>
              ×
            </button>
          </div>
        )}
        {error !== undefined && (
          <div className="toast-error" role="alert">
            {error}
          </div>
        )}
      </section>
    </main>
  );
}

function SourceRow({
  icon,
  name,
  count,
  connected,
}: {
  icon: React.ReactNode;
  name: string;
  count: number;
  connected: boolean;
}) {
  return (
    <div className="source-row">
      <span className="source-icon">{icon}</span>
      <strong>{name}</strong>
      <span className="source-count">已发现 {count} 个会话</span>
      <span className={connected ? "source-health healthy" : "source-health"}>
        <i />
        {connected ? "已连接" : "已暂停"}
      </span>
    </div>
  );
}

function SessionRow({ session }: { session: Session }) {
  const status = statusView(
    session.payload.state.activity_status,
    session.payload.state.presence,
  );
  return (
    <button className="session-row" role="row">
      <strong>{session.payload.project.display_name || "未命名项目"}</strong>
      <span>
        {session.agent_type === "claude_code" ? "Claude Code" : "Codex"}
      </span>
      <span>
        <em className={`status-chip ${status.className}`}>
          <i />
          {status.label}
        </em>
      </span>
      <time>{relativeTime(activityTime(session))}</time>
      <ChevronRight />
    </button>
  );
}

function Brand() {
  return (
    <div className="brand">
      <PulseMark />
      <strong>AgentPulse</strong>
    </div>
  );
}

function PulseMark({ large = false }: { large?: boolean }) {
  return (
    <span
      className={large ? "pulse-mark large" : "pulse-mark"}
      aria-hidden="true"
    >
      <i />
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}

function LocalCloudDiagram() {
  return (
    <div className="local-cloud" aria-hidden="true">
      <Cloud className="cloud" />
      <span className="signal-line" />
      <div className="laptop">
        <Monitor />
        <span>
          <Code2 /> ◇
        </span>
      </div>
    </div>
  );
}

function errorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim() !== "") return error;
  if (error instanceof Error && error.message !== "") return error.message;
  return "操作失败，请稍后重试";
}

const demoSnapshot: Snapshot = {
  account: {
    user: {
      user_id: "demo",
      email: "dev@example.com",
      display_name: "Developer",
    },
    subscription: { plan: "pro", status: "active" },
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
  server_url: "https://platform.tannerlab.xyz",
  sessions: [
    demoSession("claude_code", "电商平台重构", "WORKING", 2),
    demoSession("claude_code", "支付服务优化", "WAITING_PERMISSION", 8),
    demoSession("claude_code", "用户增长分析", "IDLE", 25),
    demoSession("codex", "API 网关开发", "WORKING", 1),
    demoSession("codex", "数据导出工具", "UNKNOWN", 180, "PERSISTED"),
  ],
};

function demoSession(
  agent: Session["agent_type"],
  project: string,
  activity: ActivityStatus,
  minutesAgo: number,
  presence = "LIVE",
): Session {
  const observedAt = new Date(Date.now() - minutesAgo * 60_000).toISOString();
  return {
    event_id: `${agent}-${project}`,
    device_id: "dev_demo",
    agent_type: agent,
    agent_session_id: `${agent}-${project}`,
    observed_at: observedAt,
    payload: {
      project: { display_name: project, fingerprint: "demo" },
      state: { activity_status: activity, presence },
      last_activity_at: observedAt,
    },
  };
}
