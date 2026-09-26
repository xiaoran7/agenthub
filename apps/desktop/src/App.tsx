import { Channel, invoke } from "@tauri-apps/api/core";
import { NativeHostDialog } from "./components/NativeHostDialog";
import {
  LaunchSessionModal,
  PERMISSION_OPTIONS,
} from "./components/session/LaunchSessionModal";
import { SessionControlPanel } from "./components/session/SessionControlPanel";
import { Brand, PulseMark } from "./components/layout/Brand";
import { demoSnapshot } from "./demoData";
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
  initials,
  isAttentionSession,
  permissionFieldLabel,
  relativeTime,
  rememberTitleOverride,
  sessionDisplayTitle,
  sessionKey,
  statusView,
} from "./presentation";
import { errorMessage } from "./components/auth/AuthScreen";
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
import { RenameSessionDialog } from "./components/RenameSessionDialog";
import {
  ConversationItem,
  groupConversationMessages,
  ToolCallGroupBlock,
} from "./components/ConversationRenderer";
import { DesktopSettingsModal } from "./components/DesktopSettingsModal";
import { applyAppearance, readAppearance } from "./appearance";
import { isPluginEnabled, readPluginState } from "./plugins";
import { DesktopHeader } from "./components/layout/DesktopHeader";
import { SessionEmptyState } from "./components/session/SessionEmptyState";
import { ConversationLoadState } from "./components/session/ConversationLoadState";
import { ContextCapacityRing } from "./components/session/ContextCapacityPopover";
import { ComposerPlusMenu } from "./components/session/ComposerPlusMenu";
import {
  buildDesktopSessionTree,
  isAutomatedTestWorkspace,
} from "./lib/session-tree";
import { DesktopSessionFolderTree } from "./components/sidebar/DesktopSessionFolderTree";

import { AuthScreen } from "./components/auth/AuthScreen";
import { AccountMenu, planLabel } from "./components/layout/AccountMenu";
import {
  SessionTimelineView,
  formatTimelineTimestamp,
} from "./components/session/SessionTimelineView";

type AgentFilter = "all" | Session["agent_type"];
type DeviceFilter = "all" | string;

const isManagedProvider = (
  agentType: AgentFilter,
): agentType is Session["agent_type"] =>
  agentType === "codex" ||
  agentType === "claude_code" ||
  agentType === "zcode" ||
  agentType === "antigravity" ||
  agentType === "grok_build";

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
    applyAppearance(readAppearance());
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
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("codex");
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
  const [renameSession, setRenameSession] = useState<Session | null>(null);
  const [titleOverridesVersion, setTitleOverridesVersion] = useState(0);
  const [toastMessage, setToastMessage] = useState<string>();

  useEffect(() => {
    const handleTitleOverridesChanged = () => {
      setTitleOverridesVersion((v) => v + 1);
    };
    window.addEventListener(
      "agenthub:title-overrides-changed",
      handleTitleOverridesChanged,
    );
    return () => {
      window.removeEventListener(
        "agenthub:title-overrides-changed",
        handleTitleOverridesChanged,
      );
    };
  }, []);

  const [pluginState, setPluginState] = useState(() => readPluginState());
  const reloadPlugins = useCallback(() => {
    setPluginState(readPluginState());
  }, []);

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
    // First fill from cached sessions, filtering out automated test sessions
    for (const s of cachedSessions) {
      if (s.agent_session_id.startsWith("sess_subagent")) continue;
      if (isAutomatedTestWorkspace(s.payload.project.display_name)) continue;
      const norm = { ...s, agent_type: normalizeAgentType(s.agent_type) };
      map.set(sessionKey(norm), norm);
    }
    // Then overwrite with live snapshot sessions (takes precedence)
    for (const s of snapshot.sessions) {
      if (s.agent_session_id.startsWith("sess_subagent")) continue;
      if (isAutomatedTestWorkspace(s.payload.project.display_name)) continue;
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

  const sessionTree = useMemo(() => {
    return buildDesktopSessionTree({
      sessions: sessions.filter((s) => s.archived !== true),
      localDeviceId: snapshot.device.device_id,
      localDeviceName: snapshot.device.name,
      searchQuery: query,
      registeredDevices: snapshot.devices,
    });
  }, [
    sessions,
    snapshot.device.device_id,
    snapshot.device.name,
    query,
    snapshot.devices,
    pluginState.enabledPluginIds,
    titleOverridesVersion,
  ]);

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
          key: `${normalizeAgentType(session.agent_type)}:${session.agent_session_id}`,
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
      seedConversation(
        `${normalizeAgentType(session.agent_type)}:${session.agent_session_id}`,
        sample,
      );
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

  // Real smartphone / companion device connection status
  const isMobileConnected = useMemo(() => {
    return (
      snapshot.devices?.some(
        (d) =>
          d.online && !d.revoked && d.device_id !== snapshot.device.device_id,
      ) ?? false
    );
  }, [snapshot.devices, snapshot.device.device_id]);

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
      <main
        className={`claude-app-shell ${sidebarCollapsed ? "sidebar-collapsed" : ""}`}
      >
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

          {/* Search Box if opened */}
          {searchOpen && (
            <div className="sidebar-search-box">
              <Search size={14} />
              <input
                autoFocus
                placeholder="搜索电脑、Agent、工作区或会话..."
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

          {/* 4-Level Session Folder Tree (电脑 -> Agent -> 工作区 -> Session) */}
          <div className="sidebar-providers-tree">
            <DesktopSessionFolderTree
              tree={sessionTree}
              selectedSessionKey={selectedSessionKey}
              onSelectSession={(key) => setSelectedSessionKey(key)}
              onLaunchForProvider={(provider) => openLaunchModalFor(provider)}
              onContextMenu={(e, session) =>
                setContextMenu({
                  x: e.clientX,
                  y: e.clientY,
                  session,
                })
              }
              query={query}
            />
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
                  title={
                    snapshot.realtime_connected
                      ? `远程已连接 (实时在线)${isMobileConnected ? " · 移动端已协同" : " · 点击管理工作目录与远程连接"}`
                      : snapshot.sync_running
                        ? "远程服务连接降级 (轮询中) · 点击检查连接"
                        : "远程连接未连接 · 点击管理工作目录与远程连接"
                  }
                  aria-label="远程连接与移动端协同状态"
                >
                  <Smartphone size={16} />
                  <span
                    className={
                      "footer-status-indicator " +
                      (snapshot.realtime_connected
                        ? "online"
                        : snapshot.sync_running
                          ? "fallback"
                          : "offline")
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
              activeProviderLabel={
                agentFilter === "all" ? "Agent" : agentLabel(agentFilter)
              }
              activeProviderType={agentFilter === "all" ? "codex" : agentFilter}
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
              onPluginsChanged={reloadPlugins}
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
              onRenameSession={(session) => {
                setRenameSession(session);
              }}
              onShowToast={showToast}
            />
          )}

          <RenameSessionDialog
            session={renameSession}
            onClose={() => setRenameSession(null)}
            onRename={(s, newTitle) => {
              rememberTitleOverride(sessionKey(s), newTitle);
              setTitleOverridesVersion((v) => v + 1);
              showToast("已更新会话名称");
            }}
          />

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
