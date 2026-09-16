import { Channel, invoke } from "@tauri-apps/api/core";
import { NativeHostDialog } from "./components/NativeHostDialog";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AlertTriangle,
  Archive,
  ArrowLeft,
  ArrowUp,
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
  Paperclip,
  Pin,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Sliders,
  Smartphone,
  Sparkles,
  Square,
  Terminal,
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
  fetchAgentModels,
  defaultModelFor,
  useAgentModels,
  type AgentModelOption,
} from "./agentModels";
import {
  cachedConversation,
  loadConversation,
  prefetchConversations,
  seedConversation,
} from "./conversationCache";
import {
  PROVIDER_LIST,
  activityTime,
  adjacentItem,
  agentLabel,
  agentShortLabel,
  getDefaultModel,
  groupSessionsByWorkspace,
  hasActiveTurn,
  isAttentionSession,
  permissionFieldLabel,
  relativeTime,
  rememberTitleOverride,
  sessionDisplayTitle,
  statusView,
} from "./presentation";
import {
  appendAssistantDelta,
  dropTrailingEmptyAssistant,
  replaceAssistantMessage,
  type ConversationMessage,
} from "./turn-stream";
import {
  normalizeAgentType,
  type ActivityStatus,
  type DesktopPreferences,
  type DashboardRealtimeEvent,
  type LocalSource,
  type Session,
  type SessionTimelineEvent,
  type SessionTurnEvent,
  type Snapshot,
  type TgDeviceCodeResponse,
  type TgDevicePollResponse,
} from "./types";

import {
  cancelSessionTurn,
  resolveSessionApproval,
  sendSessionMessage,
} from "./sessionClient";
import { SelectDropdown } from "./components/SelectDropdown";
import {
  SessionContextMenu,
  type ContextMenuPosition,
} from "./components/SessionContextMenu";
import {
  ConversationItem,
  groupConversationMessages,
  ToolCallGroupBlock,
} from "./components/ConversationRenderer";
import { DesktopSettingsModal } from "./components/DesktopSettingsModal";
import { DesktopHeader } from "./components/layout/DesktopHeader";
import { SessionEmptyState } from "./components/session/SessionEmptyState";
import { ContextCapacityRing } from "./components/session/ContextCapacityPopover";
import { ComposerPlusMenu } from "./components/session/ComposerPlusMenu";

type AuthMode = "login" | "register";
type AgentFilter = "all" | Session["agent_type"];
type DeviceFilter = "all" | string;

const isManagedProvider = (
  agentType: AgentFilter,
): agentType is Session["agent_type"] =>
  agentType === "codex" || agentType === "claude_code" || agentType === "zcode";

export function App() {
  const demo =
    import.meta.env.DEV &&
    new URLSearchParams(window.location.search).has("demo");
  const [snapshot, setSnapshot] = useState<Snapshot | undefined>(() =>
    demo ? demoSnapshot : undefined,
  );
  const [restoring, setRestoring] = useState(!demo);
  const [error, setError] = useState<string>();
  const [localHost, setLocalHost] = useState(false);

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
    try {
      const stored = window.localStorage.getItem("agenthub:extended-preferences");
      if (stored) {
        const parsed = JSON.parse(stored);
        if (parsed?.theme) {
          document.documentElement.setAttribute("data-theme", parsed.theme);
        }
      }
    } catch {
      // ignore
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
    <>
      <Dashboard
        initialSnapshot={snapshot}
        onLogout={() => setSnapshot(undefined)}
        onOpenLocalHost={!demo ? () => setLocalHost(true) : undefined}
      />
      {localHost && <NativeHostDialog onClose={() => setLocalHost(false)} />}
    </>
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
  const [serverUrl, setServerUrl] = useState("https://platform.tannerlab.xyz");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>(initialError);

  // TG device code auth state
  const [deviceFlow, setDeviceFlow] = useState<TgDeviceCodeResponse>();
  const [polling, setPolling] = useState(false);

  const startTgDeviceAuth = async () => {
    setBusy(true);
    setError(undefined);
    try {
      const resp = await invoke<TgDeviceCodeResponse>("start_tg_device_auth", {
        serverUrl: serverUrl.trim() || undefined,
      });
      setDeviceFlow(resp);
      setPolling(true);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!polling || !deviceFlow) return;
    let timer: number | undefined;
    let disposed = false;
    const intervalMs = Math.max(2000, deviceFlow.interval * 1000);

    const poll = async () => {
      try {
        const resp = await invoke<TgDevicePollResponse>("poll_tg_device_auth", {
          deviceCode: deviceFlow.device_code,
          serverUrl: serverUrl.trim() || undefined,
        });
        if (disposed) return;
        if (resp.status === "success" && resp.snapshot) {
          onAuthenticated(resp.snapshot);
          return;
        }
      } catch (err) {
        if (!disposed) {
          setError(errorMessage(err));
          setPolling(false);
        }
        return;
      }
      if (!disposed) {
        timer = window.setTimeout(poll, intervalMs);
      }
    };

    timer = window.setTimeout(poll, intervalMs);
    return () => {
      disposed = true;
      if (timer) window.clearTimeout(timer);
    };
  }, [polling, deviceFlow, serverUrl, onAuthenticated]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      const next = await invoke<Snapshot>("authenticate", {
        input: {
          mode,
          email: email.trim(),
          password,
          display_name:
            mode === "register" && displayName.trim()
              ? displayName.trim()
              : undefined,
          server_url: serverUrl.trim() ? serverUrl.trim() : undefined,
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
      <div className="auth-card">
        <header className="auth-header">
          <Brand size={36} />
          <h2>{mode === "login" ? "登录 AgentHub" : "注册新账号"}</h2>
          <p>
            {mode === "login"
              ? "登录后选择工作目录，连接并接续你的原生 Agent 会话"
              : "创建账号以同步管理多个开发设备"}
          </p>
        </header>

        {deviceFlow && polling ? (
          <div className="tg-device-card">
            <p className="tg-device-instructions">
              请在浏览器打开以下页面并确认授权：
            </p>
            <a
              href={
                deviceFlow.verification_uri_complete ||
                deviceFlow.verification_uri
              }
              target="_blank"
              rel="noreferrer"
              className="tg-verify-link"
            >
              {deviceFlow.verification_uri}
            </a>
            <div className="tg-user-code-box">
              <span>验证码</span>
              <strong>{deviceFlow.user_code}</strong>
            </div>
            <div className="loading-line">
              <i />
            </div>
            <p className="tg-waiting-text">正在等待浏览器确认…</p>
            <button
              type="button"
              className="outline-button"
              onClick={() => {
                setPolling(false);
                setDeviceFlow(undefined);
              }}
            >
              返回账号密码登录
            </button>
          </div>
        ) : (
          <form onSubmit={submit} className="auth-form">
            <label>
              <span>邮箱</span>
              <input
                type="email"
                placeholder="developer@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                required
                autoFocus
              />
            </label>
            <label>
              <span>密码</span>
              <input
                type="password"
                placeholder="••••••••"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                required
              />
            </label>
            {mode === "register" && (
              <label>
                <span>显示名称（可选）</span>
                <input
                  type="text"
                  placeholder="Tang Tang"
                  value={displayName}
                  onChange={(e) => setDisplayName(e.target.value)}
                />
              </label>
            )}

            <button
              type="button"
              className="advanced-toggle"
              onClick={() => setShowAdvanced((v) => !v)}
            >
              {showAdvanced ? "收起高级设置" : "高级设置（自定义服务器）"}
            </button>

            {showAdvanced && (
              <label className="advanced-field">
                <span>服务器地址</span>
                <input
                  type="url"
                  placeholder="https://platform.tannerlab.xyz"
                  value={serverUrl}
                  onChange={(e) => setServerUrl(e.target.value)}
                />
              </label>
            )}

            {error && <p className="form-error">{error}</p>}

            <button
              type="submit"
              className="primary-button submit-btn"
              disabled={busy}
            >
              {busy ? (
                <>
                  <RefreshCw className="spin" size={16} /> 正在连接...
                </>
              ) : mode === "login" ? (
                "登录并连接"
              ) : (
                "创建账号"
              )}
            </button>

            <button
              type="button"
              className="outline-button tg-oauth-btn"
              onClick={startTgDeviceAuth}
              disabled={busy}
            >
              <Sparkles size={16} />
              通过 Telegram / 浏览器快捷登录
            </button>

            <footer className="auth-footer">
              {mode === "login" ? (
                <span>
                  还没有账号？
                  <button type="button" onClick={() => setMode("register")}>
                    立即注册
                  </button>
                </span>
              ) : (
                <span>
                  已有账号？
                  <button type="button" onClick={() => setMode("login")}>
                    直接登录
                  </button>
                </span>
              )}
            </footer>
          </form>
        )}
      </div>
    </main>
  );
}

function Dashboard({
  initialSnapshot,
  onLogout,
  onOpenLocalHost,
}: {
  initialSnapshot: Snapshot;
  onLogout: () => void;
  onOpenLocalHost?: () => void;
}) {
  const [snapshot, setSnapshot] = useState(initialSnapshot);
  const [actionBusy, setActionBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string>();
  const [accountOpen, setAccountOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("all");
  const [deviceFilter, setDeviceFilter] = useState<DeviceFilter>("all");
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
    useState<Session["agent_type"]>("codex");
  const [pinnedSessions, setPinnedSessions] = useState<string[]>(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:pinned-sessions");
      return stored ? (JSON.parse(stored) as string[]) : [];
    } catch {
      return [];
    }
  });

  // Selected models per agent provider (Point 1)
  const [selectedModelsByAgent, setSelectedModelsByAgent] = useState<
    Record<string, string>
  >(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:selected-models");
      return stored ? JSON.parse(stored) : {};
    } catch {
      return {};
    }
  });

  // Native Host phase state for real-time indicator
  const [hostPhase, setHostPhase] = useState<
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected"
  >("loading");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) {
      setHostPhase("local");
      return;
    }
    let active = true;
    const check = async () => {
      try {
        const status = await invoke<{
          phase:
            | "loading"
            | "starting"
            | "stopped"
            | "local"
            | "connecting"
            | "connected";
        }>("remote_host_status");
        if (active) setHostPhase(status.phase);
      } catch {
        if (active) setHostPhase("stopped");
      }
    };
    void check();
    const timer = setInterval(() => void check(), 2500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  // Context Menu & Toast state (Point 11)
  const [contextMenu, setContextMenu] = useState<ContextMenuPosition | null>(
    null,
  );
  const [toastMessage, setToastMessage] = useState<string>();

  const showToast = (msg: string) => {
    setToastMessage(msg);
    setTimeout(
      () => setToastMessage((curr) => (curr === msg ? undefined : curr)),
      2500,
    );
  };

  const refreshInFlight = useRef(false);
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());

  // Cache all observed sessions into localStorage so that offline/disconnected views
  // still present the full ChatGPT-like history list rather than shrinking down to a few items.
  const [cachedSessions, setCachedSessions] = useState<Session[]>(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:cached-sessions");
      if (!stored) return [];
      const parsed = JSON.parse(stored) as Session[];
      return parsed
        .filter((s) => !s.agent_session_id.startsWith("sess_subagent"))
        .map((s) => ({
          ...s,
          agent_type: normalizeAgentType(s.agent_type),
        }));
    } catch {
      return [];
    }
  });

  useEffect(() => {
    if (snapshot.sessions.length > 0) {
      setCachedSessions((prev) => {
        const map = new Map<string, Session>();
        for (const s of prev) {
          if (s.agent_session_id.startsWith("sess_subagent")) continue;
          const norm = { ...s, agent_type: normalizeAgentType(s.agent_type) };
          map.set(sessionKey(norm), norm);
        }
        for (const s of snapshot.sessions) {
          if (s.agent_session_id.startsWith("sess_subagent")) continue;
          const norm = { ...s, agent_type: normalizeAgentType(s.agent_type) };
          map.set(sessionKey(norm), norm);
        }
        const merged = Array.from(map.values());
        try {
          window.localStorage.setItem(
            "agenthub:cached-sessions",
            JSON.stringify(merged.slice(0, 300)),
          );
        } catch {
          // ignore quota error
        }
        return merged;
      });
    }
  }, [snapshot.sessions]);

  const sessions = useMemo(() => {
    const map = new Map<string, Session>();
    // First fill from cached sessions
    for (const s of cachedSessions) {
      if (s.agent_session_id.startsWith("sess_subagent")) continue;
      const norm = { ...s, agent_type: normalizeAgentType(s.agent_type) };
      map.set(sessionKey(norm), norm);
    }
    // Then overwrite with live snapshot sessions (takes precedence)
    for (const s of snapshot.sessions) {
      if (s.agent_session_id.startsWith("sess_subagent")) continue;
      const norm = { ...s, agent_type: normalizeAgentType(s.agent_type) };
      map.set(sessionKey(norm), norm);
    }
    return Array.from(map.values()).sort((a, b) =>
      activityTime(b).localeCompare(activityTime(a)),
    );
  }, [snapshot.sessions, cachedSessions]);

  // Active Sessions only - Point 3 & 10 (remove archived)
  const filteredSessions = useMemo(
    () =>
      sessions.filter((session) => {
        const normAgent = normalizeAgentType(session.agent_type);
        if (agentFilter !== "all" && normAgent !== agentFilter) return false;
        if (deviceFilter !== "all" && session.device_id !== deviceFilter)
          return false;
        if (session.archived === true) return false;
        if (deferredQuery === "") return true;
        const haystack = [
          session.payload.runtime?.title,
          session.payload.project.display_name,
          session.payload.runtime?.model,
          normAgent,
        ]
          .filter(Boolean)
          .join(" ")
          .toLocaleLowerCase();
        return haystack.includes(deferredQuery);
      }),
    [agentFilter, deferredQuery, deviceFilter, sessions],
  );

  const workspaceGroups = useMemo(
    () =>
      groupSessionsByWorkspace(filteredSessions).map((group) => ({
        ...group,
        sessions: [...group.sessions].sort((a, b) => {
          const pinDiff =
            Number(pinnedSessions.includes(sessionKey(b))) -
            Number(pinnedSessions.includes(sessionKey(a)));
          if (pinDiff !== 0) return pinDiff;
          return activityTime(b).localeCompare(activityTime(a));
        }),
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
    // A single dropped SSE frame shouldn't flash "轮询连接" and shove the
    // banner in; only degrade after the channel stays down for a while.
    let degradeTimer: number | undefined;
    const markDisconnected = () => {
      if (degradeTimer !== undefined) return;
      degradeTimer = window.setTimeout(() => {
        degradeTimer = undefined;
        if (!disposed) {
          setSnapshot((current) => ({
            ...current,
            realtime_connected: false,
          }));
        }
      }, 20_000);
    };
    const markConnected = () => {
      if (degradeTimer !== undefined) {
        window.clearTimeout(degradeTimer);
        degradeTimer = undefined;
      }
      setSnapshot((current) => ({ ...current, realtime_connected: true }));
    };
    void listen<DashboardRealtimeEvent>("dashboard-realtime", (event) => {
      if (event.payload.connected) {
        markConnected();
      } else {
        markDisconnected();
      }
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
        if (!disposed) markDisconnected();
      });
    return () => {
      disposed = true;
      if (degradeTimer !== undefined) window.clearTimeout(degradeTimer);
      unlisten?.();
    };
  }, [refresh]);

  // Warm the conversation cache for pinned + recent sessions so clicking a
  // session renders instantly, then re-verifies against the agent transcript.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      const priority = [
        ...sessions.filter((session) =>
          pinnedSessions.includes(sessionKey(session)),
        ),
        ...sessions,
      ]
        // Transcript previews only exist for remotely controllable providers;
        // discovery-only sessions never leave bounded metadata.
        .filter((session) => isManagedProvider(session.agent_type))
        .slice(0, 10)
        .map((session) => ({
          key: sessionKey(session),
          agentType: session.agent_type,
          agentSessionId: session.agent_session_id,
        }));
      prefetchConversations(priority);
    }, 1_500);
    return () => window.clearTimeout(timer);
  }, [pinnedSessions, sessions]);

  // Demo mode ships a rich sample transcript so the conversation view,
  // collapsed tools, and markdown rendering can be reviewed without a CLI.
  useEffect(() => {
    const demoMode =
      import.meta.env.DEV &&
      new URLSearchParams(window.location.search).has("demo");
    if (!demoMode || sessions.length === 0) return;
    const sample = [
      {
        role: "user",
        kind: "message",
        content: "帮我检查支付服务的重试逻辑，然后把超时改成 5 秒",
      },
      {
        role: "assistant",
        kind: "thinking",
        content:
          "先定位 retry 配置，一般在 PaymentClient。超时 3s → 5s，注意单位是毫秒。",
      },
      {
        role: "assistant",
        kind: "tool",
        label: "Grep: retry_timeout",
        status: "completed",
        content:
          '{\n  "pattern": "retry_timeout",\n  "path": "services/payment"\n}\n\n〔结果〕\nservices/payment/client.ts:12:  retry_timeout_ms: 3_000,',
      },
      {
        role: "assistant",
        kind: "tool",
        label: "Edit: services/payment/client.ts",
        status: "completed",
        content:
          '{\n  "old": "retry_timeout_ms: 3_000",\n  "new": "retry_timeout_ms: 5_000"\n}\n\n〔结果〕\n已应用 1 处修改。',
      },
      {
        role: "assistant",
        kind: "tool",
        label: "Bash: cargo test -p payment",
        status: "failed",
        content:
          '{\n  "command": "cargo test -p payment"\n}\n\n〔结果〕\nerror[E0433]: failed to resolve: use of undeclared crate `payment`',
      },
      {
        role: "assistant",
        kind: "message",
        content:
          "已完成两处修改，但测试命令的包名不对：\n\n1. `retry_timeout_ms` 已从 3s 调整为 **5s**\n2. 重试逻辑本身无需改动\n\n| 项目 | 修改前 | 修改后 |\n| --- | --- | --- |\n| 超时 | 3000ms | 5000ms |\n\n```bash\ncargo test -p payment-service\n```\n\n> 注意：包名是 `payment-service`，上一条命令已失败，请用上面的命令重跑。",
      },
    ] as ConversationMessage[];
    for (const session of sessions) {
      seedConversation(sessionKey(session), sample);
    }
  }, [sessions]);

  useEffect(() => {
    if (agentFilter !== "all" && isManagedProvider(agentFilter)) {
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

  return (
    <div className="desktop-app-container">
      <DesktopHeader
        onToggleSidebar={() => setSidebarCollapsed((v) => !v)}
        sidebarCollapsed={sidebarCollapsed}
        onNavigateBack={() =>
          previousSession && setSelectedSessionKey(sessionKey(previousSession))
        }
        canNavigateBack={previousSession !== undefined}
        onNavigateForward={() =>
          nextSession && setSelectedSessionKey(sessionKey(nextSession))
        }
        canNavigateForward={nextSession !== undefined}
        title={
          selectedSession
            ? selectedSession.payload.project.display_name || "AgentHub"
            : undefined
        }
        subtitle={
          selectedSession ? sessionDisplayTitle(selectedSession) : undefined
        }
      />
      <main className="claude-app-shell">
        {/* Left Claude Desktop Sidebar */}
        <aside
          className={`claude-sidebar ${sidebarCollapsed ? "collapsed" : ""}`}
          aria-label="侧边栏导航"
        >
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
                className="top-icon-btn new-task-btn"
                onClick={() => {
                  setSelectedSessionKey(undefined);
                  setLaunchModalProvider(
                    isManagedProvider(agentFilter) ? agentFilter : "codex",
                  );
                  setLaunchModalOpen(true);
                }}
                title="新建任务"
                aria-label="新建任务"
              >
                <Plus size={16} />
              </button>
              <button
                type="button"
                className={"top-icon-btn " + (searchOpen ? "active" : "")}
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

          {/* Managed Agent Indicator & Selector */}
          <div className="sidebar-agent-picker-v2">
            <div className="picker-header-row">
              <span className="picker-label">受控 Agent</span>
              <button
                type="button"
                className={`agent-filter-all-btn ${agentFilter === "all" ? "active" : ""}`}
                onClick={() => setAgentFilter("all")}
                title="显示所有受控 Agent 会话"
              >
                全部
              </button>
            </div>
            <div className="managed-agents-list">
              {PROVIDER_LIST.map((item) => {
                const isSelected = agentFilter === item.type;
                return (
                  <button
                    key={item.type}
                    type="button"
                    className={`single-agent-badge ${isSelected ? "selected" : ""}`}
                    data-provider={item.type}
                    onClick={() =>
                      setAgentFilter(
                        agentFilter === item.type ? "all" : item.type,
                      )
                    }
                    title={`筛选 ${item.label} 会话`}
                  >
                    <span className="agent-badge-dot" />
                    <strong>{item.label}</strong>
                    <span className="agent-badge-tag">Managed</span>
                  </button>
                );
              })}
            </div>
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

          {/* The selected Agent is grouped by local workspace. */}
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
                const remaining =
                  group.sessions.length - visibleSessions.length;

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
                          disabled={
                            agentFilter === "all" ||
                            !isManagedProvider(agentFilter)
                          }
                          title={
                            agentFilter !== "all" &&
                            isManagedProvider(agentFilter)
                              ? "在 " + group.name + " 新建会话"
                              : "该 Provider 当前仅支持发现和历史读取"
                          }
                          onClick={() =>
                            agentFilter !== "all" &&
                            isManagedProvider(agentFilter) &&
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
                              className={
                                "sidebar-session-item " +
                                (isSelected ? "selected" : "")
                              }
                              data-provider={session.agent_type}
                              title={title}
                              onContextMenu={(e) => {
                                e.preventDefault();
                                setContextMenu({
                                  x: e.clientX,
                                  y: e.clientY,
                                  session,
                                });
                              }}
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
                                className={
                                  "sidebar-pin " +
                                  (pinnedSessions.includes(sessionKey(session))
                                    ? "active"
                                    : "")
                                }
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
                title="账户设置与注销"
              >
                <span className="profile-brand-icon" aria-hidden="true">
                  {initials(displayName)}
                </span>
                <span className="profile-name-text">
                  <strong>{displayName}</strong>
                </span>
              </button>

              <div className="sidebar-footer-actions">
                <button
                  type="button"
                  className="sidebar-footer-action-btn"
                  onClick={onOpenLocalHost ?? (() => {})}
                  title={`本机与出站连接: ${snapshot.realtime_connected ? "实时在线" : "轮询"} (${snapshot.local.sources.length} 个工作区)`}
                  aria-label="本机与出站连接状态"
                >
                  <Smartphone size={16} />
                  <span
                    className={
                      "footer-status-indicator " +
                      (snapshot.realtime_connected ? "online" : "fallback")
                    }
                  />
                </button>

                <button
                  type="button"
                  className="sidebar-footer-action-btn"
                  onClick={() => setSettingsOpen(true)}
                  title="系统设置 (Ctrl+,)"
                  aria-label="系统设置"
                >
                  <Settings size={16} />
                </button>
              </div>
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
            />
          ) : (
            <SessionEmptyState
              onNewSession={() => {
                setSelectedSessionKey(undefined);
                setLaunchModalProvider(
                  isManagedProvider(agentFilter) ? agentFilter : "codex",
                );
                setLaunchModalOpen(true);
              }}
              onOpenHostControl={onOpenLocalHost ?? (() => {})}
              hostPhase={hostPhase}
              workspaceCount={snapshot.local.sources.length}
              sessionCount={snapshot.sessions.length}
              displayName={displayName}
            />
          )}

          {/* Global Account, Settings, Launch, ContextMenu Modals */}
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
            <DesktopSettingsModal
              preferences={snapshot.preferences}
              snapshot={snapshot}
              onClose={() => setSettingsOpen(false)}
              onSaved={(preferences) =>
                setSnapshot((current) => ({ ...current, preferences }))
              }
              onLogout={() => void signOut()}
              onOpenLocalHost={onOpenLocalHost}
            />
          )}
          <LaunchSessionModal
            open={launchModalOpen}
            initialAgentType={launchModalProvider}
            onClose={() => setLaunchModalOpen(false)}
            onLaunched={async (agentSessionId, agentType) => {
              await refresh();
              setSelectedSessionKey(
                snapshot.device.device_id +
                  ":" +
                  agentType +
                  ":" +
                  agentSessionId,
              );
            }}
          />

          {/* Right-click Context Menu on Sessions (Point 11) */}
          {contextMenu && (
            <SessionContextMenu
              position={contextMenu}
              onClose={() => setContextMenu(null)}
              isPinned={pinnedSessions.includes(
                sessionKey(contextMenu.session),
              )}
              onTogglePin={togglePinnedSession}
              onNewSessionForAgent={(agentType) => {
                openLaunchModalFor(agentType);
              }}
              onShowToast={showToast}
            />
          )}

          {/* Toast Notification Pill */}
          {toastMessage && (
            <div className="toast-feedback">
              <CheckCircle2 size={14} style={{ color: "#38bdf8" }} />
              <span>{toastMessage}</span>
            </div>
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
    </div>
  );
}

const PERMISSION_OPTIONS: Record<
  Session["agent_type"],
  Array<{ value: string; label: string }>
> = {
  claude_code: [
    { value: "", label: "默认权限" },
    { value: "acceptEdits", label: "接受编辑" },
    { value: "plan", label: "计划模式" },
    { value: "bypassPermissions", label: "完全访问" },
  ],
  codex: [
    { value: "", label: "默认沙箱" },
    { value: "read-only", label: "只读" },
    { value: "workspace-write", label: "工作区写入" },
    { value: "danger-full-access", label: "完全访问" },
  ],
  antigravity: [{ value: "", label: "默认权限" }],
  grok_build: [{ value: "", label: "默认权限" }],
  zcode: [
    { value: "", label: "默认执行策略" },
    { value: "auto-approve", label: "全自动执行" },
    { value: "on-request", label: "写操作确认" },
    { value: "read-only", label: "只读审查" },
  ],
};

function loadRecentCwds(): string[] {
  try {
    const stored = window.localStorage.getItem("agenthub:recent-cwds");
    const parsed = stored ? (JSON.parse(stored) as string[]) : [];
    return Array.isArray(parsed)
      ? parsed.filter((item) => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

function rememberRecentCwd(cwd: string) {
  const next = [cwd, ...loadRecentCwds().filter((item) => item !== cwd)].slice(
    0,
    5,
  );
  try {
    window.localStorage.setItem("agenthub:recent-cwds", JSON.stringify(next));
  } catch {
    // ignore
  }
}

async function pickFolderDialog(start?: string): Promise<string | undefined> {
  if (!("__TAURI_INTERNALS__" in window)) return undefined;
  const picked = await invoke<string | null>("pick_folder", {
    startDir: start || null,
  });
  return picked ?? undefined;
}

function LaunchSessionModal({
  open,
  initialAgentType = "codex",
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
  const models = useAgentModels(agentType);
  const [model, setModel] = useState<string>("");
  const [permission, setPermission] = useState("");
  const [cwd, setCwd] = useState(() => loadRecentCwds()[0] || "");
  const [prompt, setPrompt] = useState("");
  const [launching, setLaunching] = useState(false);
  const [error, setError] = useState<string>();
  const recentCwds = useMemo(() => loadRecentCwds(), [open]);

  useEffect(() => {
    if (open) {
      setAgentType(initialAgentType);
      setPermission("");
      setPrompt("");
      setError(undefined);
      setCwd((current) => current || loadRecentCwds()[0] || "");
    }
  }, [initialAgentType, open]);

  useEffect(() => {
    if (!models.some((option) => option.value === model)) {
      setModel(models[0]?.value ?? "");
    }
  }, [model, models]);

  const chooseCwd = async () => {
    try {
      const picked = await pickFolderDialog(cwd);
      if (picked) setCwd(picked);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (launching || prompt.trim() === "") return;
    if (!isManagedProvider(agentType)) {
      setError("该 Provider 当前仅支持发现和历史读取");
      return;
    }
    if (!cwd.trim()) {
      setError("请选择工作目录，Agent 将在该目录中启动");
      return;
    }
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
          cwd: cwd.trim(),
          prompt: prompt.trim(),
          model: model || undefined,
          permission_mode: permission || undefined,
        },
        onEvent,
      });
      rememberRecentCwd(cwd.trim());
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
                  disabled={!isManagedProvider(item.type)}
                  title={
                    isManagedProvider(item.type)
                      ? `使用 ${item.label} 创建 managed Session`
                      : `${item.label} 当前仅支持发现和历史读取`
                  }
                  onClick={() => {
                    setAgentType(item.type);
                    setPermission("");
                  }}
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
            <span>运行模型（来自本机 Agent）</span>
            <SelectDropdown
              value={model}
              options={models.map((option) => ({
                value: option.value,
                label: option.label,
                desc: option.desc,
              }))}
              onChange={(val) => setModel(val)}
              size="md"
            />
          </label>

          <label className="launch-form-field">
            <span>权限</span>
            <SelectDropdown
              value={permission}
              options={PERMISSION_OPTIONS[agentType]}
              onChange={(val) => setPermission(val)}
              size="md"
            />
          </label>

          <div className="launch-form-field">
            <span>工作目录</span>
            <div className="cwd-picker-row">
              <button
                type="button"
                className="cwd-picker-btn"
                onClick={() => void chooseCwd()}
                title="选择工作目录"
              >
                <Folder size={14} />
                <span className="cwd-picker-text">
                  {cwd || "点击选择文件夹…"}
                </span>
              </button>
              {recentCwds.length > 1 && (
                <SelectDropdown
                  value=""
                  placeholder="最近"
                  options={recentCwds.map((item) => ({
                    value: item,
                    label: item.length > 34 ? "…" + item.slice(-32) : item,
                  }))}
                  onChange={(val) => setCwd(val)}
                  size="sm"
                  className="cwd-recent-select"
                  ariaLabel="最近使用的工作目录"
                />
              )}
            </div>
          </div>
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
              disabled={launching || !prompt.trim() || !cwd.trim()}
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

interface TurnOptionsState {
  model?: string;
  permission?: string;
}

function sameConversation(
  a: ConversationMessage[],
  b: ConversationMessage[],
): boolean {
  return (
    a.length === b.length &&
    a.every(
      (message, index) =>
        message.role === b[index].role &&
        message.kind === b[index].kind &&
        message.status === b[index].status &&
        message.content === b[index].content,
    )
  );
}

function SessionControlPanel({
  session,
  previousSession,
  nextSession,
  onNavigate,
  onClose,
}: {
  session: Session;
  previousSession?: Session;
  nextSession?: Session;
  onNavigate: (session: Session) => void;
  onClose: () => void;
}) {
  const [prompt, setPrompt] = useState("");
  const cacheKey = sessionKey(session);
  // Render from the preloaded cache immediately, then verify against the
  // agent transcript and refresh only when the content actually differs.
  const [messages, setMessages] = useState<ConversationMessage[]>(
    () => cachedConversation(cacheKey) ?? [],
  );
  const [busy, setBusy] = useState(() =>
    hasActiveTurn(session.payload.state.activity_status),
  );
  const [cancelling, setCancelling] = useState(false);
  const [pendingApproval, setPendingApproval] = useState(
    session.payload.pending_approval ?? undefined,
  );
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [agentActivity, setAgentActivity] = useState<string>();
  const [error, setError] = useState<string>();
  const [plusMenuOpen, setPlusMenuOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  const insertToken = (token: string) => {
    const el = textareaRef.current;
    if (!el) {
      setPrompt((prev) => prev + token);
      return;
    }
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const val = el.value;
    const next = val.substring(0, start) + token + val.substring(end);
    setPrompt(next);
    setTimeout(() => {
      el.focus();
      const nextPos = start + token.length;
      el.setSelectionRange(nextPos, nextPos);
    }, 10);
  };

  const [attachments, setAttachments] = useState<
    Array<{ name: string; path: string }>
  >([]);
  const [turnOptions, setTurnOptions] = useState<
    Record<string, TurnOptionsState>
  >(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:turn-options");
      return stored
        ? (JSON.parse(stored) as Record<string, TurnOptionsState>)
        : {};
    } catch {
      return {};
    }
  });
  const models = useAgentModels(session.agent_type);
  const permissionOptions = PERMISSION_OPTIONS[session.agent_type];
  const pendingDelta = useRef("");
  const deltaFrame = useRef<number | undefined>(undefined);
  const paneRef = useRef<HTMLElement | null>(null);
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
  const sessionOptions = turnOptions[cacheKey] ?? {};
  const currentModel =
    sessionOptions.model &&
    models.some((option) => option.value === sessionOptions.model)
      ? sessionOptions.model
      : (models[0]?.value ?? "");
  const currentPermission =
    sessionOptions.permission &&
    permissionOptions.some(
      (option) => option.value === sessionOptions.permission,
    )
      ? sessionOptions.permission
      : "";

  useEffect(() => {
    setPendingApproval(session.payload.pending_approval ?? undefined);
  }, [session.payload.pending_approval]);

  useEffect(() => {
    setBusy(hasActiveTurn(session.payload.state.activity_status));
    if (!hasActiveTurn(session.payload.state.activity_status)) {
      setCancelling(false);
    }
  }, [session.payload.state.activity_status]);

  const updateTurnOptions = (patch: TurnOptionsState) => {
    setTurnOptions((current) => {
      const next = {
        ...current,
        [cacheKey]: { ...current[cacheKey], ...patch },
      };
      try {
        window.localStorage.setItem(
          "agenthub:turn-options",
          JSON.stringify(next),
        );
      } catch {
        // ignore
      }
      return next;
    });
  };

  useEffect(() => {
    // Demo mode (no Tauri bridge) keeps the seeded sample transcript.
    if (!("__TAURI_INTERNALS__" in window)) return;
    // Discovery-only sessions expose bounded metadata, never full transcripts:
    // reading history is reserved for sessions that can be continued remotely.
    if (!isManagedProvider(session.agent_type)) {
      setMessages((current) => (current.length > 0 ? [] : current));
      return;
    }
    let disposed = false;
    loadConversation(session.agent_type, session.agent_session_id)
      .then((next) => {
        if (disposed) return;
        setMessages((current) =>
          current.length > 0 && sameConversation(current, next)
            ? current
            : next,
        );
        const firstUser = next.find(
          (message) =>
            message.role === "user" &&
            message.kind === undefined &&
            message.content.trim().length >= 4,
        );
        if (firstUser && !session.payload.runtime?.title) {
          rememberTitleOverride(cacheKey, firstUser.content);
        }
      })
      .catch((err) => {
        if (!disposed) setError(errorMessage(err));
      });
    return () => {
      disposed = true;
    };
  }, [
    cacheKey,
    session.agent_session_id,
    session.agent_type,
    session.payload.runtime?.title,
  ]);

  useEffect(() => {
    const pane = paneRef.current;
    if (pane) {
      pane.scrollTop = pane.scrollHeight;
    }
  }, [messages, busy, agentActivity]);

  const flushDelta = useCallback(() => {
    deltaFrame.current = undefined;
    if (pendingDelta.current === "") return;
    const delta = pendingDelta.current;
    pendingDelta.current = "";
    setMessages((current) => appendAssistantDelta(current, delta));
  }, []);

  const handleAssistantDelta = useCallback(
    (delta: string) => {
      pendingDelta.current += delta;
      if (deltaFrame.current === undefined) {
        deltaFrame.current = window.requestAnimationFrame(flushDelta);
      }
    },
    [flushDelta],
  );

  const attachFiles = async () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      setError("演示模式不支持附件");
      return;
    }
    try {
      const picked = await invoke<string[]>("pick_files", {
        startDir: null,
      });
      setAttachments((current) => {
        const merged = [...current];
        for (const path of picked) {
          if (merged.some((item) => item.path === path)) continue;
          if (merged.length >= 8) break;
          const name = path.split(/[\\/]/).pop() || path;
          merged.push({ name, path });
        }
        return merged;
      });
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const sendMessage = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || prompt.trim() === "") return;
    const nextPrompt = prompt.trim();
    setPrompt("");
    setBusy(true);
    setError(undefined);
    setPendingApproval(undefined);
    setAgentActivity(undefined);
    const sentAttachments = attachments;
    setAttachments([]);
    setMessages((current) => [
      ...current,
      { role: "user", content: nextPrompt },
      { role: "assistant", content: "" },
    ]);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      onEvent.onmessage = (event) => {
        if (event.type === "assistant_delta") {
          handleAssistantDelta(event.text);
        } else if (event.type === "assistant_message") {
          if (deltaFrame.current !== undefined) {
            window.cancelAnimationFrame(deltaFrame.current);
            deltaFrame.current = undefined;
          }
          pendingDelta.current = "";
          setMessages((current) =>
            replaceAssistantMessage(current, event.text),
          );
        } else if (event.type === "activity") {
          setAgentActivity(event.label);
        } else if (event.type === "approval_requested") {
          setPendingApproval(event.approval);
          setAgentActivity("等待审批确认...");
        }
      };
      const result = await sendSessionMessage(
        {
          agent_type: session.agent_type,
          agent_session_id: session.agent_session_id,
          prompt: nextPrompt,
          model: currentModel || undefined,
          permission_mode: currentPermission || undefined,
          attachments: sentAttachments,
        },
        onEvent,
      );
      if (deltaFrame.current !== undefined) {
        window.cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = undefined;
      }
      pendingDelta.current = "";
      setMessages((current) =>
        replaceAssistantMessage(current, result.response),
      );
    } catch (nextError) {
      if (deltaFrame.current !== undefined) {
        window.cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = undefined;
      }
      pendingDelta.current = "";
      setMessages((current) => dropTrailingEmptyAssistant(current));
      setError(errorMessage(nextError));
      // Give the user a chance to retry with the same attachments.
      setAttachments((current) =>
        current.length === 0 && sentAttachments.length > 0
          ? sentAttachments
          : current,
      );
    } finally {
      setBusy(false);
      setCancelling(false);
      setAgentActivity(undefined);
      void loadConversation(session.agent_type, session.agent_session_id).catch(
        () => undefined,
      );
    }
  };

  const cancelTurn = async () => {
    if (!busy || cancelling) return;
    setCancelling(true);
    try {
      await cancelSessionTurn({
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
      });
    } catch (nextError) {
      setError(errorMessage(nextError));
      setCancelling(false);
    }
  };

  const resolveApproval = async (approved: boolean) => {
    if (!pendingApproval || approvalBusy) return;
    setApprovalBusy(true);
    setError(undefined);
    try {
      await resolveSessionApproval({
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
        approval_id: pendingApproval.approval_id,
        approved,
      });
      setPendingApproval(undefined);
      setAgentActivity(
        approved ? "审批已允许，继续执行..." : "审批已拒绝，等待收敛...",
      );
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setApprovalBusy(false);
    }
  };

  return (
    <div
      className="session-control-panel-scrim"
      role="region"
      aria-label={title}
    >
      <aside className="session-control-panel">
        <header>
          <div className="panel-nav">
            <button
              className="icon-button"
              aria-label="返回"
              onClick={onClose}
              title="返回工作区首页"
            >
              <ArrowLeft />
            </button>
            <span className="session-breadcrumb">
              <strong>{title}</strong>
              <small>
                {session.payload.project.display_name || "未分类项目"}
              </small>
            </span>
          </div>
          <div className="panel-heading-actions">
            <span className={"status-pill " + status.className}>
              <i />
              {status.label}
            </span>
            <button
              className="icon-button"
              aria-label="上一个会话"
              title="上一个会话"
              disabled={previousSession === undefined}
              onClick={() => previousSession && onNavigate(previousSession)}
            >
              <ChevronLeft />
            </button>
            <button
              className="icon-button"
              aria-label="下一个会话"
              title="下一个会话"
              disabled={nextSession === undefined}
              onClick={() => nextSession && onNavigate(nextSession)}
            >
              <ChevronRight />
            </button>
            <button
              className="icon-button"
              aria-label="关闭面板"
              onClick={onClose}
              title="关闭会话"
            >
              <X />
            </button>
          </div>
        </header>

        <section className="conversation-pane" ref={paneRef} aria-live="polite">
          {messages.length === 0 ? (
            <div className="conversation-empty">
              <MessageSquare />
              <strong>从这里继续会话</strong>
              <p>
                {isManagedProvider(session.agent_type)
                  ? "消息交给本机 agenthubd 管理的官方 Runtime。"
                  : "该 Provider 尚未接入远程控制；接入后可在此查看历史并继续会话。"}
              </p>
            </div>
          ) : (
            groupConversationMessages(messages).map((group, groupIdx) => {
              if (group.type === "tool_group") {
                return (
                  <ToolCallGroupBlock
                    key={`tool-group-${group.startIndex}-${groupIdx}`}
                    messages={group.messages}
                  />
                );
              }
              const message = group.message;
              const index = group.index;
              return (
                <ConversationItem
                  message={message}
                  index={index}
                  key={
                    message.role + "-" + (message.kind ?? "message") + "-" + index
                  }
                />
              );
            })
          )}
          {busy && agentActivity !== undefined && (
            <div className="agent-thinking">
              <RefreshCw className="spin" /> {agentActivity}
            </div>
          )}
        </section>

        {pendingApproval !== undefined && (
          <div
            className="approval-actions"
            role="group"
            aria-label="Codex 审批操作"
          >
            <span>Codex 正在等待权限确认</span>
            <button
              type="button"
              disabled={approvalBusy || !pendingApproval.can_decline}
              onClick={() => void resolveApproval(false)}
            >
              拒绝
            </button>
            <button
              type="button"
              className="primary"
              disabled={approvalBusy || !pendingApproval.can_approve}
              onClick={() => void resolveApproval(true)}
            >
              允许
            </button>
          </div>
        )}
        {error !== undefined && <p className="panel-error">{error}</p>}

        <div className="composer-container">
          {!isManagedProvider(session.agent_type) ? (
            <div className="provider-readonly-notice" role="status">
              <Eye size={14} />
              <span>该 Provider 尚未接入远程控制，暂不能发送消息</span>
            </div>
          ) : (
            <>
              {attachments.length > 0 && (
                <div className="attachment-row">
                  {attachments.map((file) => (
                    <span
                      className="attachment-chip"
                      key={file.path}
                      title={file.path}
                    >
                      <Paperclip size={11} />
                      <span className="attachment-name">{file.name}</span>
                      <button
                        type="button"
                        aria-label={"移除附件 " + file.name}
                        onClick={() =>
                          setAttachments((current) =>
                            current.filter((item) => item.path !== file.path),
                          )
                        }
                      >
                        <X size={11} />
                      </button>
                    </span>
                  ))}
                </div>
              )}
              <form
                className="session-composer modern-composer"
                onSubmit={sendMessage}
              >
                <textarea
                  ref={textareaRef}
                  aria-label="发送给 Agent"
                  maxLength={16_000}
                  placeholder="继续这个会话… (Enter 发送，Shift+Enter 换行)"
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void sendMessage(e);
                    }
                  }}
                  disabled={busy}
                />
                <div className="composer-toolbar">
                  <div className="composer-plus-wrapper">
                    <button
                      type="button"
                      className={`tool-ghost-btn ${plusMenuOpen ? "active" : ""}`}
                      onClick={() => setPlusMenuOpen((prev) => !prev)}
                      title="快捷能力与附件"
                      aria-label="快捷能力与附件"
                      disabled={busy}
                    >
                      <Plus size={15} />
                    </button>
                    <ComposerPlusMenu
                      isOpen={plusMenuOpen}
                      onClose={() => setPlusMenuOpen(false)}
                      onAttachFiles={() => void attachFiles()}
                      onInsertToken={insertToken}
                    />
                  </div>
                  <SelectDropdown
                    className="ghost"
                    value={currentModel}
                    options={models.map((option) => ({
                      value: option.value,
                      label: option.label,
                      desc: option.desc,
                    }))}
                    onChange={(val) => updateTurnOptions({ model: val })}
                    size="sm"
                    ariaLabel="会话模型"
                  />
                  {permissionOptions.length > 1 && (
                    <SelectDropdown
                      className="ghost"
                      value={currentPermission}
                      options={permissionOptions}
                      onChange={(val) => updateTurnOptions({ permission: val })}
                      size="sm"
                      ariaLabel="权限模式"
                    />
                  )}
                  <span className="composer-spacer" />
                  <ContextCapacityRing
                    runtime={runtime}
                    messages={messages}
                    modelName={currentModel}
                  />
                  {busy ? (
                    <button
                      className="composer-send turn-cancel-btn"
                      type="button"
                      disabled={cancelling}
                      onClick={() => void cancelTurn()}
                      title="停止当前生成"
                      aria-label="停止"
                    >
                      <Square size={14} />
                    </button>
                  ) : (
                    <button
                      className="composer-send"
                      type="submit"
                      aria-label="发送"
                      disabled={busy || prompt.trim() === ""}
                    >
                      <ArrowUp size={15} />
                    </button>
                  )}
                </div>
              </form>
            </>
          )}
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
            {snapshot.account.offline
              ? "本地工作区 · 未连接 Gateway"
              : `${planLabel(snapshot.account.subscription.plan)} · ${snapshot.realtime_connected ? "实时连接" : "轮询连接"}`}
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
          <span>
            {snapshot.account.offline ? "切换连接 / 登录" : "退出登录"}
          </span>
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

function errorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim() !== "") return error;
  if (error instanceof Error && error.message !== "") return error.message;
  return "操作失败，请稍后重试";
}

function sessionKey(session: Session): string {
  return (
    session.device_id +
    ":" +
    normalizeAgentType(session.agent_type) +
    ":" +
    session.agent_session_id
  );
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
              <span className={"timeline-marker " + status.className}>
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
  if (tokens < 1_000) return tokens + " tokens";
  if (tokens < 1_000_000) return (tokens / 1_000).toFixed(1) + "K tokens";
  return (tokens / 1_000_000).toFixed(2) + "M tokens";
}

function formatContextUsage(runtime: Session["payload"]["runtime"]): string {
  const used = runtime?.context_used_tokens;
  const window = runtime?.context_window_tokens;
  if (used !== undefined && window !== undefined && window > 0) {
    const percentage = Math.min(100, Math.round((used / window) * 100));
    return (
      formatTokenCount(used) +
      " / " +
      formatTokenCount(window) +
      " · " +
      percentage +
      "%"
    );
  }
  if (window !== undefined) return formatTokenCount(window) + " 上限";
  if (used !== undefined) return formatTokenCount(used) + " 已用 · 上限未提供";
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
    demoSession("zcode", "AgentHub Remote 核心接入", "WORKING", 1),
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
    event_id: agent + "-" + project,
    device_id: "dev_demo",
    agent_type: agent,
    agent_session_id: agent + "-" + project,
    observed_at: observedAt,
    sync_state: minutesAgo < 3 ? "local" : "synced",
    payload: {
      project: { display_name: project, fingerprint: "demo" },
      state: { activity_status: activity, presence },
      last_activity_at: observedAt,
      runtime: {
        agent_type: agent,
        agent_session_id: agent + "-" + project,
        title:
          agent === "claude_code"
            ? project + "实施计划"
            : agent === "zcode"
              ? project + "全栈开发"
              : agent === "grok_build"
                ? project + "采集适配"
                : project + "接口与测试",
        model:
          agent === "claude_code"
            ? "claude-3-7-sonnet"
            : agent === "zcode"
              ? "gemini-3.8-flash-high"
              : agent === "grok_build"
                ? "grok-4.6"
                : "gpt-5.5-codex",
        reasoning_effort: agent === "codex" ? "medium" : "high",
        permission_mode:
          agent === "claude_code"
            ? "acceptEdits"
            : agent === "zcode"
              ? "auto-execute"
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
            : agent === "zcode"
              ? 1_000_000
              : agent === "grok_build"
                ? 256_000
                : 258_400,
        context_used_tokens:
          agent === "claude_code"
            ? 64_800
            : agent === "zcode"
              ? 85_600
              : agent === "grok_build"
                ? 48_200
                : 92_300,
        session_total_tokens:
          agent === "claude_code"
            ? 91_200
            : agent === "zcode"
              ? 128_000
              : agent === "grok_build"
                ? 72_400
                : 184_600,
      },
    },
  };
}
