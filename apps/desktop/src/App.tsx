import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AlertTriangle,
  Archive,
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleUserRound,
  Cloud,
  Code2,
  Cpu,
  Database,
  Eye,
  EyeOff,
  Folder,
  FolderTree,
  HardDrive,
  LogOut,
  MessageSquare,
  Monitor,
  Pause,
  Pin,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  Terminal,
  X,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
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
  PROVIDER_LIST,
  activityTime,
  adjacentItem,
  agentLabel,
  agentShortLabel,
  groupSessionsByWorkspace,
  isAttentionSession,
  permissionFieldLabel,
  relativeTime,
  sessionDisplayTitle,
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
  TgDeviceCodeResponse,
  TgDevicePollResponse,
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
  const [oauthLoading, setOauthLoading] = useState(false);
  const [oauthUrl, setOauthUrl] = useState<string>();
  const [error, setError] = useState(initialError);
  const [deviceFlow, setDeviceFlow] = useState<
    TgDeviceCodeResponse | undefined
  >();
  const [polling, setPolling] = useState(false);

  const [copiedCode, setCopiedCode] = useState(false);
  const [copiedLink, setCopiedLink] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<{ url: string }>("oauth_url_ready", (event) => {
      setOauthUrl(event.payload.url);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const openUrl = async (url: string) => {
    try {
      await invoke("open_external_url", { url });
    } catch {
      window.open(url, "_blank");
    }
  };

  const startBrowserOAuth = async () => {
    if (busy || oauthLoading) return;
    setOauthLoading(true);
    setError(undefined);
    try {
      const snapshot = await invoke<Snapshot>("start_tg_browser_oauth", {
        server_url: null,
      });
      onAuthenticated(snapshot);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setOauthLoading(false);
      setOauthUrl(undefined);
    }
  };

  const startDeviceAuth = async () => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      const codeRes = await invoke<TgDeviceCodeResponse>(
        "start_tg_device_auth",
        { server_url: null },
      );
      setDeviceFlow(codeRes);
      setPolling(true);
      const targetUrl =
        codeRes.verification_uri_complete || codeRes.verification_uri;
      void openUrl(targetUrl);
    } catch (nextError) {
      setError(errorMessage(nextError));
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!polling || !deviceFlow) return;
    let cancelled = false;
    const interval = Math.max(3, deviceFlow.interval || 5) * 1000;
    const timer = setInterval(async () => {
      if (cancelled) return;
      try {
        const res = await invoke<TgDevicePollResponse>("poll_tg_device_auth", {
          server_url: null,
          device_code: deviceFlow.device_code,
        });
        if (res.status === "success" && res.snapshot) {
          clearInterval(timer);
          setPolling(false);
          onAuthenticated(res.snapshot);
        }
      } catch (err) {
        clearInterval(timer);
        setPolling(false);
        setBusy(false);
        setError(errorMessage(err));
      }
    }, interval);

    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [polling, deviceFlow, onAuthenticated]);

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
            Claude Code、Codex、Antigravity、Grok
            Build、历史会话与记忆索引会在登录后自动盘点。只同步状态和脱敏后的项目名。
          </p>
        </div>
        <LocalCloudDiagram />
      </section>
      <section className="auth-panel">
        {deviceFlow ? (
          <div
            className="auth-form"
            style={{ textAlign: "center", gap: "16px" }}
          >
            <header>
              <span className="card-kicker">TannerLab SSO</span>
              <h2>正在等待浏览器授权</h2>
              <p>请在已打开的浏览器中完成登录并确认授权：</p>
            </header>
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: "8px",
                padding: "16px",
                background: "rgba(255, 255, 255, 0.04)",
                borderRadius: "12px",
                border: "1px dashed rgba(255, 255, 255, 0.2)",
              }}
            >
              <span
                style={{
                  fontSize: "12px",
                  color: "#888",
                  textTransform: "uppercase",
                  letterSpacing: "1px",
                }}
              >
                设备验证码 (Device Code)
              </span>
              <div
                style={{
                  fontSize: "26px",
                  fontWeight: 800,
                  letterSpacing: "4px",
                  color: "#7ed5a0",
                  fontFamily: "monospace",
                }}
              >
                {deviceFlow.user_code}
              </div>
              <button
                type="button"
                style={{
                  background: "transparent",
                  border: "none",
                  color: copiedCode ? "#7ed5a0" : "#888",
                  fontSize: "12px",
                  cursor: "pointer",
                  display: "flex",
                  alignItems: "center",
                  gap: "4px",
                  padding: "4px 8px",
                }}
                onClick={() => {
                  void navigator.clipboard.writeText(deviceFlow.user_code);
                  setCopiedCode(true);
                  setTimeout(() => setCopiedCode(false), 2000);
                }}
              >
                {copiedCode ? "✓ 已复制验证码" : "点击复制验证码"}
              </button>
            </div>
            <div
              style={{
                display: "flex",
                flexDirection: "column",
                gap: "10px",
                alignItems: "center",
              }}
            >
              <button
                type="button"
                className="primary-button"
                style={{ width: "100%", padding: "12px" }}
                onClick={() =>
                  void openUrl(
                    deviceFlow.verification_uri_complete ||
                      deviceFlow.verification_uri,
                  )
                }
              >
                在默认浏览器中重新打开授权页
              </button>
              <div style={{ display: "flex", gap: "10px", width: "100%" }}>
                <button
                  type="button"
                  style={{
                    flex: 1,
                    background: "rgba(255, 255, 255, 0.06)",
                    border: "1px solid rgba(255, 255, 255, 0.15)",
                    color: copiedLink ? "#7ed5a0" : "#ccc",
                    padding: "10px",
                    borderRadius: "10px",
                    cursor: "pointer",
                    fontSize: "13px",
                  }}
                  onClick={() => {
                    const url =
                      deviceFlow.verification_uri_complete ||
                      deviceFlow.verification_uri;
                    void navigator.clipboard.writeText(url);
                    setCopiedLink(true);
                    setTimeout(() => setCopiedLink(false), 2000);
                  }}
                >
                  {copiedLink ? "✓ 已复制授权链接" : "复制授权链接"}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setDeviceFlow(undefined);
                    setPolling(false);
                    setBusy(false);
                  }}
                  style={{
                    flex: 1,
                    background: "transparent",
                    border: "1px solid rgba(255, 255, 255, 0.15)",
                    color: "#aaa",
                    padding: "10px",
                    borderRadius: "10px",
                    cursor: "pointer",
                    fontSize: "13px",
                  }}
                >
                  返回账号登录
                </button>
              </div>
            </div>
            <div
              style={{
                fontSize: "12px",
                color: "#888",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: "6px",
              }}
            >
              <RefreshCw className="spinning" size={14} />{" "}
              正在等待网页端授权确认...
            </div>
          </div>
        ) : (
          <form onSubmit={submit} className="auth-form">
            <header>
              <span className="card-kicker">AgentHub Desktop</span>
              <h2>{mode === "login" ? "连接这台电脑" : "创建账号"}</h2>
              <p>
                {mode === "login"
                  ? "登录后立即扫描本机，不必打开终端。"
                  : "一个账号连接你的所有开发设备。"}
              </p>
            </header>
            {oauthLoading ? (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: "10px",
                  padding: "14px",
                  background: "rgba(255, 79, 135, 0.08)",
                  borderRadius: "12px",
                  border: "1px solid rgba(255, 79, 135, 0.3)",
                  textAlign: "center",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    gap: "8px",
                    color: "#ff4f87",
                    fontWeight: 700,
                    fontSize: "14px",
                  }}
                >
                  <RefreshCw className="spinning" size={16} />
                  <span>正在等待浏览器完成授权...</span>
                </div>
                <p
                  style={{
                    margin: 0,
                    fontSize: "12px",
                    color: "#aaa",
                    lineHeight: 1.4,
                  }}
                >
                  已拉起默认浏览器打开授权页。若页面关闭或丢失，请使用下方按钮操作：
                </p>
                <div style={{ display: "flex", gap: "8px", marginTop: "4px" }}>
                  {oauthUrl && (
                    <button
                      type="button"
                      style={{
                        flex: 1,
                        padding: "8px 10px",
                        background: "rgba(255, 255, 255, 0.12)",
                        border: "1px solid rgba(255, 255, 255, 0.2)",
                        borderRadius: "8px",
                        color: "#fff",
                        fontSize: "12px",
                        cursor: "pointer",
                        fontWeight: 600,
                      }}
                      onClick={() => void openUrl(oauthUrl)}
                    >
                      🔗 重新打开网页
                    </button>
                  )}
                  <button
                    type="button"
                    style={{
                      flex: 1,
                      padding: "8px 10px",
                      background: "rgba(255, 255, 255, 0.05)",
                      border: "1px solid rgba(255, 255, 255, 0.15)",
                      borderRadius: "8px",
                      color: "#aaa",
                      fontSize: "12px",
                      cursor: "pointer",
                    }}
                    onClick={() => {
                      setOauthLoading(false);
                      setOauthUrl(undefined);
                    }}
                  >
                    ❌ 取消并重新登录
                  </button>
                </div>
              </div>
            ) : (
              <button
                type="button"
                disabled={busy}
                onClick={startBrowserOAuth}
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: "10px",
                  padding: "12px",
                  borderRadius: "10px",
                  background: "rgba(255, 255, 255, 0.08)",
                  border: "1px solid rgba(255, 255, 255, 0.15)",
                  color: "#f5f2ec",
                  fontWeight: 700,
                  fontSize: "14px",
                  cursor: busy ? "not-allowed" : "pointer",
                  transition: "all 0.2s ease",
                }}
              >
                <span style={{ color: "#ff4f87" }}>◆</span>
                <span>使用 TannerLab 账号统一登录 (Google 级静默秒登)</span>
              </button>
            )}
            <div
              style={{
                display: "flex",
                alignItems: "center",
                textAlign: "center",
                color: "#777",
                fontSize: "12px",
                margin: "4px 0",
              }}
            >
              <div
                style={{
                  flex: 1,
                  height: "1px",
                  background: "rgba(255, 255, 255, 0.1)",
                }}
              />
              <span style={{ padding: "0 10px" }}>或使用邮箱密码</span>
              <div
                style={{
                  flex: 1,
                  height: "1px",
                  background: "rgba(255, 255, 255, 0.1)",
                }}
              />
            </div>
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
        )}
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
  const [searchOpen, setSearchOpen] = useState(false);
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("claude_code");
  const [deviceFilter, setDeviceFilter] = useState<DeviceFilter>("all");
  const [archiveFilter, setArchiveFilter] = useState<"active" | "archived">(
    "active",
  );
  const [selectedSessionKey, setSelectedSessionKey] = useState<string>();
  const [alertDismissed, setAlertDismissed] = useState(false);
  const [showBannerDetails, setShowBannerDetails] = useState(false);
  const [collapsedProviders, setCollapsedProviders] = useState<
    Record<string, boolean>
  >({});
  const [expandedLimits, setExpandedLimits] = useState<Record<string, number>>(
    {},
  );
  const [launchModalOpen, setLaunchModalOpen] = useState(false);
  const [launchModalProvider, setLaunchModalProvider] =
    useState<Session["agent_type"]>("claude_code");
  const [pinnedSessions, setPinnedSessions] = useState<string[]>(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:pinned-sessions");
      return stored ? (JSON.parse(stored) as string[]) : [];
    } catch {
      return [];
    }
  });

  // Quick Task Launcher state
  const [quickPrompt, setQuickPrompt] = useState("");
  const [quickProvider, setQuickProvider] =
    useState<Session["agent_type"]>("claude_code");
  const [quickCwd, setQuickCwd] = useState("d:/ClaudeSpace");
  const [quickLaunching, setQuickLaunching] = useState(false);

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

  const workspaceGroups = useMemo(
    () =>
      groupSessionsByWorkspace(filteredSessions).map((group) => ({
        ...group,
        sessions: [...group.sessions].sort(
          (a, b) =>
            Number(pinnedSessions.includes(sessionKey(b))) -
            Number(pinnedSessions.includes(sessionKey(a))),
        ),
      })),
    [filteredSessions, pinnedSessions],
  );

  const attentionSessions = useMemo(
    () => filteredSessions.filter(isAttentionSession),
    [filteredSessions],
  );

  const recentSessions = useMemo(
    () => filteredSessions.slice(0, 8),
    [filteredSessions],
  );

  const displayWelcomeSessions =
    attentionSessions.length > 0 ? attentionSessions : recentSessions;

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

  useEffect(() => {
    if (agentFilter !== "all") {
      setQuickProvider(agentFilter);
      setLaunchModalProvider(agentFilter);
    }
  }, [agentFilter]);

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

  const handleQuickLaunch = async (event?: FormEvent) => {
    if (event) event.preventDefault();
    if (quickLaunching || quickPrompt.trim() === "") return;
    setQuickLaunching(true);
    setError(undefined);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      const result = await invoke<{
        agent_session_id: string;
        agent_type: string;
      }>("launch_new_session", {
        input: {
          agent_type: quickProvider,
          cwd: quickCwd.trim() ? quickCwd.trim() : null,
          prompt: quickPrompt.trim(),
        },
        onEvent,
      });
      setQuickPrompt("");
      await refresh();
      setSelectedSessionKey(
        `${snapshot.device.device_id}:${result.agent_type}:${result.agent_session_id}`,
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setQuickLaunching(false);
    }
  };

  const openLaunchModalFor = (provider: Session["agent_type"]) => {
    setLaunchModalProvider(provider);
    setLaunchModalOpen(true);
  };

  const toggleProviderCollapse = (provider: string) => {
    setCollapsedProviders((curr) => ({
      ...curr,
      [provider]: !curr[provider],
    }));
  };

  const expandProviderLimit = (provider: string, total: number) => {
    setExpandedLimits((curr) => ({
      ...curr,
      [provider]: total,
    }));
  };

  const togglePinnedSession = (key: string) => {
    setPinnedSessions((current) => {
      const next = current.includes(key)
        ? current.filter((item) => item !== key)
        : [key, ...current];
      window.localStorage.setItem(
        "agenthub:pinned-sessions",
        JSON.stringify(next),
      );
      return next;
    });
  };

  const displayName =
    snapshot.account.user.display_name ||
    snapshot.account.user.email.split("@")[0] ||
    "Administrator";

  const sourceIssue = snapshot.local.sources.find(
    (source) => source.status === "backoff" || !source.detected,
  );
  const showSystemNotice =
    !snapshot.realtime_connected || sourceIssue !== undefined;
  const selectedProvider =
    PROVIDER_LIST.find((provider) => provider.type === agentFilter) ??
    PROVIDER_LIST[0];

  return (
    <main className="claude-app-shell">
      {/* Left Claude Desktop Sidebar */}
      <aside className="claude-sidebar" aria-label="侧边栏导航">
        <div className="sidebar-top-bar">
          <button
            type="button"
            className="sidebar-brand"
            onClick={() => setSelectedSessionKey(undefined)}
            aria-label="返回 AgentHub 首页"
          >
            <span>
              <strong>AgentHub</strong>
            </span>
          </button>
          <div className="top-nav-right">
            <button
              type="button"
              className={`top-icon-btn ${searchOpen ? "active" : ""}`}
              onClick={() => setSearchOpen((v) => !v)}
              title="搜索会话"
              aria-label="搜索会话"
            >
              <Search size={15} />
            </button>
            <button
              type="button"
              className="top-icon-btn"
              disabled={refreshing}
              onClick={() => void refresh()}
              title="刷新本机索引"
              aria-label="刷新本机索引"
            >
              <RefreshCw className={refreshing ? "spin" : ""} size={15} />
            </button>
            <button
              type="button"
              className="top-icon-btn history-button"
              disabled={previousSession === undefined}
              onClick={() =>
                previousSession &&
                setSelectedSessionKey(sessionKey(previousSession))
              }
              title="上一个会话"
            >
              <ChevronLeft size={16} />
            </button>
            <button
              type="button"
              className="top-icon-btn history-button"
              disabled={nextSession === undefined}
              onClick={() =>
                nextSession && setSelectedSessionKey(sessionKey(nextSession))
              }
              title="下一个会话"
            >
              <ChevronRight size={16} />
            </button>
          </div>
        </div>

        <div className="sidebar-mode-pill" role="tablist" aria-label="会话范围">
          <button
            type="button"
            className={archiveFilter === "active" ? "active" : ""}
            onClick={() => {
              setArchiveFilter("active");
              setSelectedSessionKey(undefined);
            }}
          >
            <Activity size={14} />
            <span>进行中</span>
          </button>
          <button
            type="button"
            className={archiveFilter === "archived" ? "active" : ""}
            onClick={() => {
              setArchiveFilter("archived");
              setSelectedSessionKey(undefined);
            }}
          >
            <Archive size={14} />
            <span>已归档</span>
          </button>
        </div>

        <label className="sidebar-agent-picker">
          <span>当前 Agent</span>
          <select
            value={agentFilter}
            onChange={(event) => {
              setAgentFilter(event.target.value as AgentFilter);
              setSelectedSessionKey(undefined);
            }}
            aria-label="选择当前 Agent"
          >
            {PROVIDER_LIST.map((provider) => (
              <option key={provider.type} value={provider.type}>
                {provider.label}
              </option>
            ))}
          </select>
        </label>

        <div className="sidebar-action-buttons">
          <button
            type="button"
            className="sidebar-new-btn"
            onClick={() => {
              setSelectedSessionKey(undefined);
              if (agentFilter !== "all") setLaunchModalProvider(agentFilter);
              setLaunchModalOpen(true);
            }}
          >
            <Plus size={16} />
            <span>新建任务</span>
          </button>
        </div>

        {/* Search Box if opened */}
        {searchOpen && (
          <div className="sidebar-search-box">
            <Search size={14} />
            <input
              autoFocus
              placeholder="按名称、模型或项目搜索..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            {query && (
              <button
                type="button"
                className="clear-query"
                onClick={() => setQuery("")}
              >
                <X size={12} />
              </button>
            )}
          </div>
        )}

        {/* The selected Agent is grouped only by local workspace. */}
        <div className="sidebar-providers-tree">
          {workspaceGroups.length === 0 ? (
            <div className="sidebar-empty">
              <span>
                {query ? "没有匹配的会话" : "正在扫描本机 Agent 会话..."}
              </span>
            </div>
          ) : (
            workspaceGroups.map((group) => {
              const isCollapsed = !!collapsedProviders[group.name];
              const visibleLimit = expandedLimits[group.name] ?? 12;
              const visibleSessions = group.sessions.slice(0, visibleLimit);
              const remaining = group.sessions.length - visibleSessions.length;

              return (
                <div
                  className="provider-group workspace-group"
                  key={group.name}
                >
                  <div
                    className="provider-group-header"
                    onClick={() => toggleProviderCollapse(group.name)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        toggleProviderCollapse(group.name);
                      }
                    }}
                    role="button"
                    tabIndex={0}
                  >
                    <div className="provider-header-left">
                      <span className="provider-chevron">
                        {isCollapsed ? (
                          <ChevronRight size={13} />
                        ) : (
                          <ChevronDown size={13} />
                        )}
                      </span>
                      <strong className="provider-title">{group.name}</strong>
                      <span className="provider-count">
                        {group.sessions.length}
                      </span>
                    </div>
                    <div
                      className="provider-header-actions"
                      onClick={(e) => e.stopPropagation()}
                    >
                      <button
                        type="button"
                        className="provider-action-icon"
                        title={`在 ${group.name} 新建会话`}
                        onClick={() =>
                          agentFilter !== "all" &&
                          openLaunchModalFor(agentFilter)
                        }
                      >
                        <Plus size={14} />
                      </button>
                    </div>
                  </div>

                  {!isCollapsed && (
                    <div className="provider-sessions-list">
                      {visibleSessions.map((session) => {
                        const isSelected =
                          selectedSessionKey === sessionKey(session);
                        const attention = isAttentionSession(session);
                        const isWorking =
                          session.payload.state.activity_status === "WORKING";
                        const title = sessionDisplayTitle(session);

                        return (
                          <div
                            key={sessionKey(session)}
                            className={`sidebar-session-item ${isSelected ? "selected" : ""}`}
                            data-provider={session.agent_type}
                            title={title}
                          >
                            <button
                              type="button"
                              className="sidebar-session-open"
                              onClick={() =>
                                setSelectedSessionKey(sessionKey(session))
                              }
                            >
                              <span className="session-status-indicator">
                                {attention ? (
                                  <AlertTriangle
                                    size={13}
                                    className="status-warn-icon"
                                  />
                                ) : isWorking ? (
                                  <i className="status-dot working" />
                                ) : (
                                  <i className="status-dot normal" />
                                )}
                              </span>
                              <span className="sidebar-session-title">
                                {title}
                              </span>
                            </button>
                            <button
                              type="button"
                              className={`sidebar-pin ${pinnedSessions.includes(sessionKey(session)) ? "active" : ""}`}
                              onClick={() =>
                                togglePinnedSession(sessionKey(session))
                              }
                              aria-label={
                                pinnedSessions.includes(sessionKey(session))
                                  ? "取消置顶"
                                  : "置顶会话"
                              }
                              title={
                                pinnedSessions.includes(sessionKey(session))
                                  ? "取消置顶"
                                  : "置顶会话"
                              }
                            >
                              <Pin size={11} />
                            </button>
                          </div>
                        );
                      })}

                      {remaining > 0 && (
                        <button
                          type="button"
                          className="sidebar-show-more"
                          onClick={() =>
                            expandProviderLimit(
                              group.name,
                              group.sessions.length,
                            )
                          }
                        >
                          再显示 {remaining} 个
                        </button>
                      )}
                      {remaining === 0 && group.sessions.length > 12 && (
                        <button
                          type="button"
                          className="sidebar-show-more collapse"
                          onClick={() =>
                            setExpandedLimits((current) => ({
                              ...current,
                              [group.name]: 12,
                            }))
                          }
                        >
                          收起
                        </button>
                      )}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>

        <div className="sidebar-footer">
          <div className="sidebar-profile-card">
            <button
              type="button"
              className="profile-account-btn"
              onClick={() => setAccountOpen((open) => !open)}
              aria-expanded={accountOpen}
            >
              <span className="profile-brand-icon" aria-hidden="true">
                {initials(displayName)}
              </span>
              <span className="profile-meta">
                <strong>{displayName}</strong>
                <small>
                  {snapshot.realtime_connected ? "实时连接" : "轮询连接"}
                  <i
                    className={`live-status-dot ${snapshot.realtime_connected ? "" : "fallback"}`}
                  />
                </small>
              </span>
              <ChevronDown size={14} className="profile-chevron" />
            </button>
          </div>
        </div>
      </aside>

      {/* Main Content Stage */}
      <section className="claude-stage">
        {selectedSession !== undefined ? (
          /* Active Session View Embedded in Stage */
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
        ) : (
          <div className="claude-home-stage">
            <header className="workspace-masthead">
              <div className="workspace-title-block">
                <span className="prompt-spark">
                  <Sparkles size={18} />
                </span>
                <h1>接下来做什么，{displayName}？</h1>
              </div>
            </header>

            {!alertDismissed && showSystemNotice && (
              <div className="claude-alert-box">
                <div className="alert-content">
                  <div className="alert-title-row">
                    <AlertTriangle size={16} className="alert-icon" />
                    <strong>
                      {!snapshot.realtime_connected
                        ? "实时通道暂不可用，已切换为安全轮询"
                        : `${agentShortLabel(sourceIssue!.agent_type)} 尚未完成本机接入`}
                    </strong>
                  </div>
                  {showBannerDetails && (
                    <p className="alert-details-text">
                      已发现 {snapshot.local.session_count}{" "}
                      个本机会话，现有数据不会丢失。 可在本机设置中检查 CLI
                      路径与索引周期。
                    </p>
                  )}
                  <div className="alert-actions-row">
                    <button
                      type="button"
                      className="alert-link"
                      onClick={() => setShowBannerDetails((v) => !v)}
                    >
                      {showBannerDetails ? "收起详情" : "查看详情"}
                    </button>
                    <button
                      type="button"
                      className="alert-btn"
                      onClick={() => setSettingsOpen(true)}
                    >
                      打开设置
                    </button>
                    <button
                      type="button"
                      className="alert-btn"
                      disabled={refreshing}
                      onClick={() => void refresh()}
                    >
                      {refreshing ? "正在检查…" : "重新检查"}
                    </button>
                  </div>
                </div>
                <button
                  type="button"
                  className="alert-close-btn"
                  onClick={() => setAlertDismissed(true)}
                  aria-label="关闭通知"
                >
                  <X size={15} />
                </button>
              </div>
            )}

            <form onSubmit={handleQuickLaunch} className="claude-prompt-dock">
              <div className="prompt-dock-heading">
                <span className="prompt-spark">
                  <Sparkles size={15} />
                </span>
                <strong>交给 {selectedProvider.shortLabel}</strong>
                <span>Enter 发送 · Shift + Enter 换行</span>
              </div>
              <div className="prompt-dock-form">
                <textarea
                  rows={3}
                  placeholder="描述一项任务或提出问题"
                  value={quickPrompt}
                  onChange={(e) => setQuickPrompt(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void handleQuickLaunch();
                    }
                  }}
                  disabled={quickLaunching}
                />
                <button
                  type="submit"
                  className="dock-submit-btn"
                  disabled={quickLaunching || !quickPrompt.trim()}
                  title="发送任务指令"
                >
                  {quickLaunching ? (
                    <RefreshCw className="spin" size={16} />
                  ) : (
                    <Send size={16} />
                  )}
                </button>
              </div>

              <div className="prompt-dock-bottom">
                <label className="dock-path-field">
                  <Folder size={13} />
                  <span className="sr-only">工作目录</span>
                  <input
                    value={quickCwd}
                    onChange={(event) => setQuickCwd(event.target.value)}
                    aria-label="工作目录"
                    spellCheck={false}
                  />
                </label>
                <div className="dock-bottom-right">
                  <span className="dock-model-label">
                    {selectedProvider.defaultModel}
                  </span>
                  <span className="dock-safety-note">
                    <ShieldCheck size={12} /> 遵循本机权限
                  </span>
                </div>
              </div>
            </form>
          </div>
        )}

        {/* Global Account, Settings, and Launch Modals */}
        {accountOpen && (
          <AccountMenu
            snapshot={snapshot}
            busy={actionBusy}
            onClose={() => setAccountOpen(false)}
            onOpenSettings={() => {
              setAccountOpen(false);
              setSettingsOpen(true);
            }}
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
        <LaunchSessionModal
          open={launchModalOpen}
          initialAgentType={launchModalProvider}
          onClose={() => setLaunchModalOpen(false)}
          onLaunched={async (agentSessionId, agentType) => {
            await refresh();
            setSelectedSessionKey(
              `${snapshot.device.device_id}:${agentType}:${agentSessionId}`,
            );
          }}
        />
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
  const title = sessionDisplayTitle(session);
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
        <strong>{agentLabel(session.agent_type)}</strong>
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

function LaunchSessionModal({
  open,
  initialAgentType = "claude_code",
  onClose,
  onLaunched,
}: {
  open: boolean;
  initialAgentType?: Session["agent_type"];
  onClose: () => void;
  onLaunched: (
    agentSessionId: string,
    agentType: Session["agent_type"],
  ) => void;
}) {
  const [agentType, setAgentType] =
    useState<Session["agent_type"]>(initialAgentType);
  const [cwd, setCwd] = useState("d:/ClaudeSpace");
  const [prompt, setPrompt] = useState("");
  const [launching, setLaunching] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (open) setAgentType(initialAgentType);
  }, [initialAgentType, open]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (launching || prompt.trim() === "") return;
    setLaunching(true);
    setError(undefined);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      const result = await invoke<{
        agent_session_id: string;
        agent_type: string;
      }>("launch_new_session", {
        input: {
          agent_type: agentType,
          cwd: cwd.trim() ? cwd.trim() : null,
          prompt: prompt.trim(),
        },
        onEvent,
      });
      onLaunched(result.agent_session_id, agentType);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setLaunching(false);
    }
  };

  if (!open) return null;

  return (
    <div className="panel-scrim launch-session-scrim" onMouseDown={onClose}>
      <div
        className="launch-modal-card"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="launch-modal-header">
          <div>
            <span className="launch-modal-mark">
              <Sparkles size={18} />
            </span>
            <span>
              <small>NEW LOCAL SESSION</small>
              <h3>发起 Agent 会话</h3>
            </span>
          </div>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="关闭新建会话"
          >
            <X size={18} />
          </button>
        </header>
        <form onSubmit={submit} className="launch-modal-form">
          <fieldset className="launch-provider-fieldset">
            <legend>选择目标 Agent</legend>
            <div className="launch-provider-grid">
              {PROVIDER_LIST.map((item) => (
                <button
                  key={item.type}
                  type="button"
                  className={agentType === item.type ? "selected" : ""}
                  data-provider={item.type}
                  onClick={() => setAgentType(item.type)}
                >
                  <i aria-hidden="true" />
                  <span>
                    <strong>{item.label}</strong>
                    <small>{item.desc}</small>
                  </span>
                </button>
              ))}
            </div>
          </fieldset>
          <label className="launch-form-field">
            <span>工作目录</span>
            <input
              type="text"
              placeholder="例如 d:/ClaudeSpace 或 d:/ClaudeSpace/Project/..."
              value={cwd}
              onChange={(e) => setCwd(e.target.value)}
              required
            />
          </label>
          <label className="launch-form-field">
            <span>初始任务指令</span>
            <textarea
              rows={3}
              placeholder="输入你想让 Agent 执行的第一条指令..."
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              required
              autoFocus
            />
          </label>
          {error && <p className="form-error">{error}</p>}
          <footer className="launch-modal-actions">
            <button
              type="button"
              className="outline-button"
              onClick={onClose}
              disabled={launching}
            >
              取消
            </button>
            <button
              type="submit"
              className="primary-button"
              disabled={launching || !prompt.trim()}
            >
              {launching ? (
                <>
                  <RefreshCw className="spin" size={16} /> 正在拉起 Agent...
                </>
              ) : (
                <>
                  <Play size={16} /> 启动并进入会话
                </>
              )}
            </button>
          </footer>
        </form>
      </div>
    </div>
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
  >("conversation");
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
  const title = sessionDisplayTitle(session);
  const permission =
    session.agent_type === "codex"
      ? (runtime?.permission_mode ?? runtime?.approval_policy)
      : runtime?.permission_mode;

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

  useEffect(() => {
    let disposed = false;
    invoke<ConversationMessage[]>("load_session_conversation", {
      input: {
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
      },
    })
      .then((history) => {
        if (!disposed && history.length > 0) setMessages(history);
      })
      .catch(() => {
        // History is best-effort; the live session remains usable.
      });
    return () => {
      disposed = true;
    };
  }, [session.agent_session_id, session.agent_type]);

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
          <div className="session-title-line">
            <h2 title={title}>{title}</h2>
            <span>{session.payload.project.display_name || "未分类项目"}</span>
          </div>
          <div className="session-header-actions">
            <button
              className="icon-button"
              aria-label="会话内容"
              title="会话内容"
              aria-pressed={panelView === "conversation"}
              onClick={() => setPanelView("conversation")}
            >
              <MessageSquare size={15} />
            </button>
            <button
              className="icon-button"
              aria-label="运行上下文"
              title="运行上下文"
              aria-pressed={panelView === "context"}
              onClick={() => setPanelView("context")}
            >
              <Terminal size={15} />
            </button>
            <button
              className="icon-button"
              aria-label="时间线"
              title="时间线"
              aria-pressed={panelView === "timeline"}
              onClick={() => setPanelView("timeline")}
            >
              <Activity size={15} />
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
                <dt>{permissionFieldLabel(session.agent_type)}</dt>
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
                  消息直接交给本机 CLI；内容不进入 AgentHub 云端或本地索引。
                </p>
              </div>
            ) : (
              messages.map((message, index) => (
                <ConversationItem
                  message={message}
                  key={`${message.role}-${message.kind ?? "message"}-${index}`}
                />
              ))
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
          <div className="composer-container">
            <div className="slash-commands-bar">
              <span className="slash-title">快捷指令:</span>
              {["/help", "/clear", "/compact", "/cost", "/review", "/test"].map(
                (cmd) => (
                  <button
                    key={cmd}
                    type="button"
                    className="slash-chip"
                    onClick={() => setPrompt((p) => (p ? `${p} ${cmd}` : cmd))}
                  >
                    {cmd}
                  </button>
                ),
              )}
            </div>
            <form className="session-composer" onSubmit={sendMessage}>
              <textarea
                aria-label="发送给 Agent"
                maxLength={16_000}
                placeholder={
                  session.archived
                    ? "恢复会话后再发送"
                    : "继续这个会话… (Enter 发送，Shift+Enter 换行)"
                }
                value={prompt}
                onChange={(event) => setPrompt(event.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    void sendMessage(e);
                  }
                }}
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
          </div>
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

function ConversationItem({ message }: { message: ConversationMessage }) {
  if (message.content === "" && message.kind !== "tool") return null;
  if (
    message.kind === "tool" ||
    message.kind === "command" ||
    message.kind === "status"
  ) {
    const failed = message.status === "failed";
    return (
      <details className={`activity-item ${failed ? "failed" : ""}`}>
        <summary>
          <span className="activity-item-icon">
            {message.kind === "command" ? (
              <Terminal size={14} />
            ) : message.kind === "status" ? (
              <Activity size={14} />
            ) : (
              <Code2 size={14} />
            )}
          </span>
          <strong>
            {message.label ||
              (message.kind === "command" ? "命令" : "工具调用")}
          </strong>
          <span className="activity-item-state">
            {failed
              ? "失败"
              : message.status === "running"
                ? "运行中"
                : message.status === "completed"
                  ? "完成"
                  : "状态"}
          </span>
          <ChevronRight size={13} className="activity-chevron" />
        </summary>
        {message.content !== "" && <pre>{message.content}</pre>}
      </details>
    );
  }
  return (
    <article className={`turn-message ${message.role}`}>
      {message.role === "user" && <span>你</span>}
      <div className="markdown-body">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>
          {message.content}
        </ReactMarkdown>
      </div>
    </article>
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
  const [grokBinary, setGrokBinary] = useState(preferences.grok_binary);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [section, setSection] = useState<"general" | "agents" | "privacy">(
    "general",
  );

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
            grok_binary: grokBinary,
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
          <h2>设置</h2>
          <button
            className="icon-button"
            aria-label="关闭设置"
            onClick={onClose}
          >
            <X />
          </button>
        </header>
        <div className="settings-layout">
          <nav className="settings-nav" aria-label="设置分类">
            <button
              className={section === "general" ? "active" : ""}
              onClick={() => setSection("general")}
            >
              <Settings size={16} />
              通用
            </button>
            <button
              className={section === "agents" ? "active" : ""}
              onClick={() => setSection("agents")}
            >
              <Cpu size={16} />
              Agent 与同步
            </button>
            <button
              className={section === "privacy" ? "active" : ""}
              onClick={() => setSection("privacy")}
            >
              <ShieldCheck size={16} />
              隐私
            </button>
          </nav>
          <form onSubmit={save}>
            {section === "general" && (
              <section className="settings-section">
                <div>
                  <h3>通用</h3>
                  <p>控制 AgentHub 桌面端的刷新与显示行为。</p>
                </div>
                <label className="settings-row">
                  <span>
                    <strong>刷新周期</strong>
                    <small>实时通道断开时用于兜底轮询。</small>
                  </span>
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
              </section>
            )}
            {section === "agents" && (
              <section className="settings-section">
                <div>
                  <h3>Agent 与同步</h3>
                  <p>自动发现本机会话，并从各 Agent 的本地存储导入历史内容。</p>
                </div>
                <label>
                  <span>Claude Code CLI</span>
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
                <label>
                  <span>Grok Build CLI</span>
                  <input
                    value={grokBinary}
                    maxLength={260}
                    onChange={(event) => setGrokBinary(event.target.value)}
                    placeholder="grok"
                    required
                  />
                </label>
                <div className="settings-info">
                  <RefreshCw size={16} />
                  <span>
                    <strong>自动导入已开启</strong>
                    <small>
                      Claude Code、Codex、Antigravity 与 Grok Build
                      会话会按工作目录整理。
                    </small>
                  </span>
                </div>
              </section>
            )}
            {section === "privacy" && (
              <section className="settings-section">
                <div>
                  <h3>隐私</h3>
                  <p>会话正文仅在当前设备解码并展示。</p>
                </div>
                <div className="settings-info">
                  <HardDrive size={17} />
                  <span>
                    <strong>本地正文</strong>
                    <small>不会写入 AgentHub 云端、日志或同步队列。</small>
                  </span>
                </div>
                <div className="settings-info">
                  <ShieldCheck size={17} />
                  <span>
                    <strong>凭据隔离</strong>
                    <small>Token、Cookie 与私钥不会随会话一起导入。</small>
                  </span>
                </div>
              </section>
            )}
            {error !== undefined && <p className="panel-error">{error}</p>}
            <footer className="settings-actions">
              <button className="primary-button" disabled={busy} type="submit">
                {busy ? <RefreshCw className="spin" /> : <CheckCircle2 />}
                保存设置
              </button>
            </footer>
          </form>
        </div>
      </aside>
    </div>
  );
}

function AccountMenu({
  snapshot,
  busy,
  onClose,
  onOpenSettings,
  onLogout,
}: {
  snapshot: Snapshot;
  busy: boolean;
  onClose: () => void;
  onOpenSettings: () => void;
  onLogout: () => void;
}) {
  return (
    <aside className="account-menu" aria-label="账号菜单">
      <button
        className="account-menu-backdrop"
        aria-label="关闭账号菜单"
        onClick={onClose}
      />
      <div className="account-menu-card">
        <div className="account-menu-identity">
          <strong>{snapshot.account.user.display_name || "未设置名称"}</strong>
          <span>{snapshot.account.user.email}</span>
          <small>
            {planLabel(snapshot.account.subscription.plan)} ·{" "}
            {snapshot.realtime_connected ? "实时连接" : "轮询连接"}
          </small>
        </div>
        <button type="button" onClick={onOpenSettings}>
          <Settings size={15} />
          <span>设置</span>
          <small>Ctrl+,</small>
        </button>
        <div className="account-menu-separator" />
        <button type="button" disabled={busy} onClick={onLogout}>
          <LogOut size={15} />
          <span>退出登录</span>
        </button>
      </div>
    </aside>
  );
}

function Brand({ size = 26 }: { size?: number }) {
  return (
    <div
      className="brand"
      style={{ display: "flex", alignItems: "center", gap: "10px" }}
    >
      <PulseMark size={size} />
      <div
        style={{ display: "flex", flexDirection: "column", lineHeight: 1.15 }}
      >
        <span
          style={{
            fontWeight: 800,
            fontSize: "15px",
            letterSpacing: "0.02em",
            color: "#f8fafc",
          }}
        >
          Agent{" "}
          <span
            style={{
              background: "linear-gradient(90deg, #00f5d4, #00bbf9, #a855f7)",
              WebkitBackgroundClip: "text",
              WebkitTextFillColor: "transparent",
            }}
          >
            Hub
          </span>
        </span>
        <span
          style={{
            fontSize: "10px",
            color: "#94a3b8",
            letterSpacing: "0.06em",
          }}
        >
          智能体集成中心
        </span>
      </div>
    </div>
  );
}

function PulseMark({ size = 24 }: { size?: number }) {
  return (
    <span
      className="pulse-mark"
      aria-hidden="true"
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: size,
        height: size,
      }}
    >
      <svg
        viewBox="0 0 512 512"
        width={size}
        height={size}
        style={{ filter: "drop-shadow(0 0 6px rgba(0, 229, 255, 0.7))" }}
      >
        <g
          stroke="#00e5ff"
          strokeWidth="20"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        >
          <line x1="256" y1="256" x2="256" y2="136" />
          <line x1="256" y1="256" x2="360" y2="196" />
          <line x1="256" y1="256" x2="360" y2="316" />
          <line x1="256" y1="256" x2="256" y2="376" />
          <line x1="256" y1="256" x2="152" y2="316" />
          <line x1="256" y1="256" x2="152" y2="196" />
          <polygon points="256,136 360,196 360,316 256,376 152,316 152,196" />
        </g>
        <g stroke="#00e5ff" strokeWidth="14" fill="#0b0f19">
          <circle cx="256" cy="136" r="22" />
          <circle cx="360" cy="196" r="22" />
          <circle cx="360" cy="316" r="22" />
          <circle cx="256" cy="376" r="22" />
          <circle cx="152" cy="316" r="22" />
          <circle cx="152" cy="196" r="22" />
        </g>
        <circle cx="256" cy="256" r="42" fill="#00e5ff" opacity="0.4" />
        <circle cx="256" cy="256" r="26" fill="#ffffff" />
      </svg>
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
          <Code2 /> <PulseMark size={14} />
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
      {
        agent_type: "antigravity",
        session_count: 7,
        detected: true,
        status: "healthy",
      },
      {
        agent_type: "grok_build",
        session_count: 4,
        detected: true,
        status: "healthy",
      },
    ],
    memory: {
      claude_files: 4,
      claude_bytes: 4_909,
      codex_indexes: 1,
      codex_bytes: 1_306_624,
      grok_sessions: 4,
      grok_bytes: 18_432,
    },
    refreshed_at: new Date().toISOString(),
  },
  preferences: {
    refresh_interval_seconds: 4,
    claude_binary: "claude",
    codex_binary: "codex",
    grok_binary: "grok",
  },
  sessions: [
    demoSession("claude_code", "电商平台重构", "WORKING", 2),
    demoSession("claude_code", "支付服务优化", "WAITING_PERMISSION", 8),
    demoSession("claude_code", "用户增长分析", "IDLE", 25),
    demoSession("codex", "API 网关开发", "WORKING", 1),
    demoSession("codex", "数据导出工具", "UNKNOWN", 180, "PERSISTED"),
    demoSession("antigravity", "搜索体验实验", "IDLE", 44),
    demoSession("grok_build", "AgentHub 采集", "WORKING", 3),
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
            : agent === "grok_build"
              ? `${project}采集适配`
              : `${project}接口与测试`,
        model:
          agent === "claude_code"
            ? "claude-opus-4-7"
            : agent === "grok_build"
              ? "grok-4.6"
              : "gpt-5.5-codex",
        reasoning_effort: agent === "codex" ? "medium" : "high",
        permission_mode:
          agent === "claude_code"
            ? "acceptEdits"
            : agent === "grok_build"
              ? "grok-build-plan"
              : undefined,
        approval_policy: agent === "codex" ? "on-request" : undefined,
        sandbox_mode:
          agent === "codex"
            ? "workspace-write"
            : agent === "grok_build"
              ? "off"
              : undefined,
        context_window_tokens:
          agent === "claude_code"
            ? 200_000
            : agent === "grok_build"
              ? 256_000
              : 258_400,
        context_used_tokens:
          agent === "claude_code"
            ? 64_800
            : agent === "grok_build"
              ? 48_200
              : 92_300,
        session_total_tokens:
          agent === "claude_code"
            ? 91_200
            : agent === "grok_build"
              ? 72_400
              : 184_600,
      },
    },
  };
}
