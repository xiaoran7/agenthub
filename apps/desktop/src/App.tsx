import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AlertTriangle,
  Archive,
  CheckCircle2,
  ChevronLeft,
  ChevronRight,
  CircleUserRound,
  Cloud,
  Code2,
  Cpu,
  Database,
  Eye,
  EyeOff,
  FolderTree,
  HardDrive,
  LogOut,
  MessageSquare,
  Monitor,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Square,
  X,
} from "lucide-react";
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import {
  activityTime,
  adjacentItem,
  relativeTime,
  statusView,
} from "./presentation";
import {
  appendAssistantDelta,
  dropTrailingEmptyAssistant,
  replaceAssistantMessage,
  type ConversationMessage,
} from "./turn-stream";
import type {
  ActivityStatus,
  DesktopPreferences,
  DashboardRealtimeEvent,
  LocalSource,
  Session,
  SessionTimelineEvent,
  SessionTurnEvent,
  SessionTurnResult,
  Snapshot,
} from "./types";

type AuthMode = "login" | "register";
type AgentFilter = "all" | Session["agent_type"];
type DeviceFilter = "all" | string;

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
      <strong>正在建立本地索引</strong>
      <span>读取会话元数据，不读取对话正文</span>
      <div className="loading-line">
        <i />
      </div>
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
    if (busy) return;
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
          <span className="story-kicker">LOCAL SESSION OBSERVATORY</span>
          <h1>
            看清每一个
            <br />
            本地 Agent 脉搏
          </h1>
          <p>
            Claude
            Code、Codex、历史会话与记忆索引会在登录后自动盘点。只同步状态和脱敏后的项目名。
          </p>
        </div>
        <LocalCloudDiagram />
      </section>
      <section className="auth-panel">
        <form onSubmit={submit} className="auth-form">
          <header>
            <span className="card-kicker">AgentPulse Desktop</span>
            <h2>{mode === "login" ? "连接这台电脑" : "创建账号"}</h2>
            <p>
              {mode === "login"
                ? "登录后立即扫描本机，不必打开终端。"
                : "一个账号连接你的所有开发设备。"}
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
                <RefreshCw className="spin" /> 正在扫描并连接
              </>
            ) : mode === "login" ? (
              "登录并扫描本机"
            ) : (
              "注册并扫描本机"
            )}
          </button>
          <p className="privacy-note">
            <ShieldCheck /> 不上传对话、Prompt、命令、Token 或记忆正文
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
  const [actionBusy, setActionBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string>();
  const [accountOpen, setAccountOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("all");
  const [deviceFilter, setDeviceFilter] = useState<DeviceFilter>("all");
  const [archiveFilter, setArchiveFilter] = useState<"active" | "archived">(
    "active",
  );
  const [selectedSessionKey, setSelectedSessionKey] = useState<string>();
  const [visibleLimit, setVisibleLimit] = useState(100);
  const refreshInFlight = useRef(false);
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());

  const sessions = useMemo(
    () =>
      [...snapshot.sessions].sort((a, b) =>
        activityTime(b).localeCompare(activityTime(a)),
      ),
    [snapshot.sessions],
  );
  const filteredSessions = useMemo(
    () =>
      sessions.filter((session) => {
        if (agentFilter !== "all" && session.agent_type !== agentFilter)
          return false;
        if (deviceFilter !== "all" && session.device_id !== deviceFilter)
          return false;
        if ((session.archived === true) !== (archiveFilter === "archived"))
          return false;
        if (deferredQuery === "") return true;
        const haystack = [
          session.payload.runtime?.title,
          session.payload.project.display_name,
          session.payload.runtime?.model,
          session.agent_type,
        ]
          .filter(Boolean)
          .join(" ")
          .toLocaleLowerCase();
        return haystack.includes(deferredQuery);
      }),
    [agentFilter, archiveFilter, deferredQuery, deviceFilter, sessions],
  );
  const selectedSession = useMemo(
    () =>
      sessions.find((session) => sessionKey(session) === selectedSessionKey),
    [selectedSessionKey, sessions],
  );
  const selectedFilteredIndex = selectedSessionKey
    ? filteredSessions.findIndex(
        (session) => sessionKey(session) === selectedSessionKey,
      )
    : -1;
  const previousSession = adjacentItem(
    filteredSessions,
    selectedFilteredIndex,
    -1,
  );
  const nextSession = adjacentItem(filteredSessions, selectedFilteredIndex, 1);
  const groupedSessions = useMemo(() => {
    const groups = new Map<string, Session[]>();
    for (const session of filteredSessions.slice(0, visibleLimit)) {
      const group = session.payload.project.display_name || "未分类项目";
      const entries = groups.get(group);
      if (entries === undefined) groups.set(group, [session]);
      else entries.push(session);
    }
    return [...groups.entries()];
  }, [filteredSessions, visibleLimit]);
  const counts = useMemo(() => {
    const next = { claude_code: 0, codex: 0 };
    for (const session of sessions) next[session.agent_type] += 1;
    return next;
  }, [sessions]);

  const refresh = useCallback(async () => {
    if (refreshInFlight.current) return;
    if (!("__TAURI_INTERNALS__" in window)) return;
    refreshInFlight.current = true;
    setRefreshing(true);
    try {
      const next = await invoke<Snapshot>("refresh_dashboard");
      setSnapshot(next);
      setError(undefined);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setRefreshing(false);
      refreshInFlight.current = false;
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    let timer = window.setTimeout(async function poll() {
      await refresh();
      if (!cancelled)
        timer = window.setTimeout(
          poll,
          snapshot.preferences.refresh_interval_seconds * 1_000,
        );
    }, snapshot.preferences.refresh_interval_seconds * 1_000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [refresh, snapshot.preferences.refresh_interval_seconds]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<DashboardRealtimeEvent>("dashboard-realtime", (event) => {
      setSnapshot((current) => ({
        ...current,
        realtime_connected: event.payload.connected,
      }));
      if (event.payload.changed) void refresh();
    })
      .then((nextUnlisten) => {
        if (disposed) nextUnlisten();
        else {
          unlisten = nextUnlisten;
          void refresh();
        }
      })
      .catch(() => {
        if (!disposed) {
          setSnapshot((current) => ({
            ...current,
            realtime_connected: false,
          }));
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh]);

  useEffect(
    () => setVisibleLimit(100),
    [agentFilter, archiveFilter, deferredQuery, deviceFilter],
  );

  const toggleSync = async () => {
    if (actionBusy) return;
    if (!("__TAURI_INTERNALS__" in window)) {
      setSnapshot((current) => ({
        ...current,
        sync_running: !current.sync_running,
      }));
      return;
    }
    setActionBusy(true);
    try {
      await invoke("set_sync_paused", { paused: snapshot.sync_running });
      setSnapshot((current) => ({
        ...current,
        sync_running: !current.sync_running,
      }));
      setError(undefined);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setActionBusy(false);
    }
  };

  const signOut = async () => {
    if (actionBusy) return;
    if (!("__TAURI_INTERNALS__" in window)) {
      onLogout();
      return;
    }
    setActionBusy(true);
    try {
      await invoke("logout");
      onLogout();
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setActionBusy(false);
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
          <button
            aria-label="桌面设置"
            aria-expanded={settingsOpen}
            onClick={() => setSettingsOpen(true)}
          >
            <Settings />
            <span>设置</span>
          </button>
        </nav>
        <button
          className="sidebar-account"
          aria-label="查看账号信息"
          aria-expanded={accountOpen}
          title={snapshot.account.user.email}
          onClick={() => setAccountOpen((value) => !value)}
        >
          <CircleUserRound />
        </button>
      </aside>

      <section className="workspace">
        <header className="workspace-header">
          <div>
            <span className="section-label">
              <Monitor /> LOCAL DEVICE
            </span>
            <h1>{snapshot.device.name}</h1>
            <p
              className={
                snapshot.local.pending_events > 0
                  ? "sync-copy pending"
                  : "sync-copy"
              }
            >
              <i />{" "}
              {snapshot.sync_running
                ? snapshot.local.pending_events > 0
                  ? `${snapshot.local.pending_events} 条等待上传`
                  : "本地与云端已同步"
                : "云同步已暂停，本地扫描继续"}
            </p>
            <span
              className={
                snapshot.realtime_connected
                  ? "live-refresh connected"
                  : "live-refresh"
              }
            >
              <i />
              {snapshot.realtime_connected
                ? "SSE 实时连接"
                : `每 ${snapshot.preferences.refresh_interval_seconds} 秒兜底刷新`}
            </span>
          </div>
          <div className="header-actions">
            <button
              className="account-summary"
              onClick={() => setAccountOpen(true)}
            >
              <span>
                {initials(
                  snapshot.account.user.display_name ||
                    snapshot.account.user.email,
                )}
              </span>
              <div>
                <strong>
                  {snapshot.account.user.display_name || "AgentPulse 用户"}
                </strong>
                <small>{snapshot.account.user.email}</small>
              </div>
            </button>
            <button
              className="outline-button"
              disabled={actionBusy}
              onClick={() => void toggleSync()}
            >
              {snapshot.sync_running ? <Pause /> : <Play />}
              {snapshot.sync_running ? "暂停云同步" : "继续云同步"}
            </button>
            <button
              className="icon-button"
              aria-label="刷新"
              disabled={refreshing}
              onClick={() => void refresh()}
            >
              <RefreshCw className={refreshing ? "spin" : undefined} />
            </button>
          </div>
        </header>

        <section className="discovery-grid" aria-label="本地发现概览">
          <div className="source-list">
            <SourceRow
              icon={<Code2 />}
              name="Claude Code"
              fallbackCount={counts.claude_code}
              source={snapshot.local.sources.find(
                (item) => item.agent_type === "claude_code",
              )}
            />
            <SourceRow
              icon={<span className="codex-cube">◆</span>}
              name="Codex"
              fallbackCount={counts.codex}
              source={snapshot.local.sources.find(
                (item) => item.agent_type === "codex",
              )}
            />
          </div>
          <div className="memory-card">
            <span className="memory-icon">
              <Database />
            </span>
            <div>
              <span className="card-kicker">本地记忆索引</span>
              <strong>
                {snapshot.local.memory.claude_files} 个 Claude 文件 ·{" "}
                {snapshot.local.memory.codex_indexes} 个 Codex 索引
              </strong>
              <p>
                {formatBytes(
                  snapshot.local.memory.claude_bytes +
                    snapshot.local.memory.codex_bytes,
                )}
                ，只读取数量和大小
              </p>
            </div>
            <ShieldCheck />
          </div>
        </section>

        <section className="session-section">
          <div className="session-heading">
            <div>
              <span className="card-kicker">DISCOVERY LEDGER</span>
              <h2>会话索引</h2>
              <p>
                本机发现 {snapshot.local.session_count} 个 · 当前显示{" "}
                {filteredSessions.length} 个
              </p>
            </div>
            <div className="session-tools">
              <div className="archive-filter" aria-label="归档筛选">
                <button
                  aria-pressed={archiveFilter === "active"}
                  onClick={() => setArchiveFilter("active")}
                >
                  活跃
                </button>
                <button
                  aria-pressed={archiveFilter === "archived"}
                  onClick={() => setArchiveFilter("archived")}
                >
                  已归档
                </button>
              </div>
              <label className="session-search">
                <Search />
                <input
                  aria-label="搜索会话、目录或模型"
                  placeholder="搜索会话、目录或模型"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                />
              </label>
              {snapshot.devices.length > 1 && (
                <label className="device-filter">
                  <Monitor />
                  <select
                    aria-label="设备筛选"
                    value={deviceFilter}
                    onChange={(event) => setDeviceFilter(event.target.value)}
                  >
                    <option value="all">全部设备</option>
                    {snapshot.devices.map((device) => (
                      <option key={device.device_id} value={device.device_id}>
                        {device.device_id === snapshot.device.device_id
                          ? `当前设备 · ${snapshot.device.name}`
                          : `设备 · ${shortDeviceId(device.device_id)}`}
                      </option>
                    ))}
                  </select>
                </label>
              )}
              <div className="agent-filter" aria-label="Agent 筛选">
                {(["all", "claude_code", "codex"] as const).map((value) => (
                  <button
                    key={value}
                    aria-pressed={agentFilter === value}
                    onClick={() => setAgentFilter(value)}
                  >
                    {value === "all"
                      ? "全部"
                      : value === "claude_code"
                        ? "Claude"
                        : "Codex"}
                  </button>
                ))}
              </div>
            </div>
          </div>
          <div className="session-table" role="table" aria-label="会话列表">
            <div className="session-table-head" role="row">
              <span>会话</span>
              <span>运行环境</span>
              <span>状态</span>
              <span>最近活动</span>
              <span>同步</span>
            </div>
            {filteredSessions.length === 0 ? (
              <div className="empty-state">
                {archiveFilter === "active" &&
                query === "" &&
                agentFilter === "all" ? (
                  <Activity />
                ) : (
                  <Search />
                )}
                <h3>
                  {archiveFilter === "archived" &&
                  query === "" &&
                  agentFilter === "all" &&
                  deviceFilter === "all"
                    ? "暂无归档会话"
                    : deviceFilter !== "all" &&
                        query === "" &&
                        agentFilter === "all"
                      ? "该设备暂无会话"
                      : query === "" && agentFilter === "all"
                        ? "正在扫描本机会话"
                        : "没有匹配的会话"}
                </h3>
                <p>
                  {archiveFilter === "archived" &&
                  query === "" &&
                  agentFilter === "all" &&
                  deviceFilter === "all"
                    ? "在会话控制面板中归档后，会话会移动到这里。"
                    : deviceFilter !== "all" &&
                        query === "" &&
                        agentFilter === "all"
                      ? "切换设备筛选，查看其他设备上的 Session。"
                      : query === "" && agentFilter === "all"
                        ? "扫描失败时会在上方来源卡片显示原因。"
                        : "换一个会话名、目录、模型或清除 Agent 筛选。"}
                </p>
              </div>
            ) : (
              groupedSessions.map(([group, groupSessions]) => (
                <div className="session-group" role="rowgroup" key={group}>
                  <div className="session-group-label" role="row">
                    <FolderTree />
                    <strong>{group}</strong>
                    <span>{groupSessions.length} 个会话</span>
                  </div>
                  {groupSessions.map((session) => (
                    <SessionRow
                      key={`${session.device_id}:${session.agent_type}:${session.agent_session_id}`}
                      session={session}
                      selected={selectedSessionKey === sessionKey(session)}
                      onSelect={() =>
                        setSelectedSessionKey(sessionKey(session))
                      }
                    />
                  ))}
                </div>
              ))
            )}
          </div>
          {filteredSessions.length > visibleLimit && (
            <button
              className="load-more"
              onClick={() => setVisibleLimit((value) => value + 100)}
            >
              再显示 {Math.min(100, filteredSessions.length - visibleLimit)}{" "}
              个会话
            </button>
          )}
        </section>

        <div className="discovery-note" role="status">
          <HardDrive />
          <span>
            本地扫描于 {relativeTime(snapshot.local.refreshed_at)}{" "}
            更新；对话、Prompt 和记忆正文不会上传。
          </span>
        </div>
        {accountOpen && (
          <AccountPanel
            snapshot={snapshot}
            busy={actionBusy}
            onClose={() => setAccountOpen(false)}
            onLogout={() => void signOut()}
          />
        )}
        {settingsOpen && (
          <DesktopSettingsPanel
            preferences={snapshot.preferences}
            onClose={() => setSettingsOpen(false)}
            onSaved={(preferences) =>
              setSnapshot((current) => ({ ...current, preferences }))
            }
          />
        )}
        {selectedSession !== undefined && (
          <SessionControlPanel
            key={sessionKey(selectedSession)}
            session={selectedSession}
            previousSession={previousSession}
            nextSession={nextSession}
            onNavigate={(next) => setSelectedSessionKey(sessionKey(next))}
            onClose={() => setSelectedSessionKey(undefined)}
            onChanged={async (archived) => {
              setSnapshot((current) => ({
                ...current,
                sessions: current.sessions.map((session) =>
                  sessionKey(session) === sessionKey(selectedSession)
                    ? { ...session, archived }
                    : session,
                ),
              }));
              setArchiveFilter(archived ? "archived" : "active");
              await refresh();
            }}
          />
        )}
        {error !== undefined && (
          <div className="toast-error" role="alert">
            <AlertTriangle />
            {error}
            <button aria-label="关闭错误" onClick={() => setError(undefined)}>
              <X />
            </button>
          </div>
        )}
      </section>
    </main>
  );
}

function SourceRow({
  icon,
  name,
  fallbackCount,
  source,
}: {
  icon: React.ReactNode;
  name: string;
  fallbackCount: number;
  source?: LocalSource;
}) {
  const healthy = source?.status === "healthy";
  const failed = source?.status === "backoff";
  return (
    <div className="source-row">
      <span className="source-icon">{icon}</span>
      <div className="source-copy">
        <strong>{name}</strong>
        <span className="source-count">
          已发现 {source?.session_count ?? fallbackCount} 个会话
        </span>
      </div>
      <span
        className={
          healthy
            ? "source-health healthy"
            : failed
              ? "source-health failed"
              : "source-health"
        }
        title={source?.error_code}
      >
        {healthy ? (
          <CheckCircle2 />
        ) : failed ? (
          <AlertTriangle />
        ) : (
          <RefreshCw className="spin" />
        )}
        {healthy ? "扫描正常" : failed ? "正在重试" : "正在扫描"}
      </span>
    </div>
  );
}

function SessionRow({
  session,
  selected,
  onSelect,
}: {
  session: Session;
  selected: boolean;
  onSelect: () => void;
}) {
  const status = statusView(
    session.payload.state.activity_status,
    session.payload.state.presence,
  );
  const runtime = session.payload.runtime;
  const title = runtime?.title || "未命名会话";
  return (
    <button
      className={selected ? "session-row selected" : "session-row"}
      role="row"
      onClick={onSelect}
    >
      <span className="session-identity">
        <strong title={title}>{title}</strong>
        <small>{session.payload.project.display_name || "未分类项目"}</small>
      </span>
      <span className="session-runtime">
        <strong>
          {session.agent_type === "claude_code" ? "Claude Code" : "Codex"}
        </strong>
        <small>{runtime?.model ?? "模型未提供"}</small>
      </span>
      <span>
        <em className={`status-chip ${status.className}`}>
          <i />
          {status.label}
        </em>
      </span>
      <time>{relativeTime(activityTime(session))}</time>
      <span className="session-row-tail">
        <i
          className={`sync-dot ${session.sync_state ?? "synced"}`}
          title={
            session.sync_state === "local"
              ? "本地已发现，等待云端确认"
              : "云端已同步"
          }
        />
        <ChevronRight />
      </span>
    </button>
  );
}

function SessionControlPanel({
  session,
  previousSession,
  nextSession,
  onNavigate,
  onClose,
  onChanged,
}: {
  session: Session;
  previousSession?: Session;
  nextSession?: Session;
  onNavigate: (session: Session) => void;
  onClose: () => void;
  onChanged: (archived: boolean) => Promise<void>;
}) {
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [agentActivity, setAgentActivity] = useState<string>();
  const [archiveBusy, setArchiveBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [panelView, setPanelView] = useState<
    "context" | "conversation" | "timeline"
  >("context");
  const [timeline, setTimeline] = useState<SessionTimelineEvent[]>([]);
  const [timelineLoading, setTimelineLoading] = useState(false);
  const [timelineError, setTimelineError] = useState<string>();
  const [timelineReload, setTimelineReload] = useState(0);
  const pendingDelta = useRef("");
  const deltaFrame = useRef<number | undefined>(undefined);
  const conversationEnd = useRef<HTMLDivElement>(null);
  const status = statusView(
    session.payload.state.activity_status,
    session.payload.state.presence,
  );
  const runtime = session.payload.runtime;
  const title = runtime?.title || "未命名会话";
  const permission =
    session.agent_type === "claude_code"
      ? runtime?.permission_mode
      : (runtime?.permission_mode ?? runtime?.approval_policy);

  useEffect(() => {
    if (panelView !== "timeline") return;
    let disposed = false;
    setTimelineLoading(true);
    setTimelineError(undefined);
    setTimeline([]);
    invoke<SessionTimelineEvent[]>("load_session_timeline", {
      input: {
        device_id: session.device_id,
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
      },
    })
      .then((next) => {
        if (!disposed) setTimeline(next);
      })
      .catch((nextError) => {
        if (!disposed) setTimelineError(errorMessage(nextError));
      })
      .finally(() => {
        if (!disposed) setTimelineLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [
    panelView,
    session.agent_session_id,
    session.agent_type,
    session.device_id,
    timelineReload,
  ]);

  const replaceLastAssistant = (content: string) => {
    setMessages((current) => replaceAssistantMessage(current, content));
  };

  const flushPendingDelta = () => {
    deltaFrame.current = undefined;
    const delta = pendingDelta.current;
    pendingDelta.current = "";
    if (delta === "") return;
    setMessages((current) => appendAssistantDelta(current, delta));
  };

  useEffect(() => {
    conversationEnd.current?.scrollIntoView({ block: "end" });
  }, [agentActivity, messages]);

  useEffect(
    () => () => {
      if (deltaFrame.current !== undefined)
        window.cancelAnimationFrame(deltaFrame.current);
    },
    [],
  );

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT")
      ) {
        return;
      }
      if (event.key === "Escape") {
        if (!busy) {
          event.preventDefault();
          onClose();
        }
        return;
      }
      if (event.key === "ArrowLeft" && previousSession !== undefined) {
        event.preventDefault();
        onNavigate(previousSession);
      } else if (event.key === "ArrowRight" && nextSession !== undefined) {
        event.preventDefault();
        onNavigate(nextSession);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [busy, nextSession, onClose, onNavigate, previousSession]);

  const sendMessage = async (event: FormEvent) => {
    event.preventDefault();
    const content = prompt.trim();
    if (busy || content === "") return;
    setPrompt("");
    setBusy(true);
    setCancelling(false);
    setAgentActivity("正在连接本机 CLI");
    setError(undefined);
    setMessages((current) => [
      ...current,
      { role: "user", content },
      { role: "assistant", content: "" },
    ]);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      onEvent.onmessage = (update) => {
        if (update.type === "started") {
          setAgentActivity("Agent 正在本机运行");
        } else if (update.type === "activity") {
          setAgentActivity(update.label);
        } else if (update.type === "assistant_message") {
          pendingDelta.current = "";
          if (deltaFrame.current !== undefined) {
            window.cancelAnimationFrame(deltaFrame.current);
            deltaFrame.current = undefined;
          }
          replaceLastAssistant(update.text);
          setAgentActivity("正在收尾");
        } else {
          pendingDelta.current += update.text;
          setAgentActivity("正在生成回复");
          if (deltaFrame.current === undefined) {
            deltaFrame.current =
              window.requestAnimationFrame(flushPendingDelta);
          }
        }
      };
      const result = await invoke<SessionTurnResult>("send_session_message", {
        input: {
          agent_type: session.agent_type,
          agent_session_id: session.agent_session_id,
          prompt: content,
        },
        onEvent,
      });
      pendingDelta.current = "";
      if (deltaFrame.current !== undefined) {
        window.cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = undefined;
      }
      replaceLastAssistant(result.response);
    } catch (nextError) {
      setError(errorMessage(nextError));
      setMessages(dropTrailingEmptyAssistant);
    } finally {
      setAgentActivity(undefined);
      setCancelling(false);
      setBusy(false);
    }
  };

  const cancelTurn = async () => {
    if (!busy || cancelling) return;
    setCancelling(true);
    setAgentActivity("正在停止本机回合");
    try {
      await invoke("cancel_session_turn", {
        input: {
          agent_type: session.agent_type,
          agent_session_id: session.agent_session_id,
        },
      });
    } catch (nextError) {
      setError(errorMessage(nextError));
      setCancelling(false);
    }
  };

  const toggleArchive = async () => {
    if (archiveBusy) return;
    setArchiveBusy(true);
    setError(undefined);
    try {
      const archived = session.archived !== true;
      await invoke("set_session_archived", {
        input: {
          agent_type: session.agent_type,
          agent_session_id: session.agent_session_id,
          project_display_name: session.payload.project.display_name,
          project_fingerprint: session.payload.project.fingerprint,
          observed_at: session.observed_at,
          archived,
        },
      });
      await onChanged(archived);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setArchiveBusy(false);
    }
  };

  return (
    <div
      className="panel-scrim"
      onMouseDown={() => {
        if (busy) setError("请先停止当前回合，再关闭会话控制面板。");
        else onClose();
      }}
    >
      <aside
        className="session-control-panel"
        aria-label="会话控制"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span className="card-kicker">SESSION CONTROL</span>
            <h2 title={title}>{title}</h2>
          </div>
          <div className="session-header-actions">
            <button
              className="icon-button"
              aria-label="上一条会话"
              title={
                previousSession === undefined
                  ? "已经是筛选结果中的第一条"
                  : "上一条会话"
              }
              disabled={previousSession === undefined || busy}
              onClick={() =>
                previousSession !== undefined && onNavigate(previousSession)
              }
            >
              <ChevronLeft />
            </button>
            <button
              className="icon-button"
              aria-label="下一条会话"
              title={
                nextSession === undefined
                  ? "已经是筛选结果中的最后一条"
                  : "下一条会话"
              }
              disabled={nextSession === undefined || busy}
              onClick={() =>
                nextSession !== undefined && onNavigate(nextSession)
              }
            >
              <ChevronRight />
            </button>
            <button
              className="icon-button"
              aria-label="关闭会话"
              disabled={busy}
              onClick={onClose}
            >
              <X />
            </button>
          </div>
        </header>

        <div className="session-control-summary">
          <span className="agent-symbol">
            {session.agent_type === "claude_code" ? <Code2 /> : "◆"}
          </span>
          <div>
            <strong>
              {session.agent_type === "claude_code" ? "Claude Code" : "Codex"}
            </strong>
            <span>{session.payload.project.display_name || "未分类项目"}</span>
            <em className={`status-chip ${status.className}`}>
              <i /> {session.archived ? "已归档" : status.label}
            </em>
          </div>
        </div>

        <nav className="session-panel-tabs" aria-label="会话面板视图">
          <button
            aria-pressed={panelView === "context"}
            onClick={() => setPanelView("context")}
          >
            运行上下文
          </button>
          <button
            aria-pressed={panelView === "conversation"}
            onClick={() => setPanelView("conversation")}
          >
            会话对话
          </button>
          <button
            aria-pressed={panelView === "timeline"}
            onClick={() => setPanelView("timeline")}
          >
            时间线
          </button>
        </nav>

        {panelView === "context" ? (
          <section className="runtime-context-view">
            <dl className="session-metadata">
              <div>
                <dt>分类目录</dt>
                <dd>{session.payload.project.display_name || "未分类项目"}</dd>
              </div>
              <div>
                <dt>会话 ID</dt>
                <dd title={session.agent_session_id}>
                  {session.agent_session_id}
                </dd>
              </div>
              <div>
                <dt>模型</dt>
                <dd title={runtime?.model}>{runtime?.model ?? "未提供"}</dd>
              </div>
              <div>
                <dt>推理强度</dt>
                <dd>{runtime?.reasoning_effort ?? "未提供"}</dd>
              </div>
              <div>
                <dt>
                  {session.agent_type === "claude_code"
                    ? "权限模式"
                    : "审批策略"}
                </dt>
                <dd>{permission ?? "未提供"}</dd>
              </div>
              <div>
                <dt>沙箱模式</dt>
                <dd>{runtime?.sandbox_mode ?? "未提供"}</dd>
              </div>
              <div>
                <dt>上下文窗口</dt>
                <dd>{formatContextUsage(runtime)}</dd>
              </div>
              <div>
                <dt>会话累计 Token</dt>
                <dd>{formatTokenCount(runtime?.session_total_tokens)}</dd>
              </div>
              <div>
                <dt>最近活动</dt>
                <dd>{relativeTime(activityTime(session))}</dd>
              </div>
              <div>
                <dt>同步状态</dt>
                <dd>
                  {session.sync_state === "local" ? "等待云端确认" : "已同步"}
                </dd>
              </div>
            </dl>
            <p className="runtime-privacy-note">
              <ShieldCheck /> 运行上下文来自本机元数据，不同步会话标题或配置。
            </p>
          </section>
        ) : panelView === "timeline" ? (
          <SessionTimelineView
            events={timeline}
            loading={timelineLoading}
            error={timelineError}
            onReload={() => setTimelineReload((value) => value + 1)}
          />
        ) : (
          <section className="conversation-pane" aria-live="polite">
            {messages.length === 0 ? (
              <div className="conversation-empty">
                <MessageSquare />
                <strong>从这里继续会话</strong>
                <p>
                  消息直接交给本机 CLI；内容不进入 AgentPulse 云端或本地索引。
                </p>
              </div>
            ) : (
              messages.map((message, index) =>
                message.content === "" ? null : (
                  <div
                    className={`turn-message ${message.role}`}
                    key={`${message.role}-${index}`}
                  >
                    <span>{message.role === "user" ? "你" : "Agent"}</span>
                    <p>{message.content}</p>
                  </div>
                ),
              )
            )}
            {busy && agentActivity !== undefined && (
              <div className="agent-thinking">
                <RefreshCw className="spin" /> {agentActivity}
              </div>
            )}
            <div ref={conversationEnd} />
          </section>
        )}

        {error !== undefined && <p className="panel-error">{error}</p>}
        {panelView === "conversation" && (
          <form className="session-composer" onSubmit={sendMessage}>
            <textarea
              aria-label="发送给 Agent"
              maxLength={16_000}
              placeholder={
                session.archived ? "恢复会话后再发送" : "继续这个会话…"
              }
              value={prompt}
              onChange={(event) => setPrompt(event.target.value)}
              disabled={busy || session.archived === true}
            />
            <button
              className="composer-send"
              type="submit"
              aria-label="发送"
              disabled={
                busy || session.archived === true || prompt.trim() === ""
              }
            >
              <Send />
            </button>
          </form>
        )}
        <footer>
          <span>
            <ShieldCheck /> 本机直连 · Prompt 不上传
          </span>
          <div className="session-footer-actions">
            {busy && (
              <button
                className="turn-cancel"
                disabled={cancelling}
                onClick={() => void cancelTurn()}
              >
                <Square />
                {cancelling ? "停止中" : "停止回合"}
              </button>
            )}
            <button
              disabled={archiveBusy || busy}
              onClick={() => void toggleArchive()}
            >
              {session.archived ? <RotateCcw /> : <Archive />}
              {session.archived ? "恢复会话" : "归档会话"}
            </button>
          </div>
        </footer>
      </aside>
    </div>
  );
}

function DesktopSettingsPanel({
  preferences,
  onClose,
  onSaved,
}: {
  preferences: DesktopPreferences;
  onClose: () => void;
  onSaved: (preferences: DesktopPreferences) => void;
}) {
  const [refreshSeconds, setRefreshSeconds] = useState(
    preferences.refresh_interval_seconds,
  );
  const [claudeBinary, setClaudeBinary] = useState(preferences.claude_binary);
  const [codexBinary, setCodexBinary] = useState(preferences.codex_binary);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const next = await invoke<DesktopPreferences>(
        "save_desktop_preferences",
        {
          input: {
            refresh_interval_seconds: refreshSeconds,
            claude_binary: claudeBinary,
            codex_binary: codexBinary,
          },
        },
      );
      onSaved(next);
      onClose();
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel-scrim" onMouseDown={onClose}>
      <aside
        className="settings-panel"
        aria-label="桌面设置"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span className="card-kicker">DESKTOP RUNTIME</span>
            <h2>连接与控制</h2>
          </div>
          <button
            className="icon-button"
            aria-label="关闭设置"
            onClick={onClose}
          >
            <X />
          </button>
        </header>
        <div className="settings-intro">
          <Cpu />
          <p>扫描和发送都在这台电脑上完成。CLI 路径只保存在本机。</p>
        </div>
        <form onSubmit={save}>
          <label>
            <span>刷新周期</span>
            <div className="range-field">
              <input
                type="range"
                min="2"
                max="60"
                value={refreshSeconds}
                onChange={(event) =>
                  setRefreshSeconds(Number(event.target.value))
                }
              />
              <strong>{refreshSeconds} 秒</strong>
            </div>
          </label>
          <label>
            <span>Claude CLI</span>
            <input
              value={claudeBinary}
              maxLength={260}
              onChange={(event) => setClaudeBinary(event.target.value)}
              placeholder="claude"
              required
            />
          </label>
          <label>
            <span>Codex CLI</span>
            <input
              value={codexBinary}
              maxLength={260}
              onChange={(event) => setCodexBinary(event.target.value)}
              placeholder="codex"
              required
            />
          </label>
          {error !== undefined && <p className="panel-error">{error}</p>}
          <button className="primary-button" disabled={busy} type="submit">
            {busy ? <RefreshCw className="spin" /> : <CheckCircle2 />}
            保存设置
          </button>
        </form>
      </aside>
    </div>
  );
}

function AccountPanel({
  snapshot,
  busy,
  onClose,
  onLogout,
}: {
  snapshot: Snapshot;
  busy: boolean;
  onClose: () => void;
  onLogout: () => void;
}) {
  return (
    <aside className="account-panel" aria-label="账号信息">
      <header>
        <div>
          <span className="card-kicker">AgentPulse 账号</span>
          <h2>{snapshot.account.user.display_name || "未设置名称"}</h2>
        </div>
        <button
          className="icon-button"
          aria-label="关闭账号信息"
          onClick={onClose}
        >
          <X />
        </button>
      </header>
      <dl>
        <div>
          <dt>邮箱</dt>
          <dd>{snapshot.account.user.email}</dd>
        </div>
        <div>
          <dt>套餐</dt>
          <dd>{planLabel(snapshot.account.subscription.plan)}</dd>
        </div>
        <div>
          <dt>账号状态</dt>
          <dd>
            {snapshot.account.subscription.status === "active"
              ? "正常"
              : snapshot.account.subscription.status}
          </dd>
        </div>
        <div>
          <dt>历史保留</dt>
          <dd>{snapshot.account.entitlements?.history_days ?? "—"} 天</dd>
        </div>
        <div>
          <dt>当前设备</dt>
          <dd>{snapshot.device.name}</dd>
        </div>
        <div>
          <dt>服务</dt>
          <dd>{new URL(snapshot.server_url).host}</dd>
        </div>
      </dl>
      <button className="logout-button" disabled={busy} onClick={onLogout}>
        <LogOut /> 退出登录
      </button>
    </aside>
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

function PulseMark() {
  return (
    <span className="pulse-mark" aria-hidden="true">
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
          <Code2 /> ◆
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

function sessionKey(session: Session): string {
  return `${session.device_id}:${session.agent_type}:${session.agent_session_id}`;
}

function shortDeviceId(deviceId: string): string {
  if (deviceId.length <= 14) return deviceId;
  return `${deviceId.slice(0, 6)}…${deviceId.slice(-5)}`;
}

function SessionTimelineView({
  events,
  loading,
  error,
  onReload,
}: {
  events: SessionTimelineEvent[];
  loading: boolean;
  error?: string;
  onReload: () => void;
}) {
  if (loading) {
    return (
      <section className="timeline-view timeline-state" aria-live="polite">
        <RefreshCw className="spin" />
        <strong>正在读取状态时间线</strong>
        <p>只读取这一个会话的状态观察，不读取对话正文。</p>
      </section>
    );
  }
  if (error !== undefined) {
    return (
      <section className="timeline-view timeline-state" aria-live="polite">
        <AlertTriangle />
        <strong>时间线暂时不可用</strong>
        <p>{error}</p>
      </section>
    );
  }
  if (events.length === 0) {
    return (
      <section className="timeline-view timeline-state">
        <Activity />
        <strong>还没有云端状态记录</strong>
        <p>本地会话可能尚未完成一次同步，当前状态仍可在运行上下文中查看。</p>
      </section>
    );
  }
  return (
    <section className="timeline-view" aria-label="会话状态时间线">
      <header className="timeline-heading">
        <div>
          <span className="card-kicker">SESSION HISTORY</span>
          <strong>{events.length} 条状态观察</strong>
        </div>
        <div className="timeline-heading-actions">
          <span>最新在上</span>
          <button
            className="icon-button"
            aria-label="刷新时间线"
            title="刷新时间线"
            disabled={loading}
            onClick={onReload}
          >
            <RefreshCw />
          </button>
        </div>
      </header>
      <ol className="timeline-list">
        {events.map((event) => {
          const status = statusView(
            event.payload.state.activity_status,
            event.payload.state.presence,
          );
          return (
            <li className="timeline-item" key={event.event_id}>
              <span className={`timeline-marker ${status.className}`}>
                <i />
              </span>
              <div className="timeline-event">
                <header>
                  <strong>{status.label}</strong>
                  <span>序列 #{event.sequence}</span>
                  <time title={event.observed_at}>
                    {formatTimelineTimestamp(event.observed_at)}
                  </time>
                </header>
                <p>
                  {event.payload.project.display_name || "未分类项目"}
                  <span> · </span>
                  {event.payload.state.presence === "LIVE"
                    ? "实时观察"
                    : "历史观察"}
                </p>
                <small>接收于 {relativeTime(event.received_at)}</small>
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1_024) return `${bytes} B`;
  if (bytes < 1_048_576) return `${(bytes / 1_024).toFixed(1)} KB`;
  return `${(bytes / 1_048_576).toFixed(1)} MB`;
}

function formatTimelineTimestamp(value: string): string {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "时间未知";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(timestamp);
}

function formatTokenCount(tokens: number | undefined): string {
  if (tokens === undefined) return "未提供";
  if (tokens < 1_000) return `${tokens} tokens`;
  if (tokens < 1_000_000) return `${(tokens / 1_000).toFixed(1)}K tokens`;
  return `${(tokens / 1_000_000).toFixed(2)}M tokens`;
}

function formatContextUsage(runtime: Session["payload"]["runtime"]): string {
  const used = runtime?.context_used_tokens;
  const window = runtime?.context_window_tokens;
  if (used !== undefined && window !== undefined && window > 0) {
    const percentage = Math.min(100, Math.round((used / window) * 100));
    return `${formatTokenCount(used)} / ${formatTokenCount(window)} · ${percentage}%`;
  }
  if (window !== undefined) return `${formatTokenCount(window)} 上限`;
  if (used !== undefined) return `${formatTokenCount(used)} 已用 · 上限未提供`;
  return "未提供";
}

function planLabel(plan: string): string {
  if (plan === "pro") return "Pro";
  if (plan === "team") return "Team";
  return "Free";
}

function initials(value: string): string {
  const parts = value.trim().split(/\s+/u).filter(Boolean);
  if (parts.length === 0) return "AP";
  return parts
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
}

const demoSnapshot: Snapshot = {
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
    ],
    memory: {
      claude_files: 4,
      claude_bytes: 4_909,
      codex_indexes: 1,
      codex_bytes: 1_306_624,
    },
    refreshed_at: new Date().toISOString(),
  },
  preferences: {
    refresh_interval_seconds: 4,
    claude_binary: "claude",
    codex_binary: "codex",
  },
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
    sync_state: minutesAgo < 3 ? "local" : "synced",
    payload: {
      project: { display_name: project, fingerprint: "demo" },
      state: { activity_status: activity, presence },
      last_activity_at: observedAt,
      runtime: {
        agent_type: agent,
        agent_session_id: `${agent}-${project}`,
        title:
          agent === "claude_code"
            ? `${project}实施计划`
            : `${project}接口与测试`,
        model: agent === "claude_code" ? "claude-opus-4-7" : "gpt-5.5-codex",
        reasoning_effort: agent === "claude_code" ? "high" : "medium",
        permission_mode: agent === "claude_code" ? "acceptEdits" : undefined,
        approval_policy: agent === "codex" ? "on-request" : undefined,
        sandbox_mode: agent === "codex" ? "workspace-write" : undefined,
        context_window_tokens: agent === "claude_code" ? 200_000 : 258_400,
        context_used_tokens: agent === "claude_code" ? 64_800 : 92_300,
        session_total_tokens: agent === "claude_code" ? 91_200 : 184_600,
      },
    },
  };
}
