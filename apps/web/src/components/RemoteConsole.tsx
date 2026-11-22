import {
  Suspense,
  lazy,
  memo,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import type {
  AgentPlugin,
  RemotePluginsResponse,
} from "@agent-pulse/contracts";
import type { AccountView } from "../lib/account";
import { safeRandomUUID } from "../lib/uuid";
import {
  cleanBannerText,
  conversationItems,
  groupConversationItems,
  isTransientRemoteError,
  isTransientRemoteMessage,
  latestTurnFailure,
  nextOptimisticPromptNeeded,
  pendingApprovals,
  pendingOptimisticPrompts,
  pendingQuestions,
  pruneOptimisticPrompts,
  readableRemoteText,
  remoteError,
  settledConversationItems,
  remoteRequest,
  type ConversationItem,
  type DiscoveryCandidate,
  type OptimisticPrompt,
  type RemoteAttachment,
  type RemoteCommandRecord,
  type RemoteEvent,
  type RemotePermissionMode,
  type RemoteRequest,
  type RemoteRunner,
  type RemoteRunnerHello,
  type RemoteRunnerHelloResponse,
  type RemoteSession,
  type RemoteSessionDetail,
  type RemoteSnapshot,
} from "../lib/remote";
import { Select } from "./Select";
import { RemoteContextCapacity } from "./RemoteContextCapacity";
import { ConversationSkeleton } from "./ConversationSkeleton";
import { QueuedPromptBar, type QueuedPrompt } from "./QueuedPromptBar";
import { RemoteLaunchModal, type LaunchAgentType } from "./RemoteLaunchModal";
import {
  MobileAttentionBanner,
  MobileHeaderActions,
} from "./MobileHeaderAccessories";
import { useMobileConsoleLayout } from "./useMobileConsoleLayout";
import { CheckIcon, CopyIcon, RemoteMessageCard } from "./RemoteMessageCard";
import { parseDiffStats, RemoteToolCard } from "./RemoteToolCard";
import {
  buildCommandAttemptKey,
  checkAttachmentSize,
  mergeMonotonicEvents,
  setBoundedSessionRecord,
} from "../lib/remote-session-state";
import {
  RunnerHelloSyncController,
  isRunnerHelloBlocking,
  mergeRunnerHelloEntries,
  type RunnerHelloEntry,
} from "../lib/runner-hello-sync";
import { parseSessionRef } from "../lib/types";
const SettingsPage = lazy(() =>
  import("./SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
import { AttentionQueue } from "./AttentionQueue";
import { parseNotificationPresentedAt } from "../lib/notifications";
import {
  followContentStream,
  streamBatchAffectsSession,
  streamBatchAffectsSnapshot,
} from "../lib/stream";
import type { PanelConfig } from "../lib/config";
import { loadConfig, saveConfig } from "../lib/config";
import {
  attentionEntries,
  buildSessionTree,
  formatFallbackTitle,
  getDeviceLabel,
  getProviderInfo,
  getWorkspaceActivityMs,
  groupPendingApprovals,
  workspaceLabel,
  workspaceTooltip,
  type ProviderKind,
  type TreeSessionItem,
} from "../lib/session-tree";
import {
  ChevronDownIcon,
  CORE_LAUNCH_AGENTS,
  EFFORT_OPTIONS,
  formatSessionTokens,
  LAUNCH_AGENT_OPTIONS,
  LAUNCH_AGENTS,
  PERMISSION_MODE_OPTIONS,
  QUICK_PROMPT_SNIPPETS,
  SANDBOX_OPTIONS,
  SettingsIcon,
  type LaunchAgentSpec,
} from "./remote-console-constants";
import "./remote.css";
import "./remote-desktop.css";

const empty: RemoteSnapshot = { cursor: 0, runners: [], sessions: [] };

const SNAPSHOT_FALLBACK_MS = 30_000;
const DETAIL_FALLBACK_MS = 30_000;

const key = (session: RemoteSession) =>
  `${session.runner_id}/${session.binding.hub_session_id}`;

const EMPTY_EVENTS: RemoteEvent[] = [];
const EMPTY_COMMANDS: RemoteCommandRecord[] = [];
const EMPTY_OPTIMISTIC: OptimisticPrompt[] = [];

/**
 * A one-line preview of what a tool group actually did.
 *
 * Only distinct command snippets are listed: repeating one label three times
 * (`exec · exec · exec`) or echoing an empty detail buries the single bit that
 * tells a reader what the turn is working on.
 */
function isCodexThreadId(id: string): boolean {
  return /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/iu.test(id);
}

function groupPreview(items: ConversationItem[]): string {
  const seen = new Set<string>();
  const parts: string[] = [];
  for (const item of items) {
    const detail = (item.description ?? item.command ?? "").trim();
    const line = detail.split("\n").find((part) => part.trim().length > 0);
    if (!line || line.includes("[local_path_redacted]")) continue;
    const snippet = line.trim().slice(0, 40);
    if (seen.has(snippet)) continue;
    seen.add(snippet);
    parts.push(`${item.title ?? "工具"} ${snippet}`);
    if (parts.length === 2) break;
  }
  return parts.join(" · ");
}

function sameConversationItemArray(
  prev: { items: ConversationItem[] },
  next: { items: ConversationItem[] },
): boolean {
  if (prev.items === next.items) return true;
  if (prev.items.length !== next.items.length) return false;
  for (let i = 0; i < prev.items.length; i++) {
    if (prev.items[i] !== next.items[i]) return false;
  }
  return true;
}

const RemoteToolGroup = memo(function RemoteToolGroup({
  items,
}: {
  items: ConversationItem[];
}) {
  const status = items.some((item) => item.status === "failed")
    ? "failed"
    : items.some((item) => item.status === "running")
      ? "running"
      : "completed";
  const isRunning = status === "running";
  return (
    <details className={`remote-tool-group state-${status}`} open={isRunning}>
      <summary className="remote-tool-group-summary">
        <span className="remote-disclosure" aria-hidden="true">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="9 6 15 12 9 18" />
          </svg>
        </span>
        <span className="remote-tool-group-icon" aria-hidden="true">
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
          </svg>
        </span>
        <span className="remote-tool-group-title">
          执行了 {items.length} 项操作
        </span>
        <span className="remote-tool-group-preview">{groupPreview(items)}</span>
        <span className={`remote-tool-status ${status}`}>
          {status === "running" && (
            <span className="remote-tool-pulse-dots" aria-hidden="true">
              <span className="pulse-dot" />
              <span className="pulse-dot" />
              <span className="pulse-dot" />
            </span>
          )}
          <span className="status-label">
            {status === "running"
              ? "执行中…"
              : status === "failed"
                ? "异常"
                : "已完成"}
          </span>
        </span>
      </summary>
      <div className="remote-tool-group-body">
        {items.map((item) => (
          <RemoteToolCard key={item.id} item={item} />
        ))}
      </div>
    </details>
  );
}, sameConversationItemArray);

export function RemoteConsole({
  request = remoteRequest,
  onAuthenticationRequired,
  account,
  onLogout,
  onLoggedOut,
  config,
}: {
  request?: RemoteRequest;
  onAuthenticationRequired?: () => void;
  account?: AccountView | null;
  onLogout?: () => void;
  /** Fired when Settings revokes the current session, so the page can sign out. */
  onLoggedOut?: () => void;
  /** Gateway config the settings view reads and writes. */
  config?: PanelConfig | undefined;
}) {
  const [snapshot, setSnapshot] = useState(empty);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  const [selected, setSelected] = useState("");
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const [eventsBySession, setEventsBySession] = useState<
    Record<string, RemoteEvent[]>
  >({});
  const eventsBySessionRef = useRef(eventsBySession);
  eventsBySessionRef.current = eventsBySession;
  const eventsOrderRef = useRef<string[]>([]);
  const [commandsBySession, setCommandsBySession] = useState<
    Record<string, RemoteCommandRecord[]>
  >({});
  const commandsOrderRef = useRef<string[]>([]);
  const sessionCursorRef = useRef<
    Map<string, { cursor: number; lastPolledGlobalCursor: number }>
  >(new Map());
  const events = selected
    ? (eventsBySession[selected] ?? EMPTY_EVENTS)
    : EMPTY_EVENTS;
  const commands = selected
    ? (commandsBySession[selected] ?? EMPTY_COMMANDS)
    : EMPTY_COMMANDS;
  const setEventsForSession = (sessionKey: string, next: RemoteEvent[]) => {
    setEventsBySession((prev) =>
      setBoundedSessionRecord(prev, eventsOrderRef.current, sessionKey, next),
    );
  };
  const setCommandsForSession = (
    sessionKey: string,
    next: RemoteCommandRecord[],
  ) => {
    setCommandsBySession((prev) =>
      setBoundedSessionRecord(prev, commandsOrderRef.current, sessionKey, next),
    );
  };
  const [error, setError] = useState("");
  const errorRef = useRef(error);
  errorRef.current = error;

  useEffect(() => {
    if (!error || !isTransientRemoteMessage(error)) return;
    const timer = setTimeout(() => {
      setError((current) => (isTransientRemoteMessage(current) ? "" : current));
    }, 6000);
    return () => clearTimeout(timer);
  }, [error]);
  const [text, setText] = useState("");
  const textRef = useRef(text);
  textRef.current = text;
  const [optimisticBySession, setOptimisticBySession] = useState<
    Record<string, OptimisticPrompt[]>
  >({});
  const optimisticOrderRef = useRef<string[]>([]);
  const optimisticPrompts = selected
    ? (optimisticBySession[selected] ?? EMPTY_OPTIMISTIC)
    : EMPTY_OPTIMISTIC;
  const updateOptimisticForSession = (
    sessionKey: string,
    updater: (cur: OptimisticPrompt[]) => OptimisticPrompt[],
  ) => {
    setOptimisticBySession((prev) => {
      const cur = prev[sessionKey] ?? EMPTY_OPTIMISTIC;
      return setBoundedSessionRecord(
        prev,
        optimisticOrderRef.current,
        sessionKey,
        updater(cur),
      );
    });
  };
  const [queuedPrompts, setQueuedPrompts] = useState<
    Record<string, QueuedPrompt>
  >({});
  const [resumingCandidateKey, setResumingCandidateKey] = useState<
    string | null
  >(null);
  const draftsBySessionRef = useRef<
    Record<string, { text: string; attachments: RemoteAttachment[] }>
  >({});
  const wasInterruptedBySessionRef = useRef<Record<string, boolean>>({});
  const wasTurnExecutingBySessionRef = useRef<Record<string, boolean>>({});
  const lastCheckedQueueSessionRef = useRef("");
  const prevSelectedSessionRef = useRef("");
  const [questionAnswers, setQuestionAnswers] = useState<
    Record<string, string>
  >({});
  const [submittingApproval, setSubmittingApproval] = useState<{
    id: string;
    action: "approve" | "decline";
  } | null>(null);
  const [copiedApprovalId, setCopiedApprovalId] = useState<string | null>(null);
  const [collapsedApprovalDiffs, setCollapsedApprovalDiffs] = useState<
    Record<string, boolean>
  >({});
  const [submittingQuestionId, setSubmittingQuestionId] = useState<
    string | null
  >(null);
  const [selectedOptions, setSelectedOptions] = useState<
    Record<string, string[]>
  >({});
  const [effort, setEffort] = useState<
    "default" | "low" | "medium" | "high" | "max"
  >("default");
  const [permissionMode, setPermissionMode] =
    useState<RemotePermissionMode>("default");
  const [permissionModeTouched, setPermissionModeTouched] = useState(false);
  const [launchPermissionMode, setLaunchPermissionMode] =
    useState<RemotePermissionMode>("default");
  const [sessionModel, setSessionModel] = useState("");
  const [attachments, setAttachments] = useState<RemoteAttachment[]>([]);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;
  const [attachMenuOpen, setAttachMenuOpen] = useState(false);
  const [attachQuickPromptsOpen, setAttachQuickPromptsOpen] = useState(false);
  const attachMenuRef = useRef<HTMLDivElement>(null);
  const [sessionLoading, setSessionLoading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [submittingInterrupt, setSubmittingInterrupt] = useState(false);
  const [busyBySession, setBusyBySession] = useState<Record<string, number>>(
    {},
  );
  const [launchBusy, setLaunchBusy] = useState(false);
  const busy = Boolean(
    launchBusy || (selected && (busyBySession[selected] ?? 0) > 0),
  );
  const setSessionBusy = (sessionKey: string, delta: 1 | -1) => {
    setBusyBySession((prev) => {
      const nextCount = Math.max(0, (prev[sessionKey] ?? 0) + delta);
      if (nextCount === 0) {
        if (!(sessionKey in prev)) return prev;
        const copy = { ...prev };
        delete copy[sessionKey];
        return copy;
      }
      return { ...prev, [sessionKey]: nextCount };
    });
  };
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The settings view needs the Gateway address even on the routes that do
  // not pass one down, so the console falls back to the stored config itself.
  const [panelConfig, setPanelConfig] = useState<PanelConfig | undefined>(
    config,
  );
  const [notificationPresentedAt, setNotificationPresentedAt] = useState<
    number | undefined
  >(undefined);
  const [followBottom, setFollowBottom] = useState(true);
  // A dismissed failure stays dismissed until the session changes or a new
  // failure arrives, so it cannot reappear on every poll.
  const [dismissedFailure, setDismissedFailure] = useState<string | null>(null);
  // Keep an uncertain POST's identity when the user retries the same operation.
  const attempts = useRef(new Map<string, RemoteCommand>());
  const previewedHistory = useRef("");
  const [takeoverDrafts, setTakeoverDrafts] = useState<
    Record<
      string,
      { id: string; text: string; attachments: RemoteAttachment[] }
    >
  >({});
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (config !== undefined) {
      setPanelConfig(config);
      return;
    }
    setPanelConfig(loadConfig(window.localStorage));
  }, [config]);

  // The console's requests carry the signed-in account's identity. Without
  // it the Gateway falls back to the development header, which resolves a
  // different owner than the one the devices are bound to — the console then
  // shows an empty list even though the Runner is publishing sessions. The
  // panel config's id is only the fallback for a Gateway without accounts.
  const requestWithIdentity = useMemo<RemoteRequest>(
    () =>
      (path, body, timeoutMs = 25000, _userId?: string, signal?: AbortSignal) =>
        request(
          path,
          body,
          timeoutMs,
          account?.user.user_id ?? panelConfig?.userId,
          signal,
        ),
    [request, account?.user.user_id, panelConfig?.userId],
  );

  const refresh = useRef(() => {});
  const refreshSessionDetail = useRef(() => {});

  /**
   * Full `hello` per runner, fetched on demand and kept alive across `/sessions`
   * refreshes so SSE `ready` pushes never discard an in-flight `/hello` load.
   */
  const [helloEntries, setHelloEntries] = useState<
    Record<string, RunnerHelloEntry>
  >({});
  const helloSyncRef = useRef<RunnerHelloSyncController | null>(null);

  const runners = useMemo<RemoteRunner[]>(
    () => mergeRunnerHelloEntries(snapshot.runners, helloEntries),
    [snapshot.runners, helloEntries],
  );

  useEffect(() => {
    setHelloEntries({});
    const controller = new RunnerHelloSyncController({
      fetcher: (id, signal) =>
        requestWithIdentity<RemoteRunnerHelloResponse>(
          `/runners/${encodeURIComponent(id)}/hello`,
          undefined,
          25000,
          undefined,
          signal,
        ),
      onChange: setHelloEntries,
      ttlMs: SNAPSHOT_FALLBACK_MS,
    });
    helloSyncRef.current = controller;
    return () => {
      controller.dispose();
      if (helloSyncRef.current === controller) {
        helloSyncRef.current = null;
      }
    };
  }, [requestWithIdentity]);

  useEffect(() => {
    const targets = snapshot.runners
      .filter((runner) => !runner.revoked)
      .map((runner) => ({
        runnerId: runner.runner_id,
        hasThinHello: runner.hello !== null,
        hostGeneration: runner.hello?.host_generation ?? null,
      }));
    helloSyncRef.current?.sync(targets);
  }, [snapshot.runners, requestWithIdentity]);

  // When a user returns to the tab after sleep or background throttling,
  // immediately trigger fresh polls to resume live conversation state.
  useEffect(() => {
    if (typeof document === "undefined") return;
    const onVisibilityChange = () => {
      if (!document.hidden) {
        refresh.current();
        refreshSessionDetail.current();
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  // The service worker reports back when it actually presented a waiting
  // notification, so Settings can show a receipt instead of assuming delivery.
  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
    const onMessage = (event: MessageEvent) => {
      const presentedAt = parseNotificationPresentedAt(event.data);
      if (presentedAt !== undefined) setNotificationPresentedAt(presentedAt);
      if (
        event.data &&
        typeof event.data === "object" &&
        event.data.type === "select-session"
      ) {
        const rawSession = event.data.session;
        if (typeof rawSession === "string") {
          setSelected(rawSession);
        } else {
          const parsed = parseSessionRef(rawSession);
          if (parsed) {
            const found = snapshotRef.current.sessions.find(
              (s) =>
                s.runner_id === parsed.device_id &&
                s.binding.provider_session_id === parsed.agent_session_id,
            );
            if (found) {
              setSelected(`${found.runner_id}/${found.binding.hub_session_id}`);
            }
          }
        }
      }
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
    navigator.serviceWorker.controller?.postMessage({
      type: "consume-pending-session",
    });
    void navigator.serviceWorker.ready
      .then((reg) =>
        reg.active?.postMessage({ type: "consume-pending-session" }),
      )
      .catch(() => undefined);
    return () =>
      navigator.serviceWorker.removeEventListener("message", onMessage);
  }, []);

  // 侧边栏宽度与折叠状态持久化
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    if (typeof window !== "undefined") {
      const saved = localStorage.getItem("agenthub_remote_sidebar_width");
      if (saved) {
        const parsed = parseInt(saved, 10);
        if (!isNaN(parsed) && parsed >= 180 && parsed <= 520) return parsed;
      }
    }
    return 280;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    if (typeof window !== "undefined") {
      return (
        localStorage.getItem("agenthub_remote_sidebar_collapsed") === "true"
      );
    }
    return false;
  });
  const {
    isMobile,
    mobileDrawerOpen,
    drawerAnimated,
    consoleRootRef,
    mobileMenuBtnRef,
    openMobileDrawer,
    closeMobileDrawer,
    setMobileDrawerOpen,
    onSidebarTouchStart,
    onSidebarTouchMove,
    onSidebarTouchEnd,
    onMainTouchStart,
    onMainTouchMove,
    onMainTouchEnd,
  } = useMobileConsoleLayout();
  const searchInputRef = useRef<HTMLInputElement>(null);

  // The mobile drawer covers the console, so Escape has to close it and the
  // keyboard must not wander into the content behind it.
  const mobileDrawerRef = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!isMobile || !mobileDrawerOpen) return;
    const drawer = mobileDrawerRef.current;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeMobileDrawer(true);
        return;
      }
      if (event.key !== "Tab" || drawer === null) return;
      const focusable = drawer.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) return;
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !drawer.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [isMobile, mobileDrawerOpen, closeMobileDrawer]);
  const currentSidebarWidthRef = useRef(sidebarWidth);
  currentSidebarWidthRef.current = sidebarWidth;
  const isResizingRef = useRef(false);
  const cleanResizerListenersRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      isResizingRef.current = false;
      cleanResizerListenersRef.current?.();
    };
  }, []);

  useEffect(() => {
    if (!attachMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (
        attachMenuRef.current &&
        !attachMenuRef.current.contains(event.target as Node)
      ) {
        setAttachMenuOpen(false);
        setAttachQuickPromptsOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setAttachMenuOpen(false);
        setAttachQuickPromptsOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [attachMenuOpen]);

  const toggleSidebar = () => {
    setSidebarCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem("agenthub_remote_sidebar_collapsed", String(next));
      return next;
    });
  };

  const handleResizerMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    isResizingRef.current = true;
    const startX = e.clientX;
    const startWidth = currentSidebarWidthRef.current;
    let latestWidth = startWidth;

    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!isResizingRef.current) return;
      const delta = moveEvent.clientX - startX;
      latestWidth = Math.min(Math.max(startWidth + delta, 180), 520);
      setSidebarWidth(latestWidth);
    };

    const onMouseUp = () => {
      isResizingRef.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      cleanResizerListenersRef.current = null;
      localStorage.setItem(
        "agenthub_remote_sidebar_width",
        String(latestWidth),
      );
    };

    cleanResizerListenersRef.current = () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  };

  const [providerFilter, setProviderFilter] = useState<ProviderKind>("claude");
  const userSelectedProviderRef = useRef(false);
  const [selectedRunnerId, setSelectedRunnerId] = useState<string>("");
  const [deviceMenuOpen, setDeviceMenuOpen] = useState(false);
  const [agentMenuOpen, setAgentMenuOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const deviceMenuRef = useRef<HTMLDivElement>(null);
  const deviceMenuButtonRef = useRef<HTMLButtonElement>(null);
  const agentMenuRef = useRef<HTMLDivElement>(null);
  const agentMenuButtonRef = useRef<HTMLButtonElement>(null);
  const [runner, setRunner] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [agentType, setAgentType] = useState<LaunchAgentType>("codex");

  const [pluginsResponse, setPluginsResponse] =
    useState<RemotePluginsResponse | null>(null);
  useEffect(() => {
    let cancelled = false;
    const loadPlugins = () => {
      requestWithIdentity<RemotePluginsResponse>("/plugins")
        .then((res) => {
          if (!cancelled && res && Array.isArray(res.plugins)) {
            setPluginsResponse(res);
          }
        })
        .catch(() => {});
    };
    loadPlugins();
    const unfollowPlugins = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "plugins",
      onChange: () => {
        if (!cancelled) loadPlugins();
      },
    });
    return () => {
      cancelled = true;
      unfollowPlugins();
    };
  }, [requestWithIdentity, settingsOpen, panelConfig?.gatewayBase]);

  const plugins = useMemo(() => {
    if (!pluginsResponse) return [];
    if (runner && Array.isArray(pluginsResponse.runners)) {
      const snap = pluginsResponse.runners.find((r) => r.runner === runner);
      if (snap) return snap.plugins;
      if (pluginsResponse.runners.length > 0) return [];
    }
    return pluginsResponse.plugins;
  }, [pluginsResponse, runner]);

  const allKnownPlugins = useMemo(() => {
    if (!pluginsResponse) return [];
    const merged = new Map<string, AgentPlugin>();
    for (const r of pluginsResponse.runners ?? []) {
      for (const p of r.plugins) {
        if (!merged.has(p.id)) merged.set(p.id, p);
      }
    }
    for (const p of pluginsResponse.plugins) {
      if (!merged.has(p.id)) merged.set(p.id, p);
    }
    return [...merged.values()];
  }, [pluginsResponse]);

  const availableLaunchAgents = useMemo(() => {
    const list: Array<{
      value: string;
      label: string;
      catalog: readonly string[];
    }> = [...CORE_LAUNCH_AGENTS];
    const seen = new Set<string>(list.map((item) => item.value));
    for (const plugin of plugins) {
      if (
        plugin.installed !== false &&
        plugin.enabled &&
        plugin.available !== false &&
        plugin.capabilities?.launch !== false &&
        !seen.has(plugin.id)
      ) {
        seen.add(plugin.id);
        list.push({
          value: plugin.id,
          label: plugin.displayName || plugin.name || plugin.id,
          catalog: [plugin.id, ...(plugin.aliases ?? [])],
        });
      }
    }
    return list;
  }, [plugins]);

  const launchAgentOptions = useMemo(
    () => availableLaunchAgents.map(({ value, label }) => ({ value, label })),
    [availableLaunchAgents],
  );

  useEffect(() => {
    if (!availableLaunchAgents.some((a) => a.value === agentType)) {
      setAgentType(availableLaunchAgents[0]?.value ?? "codex");
    }
  }, [availableLaunchAgents, agentType]);
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [nativeId, setNativeId] = useState("");
  const [launchModalOpen, setLaunchModalOpen] = useState(false);
  const timeline = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const session = snapshot.sessions.find((item) => key(item) === selected);
  const runnerInfo = runners.find((item) => item.runner_id === runner);
  const connected = !!session?.online && !!session.binding.control_connected;
  const supportsCodexControl = (runnerId: string) =>
    runners.find((item) => item.runner_id === runnerId)
      ?.codex_session_control === true;
  const codexSession = Boolean(
    session &&
    isCodexThreadId(session.binding.provider_session_id) &&
    supportsCodexControl(session.runner_id),
  );
  const releasingControl =
    session?.online && session.binding.execution_state === "releasing_control";
  const canTakeControl = Boolean(
    codexSession && session?.online && !connected && !releasingControl,
  );

  const discovered = useMemo(() => {
    const candidates = new Map<string, DiscoveryCandidate>();
    for (const r of runners) {
      if (r.revoked) continue;
      for (const candidate of r.hello?.discovery?.candidates ?? [])
        candidates.set(
          `${r.runner_id}/${candidate.provider_session_id}`,
          candidate,
        );
    }
    return candidates;
  }, [runners]);

  /** Every online computer, for the launch dialog's first field. */
  const onlineDeviceOptions = useMemo(
    () =>
      runners
        .filter((item) => !item.revoked && item.online)
        .map((item) => ({
          value: item.runner_id,
          label: getDeviceLabel(item.runner_id, item.hello),
        })),
    [runners],
  );

  /**
   * Workspaces read as their folder name, with the full path on a second line.
   *
   * The Host lists workspaces by opaque `ws-` id or by absolute path; neither
   * belongs in a menu a person reads. The id/path stays as the option's value,
   * and the path is kept visible underneath because two checkouts can share a
   * folder name.
   */
  const launchWorkspaceOptions = useMemo(() => {
    const paths = runnerInfo?.hello?.workspaces ?? [];
    const labels = paths.map(workspaceLabel);
    const seen = new Map<string, number>();
    for (const label of labels) seen.set(label, (seen.get(label) ?? 0) + 1);
    return paths.map((path, index) => {
      const label = labels[index] ?? path;
      // Two checkouts can share a folder name. Only then does the id's own
      // tail earn a place on screen, and it goes on the muted second line.
      const tail = path.startsWith("ws-")
        ? (path.match(/-([0-9a-f]{16})$/u)?.[1] ?? "").slice(0, 6)
        : "";
      const duplicated = (seen.get(label) ?? 0) > 1 && tail !== "";
      return {
        value: path,
        label,
        ...(duplicated ? { description: `…${tail}` } : {}),
      };
    });
  }, [runnerInfo]);

  /**
   * Sessions already on disk in the chosen workspace that this dialog can
   * take over. They are labelled with their own engine and tagged so two
   * engines with the same title stay tellable apart.
   */
  const launchSessionOptions = useMemo(() => {
    const normalize = (path: string) =>
      path
        .replace(/^\\\\\?\\/u, "")
        .replaceAll("\\", "/")
        .toLowerCase();
    const target = normalize(workspace);
    const candidates = (runnerInfo?.hello?.discovery?.candidates ?? []).filter(
      (item) => normalize(item.workspace_path) === target,
    );
    return [
      { value: "", label: "创建新会话" },
      ...candidates.map((item) => {
        const provider = getProviderInfo(
          item.provider_session_id,
          item.provider,
        );
        return {
          value: item.provider_session_id,
          label: item.title ?? "未命名会话",
          title: item.title ?? item.provider_session_id,
          tag: provider.name,
          tagTone: provider.badgeClass,
        };
      }),
    ];
  }, [runnerInfo, workspace]);

  /**
   * The chosen engine's own models.
   *
   * `models.agents` is the Host's per-engine answer. A Host that predates it
   * only sends the flat `data` list, and that list belongs to whichever
   * provider answered — so it is offered for Codex, whose `model/list` RPC is
   * exactly that list, and nowhere else.
   */
  const launchModelOptions = useMemo(() => {
    const hostModels = runnerInfo?.hello?.models;
    const engine = availableLaunchAgents.find(
      (item) => item.value === agentType,
    );
    const seen = new Set<string>();
    const models: Array<{ id: string; display_name?: string }> = [];
    const pushModel = (id?: string | null, displayName?: string | null) => {
      const clean = id?.trim();
      if (!clean || clean === "<synthetic>") return;
      const lower = clean.toLowerCase();
      if (seen.has(lower)) return;
      seen.add(lower);
      models.push({
        id: clean,
        display_name: displayName?.trim() || clean.replace(/^models\//u, ""),
      });
    };
    const keys: readonly string[] =
      agentType === "claude_code" &&
      hostModels?.agents?.claude_code === undefined
        ? ["claude"]
        : (engine?.catalog ?? []);
    for (const key of keys) {
      const list = hostModels?.agents?.[key];
      if (list !== undefined && list.length > 0) {
        for (const item of list) pushModel(item.id, item.display_name);
      }
    }
    if (models.length === 0 && agentType === "codex") {
      for (const item of hostModels?.data ?? []) {
        pushModel(
          item.id,
          item.display_name ?? item.displayName ?? item.name ?? item.id,
        );
      }
    }
    return [
      { value: "", label: "默认模型" },
      ...models.map((item) => ({
        value: item.id,
        label: item.display_name ?? item.id,
      })),
    ];
  }, [runnerInfo, agentType]);

  const sessionModelOptions = useMemo(() => {
    if (!session) return [{ value: "", label: "默认模型" }];
    const sessionRunner = runners.find(
      (item) => item.runner_id === session.runner_id,
    );
    const hostModels = sessionRunner?.hello?.models;
    const candidate = discovered.get(
      `${session.runner_id}/${session.binding.provider_session_id}`,
    );
    const effectiveProvider = candidate?.provider ?? session.provider;
    const providerType = getProviderInfo(
      session.binding.provider_session_id,
      effectiveProvider,
    ).type;
    const rawProvider = (effectiveProvider ?? "").toLowerCase();
    const sid = session.binding.provider_session_id;
    const catalogKeys =
      rawProvider === "claude_desktop" || providerType === "claude_desktop"
        ? ["claude_desktop"]
        : rawProvider === "claude_code" ||
            rawProvider === "claude" ||
            providerType === "claude" ||
            sid.startsWith("claude_")
          ? hostModels?.agents?.claude_code !== undefined
            ? ["claude_code"]
            : ["claude_code", "claude"]
          : providerType === "antigravity" || rawProvider === "antigravity"
            ? ["antigravity"]
            : rawProvider === "deepseek_harness" ||
                providerType === "deepseek_harness"
              ? ["deepseek_harness"]
              : rawProvider && rawProvider !== "codex"
                ? [rawProvider]
                : providerType !== "codex"
                  ? [providerType]
                  : ["codex"];
    const seen = new Set<string>();
    const models: Array<{ id: string; display_name?: string }> = [];
    const pushModel = (id?: string | null, displayName?: string | null) => {
      const clean = id?.trim();
      if (!clean || clean === "<synthetic>") return;
      const lower = clean.toLowerCase();
      if (seen.has(lower)) return;
      seen.add(lower);
      models.push({
        id: clean,
        display_name: displayName?.trim() || clean.replace(/^models\//u, ""),
      });
    };
    for (const k of catalogKeys) {
      for (const item of hostModels?.agents?.[k] ?? []) {
        pushModel(item.id, item.display_name);
      }
    }
    if (catalogKeys.includes("codex") && models.length === 0) {
      for (const item of hostModels?.data ?? []) {
        pushModel(
          item.id,
          item.display_name ?? item.displayName ?? item.name ?? item.id,
        );
      }
    }
    const normalizedCandidateModel = candidate?.model
      ?.trim()
      .replace(/\[1m\]$/iu, "")
      .toLowerCase();
    const matchedCatalogModel = normalizedCandidateModel
      ? models.find(
          (item) =>
            item.id.toLowerCase() === normalizedCandidateModel ||
            item.id.replace(/^models\//u, "").toLowerCase() ===
              normalizedCandidateModel,
        )
      : undefined;
    const defaultLabel = matchedCatalogModel
      ? `默认 (${matchedCatalogModel.display_name ?? matchedCatalogModel.id})`
      : "默认模型";
    return [
      { value: "", label: defaultLabel },
      ...models.map((item) => ({
        value: item.id,
        label: item.display_name ?? item.id,
      })),
    ];
  }, [session, runners, discovered]);

  const handleFilesSelected = async (fileList: FileList | File[]) => {
    const files = Array.from(fileList);
    if (files.length === 0) return;
    const TEXT_EXT_RE =
      /\.(txt|md|markdown|json|jsonl|yaml|yml|toml|xml|csv|tsv|py|ts|tsx|js|jsx|mjs|cjs|rs|go|java|kt|c|cpp|h|hpp|cs|rb|php|swift|sh|bash|zsh|ps1|bat|cmd|sql|html|css|scss|less|vue|svelte|ini|conf|cfg|env|log)$/iu;
    const loaded: RemoteAttachment[] = [];
    for (const file of files.slice(0, 16)) {
      const sizeStatus = checkAttachmentSize(file.size);
      if (sizeStatus === "empty") {
        setError(`附件 ${file.name} 为空文件（0 字节），已跳过`);
        continue;
      }
      if (sizeStatus === "too_large") {
        setError(`附件 ${file.name} 超过 7.5MB 大小限制`);
        continue;
      }
      const isText =
        file.type.startsWith("text/") ||
        file.type === "application/json" ||
        file.type === "application/xml" ||
        file.type === "application/javascript" ||
        TEXT_EXT_RE.test(file.name);
      try {
        if (isText) {
          const content = await file.text();
          loaded.push({
            name: file.name || "attachment.txt",
            media_type: file.type || "text/plain",
            data: content,
            encoding: "utf8",
          });
        } else {
          const base64 = await new Promise<string>((resolve, reject) => {
            const reader = new FileReader();
            reader.onload = () => {
              const result =
                typeof reader.result === "string" ? reader.result : "";
              const comma = result.indexOf(",");
              resolve(comma >= 0 ? result.slice(comma + 1) : result);
            };
            reader.onerror = () => reject(reader.error);
            reader.readAsDataURL(file);
          });
          if (base64) {
            loaded.push({
              name: file.name || "image.png",
              media_type: file.type || "application/octet-stream",
              data: base64,
              encoding: "base64",
            });
          }
        }
      } catch {
        setError(`读取附件 ${file.name} 失败`);
      }
    }
    if (loaded.length > 0) {
      setAttachments((prev) => [...prev, ...loaded].slice(0, 16));
    }
  };
  const hasInFlightPrompt = useMemo(
    () =>
      commands.some(
        (c) => c.command.command.action === "send_prompt" && c.receipt === null,
      ),
    [commands],
  );
  const isTurnExecuting = Boolean(
    session &&
    connected &&
    (session.binding.execution_state === "running" ||
      Boolean(session.binding.current_turn_id) ||
      hasInFlightPrompt ||
      optimisticPrompts.length > 0),
  );
  const canSend =
    !takeoverDrafts[selected] &&
    (canTakeControl ||
      (connected &&
        !isTurnExecuting &&
        ["idle", "completed", "error", "failed", "interrupted"].includes(
          session?.binding.execution_state ?? "",
        )));

  const adjustTextareaHeight = () => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const scrollH = el.scrollHeight;
    const clamped = Math.min(Math.max(scrollH, 28), 220);
    el.style.height = `${clamped}px`;
    el.style.overflowY = scrollH > 220 ? "auto" : "hidden";
  };

  const handlePasteFromClipboardMenu = async () => {
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
    if (typeof navigator === "undefined" || !navigator.clipboard) {
      setError("当前浏览器环境不支持直接读取剪贴板，请使用 Ctrl+V 粘贴。");
      return;
    }
    try {
      if (typeof navigator.clipboard.read === "function") {
        const items = await navigator.clipboard.read();
        const files: File[] = [];
        for (const clipItem of items) {
          const imgType = clipItem.types.find((t) => t.startsWith("image/"));
          if (imgType) {
            const blob = await clipItem.getType(imgType);
            const ext = imgType.split("/")[1] || "png";
            files.push(
              new File([blob], `clipboard-${Date.now()}.${ext}`, {
                type: imgType,
              }),
            );
          }
        }
        if (files.length > 0) {
          await handleFilesSelected(files);
          textareaRef.current?.focus();
          return;
        }
      }
      const clipText = await navigator.clipboard.readText();
      if (clipText) {
        if (clipText.length > 8192) {
          const textFile = new File([clipText], `clipboard-${Date.now()}.txt`, {
            type: "text/plain",
          });
          await handleFilesSelected([textFile]);
        } else {
          setText((prev) => (prev ? `${prev}\n${clipText}` : clipText));
          requestAnimationFrame(adjustTextareaHeight);
        }
        textareaRef.current?.focus();
      }
    } catch {
      setError("浏览器未授权读取剪贴板，请直接在输入框按 Ctrl+V 粘贴。");
    }
  };

  const handleInsertCodeSnippet = () => {
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
    const el = textareaRef.current;
    const start = el?.selectionStart ?? text.length;
    const end = el?.selectionEnd ?? text.length;
    const selectedSlice = text.slice(start, end);
    const prefix = start > 0 && text[start - 1] !== "\n" ? "\n" : "";
    const block = `${prefix}\`\`\`\n${selectedSlice}\n\`\`\`\n`;
    const nextText = text.slice(0, start) + block + text.slice(end);
    setText(nextText);
    const cursorOffset = start + prefix.length + 4 + selectedSlice.length;
    requestAnimationFrame(() => {
      adjustTextareaHeight();
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(cursorOffset, cursorOffset);
      }
    });
  };
  useEffect(() => {
    let stopped = false;
    const abortController = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    let inFlight = false;
    // A push that lands while a read is out asks for one more read after it,
    // never a second concurrent request: the answer would be older than the
    // read already in flight.
    let again = false;
    const poll = async () => {
      if (stopped) return;
      if (inFlight) {
        again = true;
        return;
      }
      inFlight = true;
      again = false;
      clearTimeout(timer);
      try {
        const value = await requestWithIdentity<RemoteSnapshot>(
          "/sessions",
          undefined,
          25000,
          undefined,
          abortController.signal,
        );
        if (!stopped) {
          setSnapshot(value);
          setLoading(false);
          if (isTransientRemoteMessage(errorRef.current)) {
            setError("");
          }
        }
      } catch (err) {
        if (
          !stopped &&
          !(err instanceof Error && err.message === "request_aborted")
        ) {
          if (
            typeof document !== "undefined" &&
            document.hidden &&
            isTransientRemoteError(err)
          ) {
            // Keep background poll silent when tab is suspended or hidden
          } else {
            setError(remoteError(err));
          }
          setLoading(false);
          if (err instanceof Error && err.message === "authentication_required")
            onAuthenticationRequired?.();
        }
      } finally {
        inFlight = false;
        if (!stopped) {
          if (again) {
            void poll();
          } else {
            timer = setTimeout(() => void poll(), SNAPSHOT_FALLBACK_MS);
          }
        }
      }
    };
    refresh.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    const unfollow = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "remote",
      onChange: (frames) => {
        if (streamBatchAffectsSnapshot(frames)) {
          void poll();
        }
      },
    });
    return () => {
      stopped = true;
      abortController.abort();
      clearTimeout(timer);
      unfollow();
    };
  }, [
    requestWithIdentity,
    onAuthenticationRequired,
    panelConfig?.gatewayBase,
    panelConfig?.userId,
    account?.user.user_id,
  ]);
  useEffect(() => {
    const previousKey = prevSelectedSessionRef.current;
    if (previousKey && previousKey !== selected) {
      draftsBySessionRef.current[previousKey] = {
        text: textRef.current,
        attachments: [...attachmentsRef.current],
      };
    }
    prevSelectedSessionRef.current = selected;
    const savedDraft = selected
      ? draftsBySessionRef.current[selected]
      : undefined;
    setText(savedDraft?.text ?? "");
    setAttachments(savedDraft ? [...savedDraft.attachments] : []);
    setSessionModel("");
    setPermissionMode("default");
    setPermissionModeTouched(false);
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
    setQuestionAnswers({});
    setSelectedOptions({});
    setSubmittingApproval(null);
    setSubmittingQuestionId(null);
    setFollowBottom(true);
    setDismissedFailure(null);
    if (!selected) {
      setSessionLoading(false);
      return;
    }
    const [runnerId, hub] = selected.split("/");
    if (!runnerId || !hub) {
      setSessionLoading(false);
      return;
    }
    const sessionKey = `${runnerId}/${hub}`;
    const abortController = new AbortController();
    const existingMeta = sessionCursorRef.current.get(sessionKey);
    let accumulated = eventsBySessionRef.current[sessionKey] ?? [];
    setSessionLoading(accumulated.length === 0);
    let stopped = false;
    let inFlight = false;
    let again = false;
    let cursor = existingMeta?.cursor ?? 0;
    let lastPolledGlobalCursor = existingMeta?.lastPolledGlobalCursor ?? -1;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (stopped) return;
      if (inFlight) {
        again = true;
        return;
      }
      again = false;
      inFlight = true;
      let latestDetail: RemoteSessionDetail | null = null;
      try {
        // One request, not two. The session detail route already carries the
        // binding, the event cursor and the command history, so the event fetch
        // below only runs when the cursor has actually moved.
        const detail = await requestWithIdentity<RemoteSessionDetail>(
          `/sessions/${encodeURIComponent(runnerId)}/${encodeURIComponent(hub)}`,
          undefined,
          25000,
          undefined,
          abortController.signal,
        );
        if (stopped) return;
        latestDetail = detail;
        if (detail.cursor > lastPolledGlobalCursor) {
          // First read of this session: start where both the transcript
          // snapshot and the pending-approval bearer agree, so neither an old
          // delta wall nor an outstanding approval is skipped. Later reads
          // resume from the cursor this loop already reached.
          if (lastPolledGlobalCursor === -1 && cursor === 0 && detail.resume) {
            const marks = [detail.resume.transcript, detail.resume.approvals]
              .filter(
                (mark): mark is number => typeof mark === "number" && mark > 0,
              )
              .sort((a, b) => a - b);
            if (marks.length > 0) cursor = Math.max(0, marks[0]! - 1);
          }
          let hasNewEvents = false;
          while (true) {
            const batch = await requestWithIdentity<{ events: RemoteEvent[] }>(
              `/events?runner=${encodeURIComponent(runnerId)}&hub=${encodeURIComponent(hub)}&after=${cursor}`,
              undefined,
              25000,
              undefined,
              abortController.signal,
            );
            if (stopped) return;
            if (batch.events.length > 0) {
              const merged = mergeMonotonicEvents(
                accumulated,
                batch.events,
                cursor,
              );
              cursor = merged.nextCursor;
              if (merged.changed) {
                accumulated = merged.nextEvents;
                hasNewEvents = true;
              }
            }
            if (batch.events.length < 256) break;
          }
          if (hasNewEvents) {
            setEventsForSession(sessionKey, accumulated);
          }
          lastPolledGlobalCursor = detail.cursor;
          sessionCursorRef.current.set(sessionKey, {
            cursor,
            lastPolledGlobalCursor,
          });
        }
        setCommandsForSession(sessionKey, detail.commands);
        setSessionLoading(false);
        if (isTransientRemoteMessage(errorRef.current)) {
          setError("");
        }
        const resolvedBinding = detail.binding;
        if (resolvedBinding) {
          setSnapshot((prev) => {
            const idx = prev.sessions.findIndex(
              (s) =>
                s.runner_id === runnerId && s.binding.hub_session_id === hub,
            );
            const prevBinding =
              idx >= 0 ? prev.sessions[idx]?.binding : undefined;
            if (
              prevBinding &&
              (prevBinding.control_connected !==
                resolvedBinding.control_connected ||
                prevBinding.execution_state !==
                  resolvedBinding.execution_state ||
                prevBinding.current_turn_id !== resolvedBinding.current_turn_id)
            ) {
              const updated = [...prev.sessions];
              updated[idx] = {
                ...updated[idx]!,
                binding: resolvedBinding,
              };
              return { ...prev, sessions: updated };
            }
            return prev;
          });
        }
      } catch (err) {
        if (
          !stopped &&
          !(err instanceof Error && err.message === "request_aborted")
        ) {
          if (
            typeof document !== "undefined" &&
            document.hidden &&
            isTransientRemoteError(err)
          ) {
            // Keep background poll silent when tab is suspended or hidden
          } else {
            setError(remoteError(err));
          }
          setSessionLoading(false);
        }
      } finally {
        inFlight = false;
        // A command still waiting for its receipt is the one thing that
        // needs a fast read: the Runner answers it, and the receipt is the
        // conversation moving. A stored frame wakes this loop by itself, so
        // nothing else runs on a short timer.
        if (!stopped) {
          if (again) {
            void poll();
          } else {
            const hasPending = latestDetail?.commands?.some((c) => !c.receipt);
            const nextDelay = hasPending ? 200 : DETAIL_FALLBACK_MS;
            timer = setTimeout(() => void poll(), nextDelay);
          }
        }
      }
    };
    refreshSessionDetail.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    const unfollow = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "remote",
      onChange: (frames) => {
        if (!streamBatchAffectsSession(frames, runnerId, hub)) return;
        clearTimeout(timer);
        void poll();
      },
    });
    return () => {
      stopped = true;
      abortController.abort();
      clearTimeout(timer);
      unfollow();
      refreshSessionDetail.current = () => {};
    };
  }, [requestWithIdentity, selected, panelConfig?.gatewayBase]);
  const activeTurnId = session?.binding.current_turn_id ?? null;
  const items = useMemo(
    () => conversationItems(events, commands, `${selected}/`, activeTurnId),
    [events, commands, selected, activeTurnId],
  );
  const settledItems = useMemo(
    () => settledConversationItems(items, events, commands, activeTurnId),
    [items, events, commands, activeTurnId],
  );
  const effectiveItems = useMemo(() => {
    if (optimisticPrompts.length === 0) return items;
    const { visible } = pruneOptimisticPrompts(
      optimisticPrompts,
      settledItems,
      Date.now(),
    );
    const renderedIds = new Set(items.map((item) => item.id));
    const rejectedIds = new Set(
      commands
        .filter((c) => c.receipt && c.receipt.status !== "accepted")
        .map((c) => c.command.client_cmd_id),
    );
    const pendingVisible = visible.filter(
      (opt) => !renderedIds.has(opt.id) && !rejectedIds.has(opt.id),
    );

    if (pendingVisible.length === 0) return items;

    const optConversationItems: ConversationItem[] = pendingVisible.map(
      (opt) => ({
        id: opt.id,
        kind: "user",
        text: opt.text,
        status: "running",
      }),
    );

    return [...items, ...optConversationItems];
  }, [items, settledItems, commands, optimisticPrompts]);

  const itemGroups = useMemo(
    () => groupConversationItems(effectiveItems),
    [effectiveItems],
  );

  // A prompt leaves the queue when its row landed or when it outlived the TTL.
  // Both verdicts are per prompt: a prompt sent moments ago keeps its bubble
  // even when an older one expires in the same tick. The timer exists because
  // a send that silently died produces no new items at all — nothing would
  // re-run this effect, and the ghost would never leave the screen.
  useEffect(() => {
    if (optimisticPrompts.length === 0) return;
    const rejectedIds = new Set(
      commands
        .filter((c) => c.receipt && c.receipt.status !== "accepted")
        .map((c) => c.command.client_cmd_id),
    );
    const pruneKey = selected;
    if (!pruneKey) return;
    const tick = () => {
      const now = Date.now();
      updateOptimisticForSession(pruneKey, (prev) => {
        const { kept } = pruneOptimisticPrompts(prev, settledItems, now);
        const filtered = kept.filter((opt) => !rejectedIds.has(opt.id));
        return filtered.length === prev.length ? prev : filtered;
      });
    };
    tick();
    const timer = setInterval(tick, 1_000);
    return () => clearInterval(timer);
  }, [selected, settledItems, commands, optimisticPrompts]);
  const approvals = useMemo(
    () => pendingApprovals(events, session?.binding.hub_session_id ?? ""),
    [events, session?.binding.hub_session_id],
  );
  const questions = useMemo(
    () => pendingQuestions(events, session?.binding.hub_session_id ?? ""),
    [events, session?.binding.hub_session_id],
  );
  // The Runner only forwards an allowlisted error summary, so a bare
  // "执行出错" state is all the console used to be able to show. This surfaces
  // the cause the provider actually reported, including the common
  // "this model is not available on your account" case.
  const turnFailure = useMemo(
    () => latestTurnFailure(events, session?.binding.hub_session_id ?? ""),
    [events, session?.binding.hub_session_id],
  );
  // The skeleton stays until the assistant's own text starts arriving — not
  // until the tail stops being a user row. A turn that opens with a tool call
  // used to hide it the moment the tool started, which is exactly the stretch
  // where a person wonders whether the console is still listening.
  const lastUserIndex = effectiveItems.findLastIndex(
    (item) => item.kind === "user",
  );
  const answeredSincePrompt =
    lastUserIndex >= 0 &&
    effectiveItems
      .slice(lastUserIndex + 1)
      .some((item) => item.kind === "assistant");
  const isWaitingFirstAssistant = Boolean(
    (optimisticPrompts.length > 0 || isTurnExecuting) &&
    lastUserIndex >= 0 &&
    !answeredSincePrompt &&
    approvals.length === 0 &&
    questions.length === 0,
  );
  useEffect(() => {
    const confirmed = new Set(
      commands
        .filter((item) => item.receipt)
        .map((item) => item.command.client_cmd_id),
    );
    for (const [key, command] of attempts.current)
      if (confirmed.has(command.client_cmd_id)) attempts.current.delete(key);
  }, [commands]);

  const handleTimelineScroll = () => {
    const pane = timeline.current;
    if (!pane) return;
    const distanceFromBottom =
      pane.scrollHeight - pane.scrollTop - pane.clientHeight;
    setFollowBottom(distanceFromBottom < 80);
  };

  const scrollToBottom = () => {
    const pane = timeline.current;
    if (pane) {
      pane.scrollTo({ top: pane.scrollHeight, behavior: "smooth" });
    }
    setFollowBottom(true);
  };

  useEffect(() => {
    const pane = timeline.current;
    if (pane && followBottom) {
      pane.scrollTop = pane.scrollHeight;
    }
  }, [effectiveItems, isTurnExecuting, turnFailure, followBottom]);

  // History hydration never acquires native control.
  useEffect(() => {
    if (
      !session ||
      !session.online ||
      !codexSession ||
      sessionLoading ||
      items.length
    )
      return;
    const previewKey = key(session) + "/" + session.binding.host_generation;
    if (previewedHistory.current === previewKey) return;
    previewedHistory.current = previewKey;
    void submit(
      {
        action: "connect",
        mode: "preview",
        workspace_path: session.binding.workspace_path,
        provider_session_id: session.binding.provider_session_id,
      },
      session,
    );
  }, [session, codexSession, sessionLoading, items.length]);

  useEffect(() => {
    const draft = takeoverDrafts[selected];
    if (!draft) return;
    const receipt = commands.find(
      (item) => item.command.client_cmd_id === draft.id,
    )?.receipt;
    if (!receipt) return;
    forgetTakeoverDraft(selected);
    if (
      receipt.status === "accepted" &&
      textRef.current === draft.text &&
      attachmentsRef.current.length === draft.attachments.length &&
      attachmentsRef.current.every(
        (item, index) => item === draft.attachments[index],
      )
    ) {
      clearComposerInput();
    }
  }, [commands, selected, takeoverDrafts]);

  const lastCompletedCommand = [...commands]
    .reverse()
    .find((item) => item.receipt !== null);
  const latestFailure =
    lastCompletedCommand &&
    ["rejected", "indeterminate"].includes(
      lastCompletedCommand.receipt?.status ?? "",
    ) &&
    !["session_already_bound", "native_session_already_bound"].includes(
      lastCompletedCommand.receipt?.code ?? "",
    )
      ? lastCompletedCommand
      : undefined;
  const handleCopyText = (id: string, textToCopy: string) => {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(textToCopy);
      setCopiedApprovalId(id);
      setTimeout(() => {
        setCopiedApprovalId((current) => (current === id ? null : current));
      }, 2000);
    }
  };

  const forgetTakeoverDraft = (sessionKey: string) => {
    setTakeoverDrafts((previous) => {
      const next = { ...previous };
      delete next[sessionKey];
      return next;
    });
  };
  const submit = async (
    action: RemoteCommand["command"],
    target = session,
    clientCmdId?: string,
    rollbackDraft?: { text: string; attachments: RemoteAttachment[] },
  ) => {
    if (!target) return;
    const targetKey = key(target);
    setSessionBusy(targetKey, 1);
    if (action.action === "resolve_approval") {
      setSubmittingApproval({
        id: action.approval_id,
        action: action.approved ? "approve" : "decline",
      });
    } else if (action.action === "answer_question") {
      setSubmittingQuestionId(action.question_id);
    } else if (action.action === "interrupt") {
      setSubmittingInterrupt(true);
    }
    setError("");
    const targetRunner = runners.find(
      (item) => item.runner_id === target.runner_id,
    );
    const activeGeneration =
      targetRunner?.hello?.host_generation ?? target.binding.host_generation;
    const attemptKey = buildCommandAttemptKey(
      target.runner_id,
      target.binding.hub_session_id,
      action,
      activeGeneration,
    );
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: clientCmdId ?? safeRandomUUID(),
        hub_session_id: target.binding.hub_session_id,
        host_generation: activeGeneration,
        command: action,
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    if (
      action.action === "send_prompt" &&
      action.acquire_control &&
      rollbackDraft
    ) {
      setTakeoverDrafts((previous) => ({
        ...previous,
        [targetKey]: { id: command.client_cmd_id, ...rollbackDraft },
      }));
      if (clientCmdId !== command.client_cmd_id) {
        updateOptimisticForSession(targetKey, (previous) =>
          previous.map((entry) =>
            entry.id === clientCmdId
              ? { ...entry, id: command.client_cmd_id }
              : entry,
          ),
        );
      }
    }
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(target.runner_id)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      if (action.action === "send_prompt") {
        setDismissedFailure(null);
      }
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
      if (!(err instanceof Error) || err.name === "RemoteRequestRejected") {
        attempts.current.delete(attemptKey);
      }
      if (action.action === "send_prompt") {
        if (action.acquire_control) forgetTakeoverDraft(targetKey);
        if (clientCmdId) {
          updateOptimisticForSession(targetKey, (prev) =>
            prev.filter((opt) => opt.id !== command.client_cmd_id),
          );
        }
        if (rollbackDraft) {
          if (selectedRef.current === targetKey) {
            if (
              !textRef.current.trim() &&
              attachmentsRef.current.length === 0
            ) {
              setText(rollbackDraft.text);
              setAttachments([...rollbackDraft.attachments]);
            }
          } else if (!draftsBySessionRef.current[targetKey]?.text) {
            draftsBySessionRef.current[targetKey] = {
              text: rollbackDraft.text,
              attachments: [...rollbackDraft.attachments],
            };
          }
        }
      }
      setError(remoteError(err));
    } finally {
      setSessionBusy(targetKey, -1);
      setSubmittingApproval(null);
      setSubmittingQuestionId(null);
      setSubmittingInterrupt(false);
    }
  };

  const dispatchPromptPayload = (
    rawText: string,
    rawAttachments: RemoteAttachment[],
    acquireControl = false,
  ) => {
    if (!session || (!rawText.trim() && rawAttachments.length === 0)) return;
    const targetKey = key(session);
    wasInterruptedBySessionRef.current[targetKey] = false;
    const sessionRunner = runners.find(
      (item) => item.runner_id === session.runner_id,
    );
    const supportsModernCommandFields = Boolean(
      sessionRunner?.hello?.models?.agents?.claude_desktop !== undefined ||
      sessionRunner?.hello?.models?.agents?.antigravity !== undefined ||
      sessionRunner?.hello?.discovery?.candidates?.some(
        (c) => c.context_window_tokens !== undefined,
      ),
    );
    let promptText =
      rawText.trim() ||
      `请查看附件：${rawAttachments.map((a) => a.name).join("、")}`;
    if (!supportsModernCommandFields && rawAttachments.length > 0) {
      const inlineParts = rawAttachments.map((att) =>
        att.encoding === "utf8"
          ? `[附件 ${att.name}]:\n\`\`\`\n${att.data.slice(0, 64 * 1024)}\n\`\`\``
          : `[附件 ${att.name} (${att.media_type})]`,
      );
      promptText = `${inlineParts.join("\n\n")}\n\n${promptText}`;
    }

    const clientCmdId = safeRandomUUID();
    const sentAttachments = [...rawAttachments];
    const userRowCount = settledItems.filter(
      (item) => item.kind === "user",
    ).length;
    const neededUserRows = nextOptimisticPromptNeeded(
      pendingOptimisticPrompts(optimisticPrompts, settledItems).map(
        (opt) => opt.neededUserRows,
      ),
      userRowCount,
    );
    updateOptimisticForSession(targetKey, (prev) => [
      ...prev,
      {
        id: clientCmdId,
        text: promptText,
        createdAt: Date.now(),
        neededUserRows,
      },
    ]);
    setFollowBottom(true);
    setDismissedFailure(null);

    const shouldSendPermissionMode =
      permissionMode !== "default" || permissionModeTouched;
    void submit(
      {
        action: "send_prompt",
        text: promptText,
        ...(acquireControl ? { acquire_control: true } : {}),
        ...(effort !== "default" ? { effort } : {}),
        ...(sessionModel ? { model: sessionModel } : {}),
        ...(shouldSendPermissionMode
          ? { permission_mode: permissionMode }
          : {}),
        ...(supportsModernCommandFields && sentAttachments.length > 0
          ? { attachments: sentAttachments }
          : {}),
      },
      session,
      clientCmdId,
      { text: rawText, attachments: sentAttachments },
    );
  };

  const clearComposerInput = () => {
    setText("");
    setAttachments([]);
    if (selectedRef.current) {
      delete draftsBySessionRef.current[selectedRef.current];
    }
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      textareaRef.current.style.overflowY = "hidden";
    }
  };

  const enqueueCurrentPrompt = () => {
    if (!selected || !connected || (!text.trim() && attachments.length === 0))
      return;
    const draftText = text;
    const draftAttachments = [...attachments];
    clearComposerInput();
    setQueuedPrompts((prev) => {
      const existing = prev[selected];
      const mergedText = existing
        ? [existing.text.trim(), draftText.trim()].filter(Boolean).join("\n")
        : draftText;
      const mergedAttachments = existing
        ? [...existing.attachments, ...draftAttachments].slice(0, 16)
        : draftAttachments;
      return {
        ...prev,
        [selected]: {
          text: mergedText,
          attachments: mergedAttachments,
          queuedAt: Date.now(),
        },
      };
    });
  };

  const sendCurrentPrompt = () => {
    if (busy) return;
    if (!text.trim() && attachments.length === 0) return;
    if (isTurnExecuting && connected) {
      enqueueCurrentPrompt();
      return;
    }
    if (!canSend) return;
    const draftText = text;
    const draftAttachments = [...attachments];
    if (!canTakeControl) clearComposerInput();
    dispatchPromptPayload(draftText, draftAttachments, canTakeControl);
  };

  const activeQueuedPrompt = selected ? queuedPrompts[selected] : undefined;

  useEffect(() => {
    if (!selected) return;
    const switched = lastCheckedQueueSessionRef.current !== selected;
    lastCheckedQueueSessionRef.current = selected;
    const previouslyExecuting =
      wasTurnExecutingBySessionRef.current[selected] === true;
    wasTurnExecutingBySessionRef.current[selected] = isTurnExecuting;
    if (switched || !previouslyExecuting || isTurnExecuting) return;
    const queued = queuedPrompts[selected];
    if (!queued) return;
    const hasFailed =
      Boolean(turnFailure && !turnFailure.retrying) ||
      session?.binding.execution_state === "error" ||
      session?.binding.execution_state === "failed" ||
      session?.binding.execution_state === "interrupted" ||
      wasInterruptedBySessionRef.current[selected] === true;
    if (hasFailed || !connected || !canSend) return;
    setQueuedPrompts((prev) => {
      const next = { ...prev };
      delete next[selected];
      return next;
    });
    dispatchPromptPayload(queued.text, queued.attachments);
  });

  const launch = async () => {
    const launchRunner = runners.find((item) => item.runner_id === runner);
    const generation = launchRunner?.hello?.host_generation;
    if (!runner || !workspace || !generation) return;
    setLaunchBusy(true);
    setError("");
    const effectiveSandbox =
      !nativeId && agentType !== "codex"
        ? launchPermissionMode === "plan"
          ? "read-only"
          : "workspace-write"
        : sandbox;
    const attemptKey = JSON.stringify([
      "launch",
      runner,
      workspace,
      nativeId,
      agentType,
      effectiveSandbox,
      launchPermissionMode,
      model,
    ]);
    const existingSession = nativeId
      ? snapshot.sessions.find(
          (s) =>
            s.runner_id === runner &&
            s.binding.provider_session_id === nativeId,
        )
      : undefined;
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id:
          existingSession?.binding.hub_session_id ?? safeRandomUUID(),
        host_generation: generation,
        command: {
          action: "connect",
          mode: nativeId
            ? launchRunner?.codex_session_control &&
              isCodexThreadId(nativeId.trim())
              ? "take_control"
              : "resume"
            : "create",
          workspace_path: workspace,
          sandbox: effectiveSandbox,
          approval_policy:
            launchPermissionMode === "bypassPermissions"
              ? "never"
              : launchPermissionMode === "acceptEdits"
                ? "on-request"
                : "untrusted",
          ...(launchPermissionMode !== "default"
            ? { permission_mode: launchPermissionMode }
            : {}),
          ...(model ? { model } : {}),
          ...(nativeId
            ? { provider_session_id: nativeId }
            : { agent_type: agentType }),
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(runner)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelectedRunnerId(runner);
      if (!nativeId) {
        const nextProvider: ProviderKind =
          agentType === "claude_code"
            ? "claude"
            : agentType === "antigravity"
              ? "antigravity"
              : agentType === "codex"
                ? "codex"
                : agentType;
        userSelectedProviderRef.current = true;
        setProviderFilter(nextProvider);
      }
      setPermissionMode(launchPermissionMode);
      setPermissionModeTouched(launchPermissionMode !== "default");
      setSelected(`${runner}/${command.hub_session_id}`);
      setLaunchModalOpen(false);
      if (isMobile) setMobileDrawerOpen(false);
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setLaunchBusy(false);
    }
  };
  const resumeCandidate = async (
    targetRunnerId: string,
    targetWorkspace: string,
    providerSessionId: string,
  ) => {
    const runnerObj = runners.find((item) => item.runner_id === targetRunnerId);
    const generation = runnerObj?.hello?.host_generation;
    if (!runnerObj || !generation || launchBusy) return;
    const candidateKey = `${targetRunnerId}/${providerSessionId}`;
    setResumingCandidateKey(candidateKey);
    setLaunchBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      targetRunnerId,
      targetWorkspace,
      providerSessionId,
      "read-only",
      "",
    ]);
    const existingSession = snapshot.sessions.find(
      (s) =>
        s.runner_id === targetRunnerId &&
        s.binding.provider_session_id === providerSessionId,
    );
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id:
          existingSession?.binding.hub_session_id ??
          (isCodexThreadId(providerSessionId)
            ? providerSessionId
            : safeRandomUUID()),
        host_generation: generation,
        command: {
          action: "connect",
          mode:
            runnerObj.codex_session_control &&
            isCodexThreadId(providerSessionId)
              ? "preview"
              : "resume",
          workspace_path: targetWorkspace,
          sandbox: "read-only",
          approval_policy: "untrusted",
          provider_session_id: providerSessionId,
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(targetRunnerId)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelected(`${targetRunnerId}/${command.hub_session_id}`);
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setLaunchBusy(false);
      setResumingCandidateKey(null);
    }
  };

  // Each tree level collapses independently, so the open path down to the
  // selected session survives switching agents or devices.
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<
    Record<string, boolean>
  >({});
  // Progressive session loading: initial limit per workspace with incremental "load more"
  const [workspaceVisibleCounts, setWorkspaceVisibleCounts] = useState<
    Record<string, number>
  >({});

  const toggleNode = (
    setter: React.Dispatch<React.SetStateAction<Record<string, boolean>>>,
    nodeKey: string,
  ) => setter((prev) => ({ ...prev, [nodeKey]: !prev[nodeKey] }));

  const deviceGroups = useMemo(
    () =>
      buildSessionTree({
        discovered,
        devices: runners
          .filter((runner) => !runner.revoked)
          .map((runner) => ({
            runner_id: runner.runner_id,
            online: runner.online,
            hello: runner.hello as {
              device_name?: unknown;
            } | null,
          })),
        activeSessions: snapshot.sessions.map((item) => ({
          runner_id: item.runner_id,
          hub_session_id: item.binding.hub_session_id,
          provider_session_id: item.binding.provider_session_id,
          workspace_path: item.binding.workspace_path,
          title: discovered.get(
            `${item.runner_id}/${item.binding.provider_session_id}`,
          )?.title,
          provider: item.provider,
          online: item.online,
          control_connected: item.binding.control_connected,
          execution_state: item.binding.execution_state,
        })),
        candidates: runners
          .filter((runner) => !runner.revoked)
          .flatMap((runner) =>
            (runner.hello?.discovery?.candidates ?? []).map((candidate) => ({
              runner_id: runner.runner_id,
              provider_session_id: candidate.provider_session_id,
              workspace_path: candidate.workspace_path,
              title: candidate.title,
              provider: candidate.provider,
              resumable: candidate.resumable,
              model: candidate.model,
              total_tokens: candidate.total_tokens,
              last_activity_at_ms: candidate.last_activity_at_ms,
            })),
          ),
        providerFilter,
        plugins: allKnownPlugins,
      }),
    [snapshot.sessions, runners, providerFilter, discovered, allKnownPlugins],
  );

  const [sessionSearchQuery, setSessionSearchQuery] = useState("");
  const [copiedSessionId, setCopiedSessionId] = useState<string | null>(null);
  const copyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    const onGlobalKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (settingsOpen) setSettingsOpen(false);
        if (!isMobile && sidebarCollapsed) toggleSidebar();
        if (isMobile && !mobileDrawerOpen) {
          openMobileDrawer();
        }
        setSearchOpen(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
        return;
      }
      if (
        (e.ctrlKey || e.metaKey) &&
        e.key.toLowerCase() === "b" &&
        !isMobile
      ) {
        e.preventDefault();
        toggleSidebar();
        return;
      }
      if (e.key === "Escape") {
        if (launchModalOpen) {
          setLaunchModalOpen(false);
          return;
        }
        if (settingsOpen) {
          setSettingsOpen(false);
          return;
        }
        if (searchOpen || sessionSearchQuery) {
          setSessionSearchQuery("");
          setSearchOpen(false);
        }
      }
    };
    window.addEventListener("keydown", onGlobalKeyDown);
    return () => window.removeEventListener("keydown", onGlobalKeyDown);
  }, [
    launchModalOpen,
    settingsOpen,
    searchOpen,
    sessionSearchQuery,
    isMobile,
    sidebarCollapsed,
    mobileDrawerOpen,
  ]);

  const handleCopySessionId = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(id);
      setCopiedSessionId(id);
      if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
      copyTimeoutRef.current = setTimeout(() => {
        setCopiedSessionId((cur) => (cur === id ? null : cur));
      }, 1500);
    }
  };

  const filteredDeviceGroups = useMemo(() => {
    const q = sessionSearchQuery.trim().toLowerCase();
    if (!q) return deviceGroups;

    return deviceGroups
      .map((dev) => {
        const matchesDev = dev.label.toLowerCase().includes(q);
        const filteredAgents = dev.workspaces
          .map((agent) => {
            const matchesAgent = agent.label.toLowerCase().includes(q);
            const filteredWorkspaces = agent.workspaces
              .map((ws) => {
                const matchesWs = ws.label.toLowerCase().includes(q);
                const filteredItems = ws.items.filter((item) => {
                  if (matchesDev || matchesAgent || matchesWs) return true;
                  const itemKey =
                    item.kind === "active" ? item.sessionKey : item.key;
                  return (
                    item.title.toLowerCase().includes(q) ||
                    itemKey.toLowerCase().includes(q)
                  );
                });
                if (filteredItems.length === 0 && !matchesWs) return null;
                return {
                  ...ws,
                  items: filteredItems,
                };
              })
              .filter((ws): ws is NonNullable<typeof ws> => ws !== null);

            if (filteredWorkspaces.length === 0 && !matchesAgent) return null;
            return {
              ...agent,
              totalCount: filteredWorkspaces.reduce(
                (sum, w) => sum + w.items.length,
                0,
              ),
              workspaces: filteredWorkspaces,
            };
          })
          .filter(
            (agent): agent is NonNullable<typeof agent> => agent !== null,
          );

        if (filteredAgents.length === 0 && !matchesDev) return null;
        return {
          ...dev,
          totalCount: filteredAgents.reduce((sum, a) => sum + a.totalCount, 0),
          workspaces: filteredAgents,
        };
      })
      .filter((dev): dev is NonNullable<typeof dev> => dev !== null);
  }, [deviceGroups, sessionSearchQuery]);

  const deviceList = useMemo(() => {
    return runners
      .filter((r) => !r.revoked)
      .map((r) => ({
        runnerId: r.runner_id,
        label: getDeviceLabel(r.runner_id, r.hello),
        online: r.online,
        workspaces: r.hello?.workspaces ?? [],
      }))
      .sort((a, b) => {
        if (a.online !== b.online) return a.online ? -1 : 1;
        return a.label.localeCompare(b.label, "zh-Hans-CN");
      });
  }, [runners]);

  const activeRunnerId = useMemo(() => {
    if (
      selectedRunnerId &&
      deviceList.some((d) => d.runnerId === selectedRunnerId)
    ) {
      return selectedRunnerId;
    }
    const firstOnline = deviceList.find((d) => d.online);
    if (firstOnline) return firstOnline.runnerId;
    return deviceList[0]?.runnerId ?? "";
  }, [selectedRunnerId, deviceList]);

  const activeDevice = useMemo(() => {
    return deviceList.find((d) => d.runnerId === activeRunnerId);
  }, [deviceList, activeRunnerId]);

  const activeSnapshotRunner = useMemo(
    () => snapshot.runners.find((r) => r.runner_id === activeRunnerId),
    [snapshot.runners, activeRunnerId],
  );

  const isActiveRunnerHelloPending = isRunnerHelloBlocking(
    activeRunnerId ? helloEntries[activeRunnerId] : undefined,
    activeSnapshotRunner,
  );

  /**
   * Available Agents derived from the currently selected computer's runtime.
   * "先选电脑再有 agent（根据所选电脑 runtime 返回结果列出选项）"
   * 严禁硬编码！无电脑时返回空，有电脑时严格依据该电脑上报的活跃会话与发现候选动态提取。
   */
  const availableAgentsForCurrentDevice = useMemo(() => {
    if (!activeRunnerId) {
      return [];
    }

    const map = new Map<
      ProviderKind,
      { type: ProviderKind; name: string; count: number }
    >();

    const countedKeys = new Set<string>();

    // 1. 该电脑当前的活跃会话
    for (const s of snapshot.sessions) {
      if (s.runner_id !== activeRunnerId) continue;
      const key = `${s.runner_id}/${s.binding.provider_session_id}`;
      countedKeys.add(key);
      const disc = discovered.get(key);
      const info = getProviderInfo(
        s.binding.provider_session_id,
        disc?.provider ?? s.provider,
      );
      const existing = map.get(info.type);
      map.set(info.type, {
        type: info.type,
        name: info.name,
        count: (existing?.count ?? 0) + 1,
      });
    }

    // 2. 该电脑 Runner runtime 探测上报的 candidates
    const activeRunner = runners.find(
      (r) => r.runner_id === activeRunnerId && !r.revoked,
    );
    for (const c of activeRunner?.hello?.discovery?.candidates ?? []) {
      const key = `${activeRunnerId}/${c.provider_session_id}`;
      if (countedKeys.has(key)) continue;
      countedKeys.add(key);
      const info = getProviderInfo(c.provider_session_id, c.provider);
      const existing = map.get(info.type);
      map.set(info.type, {
        type: info.type,
        name: info.name,
        count: (existing?.count ?? 0) + 1,
      });
    }

    // 3. 该电脑 Runner hello 自身声明的默认 provider（如果有且未被加入）
    const runnerHello = activeRunner?.hello as
      Record<string, unknown> | null | undefined;
    if (
      runnerHello &&
      typeof runnerHello.provider === "string" &&
      runnerHello.provider.trim()
    ) {
      const info = getProviderInfo("", runnerHello.provider.trim());
      if (!map.has(info.type)) {
        map.set(info.type, {
          type: info.type,
          name: info.name,
          count: 0,
        });
      }
    }

    // 动态排序：有活跃会话的排前面，同等按名称拼音字母排序；决不注入任何硬编码列表
    return Array.from(map.values()).sort((a, b) => {
      if (a.count !== b.count) return b.count - a.count;
      return a.name.localeCompare(b.name, "zh-Hans-CN");
    });
  }, [activeRunnerId, runners, snapshot.sessions, discovered]);

  const currentAgentInfo = useMemo(() => {
    if (!activeRunnerId) {
      return {
        type: "" as ProviderKind,
        name: "请先选择电脑",
        count: 0,
      };
    }
    if (isActiveRunnerHelloPending) {
      return {
        type: "" as ProviderKind,
        name: "正在同步 Agent…",
        count: 0,
      };
    }
    const found = availableAgentsForCurrentDevice.find(
      (a) => a.type === providerFilter,
    );
    if (found) return found;
    if (availableAgentsForCurrentDevice.length > 0) {
      return availableAgentsForCurrentDevice[0]!;
    }
    return {
      type: "" as ProviderKind,
      name: "暂无可用 Agent",
      count: 0,
    };
  }, [
    activeRunnerId,
    isActiveRunnerHelloPending,
    availableAgentsForCurrentDevice,
    providerFilter,
  ]);

  // 级联规则：等待当前电脑首次完整 hello 落地后再确定默认 Agent，避免先锁定在残缺/盲猜分类上
  useEffect(() => {
    if (
      !activeRunnerId ||
      isActiveRunnerHelloPending ||
      availableAgentsForCurrentDevice.length === 0
    ) {
      return;
    }
    const exists = availableAgentsForCurrentDevice.some(
      (a) => a.type === providerFilter,
    );
    if (!exists || (!userSelectedProviderRef.current && !selected)) {
      const first = availableAgentsForCurrentDevice[0];
      if (first && first.type !== providerFilter) {
        setProviderFilter(first.type);
      }
    }
  }, [
    activeRunnerId,
    isActiveRunnerHelloPending,
    availableAgentsForCurrentDevice,
    providerFilter,
    selected,
  ]);

  const currentDeviceGroup = useMemo(() => {
    return filteredDeviceGroups.find((d) => d.runnerId === activeRunnerId);
  }, [filteredDeviceGroups, activeRunnerId]);

  const currentAgentWorkspaces = useMemo(() => {
    const agent = currentDeviceGroup?.workspaces.find(
      (a) => a.provider === providerFilter,
    );
    return agent?.workspaces ?? [];
  }, [currentDeviceGroup, providerFilter]);

  const allWorkspacesForDevice = useMemo(() => {
    const map = new Map<
      string,
      { id: string; label: string; items: TreeSessionItem[] }
    >();
    for (const ws of currentAgentWorkspaces) {
      if (ws.items.length > 0) {
        map.set(ws.id, { id: ws.id, label: ws.label, items: [...ws.items] });
      }
    }
    return Array.from(map.values()).sort((a, b) => {
      const timeA = getWorkspaceActivityMs(a);
      const timeB = getWorkspaceActivityMs(b);
      if (timeA !== timeB) return timeB - timeA;
      return a.label.localeCompare(b.label, "zh-Hans-CN");
    });
  }, [currentAgentWorkspaces]);

  useEffect(() => {
    if (!deviceMenuOpen && !agentMenuOpen) return;
    const onClickOutside = (e: MouseEvent) => {
      if (
        deviceMenuOpen &&
        deviceMenuRef.current &&
        !deviceMenuRef.current.contains(e.target as Node)
      ) {
        setDeviceMenuOpen(false);
      }
      if (
        agentMenuOpen &&
        agentMenuRef.current &&
        !agentMenuRef.current.contains(e.target as Node)
      ) {
        setAgentMenuOpen(false);
      }
    };
    window.addEventListener("pointerdown", onClickOutside);
    return () => window.removeEventListener("pointerdown", onClickOutside);
  }, [deviceMenuOpen, agentMenuOpen]);

  // Escape closes any open menu and hands the focus back to the button
  // that opened it, so the keyboard never ends up on a closed popup.
  useEffect(() => {
    if (!deviceMenuOpen && !agentMenuOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (deviceMenuOpen) {
        setDeviceMenuOpen(false);
        deviceMenuButtonRef.current?.focus();
      }
      if (agentMenuOpen) {
        setAgentMenuOpen(false);
        agentMenuButtonRef.current?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [deviceMenuOpen, agentMenuOpen]);

  const lastSelectedForSyncRef = useRef<string>("");

  useEffect(() => {
    if (!selected) {
      lastSelectedForSyncRef.current = "";
      return;
    }
    if (selected === lastSelectedForSyncRef.current) return;
    lastSelectedForSyncRef.current = selected;

    const s = snapshot.sessions.find((entry) => key(entry) === selected);
    if (s && s.runner_id && s.runner_id !== selectedRunnerId) {
      setSelectedRunnerId(s.runner_id);
    }
    if (s) {
      const disc = discovered.get(
        `${s.runner_id}/${s.binding.provider_session_id}`,
      );
      const info = getProviderInfo(
        s.binding.provider_session_id,
        disc?.provider ?? s.provider,
      );
      if (info.type) {
        setProviderFilter(info.type);
      }
    }
  }, [selected, snapshot.sessions, selectedRunnerId, discovered]);

  /**
   * Pending approvals from every computer, keyed `${runnerId}/${hubSessionId}`.
   *
   * This is the Host's own snapshot rather than the open session's event
   * window, which is what makes it correct for sessions this browser is not
   * attached to — the whole point of the strip above the device selector.
   */
  const pendingApprovalsBySessionKey = useMemo(
    () => groupPendingApprovals(runners),
    [runners],
  );

  const pendingApprovalCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const [k, list] of pendingApprovalsBySessionKey) {
      counts.set(k, list.length);
    }
    return counts;
  }, [pendingApprovalsBySessionKey]);

  /**
   * Sessions blocked on a person, across every computer.
   *
   * Built from `deviceGroups`, not from the device-scoped view below: the
   * strip exists to show what the drill-down hides.
   */
  const attention = useMemo(
    () => attentionEntries(deviceGroups, pendingApprovalsBySessionKey),
    [deviceGroups, pendingApprovalsBySessionKey],
  );

  /**
   * The folder path to the selected session, so the user can always see where
   * they are in a four-level tree and step back out of it.
   *
   * The tree is shown as a device selector plus an agent filter, so a
   * breadcrumb step navigates one of those rather than expanding a node.
   */
  const selectedSessionPath = useMemo(() => {
    if (!selected) return undefined;
    for (const device of deviceGroups) {
      for (const agent of device.workspaces) {
        for (const workspace of agent.workspaces) {
          for (const item of workspace.items) {
            if (item.kind !== "active" || item.sessionKey !== selected) {
              continue;
            }
            return [
              {
                key: device.runnerId,
                label: device.label,
                runnerId: device.runnerId,
              },
              {
                key: `${device.runnerId}/${agent.provider}`,
                label: agent.label,
                runnerId: device.runnerId,
                provider: agent.provider,
              },
              {
                key: `${device.runnerId}/${agent.provider}/${workspace.id}`,
                label: workspace.label,
                runnerId: device.runnerId,
                provider: agent.provider,
              },
              {
                key: item.key,
                label: item.title,
                sessionKey: selected,
              },
            ];
          }
        }
      }
    }
    return undefined;
  }, [deviceGroups, selected, snapshot.sessions]);

  const openLaunchModal = (preferredWorkspace?: string) => {
    const available = runners.find(
      (item) => !item.revoked && item.online && item.hello?.workspaces.length,
    );
    const nextRunnerId =
      activeRunnerId || available?.runner_id || runners[0]?.runner_id || "";
    const rInfo = runners.find(
      (r) => !r.revoked && r.runner_id === nextRunnerId,
    );
    const authorized = rInfo?.hello?.workspaces ?? [];
    const nextWorkspace =
      preferredWorkspace && authorized.includes(preferredWorkspace)
        ? preferredWorkspace
        : (authorized[0] ?? "");
    const mappedAgent: LaunchAgentType =
      providerFilter === "claude" || providerFilter === "claude_desktop"
        ? "claude_code"
        : providerFilter === "antigravity"
          ? "antigravity"
          : providerFilter !== "all" && providerFilter !== "codex"
            ? providerFilter
            : "codex";
    if (availableLaunchAgents.some((a) => a.value === mappedAgent)) {
      setAgentType(mappedAgent);
    } else if (availableLaunchAgents[0]) {
      setAgentType(availableLaunchAgents[0].value as LaunchAgentType);
    }
    setRunner(nextRunnerId);
    setWorkspace(nextWorkspace);
    setNativeId("");
    setModel("");
    if (isMobile) setMobileDrawerOpen(false);
    setLaunchModalOpen(true);
  };

  const reconnectActiveSession = (activeSession: RemoteSession) => {
    void submit(
      {
        action: "connect",
        mode:
          supportsCodexControl(activeSession.runner_id) &&
          isCodexThreadId(activeSession.binding.provider_session_id)
            ? "take_control"
            : "resume",
        workspace_path: activeSession.binding.workspace_path,
        provider_session_id: activeSession.binding.provider_session_id,
      },
      activeSession,
    );
  };

  return (
    <div className="remote-console" ref={consoleRootRef}>
      {/* 移动端抽屉遮罩 */}
      {isMobile && (
        <div
          className={`remote-drawer-backdrop ${mobileDrawerOpen ? "open" : ""}`}
          onClick={() => closeMobileDrawer(true)}
          aria-hidden="true"
        />
      )}
      <aside
        className={`remote-sidebar ${
          isMobile
            ? mobileDrawerOpen
              ? "drawer-open"
              : ""
            : sidebarCollapsed
              ? "collapsed"
              : ""
        }`}
        data-drawer-animated={drawerAnimated ? "true" : undefined}
        ref={mobileDrawerRef}
        role={isMobile ? "dialog" : undefined}
        aria-modal={isMobile && mobileDrawerOpen ? "true" : undefined}
        aria-label={isMobile ? "会话导航抽屉" : "会话侧边栏"}
        aria-hidden={isMobile && !mobileDrawerOpen ? true : undefined}
        inert={isMobile && !mobileDrawerOpen ? true : undefined}
        onTouchStart={onSidebarTouchStart}
        onTouchMove={onSidebarTouchMove}
        onTouchEnd={onSidebarTouchEnd}
        style={
          isMobile
            ? undefined
            : {
                width: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
                minWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
                maxWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
              }
        }
      >
        <div className="remote-sidebar-header">
          <div className="remote-brand">
            AgentHub <span>Remote</span>
          </div>
          <div className="remote-header-actions">
            <button
              type="button"
              className={`remote-header-icon-btn ${
                searchOpen || sessionSearchQuery ? "active" : ""
              }`}
              title={searchOpen ? "收起搜索 (Ctrl+K)" : "搜索会话 (Ctrl+K)"}
              aria-label="搜索会话"
              onClick={() => setSearchOpen((prev) => !prev)}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <circle cx="11" cy="11" r="8" />
                <line x1="21" y1="21" x2="16.65" y2="16.65" />
              </svg>
            </button>

            <button
              type="button"
              className="remote-header-icon-btn"
              title="新建会话"
              aria-label="新建会话"
              onClick={() => openLaunchModal()}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
            </button>

            {isMobile ? (
              <button
                type="button"
                className="remote-header-icon-btn"
                title="关闭菜单"
                aria-label="关闭菜单"
                onClick={() => closeMobileDrawer(true)}
              >
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            ) : (
              <button
                type="button"
                className="remote-header-icon-btn"
                title="收起侧边栏 (Ctrl+B)"
                aria-label="收起侧边栏"
                onClick={toggleSidebar}
              >
                <svg
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="9" y1="3" x2="9" y2="21" />
                  <path d="M15 9l-3 3 3 3" />
                </svg>
              </button>
            )}
          </div>
        </div>

        {(searchOpen || sessionSearchQuery) && (
          <div className="remote-sidebar-search-compact">
            <svg
              className="remote-search-icon"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              aria-hidden="true"
            >
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              ref={searchInputRef}
              type="text"
              className="remote-sidebar-search-input"
              placeholder="搜索会话或工作区…"
              value={sessionSearchQuery}
              autoFocus
              onChange={(e) => setSessionSearchQuery(e.target.value)}
            />
            {sessionSearchQuery && (
              <button
                type="button"
                className="remote-search-clear"
                title="清空搜索"
                aria-label="清空搜索"
                onClick={() => setSessionSearchQuery("")}
              >
                <svg
                  width="12"
                  height="12"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.5"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            )}
          </div>
        )}

        <AttentionQueue
          entries={attention}
          approvalsBySession={pendingApprovalsBySessionKey}
          submitting={submittingApproval}
          onOpen={(entry) => {
            setSelectedRunnerId(entry.runnerId);
            setSelected(entry.sessionKey);
            setError("");
            if (isMobile) setMobileDrawerOpen(false);
          }}
          onResolve={(entry, approval, approved) => {
            const target = snapshot.sessions.find(
              (item) => key(item) === entry.sessionKey,
            );
            if (target === undefined) {
              setError("该会话已不在当前快照中，请刷新后重试");
              return;
            }
            void submit(
              {
                action: "resolve_approval",
                approval_id: approval.approval_id,
                approved,
              },
              target,
            );
          }}
        />

        <div className="remote-selectors-bar">
          {/* 电脑选择器 */}
          <div className="remote-selector-dropdown" ref={deviceMenuRef}>
            <button
              type="button"
              className={`remote-selector-btn ${deviceMenuOpen ? "active" : ""}`}
              ref={deviceMenuButtonRef}
              onClick={() => {
                setDeviceMenuOpen((prev) => !prev);
                setAgentMenuOpen(false);
              }}
              aria-expanded={deviceMenuOpen}
              aria-haspopup="menu"
              title={`${activeDevice?.label ?? "未连接设备"} · 点击切换电脑`}
            >
              <span className="remote-selector-btn-left">
                <span className="remote-selector-btn-icon" aria-hidden="true">
                  <svg
                    width="15"
                    height="15"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.9"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                    <line x1="8" y1="21" x2="16" y2="21" />
                    <line x1="12" y1="17" x2="12" y2="21" />
                  </svg>
                </span>
                <span className="remote-selector-btn-text">
                  {activeDevice?.label ??
                    (activeRunnerId
                      ? `电脑 · ${activeRunnerId.slice(-6)}`
                      : "选择电脑")}
                </span>
              </span>
              <span className="remote-selector-btn-right">
                <span
                  className={`remote-status-dot ${
                    activeDevice?.online ? "online" : "offline"
                  }`}
                  title={activeDevice?.online ? "电脑在线" : "电脑离线"}
                />
                <span
                  className={`remote-selector-arrow ${
                    deviceMenuOpen ? "open" : ""
                  }`}
                  aria-hidden="true"
                >
                  <ChevronDownIcon size={13} />
                </span>
              </span>
            </button>

            {deviceMenuOpen && (
              <div
                className="remote-selector-popover remote-device-popover"
                role="menu"
              >
                <div className="remote-popover-header">选择电脑</div>
                <div className="remote-popover-list">
                  {deviceList.length === 0 ? (
                    <div className="remote-popover-empty">暂无可用电脑</div>
                  ) : (
                    deviceList.map((dev) => (
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={dev.runnerId === activeRunnerId}
                        className={`remote-popover-item ${
                          dev.runnerId === activeRunnerId ? "selected" : ""
                        }`}
                        key={dev.runnerId}
                        onClick={() => {
                          if (dev.runnerId !== activeRunnerId) {
                            setSelected("");
                            userSelectedProviderRef.current = false;
                          }
                          setSelectedRunnerId(dev.runnerId);
                          setDeviceMenuOpen(false);
                        }}
                      >
                        <span
                          className={`remote-status-dot ${
                            dev.online ? "online" : "offline"
                          }`}
                        />
                        <span className="remote-popover-item-name">
                          {dev.label}
                        </span>
                        <span className="remote-popover-item-badge">
                          {dev.online ? "在线" : "离线"}
                        </span>
                        {dev.runnerId === activeRunnerId && (
                          <svg
                            className="remote-popover-check"
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2.5"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <polyline points="20 6 9 17 4 12" />
                          </svg>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>
            )}
          </div>

          {/* Agent 选择器 (级联于当前电脑) */}
          <div className="remote-selector-dropdown" ref={agentMenuRef}>
            {(() => {
              const isAgentSelectorDisabled =
                !activeRunnerId ||
                isActiveRunnerHelloPending ||
                availableAgentsForCurrentDevice.length === 0;
              return (
                <button
                  type="button"
                  disabled={isAgentSelectorDisabled}
                  className={`remote-selector-btn ${agentMenuOpen ? "active" : ""} ${
                    isAgentSelectorDisabled ? "disabled" : ""
                  }`}
                  ref={agentMenuButtonRef}
                  onClick={() => {
                    if (isAgentSelectorDisabled) return;
                    setAgentMenuOpen((prev) => !prev);
                    setDeviceMenuOpen(false);
                  }}
                  aria-expanded={agentMenuOpen}
                  aria-haspopup="menu"
                  aria-disabled={isAgentSelectorDisabled}
                  title={
                    !activeRunnerId
                      ? "请先选择电脑"
                      : isActiveRunnerHelloPending
                        ? "正在同步当前电脑的 Agent 与会话列表…"
                        : availableAgentsForCurrentDevice.length === 0
                          ? "当前电脑未检测到可用 Agent"
                          : `${currentAgentInfo.name} · 点击切换 Agent`
                  }
                >
                  <span className="remote-selector-btn-left">
                    <span
                      className="remote-selector-btn-icon"
                      aria-hidden="true"
                    >
                      <svg
                        width="15"
                        height="15"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.9"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z" />
                        <path d="M5 3v4M19 17v4M3 5h4M17 19h4" />
                      </svg>
                    </span>
                    <span className="remote-selector-btn-text">
                      {currentAgentInfo.name}
                    </span>
                  </span>
                  <span className="remote-selector-btn-right">
                    {currentAgentInfo.count > 0 && (
                      <span
                        className="remote-selector-count-badge"
                        title={`${currentAgentInfo.count} 个会话`}
                      >
                        {currentAgentInfo.count}
                      </span>
                    )}
                    {!isAgentSelectorDisabled && (
                      <span
                        className={`remote-selector-arrow ${
                          agentMenuOpen ? "open" : ""
                        }`}
                        aria-hidden="true"
                      >
                        <ChevronDownIcon size={13} />
                      </span>
                    )}
                  </span>
                </button>
              );
            })()}

            {agentMenuOpen && activeRunnerId && !isActiveRunnerHelloPending && (
              <div
                className="remote-selector-popover remote-agent-popover"
                role="menu"
              >
                <div className="remote-popover-header">
                  <span>选择 Agent</span>
                  <span className="remote-popover-sub">
                    {activeDevice?.label
                      ? `来自 ${activeDevice.label}`
                      : "当前电脑"}
                  </span>
                </div>
                <div className="remote-popover-list">
                  {availableAgentsForCurrentDevice.length === 0 ? (
                    <div className="remote-popover-empty">
                      当前电脑未检测到 Agent
                    </div>
                  ) : (
                    availableAgentsForCurrentDevice.map((agent) => (
                      <button
                        type="button"
                        role="menuitemradio"
                        aria-checked={providerFilter === agent.type}
                        className={`remote-popover-item ${
                          providerFilter === agent.type ? "selected" : ""
                        }`}
                        key={agent.type}
                        onClick={() => {
                          if (agent.type !== providerFilter) {
                            setSelected("");
                          }
                          userSelectedProviderRef.current = true;
                          setProviderFilter(agent.type);
                          setAgentMenuOpen(false);
                        }}
                      >
                        <span
                          className="remote-popover-agent-indicator"
                          data-agent={agent.type}
                        />
                        <span className="remote-popover-item-name">
                          {agent.name}
                        </span>
                        {agent.count > 0 ? (
                          <span className="remote-popover-item-badge active-count">
                            {agent.count}
                          </span>
                        ) : (
                          <span className="remote-popover-item-badge">0</span>
                        )}
                        {providerFilter === agent.type && (
                          <svg
                            className="remote-popover-check"
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2.5"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <polyline points="20 6 9 17 4 12" />
                          </svg>
                        )}
                      </button>
                    ))
                  )}
                </div>
              </div>
            )}
          </div>
        </div>

        <div className="remote-session-tree">
          {!activeRunnerId ? (
            <div className="remote-tree-empty-card">
              <p className="remote-tree-empty-text">请先在上方选择电脑</p>
            </div>
          ) : isActiveRunnerHelloPending ? (
            <div className="remote-tree-empty-card">
              <p className="remote-tree-empty-text">正在同步会话列表…</p>
            </div>
          ) : availableAgentsForCurrentDevice.length === 0 ? (
            <div className="remote-tree-empty-card">
              <p className="remote-tree-empty-text">
                当前电脑未检测到 Agent 运行环境
              </p>
            </div>
          ) : allWorkspacesForDevice.length === 0 ? (
            <div className="remote-tree-empty-card">
              <p className="remote-tree-empty-text">
                {sessionSearchQuery
                  ? `未找到与 "${sessionSearchQuery}" 匹配的会话`
                  : activeDevice?.online
                    ? `暂无 ${currentAgentInfo.name} 的会话`
                    : "电脑已离线"}
              </p>
              {activeDevice?.online && !sessionSearchQuery && (
                <button
                  type="button"
                  className="remote-empty-new-btn"
                  onClick={() => openLaunchModal()}
                >
                  <svg
                    width="12"
                    height="12"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    aria-hidden="true"
                    style={{ marginRight: 6 }}
                  >
                    <line x1="12" y1="5" x2="12" y2="19" />
                    <line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                  新建会话
                </button>
              )}
            </div>
          ) : (
            allWorkspacesForDevice.map((workspaceGroup) => {
              const workspaceKey = `${activeRunnerId}/${providerFilter}/${workspaceGroup.id}`;
              const isWorkspaceCollapsed =
                !sessionSearchQuery &&
                collapsedWorkspaces[workspaceKey] === true;
              const INITIAL_VISIBLE_COUNT = 5;
              const STEP_COUNT = 5;
              const visibleLimit = sessionSearchQuery
                ? workspaceGroup.items.length
                : (workspaceVisibleCounts[workspaceKey] ??
                  INITIAL_VISIBLE_COUNT);
              const visibleItems = workspaceGroup.items.slice(0, visibleLimit);
              const hasMore = workspaceGroup.items.length > visibleLimit;
              const remainingCount = workspaceGroup.items.length - visibleLimit;
              return (
                <div className="remote-workspace-group" key={workspaceKey}>
                  <div className="remote-workspace-header">
                    <button
                      type="button"
                      className="remote-workspace-title-btn"
                      onClick={() =>
                        toggleNode(setCollapsedWorkspaces, workspaceKey)
                      }
                      aria-expanded={!isWorkspaceCollapsed}
                      title={workspaceTooltip(workspaceGroup.id)}
                    >
                      <span
                        className={`remote-tree-caret ${
                          isWorkspaceCollapsed ? "collapsed" : ""
                        }`}
                        aria-hidden="true"
                      >
                        <ChevronDownIcon size={12} />
                      </span>
                      <svg
                        className="remote-workspace-icon"
                        width="13"
                        height="13"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        aria-hidden="true"
                      >
                        <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                      </svg>
                      <span className="remote-workspace-name">
                        {workspaceGroup.label}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="remote-workspace-quick-add"
                      title={`在 ${workspaceGroup.label} 中新建会话`}
                      aria-label={`在 ${workspaceGroup.label} 中新建会话`}
                      onClick={(e) => {
                        e.stopPropagation();
                        openLaunchModal(workspaceGroup.id);
                      }}
                    >
                      <svg
                        width="11"
                        height="11"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        aria-hidden="true"
                      >
                        <line x1="12" y1="5" x2="12" y2="19" />
                        <line x1="5" y1="12" x2="19" y2="12" />
                      </svg>
                    </button>
                  </div>

                  {!isWorkspaceCollapsed && (
                    <div className="remote-node-sessions">
                      {workspaceGroup.items.length === 0 ? (
                        <div className="remote-workspace-empty-hint">
                          暂无会话，点击新建开始
                        </div>
                      ) : (
                        <>
                          {visibleItems.map((item) => {
                            if (item.kind === "active") {
                              const activeSession = snapshot.sessions.find(
                                (entry) => key(entry) === item.sessionKey,
                              );
                              const isSelected = item.sessionKey === selected;
                              const pendingCount =
                                pendingApprovalCounts.get(item.sessionKey) ?? 0;
                              return (
                                <div
                                  className={`remote-session ${
                                    isSelected ? "selected" : ""
                                  }`}
                                  key={item.key}
                                >
                                  <button
                                    type="button"
                                    className="remote-session-main"
                                    onClick={() => {
                                      if (activeSession === undefined) return;
                                      if (isMobile) setMobileDrawerOpen(false);
                                      if (!isSelected) {
                                        setSelected(item.sessionKey);
                                        setError("");
                                      }
                                    }}
                                  >
                                    <span className="remote-session-top-line">
                                      <span
                                        className={`remote-session-bullet ${
                                          !item.session.control_connected &&
                                          activeSession?.online
                                            ? "reconnecting"
                                            : item.session.execution_state ===
                                                "running"
                                              ? "running"
                                              : item.session.execution_state ===
                                                    "awaiting_permission" ||
                                                  item.session
                                                    .execution_state ===
                                                    "awaiting_input"
                                                ? "waiting"
                                                : "idle"
                                        }`}
                                        aria-hidden="true"
                                      />
                                      <span className="remote-session-title">
                                        {item.title}
                                      </span>
                                    </span>
                                    <span className="remote-session-meta">
                                      {!item.session.control_connected &&
                                      activeSession?.online ? (
                                        <span className="remote-session-badge reconnecting">
                                          {item.session.execution_state ===
                                          "releasing_control"
                                            ? "正在归还"
                                            : busy && isSelected
                                              ? "处理中"
                                              : "只读"}
                                        </span>
                                      ) : item.session.execution_state ===
                                        "running" ? (
                                        <span className="remote-session-badge running">
                                          正在工作
                                        </span>
                                      ) : item.session.execution_state ===
                                        "awaiting_permission" ? (
                                        <span className="remote-session-badge waiting">
                                          待审批
                                        </span>
                                      ) : item.session.execution_state ===
                                        "awaiting_input" ? (
                                        <span className="remote-session-badge waiting">
                                          待输入
                                        </span>
                                      ) : null}
                                      {item.model && (
                                        <span
                                          className="remote-session-pill-tag"
                                          title={`模型: ${item.model}`}
                                        >
                                          {item.model.replace(/^models\//u, "")}
                                        </span>
                                      )}
                                      {formatSessionTokens(
                                        item.totalTokens,
                                      ) && (
                                        <span
                                          className="remote-session-tokens"
                                          title={`Token 用量: ${item.totalTokens?.toLocaleString()}`}
                                        >
                                          {formatSessionTokens(
                                            item.totalTokens,
                                          )}
                                        </span>
                                      )}
                                    </span>
                                  </button>
                                  <div className="remote-session-actions-hover">
                                    {pendingCount > 0 && (
                                      <span
                                        className="remote-tree-badge-pending"
                                        title={`有 ${pendingCount} 项待确认审批`}
                                        aria-label={`有 ${pendingCount} 项待确认审批`}
                                      >
                                        !
                                        {pendingCount > 1
                                          ? ` ${pendingCount}`
                                          : ""}
                                      </span>
                                    )}
                                    <button
                                      type="button"
                                      className={`remote-session-action-btn ${
                                        copiedSessionId === item.sessionKey
                                          ? "copied"
                                          : ""
                                      }`}
                                      title={
                                        copiedSessionId === item.sessionKey
                                          ? "已复制会话ID"
                                          : "复制会话ID"
                                      }
                                      aria-label="复制会话ID"
                                      onClick={(e) =>
                                        handleCopySessionId(item.sessionKey, e)
                                      }
                                    >
                                      {copiedSessionId === item.sessionKey ? (
                                        <>
                                          <CheckIcon size={12} />
                                          <span className="remote-copy-inline-label">
                                            已复制
                                          </span>
                                        </>
                                      ) : (
                                        <CopyIcon size={12} />
                                      )}
                                    </button>
                                  </div>
                                </div>
                              );
                            }

                            const isResumingThis =
                              resumingCandidateKey === item.key;
                            const canPreview =
                              supportsCodexControl(item.runnerId) &&
                              isCodexThreadId(item.providerSessionId);
                            return (
                              <div
                                className={`remote-session candidate ${isResumingThis ? "resuming" : ""}`}
                                key={item.key}
                              >
                                <button
                                  type="button"
                                  className="remote-session-main"
                                  title={
                                    canPreview
                                      ? "查看历史会话"
                                      : item.candidate.resumable === false
                                        ? "电脑端目前仅发现会话元数据，尚不能远程恢复"
                                        : `点击恢复：${item.title}`
                                  }
                                  disabled={
                                    busy ||
                                    (!canPreview &&
                                      item.candidate.resumable === false)
                                  }
                                  onClick={() => {
                                    if (isMobile) setMobileDrawerOpen(false);
                                    void resumeCandidate(
                                      item.runnerId,
                                      item.workspaceId,
                                      item.providerSessionId,
                                    );
                                  }}
                                >
                                  <div className="remote-session-top-line">
                                    <span
                                      className={`remote-session-bullet ${isResumingThis ? "reconnecting" : "candidate"}`}
                                      aria-hidden="true"
                                    />
                                    <span className="remote-session-title">
                                      {item.title}
                                    </span>
                                  </div>
                                  <span className="remote-session-meta">
                                    <span
                                      className={`remote-session-badge ${isResumingThis ? "reconnecting" : "candidate"}`}
                                    >
                                      {isResumingThis
                                        ? "打开中…"
                                        : canPreview
                                          ? "可查看"
                                          : "可接管"}
                                    </span>
                                    {item.model && (
                                      <span
                                        className="remote-session-pill-tag"
                                        title={`模型: ${item.model}`}
                                      >
                                        {item.model.replace(/^models\//u, "")}
                                      </span>
                                    )}
                                    {formatSessionTokens(item.totalTokens) && (
                                      <span
                                        className="remote-session-tokens"
                                        title={`Token 用量: ${item.totalTokens?.toLocaleString()}`}
                                      >
                                        {formatSessionTokens(item.totalTokens)}
                                      </span>
                                    )}
                                  </span>
                                </button>
                                <div className="remote-session-actions-hover">
                                  <button
                                    type="button"
                                    className={`remote-session-action-btn ${
                                      copiedSessionId === item.providerSessionId
                                        ? "copied"
                                        : ""
                                    }`}
                                    title={
                                      copiedSessionId === item.providerSessionId
                                        ? "已复制会话ID"
                                        : "复制会话ID"
                                    }
                                    aria-label="复制会话ID"
                                    onClick={(e) =>
                                      handleCopySessionId(
                                        item.providerSessionId,
                                        e,
                                      )
                                    }
                                  >
                                    {copiedSessionId ===
                                    item.providerSessionId ? (
                                      <>
                                        <CheckIcon size={12} />
                                        <span className="remote-copy-inline-label">
                                          已复制
                                        </span>
                                      </>
                                    ) : (
                                      <CopyIcon size={12} />
                                    )}
                                  </button>
                                </div>
                              </div>
                            );
                          })}
                          {hasMore && (
                            <div className="remote-workspace-more-row">
                              <button
                                type="button"
                                className="remote-workspace-more-btn"
                                onClick={() => {
                                  setWorkspaceVisibleCounts((prev) => ({
                                    ...prev,
                                    [workspaceKey]:
                                      (prev[workspaceKey] ??
                                        INITIAL_VISIBLE_COUNT) + STEP_COUNT,
                                  }));
                                }}
                              >
                                <ChevronDownIcon size={12} />
                                <span>
                                  展开更多（还有 {remainingCount} 条）
                                </span>
                              </button>
                            </div>
                          )}
                        </>
                      )}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
        <div className="remote-sidebar-footer">
          <div className="remote-runner-status">
            <span
              className={`remote-status-dot ${runners.some((item) => !item.revoked && item.online) ? "online" : "offline"}`}
            />
            <span>
              {runners.filter((item) => !item.revoked && item.online).length}{" "}
              台电脑在线
            </span>
          </div>
          {account?.user ? (
            <div className="remote-account-card">
              <div className="remote-account-avatar" aria-hidden="true">
                {account.user.avatar_data_url &&
                account.user.avatar_data_url.trim().length > 0 ? (
                  <img
                    src={account.user.avatar_data_url}
                    alt=""
                    referrerPolicy="no-referrer"
                  />
                ) : (
                  (
                    account.user.display_name?.[0] ||
                    account.user.email?.[0] ||
                    "U"
                  ).toUpperCase()
                )}
              </div>
              <span className="remote-account-name" title={account.user.email}>
                {account.user.display_name || account.user.email}
              </span>
              <div className="remote-account-actions">
                <button
                  className="remote-account-btn"
                  onClick={() => {
                    if (isMobile) setMobileDrawerOpen(false);
                    setSettingsOpen(true);
                  }}
                  type="button"
                  title="设置"
                >
                  <SettingsIcon size={14} />
                  <span>设置</span>
                </button>
                {onLogout && (
                  <button
                    className="remote-account-btn logout"
                    onClick={onLogout}
                    type="button"
                    title="退出登录"
                  >
                    退出
                  </button>
                )}
              </div>
            </div>
          ) : (
            <div
              className="remote-account-actions"
              style={{ marginTop: "4px" }}
            >
              <button
                className="remote-account-btn"
                onClick={() => {
                  if (isMobile) setMobileDrawerOpen(false);
                  setSettingsOpen(true);
                }}
                type="button"
                style={{ width: "100%", justifyContent: "center" }}
                title="设置"
              >
                <SettingsIcon size={14} />
                <span>设置</span>
              </button>
            </div>
          )}
        </div>
      </aside>
      {!sidebarCollapsed && (
        <div
          className="remote-sidebar-resizer"
          onMouseDown={handleResizerMouseDown}
          title="拖拽调整侧边栏宽度"
        />
      )}
      <main
        className="remote-main"
        inert={isMobile && mobileDrawerOpen ? true : undefined}
        onTouchStart={onMainTouchStart}
        onTouchMove={onMainTouchMove}
        onTouchEnd={onMainTouchEnd}
      >
        <header>
          <div className="remote-header-left">
            {isMobile ? (
              <button
                type="button"
                ref={mobileMenuBtnRef}
                className="remote-icon-btn mobile-menu-btn"
                title={
                  attention.length > 0
                    ? `打开菜单（${attention.length} 项待处理）`
                    : "打开菜单"
                }
                aria-label={
                  attention.length > 0
                    ? `打开菜单，有 ${attention.length} 项待处理`
                    : "打开菜单"
                }
                aria-expanded={mobileDrawerOpen}
                onClick={openMobileDrawer}
              >
                <svg
                  width="20"
                  height="20"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <line x1="3" y1="12" x2="21" y2="12" />
                  <line x1="3" y1="6" x2="21" y2="6" />
                  <line x1="3" y1="18" x2="21" y2="18" />
                </svg>
                {attention.length > 0 && (
                  <span className="remote-mobile-menu-badge" aria-hidden="true">
                    {attention.length > 9 ? "9+" : attention.length}
                  </span>
                )}
              </button>
            ) : (
              sidebarCollapsed && (
                <button
                  type="button"
                  className="remote-icon-btn expand-btn"
                  title="展开侧边栏"
                  aria-label="展开侧边栏"
                  onClick={toggleSidebar}
                >
                  <svg
                    width="18"
                    height="18"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                  >
                    <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                    <line x1="9" y1="3" x2="9" y2="21" />
                    <path d="M13 15l3-3-3-3" />
                  </svg>
                </button>
              )
            )}
            <div>
              {selectedSessionPath && selectedSessionPath.length > 0 && (
                <nav
                  className="remote-main-breadcrumb"
                  aria-label="会话层级位置"
                >
                  <button
                    type="button"
                    className="remote-main-breadcrumb-root"
                    onClick={() => {
                      setSelected("");
                      if (isMobile) setMobileDrawerOpen(false);
                    }}
                  >
                    全部会话
                  </button>
                  {selectedSessionPath.map((crumb, index) => {
                    const isLast = index === selectedSessionPath.length - 1;
                    return (
                      <span
                        className="remote-main-breadcrumb-item"
                        key={crumb.key}
                      >
                        <span
                          className="remote-main-breadcrumb-sep"
                          aria-hidden="true"
                        >
                          /
                        </span>
                        <button
                          type="button"
                          className="remote-main-breadcrumb-crumb"
                          data-current={isLast}
                          onClick={() => {
                            if (isLast) return;
                            if (isMobile) setMobileDrawerOpen(false);
                            if (crumb.sessionKey !== undefined) {
                              setSelected(crumb.sessionKey);
                            } else {
                              if (crumb.runnerId !== undefined) {
                                setSelectedRunnerId(crumb.runnerId);
                              }
                              if (crumb.provider !== undefined) {
                                userSelectedProviderRef.current = true;
                                setProviderFilter(crumb.provider);
                              }
                              setSelected("");
                            }
                          }}
                        >
                          {crumb.label}
                        </button>
                      </span>
                    );
                  })}
                </nav>
              )}
              <h1>
                {session
                  ? workspaceLabel(session.binding.workspace_path)
                  : "随时接着做"}
              </h1>
              <p>
                {session
                  ? (() => {
                      const providerName = getProviderInfo(
                        session.binding.provider_session_id,
                        discovered.get(
                          `${session.runner_id}/${session.binding.provider_session_id}`,
                        )?.provider ?? session.provider,
                      ).name;
                      if (!session.online) return `${providerName} · 电脑离线`;
                      if (releasingControl)
                        return `${providerName} · 正在归还控制`;
                      if (sessionLoading && items.length === 0)
                        return `${providerName} · 正在同步历史`;
                      if (!connected) return `${providerName} · 只读`;
                      return providerName;
                    })()
                  : "电脑执行，随处继续。"}
              </p>
            </div>
          </div>
          <MobileHeaderActions
            isMobile={isMobile}
            selectedSessionKey={selected}
            copiedSessionId={copiedSessionId}
            onCopySessionId={handleCopySessionId}
            onNewSession={() =>
              openLaunchModal(session?.binding.workspace_path)
            }
          />
        </header>
        {isMobile && !mobileDrawerOpen && (
          <MobileAttentionBanner
            entries={attention}
            selectedSessionKey={selected}
            onOpenSingle={(entry) => {
              setSelectedRunnerId(entry.runnerId);
              setSelected(entry.sessionKey);
              setError("");
            }}
            onOpenDrawer={openMobileDrawer}
          />
        )}
        {error && (
          <div role="alert" className="remote-notice">
            {cleanBannerText(error)}
            <button aria-label="关闭提示" onClick={() => setError("")}>
              ×
            </button>
          </div>
        )}
        {!selected ? (
          <div key="welcome" className="remote-welcome remote-view-enter">
            <span className="remote-eyebrow">YOUR WORK IN FLOW</span>
            <h2>{loading ? "正在连接…" : "打开一个会话，继续刚才的工作。"}</h2>
            <p>
              消息、工具执行与权限审批均直接同步自你的原生终端 Agent。
              即便离开网页，已派发的工作流依然在电脑后台稳步推进。
            </p>
            <div className="remote-welcome-chips">
              {[
                {
                  title: "新建会话",
                  desc: "在指定设备工作区派发新 Agent",
                  action: () => openLaunchModal(),
                },
                {
                  title: "外观设置",
                  desc: "在温润纸张与黑曜石暖炭间切换",
                  action: () => setSettingsOpen(true),
                },
              ].map((chip) => (
                <button
                  key={chip.title}
                  type="button"
                  className="remote-welcome-chip"
                  onClick={chip.action}
                >
                  <strong>{chip.title}</strong>
                  <span>{chip.desc}</span>
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div key={selected} className="remote-session-view remote-view-enter">
            {session && !connected && (
              <div className="remote-notice session-control">
                <span>
                  {releasingControl
                    ? "正在归还控制…"
                    : session.online
                      ? "正在只读查看会话"
                      : "电脑离线，可查看已同步的历史"}
                </span>
                {session.online && !releasingControl && (
                  <button
                    disabled={busy}
                    onClick={() => reconnectActiveSession(session)}
                  >
                    接管会话
                  </button>
                )}
              </div>
            )}
            {codexSession &&
              connected &&
              !isTurnExecuting &&
              approvals.length === 0 &&
              questions.length === 0 && (
                <div className="remote-notice session-control">
                  <span>AgentHub 已接管会话</span>
                  <button
                    disabled={busy}
                    onClick={() => void submit({ action: "release_control" })}
                  >
                    归还控制
                  </button>
                </div>
              )}
            <div
              className="remote-timeline cds-scroll-fade-mask"
              ref={timeline}
              onScroll={handleTimelineScroll}
              aria-label="会话内容"
            >
              {itemGroups.map((group) => {
                if (group.type === "tool_group") {
                  return (
                    <RemoteToolGroup
                      key={group.items[0]?.id}
                      items={group.items}
                    />
                  );
                }
                const item = group.item;
                if (item.kind === "tool")
                  return <RemoteToolCard key={item.id} item={item} />;

                const isStreamingThis = Boolean(
                  item.kind === "assistant" &&
                  session?.binding.current_turn_id &&
                  item === effectiveItems[effectiveItems.length - 1],
                );
                const providerName =
                  item.kind !== "user"
                    ? getProviderInfo(
                        session?.binding.provider_session_id ?? "",
                        discovered.get(
                          `${session?.runner_id ?? ""}/${session?.binding.provider_session_id ?? ""}`,
                        )?.provider ?? session?.provider,
                      ).name
                    : undefined;

                return (
                  <RemoteMessageCard
                    key={item.id}
                    item={item}
                    isStreamingThis={isStreamingThis}
                    providerName={providerName}
                  />
                );
              })}
              {isTurnExecuting &&
                approvals.length === 0 &&
                questions.length === 0 &&
                (isWaitingFirstAssistant ||
                  (effectiveItems[effectiveItems.length - 1]?.kind === "tool" &&
                    effectiveItems[effectiveItems.length - 1]?.status ===
                      "running")) && (
                  <div
                    className="remote-turn-live"
                    role="status"
                    aria-live="polite"
                  >
                    <div className="remote-turn-live-main">
                      <span
                        className="remote-turn-live-dots"
                        aria-hidden="true"
                      >
                        <i />
                        <i />
                        <i />
                      </span>
                      <span className="remote-turn-live-label">
                        {optimisticPrompts.length > 0 ||
                        (hasInFlightPrompt && !session?.binding.current_turn_id)
                          ? "已发送，正在等待响应…"
                          : effectiveItems[effectiveItems.length - 1]?.kind ===
                                "tool" &&
                              effectiveItems[effectiveItems.length - 1]
                                ?.status === "running"
                            ? `正在执行 · ${effectiveItems[effectiveItems.length - 1]?.title || "工具调用"}`
                            : "正在思考并生成回复…"}
                      </span>
                      {session && (
                        <span className="remote-turn-live-provider">
                          {
                            getProviderInfo(
                              session.binding.provider_session_id,
                              discovered.get(
                                `${session.runner_id}/${session.binding.provider_session_id}`,
                              )?.provider ?? session.provider,
                            ).name
                          }
                        </span>
                      )}
                    </div>
                  </div>
                )}
              {!isTurnExecuting &&
                (() => {
                  const failText =
                    turnFailure && !turnFailure.retrying
                      ? cleanBannerText(turnFailure.message)
                      : latestFailure
                        ? remoteError(
                            new Error(latestFailure.receipt?.code ?? ""),
                          )
                        : session?.binding.execution_state === "error"
                          ? "上一轮执行异常中断，请检查当前客户端模型或网关配置后重试。"
                          : "";
                  if (!failText || failText === dismissedFailure) return null;
                  return (
                    <div className="remote-timeline-error" role="alert">
                      <div className="remote-timeline-error-body">
                        <strong>
                          {latestFailure?.receipt?.code === "native_writer_busy"
                            ? "接管失败"
                            : "本轮执行未完成"}
                        </strong>
                        <span>{failText}</span>
                      </div>
                      <button
                        type="button"
                        aria-label="关闭提示"
                        onClick={() => setDismissedFailure(failText)}
                      >
                        ×
                      </button>
                    </div>
                  );
                })()}
              {!session || ((sessionLoading || busy) && items.length === 0) ? (
                <ConversationSkeleton label="正在加载会话内容…" />
              ) : !items.length ? (
                <div className="remote-timeline-empty">
                  <div
                    className="remote-timeline-empty-glyph"
                    aria-hidden="true"
                  >
                    <svg
                      width="26"
                      height="26"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                    </svg>
                  </div>
                  <h3 className="remote-timeline-empty-title">
                    {discovered.get(
                      `${session.runner_id}/${session.binding.provider_session_id}`,
                    )?.title || formatFallbackTitle()}
                  </h3>
                  <p className="remote-timeline-empty-desc">
                    {session.online ? "暂无已同步的消息" : "电脑离线"}
                  </p>
                </div>
              ) : null}
              {approvals.map((approval) => {
                const isSubmittingThis =
                  submittingApproval?.id === approval.approval_id;
                const isApproving =
                  isSubmittingThis && submittingApproval?.action === "approve";
                const isDeclining =
                  isSubmittingThis && submittingApproval?.action === "decline";
                const isCopied = copiedApprovalId === approval.approval_id;
                return (
                  <section
                    className="remote-approval mcp-tool-decision-card state-pending-approval"
                    key={approval.approval_id}
                  >
                    <div className="remote-approval-header">
                      <div className="remote-approval-title-wrap">
                        <span
                          className="remote-approval-shield-icon"
                          aria-hidden="true"
                        >
                          <svg
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                          </svg>
                        </span>
                        <h3>权限与工具执行审批</h3>
                        <span className="remote-approval-tool-badge">
                          {approval.tool_name ?? "系统工具"}
                        </span>
                      </div>
                      <div className="remote-approval-meta-right">
                        {approval.risk_level && (
                          <span
                            className={`remote-approval-badge ${approval.risk_level}`}
                          >
                            {approval.risk_level === "high"
                              ? "高风险拦截"
                              : approval.risk_level === "medium"
                                ? "中风险操作"
                                : "受控操作"}
                          </span>
                        )}
                        <span className="remote-approval-awaiting-pill">
                          <span className="awaiting-dot" />
                          等待决策
                        </span>
                      </div>
                    </div>
                    <div className="remote-approval-desc">
                      <strong>
                        {
                          getProviderInfo(
                            session?.binding.provider_session_id ?? "",
                            discovered.get(
                              `${session?.runner_id ?? ""}/${session?.binding.provider_session_id ?? ""}`,
                            )?.provider ?? session?.provider,
                          ).name
                        }
                      </strong>{" "}
                      请求在当前工作区执行 "{approval.tool_name ?? "系统工具"}"
                      {approval.description && `：${approval.description}`}
                    </div>
                    {approval.command_preview && (
                      <div className="remote-approval-box-wrapper">
                        <div className="remote-approval-box-bar">
                          <span className="remote-approval-box-label">
                            待执行命令参数
                          </span>
                          <button
                            type="button"
                            className="remote-card-mini-copy"
                            onClick={() =>
                              handleCopyText(
                                approval.approval_id,
                                approval.command_preview!,
                              )
                            }
                            title="复制完整命令"
                          >
                            {isCopied ? (
                              <span className="remote-copied-badge">
                                <CheckIcon size={12} />
                                <span>已复制</span>
                              </span>
                            ) : (
                              <span className="remote-copy-action">
                                <CopyIcon size={12} />
                                <span>复制命令</span>
                              </span>
                            )}
                          </button>
                        </div>
                        <div className="remote-approval-command-box">
                          {approval.command_preview}
                        </div>
                      </div>
                    )}
                    {approval.path && !approval.command_preview && (
                      <div className="remote-approval-box-wrapper">
                        <div className="remote-approval-box-bar">
                          <span className="remote-approval-box-label">
                            涉及目标文件路径
                          </span>
                          <button
                            type="button"
                            className="remote-card-mini-copy"
                            onClick={() =>
                              handleCopyText(
                                approval.approval_id,
                                approval.path!,
                              )
                            }
                            title="复制文件路径"
                          >
                            {isCopied ? (
                              <span className="remote-copied-badge">
                                <CheckIcon size={12} />
                                <span>已复制</span>
                              </span>
                            ) : (
                              <span className="remote-copy-action">
                                <CopyIcon size={12} />
                                <span>复制路径</span>
                              </span>
                            )}
                          </button>
                        </div>
                        <div className="remote-approval-command-box">
                          {approval.path}
                        </div>
                      </div>
                    )}
                    {approval.diff &&
                      (() => {
                        const diffStats = parseDiffStats(approval.diff);
                        const isCollapsed =
                          collapsedApprovalDiffs[approval.approval_id] ?? false;
                        const isDiffCopied =
                          copiedApprovalId === `${approval.approval_id}-diff`;
                        return (
                          <div className="remote-approval-diff-section">
                            <div className="remote-approval-diff-header">
                              <div className="remote-approval-diff-title-wrap">
                                <span className="remote-approval-box-label">
                                  拟修改代码变更 (Diff)
                                </span>
                                {(diffStats.additions > 0 ||
                                  diffStats.deletions > 0) && (
                                  <span className="remote-approval-diff-badge">
                                    {diffStats.additions > 0 && (
                                      <span className="remote-diff-stat-add">
                                        +{diffStats.additions}
                                      </span>
                                    )}
                                    {diffStats.deletions > 0 && (
                                      <span className="remote-diff-stat-del">
                                        -{diffStats.deletions}
                                      </span>
                                    )}
                                  </span>
                                )}
                              </div>
                              <div className="remote-approval-diff-actions">
                                <button
                                  type="button"
                                  className="remote-card-mini-copy"
                                  onClick={() =>
                                    handleCopyText(
                                      `${approval.approval_id}-diff`,
                                      approval.diff!,
                                    )
                                  }
                                  title="复制完整 Diff 内容"
                                >
                                  {isDiffCopied ? (
                                    <span className="remote-copied-badge">
                                      <CheckIcon size={12} />
                                      <span>已复制</span>
                                    </span>
                                  ) : (
                                    <span className="remote-copy-action">
                                      <CopyIcon size={12} />
                                      <span>复制变更</span>
                                    </span>
                                  )}
                                </button>
                                <button
                                  type="button"
                                  className="remote-approval-diff-toggle-btn"
                                  onClick={() =>
                                    setCollapsedApprovalDiffs((prev) => ({
                                      ...prev,
                                      [approval.approval_id]: !isCollapsed,
                                    }))
                                  }
                                  title={isCollapsed ? "展开比对" : "折叠比对"}
                                >
                                  <span>
                                    {isCollapsed ? "展开比对" : "折叠比对"}
                                  </span>
                                  <ChevronDownIcon
                                    size={12}
                                    className={`remote-diff-chevron ${isCollapsed ? "" : "open"}`}
                                  />
                                </button>
                              </div>
                            </div>
                            {!isCollapsed && (
                              <div className="remote-approval-diff-body">
                                {diffStats.oldText || diffStats.newText ? (
                                  <div className="remote-diff-viewer">
                                    {diffStats.oldText && (
                                      <div className="remote-diff-chunk del">
                                        <div className="remote-diff-bar">
                                          <span className="remote-diff-tag">
                                            - 变更前代码
                                          </span>
                                          <button
                                            type="button"
                                            className="remote-card-mini-copy"
                                            onClick={() =>
                                              handleCopyText(
                                                `${approval.approval_id}-diff-del`,
                                                diffStats.oldText!,
                                              )
                                            }
                                          >
                                            {copiedApprovalId ===
                                            `${approval.approval_id}-diff-del` ? (
                                              <span className="remote-copied-badge">
                                                <CheckIcon size={12} />
                                                <span>已复制</span>
                                              </span>
                                            ) : (
                                              <span className="remote-copy-action">
                                                <CopyIcon size={12} />
                                                <span>复制</span>
                                              </span>
                                            )}
                                          </button>
                                        </div>
                                        <pre className="remote-diff-pre">
                                          {diffStats.oldText}
                                        </pre>
                                      </div>
                                    )}
                                    {diffStats.newText && (
                                      <div className="remote-diff-chunk add">
                                        <div className="remote-diff-bar">
                                          <span className="remote-diff-tag">
                                            + 变更后代码
                                          </span>
                                          <button
                                            type="button"
                                            className="remote-card-mini-copy"
                                            onClick={() =>
                                              handleCopyText(
                                                `${approval.approval_id}-diff-add`,
                                                diffStats.newText!,
                                              )
                                            }
                                          >
                                            {copiedApprovalId ===
                                            `${approval.approval_id}-diff-add` ? (
                                              <span className="remote-copied-badge">
                                                <CheckIcon size={12} />
                                                <span>已复制</span>
                                              </span>
                                            ) : (
                                              <span className="remote-copy-action">
                                                <CopyIcon size={12} />
                                                <span>复制</span>
                                              </span>
                                            )}
                                          </button>
                                        </div>
                                        <pre className="remote-diff-pre">
                                          {diffStats.newText}
                                        </pre>
                                      </div>
                                    )}
                                  </div>
                                ) : (
                                  <pre className="remote-diff-pre remote-approval-raw-diff">
                                    {approval.diff}
                                  </pre>
                                )}
                              </div>
                            )}
                          </div>
                        );
                      })()}
                    <div className="remote-approval-actions">
                      <button
                        type="button"
                        className="remote-approval-btn-decline"
                        disabled={
                          !connected ||
                          busy ||
                          !approval.can_decline ||
                          isSubmittingThis
                        }
                        onClick={() =>
                          void submit({
                            action: "resolve_approval",
                            approval_id: approval.approval_id,
                            approved: false,
                          })
                        }
                      >
                        {isDeclining ? "正在拒绝…" : "拒绝授权"}
                      </button>
                      <button
                        type="button"
                        className="remote-primary remote-approval-btn-approve"
                        disabled={
                          !connected ||
                          busy ||
                          !approval.can_approve ||
                          isSubmittingThis
                        }
                        onClick={() =>
                          void submit({
                            action: "resolve_approval",
                            approval_id: approval.approval_id,
                            approved: true,
                          })
                        }
                      >
                        {isApproving ? "正在授权…" : "允许此次执行"}
                      </button>
                    </div>
                  </section>
                );
              })}
              {questions.map((q) => {
                const isSubmitting = submittingQuestionId === q.question_id;
                const isMulti = Boolean(q.multi_select);
                const currentSelections = selectedOptions[q.question_id] ?? [];
                return (
                  <section
                    className="remote-question mcp-tool-decision-card state-pending-question"
                    key={q.question_id}
                  >
                    <div className="remote-question-header">
                      <div className="remote-question-title-wrap">
                        <span
                          className="remote-question-glyph"
                          aria-hidden="true"
                        >
                          ?
                        </span>
                        <span>{q.header ?? "Agent 向你提问"}</span>
                      </div>
                      <div className="remote-question-meta-right">
                        {isMulti ? (
                          <span className="remote-question-type-badge">
                            可多选
                          </span>
                        ) : (
                          <span className="remote-question-type-badge single">
                            单项选择
                          </span>
                        )}
                        <span className="remote-approval-awaiting-pill">
                          <span className="awaiting-dot question" />
                          等待答复
                        </span>
                      </div>
                    </div>
                    <div className="remote-question-prompt">{q.prompt}</div>
                    {q.options && q.options.length > 0 && (
                      <div className="remote-question-options">
                        {q.options.map((opt, idx) => {
                          const isOptionSelected = currentSelections.includes(
                            opt.label,
                          );
                          return (
                            <button
                              key={idx}
                              type="button"
                              className={`remote-option-btn ${
                                isOptionSelected ? "selected" : ""
                              }`}
                              disabled={!connected || busy || isSubmitting}
                              onClick={() => {
                                if (isMulti) {
                                  setSelectedOptions((prev) => {
                                    const list = prev[q.question_id] ?? [];
                                    const nextList = list.includes(opt.label)
                                      ? list.filter(
                                          (item) => item !== opt.label,
                                        )
                                      : [...list, opt.label];
                                    return {
                                      ...prev,
                                      [q.question_id]: nextList,
                                    };
                                  });
                                } else {
                                  void submit({
                                    action: "answer_question",
                                    question_id: q.question_id,
                                    answer: opt.label,
                                  });
                                }
                              }}
                            >
                              <div className="remote-option-top-row">
                                {isMulti && (
                                  <span
                                    className={`remote-option-check ${
                                      isOptionSelected ? "checked" : ""
                                    }`}
                                    aria-hidden="true"
                                  >
                                    {isOptionSelected ? (
                                      <CheckIcon size={12} />
                                    ) : null}
                                  </span>
                                )}
                                <strong>{opt.label}</strong>
                              </div>
                              {opt.description && (
                                <span className="remote-option-desc">
                                  {opt.description}
                                </span>
                              )}
                              {opt.preview && (
                                <pre className="remote-option-preview">
                                  {opt.preview}
                                </pre>
                              )}
                            </button>
                          );
                        })}
                      </div>
                    )}
                    {isMulti && (
                      <div className="remote-question-multi-actions">
                        <button
                          type="button"
                          className="remote-primary remote-question-submit-btn"
                          disabled={
                            !connected ||
                            busy ||
                            isSubmitting ||
                            currentSelections.length === 0
                          }
                          onClick={() => {
                            if (currentSelections.length > 0) {
                              void submit({
                                action: "answer_question",
                                question_id: q.question_id,
                                answer: currentSelections,
                              });
                              setSelectedOptions((prev) => {
                                const next = { ...prev };
                                delete next[q.question_id];
                                return next;
                              });
                            }
                          }}
                        >
                          {isSubmitting
                            ? "提交中…"
                            : `确认已选项目 (${currentSelections.length})`}
                        </button>
                      </div>
                    )}
                    <div className="remote-question-input-row">
                      <input
                        type="text"
                        className="remote-question-input"
                        placeholder="或者输入自定义回答..."
                        disabled={!connected || busy || isSubmitting}
                        value={questionAnswers[q.question_id] ?? ""}
                        onChange={(e) =>
                          setQuestionAnswers((prev) => ({
                            ...prev,
                            [q.question_id]: e.target.value,
                          }))
                        }
                        onKeyDown={(e) => {
                          if (e.key === "Enter") {
                            e.preventDefault();
                            const ans = questionAnswers[q.question_id]?.trim();
                            if (ans) {
                              void submit({
                                action: "answer_question",
                                question_id: q.question_id,
                                answer: ans,
                              });
                              setQuestionAnswers((prev) => ({
                                ...prev,
                                [q.question_id]: "",
                              }));
                            }
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="remote-primary"
                        disabled={
                          !connected ||
                          busy ||
                          isSubmitting ||
                          !questionAnswers[q.question_id]?.trim()
                        }
                        onClick={() => {
                          const ans = questionAnswers[q.question_id]?.trim();
                          if (ans) {
                            void submit({
                              action: "answer_question",
                              question_id: q.question_id,
                              answer: ans,
                            });
                            setQuestionAnswers((prev) => ({
                              ...prev,
                              [q.question_id]: "",
                            }));
                          }
                        }}
                      >
                        {isSubmitting ? "提交中…" : "提交回答"}
                      </button>
                    </div>
                  </section>
                );
              })}
            </div>
            {!followBottom && items.length > 0 && (
              <button
                type="button"
                className="remote-scroll-bottom-btn"
                onClick={scrollToBottom}
                aria-label="回到最新消息"
              >
                <svg
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <polyline points="6 9 12 15 18 9" />
                </svg>
                <span>回到最新</span>
              </button>
            )}
            <form
              className="remote-composer-card"
              onSubmit={(event) => {
                event.preventDefault();
                sendCurrentPrompt();
              }}
            >
              {activeQueuedPrompt && (
                <QueuedPromptBar
                  queued={activeQueuedPrompt}
                  canSendNow={Boolean(connected && canSend && !isTurnExecuting)}
                  onSendNow={() => {
                    if (!selected || !connected || !canSend) return;
                    const queued = activeQueuedPrompt;
                    setQueuedPrompts((prev) => {
                      const next = { ...prev };
                      delete next[selected];
                      return next;
                    });
                    dispatchPromptPayload(queued.text, queued.attachments);
                  }}
                  onEdit={() => {
                    if (!selected) return;
                    setText((prev) =>
                      prev.trim()
                        ? `${activeQueuedPrompt.text}\n${prev}`
                        : activeQueuedPrompt.text,
                    );
                    if (activeQueuedPrompt.attachments.length > 0) {
                      setAttachments((prev) =>
                        [...activeQueuedPrompt.attachments, ...prev].slice(
                          0,
                          16,
                        ),
                      );
                    }
                    setQueuedPrompts((prev) => {
                      const next = { ...prev };
                      delete next[selected];
                      return next;
                    });
                    requestAnimationFrame(() => {
                      adjustTextareaHeight();
                      textareaRef.current?.focus();
                    });
                  }}
                  onCancel={() => {
                    if (!selected) return;
                    setQueuedPrompts((prev) => {
                      const next = { ...prev };
                      delete next[selected];
                      return next;
                    });
                  }}
                />
              )}
              {attachments.length > 0 && (
                <div
                  className="remote-composer-attachments"
                  aria-label="已选附件"
                >
                  {attachments.map((att, idx) => (
                    <div
                      className="remote-attachment-chip"
                      key={`${att.name}-${idx}`}
                    >
                      {att.encoding === "base64" &&
                      att.data.length > 0 &&
                      att.media_type.startsWith("image/") ? (
                        <img
                          className="remote-attachment-thumb"
                          src={`data:${att.media_type};base64,${att.data}`}
                          alt={att.name}
                        />
                      ) : (
                        <span
                          className="remote-attachment-icon"
                          aria-hidden="true"
                        >
                          <svg
                            width="13"
                            height="13"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          >
                            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                            <polyline points="14 2 14 8 20 8" />
                          </svg>
                        </span>
                      )}
                      <span className="remote-attachment-name" title={att.name}>
                        {att.name}
                      </span>
                      <button
                        type="button"
                        className="remote-attachment-remove"
                        aria-label={`移除附件 ${att.name}`}
                        onClick={() =>
                          setAttachments((prev) =>
                            prev.filter((_, i) => i !== idx),
                          )
                        }
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="remote-composer-input-wrapper">
                <textarea
                  ref={textareaRef}
                  id="remote-prompt"
                  value={text}
                  onChange={(event) => {
                    setText(event.target.value);
                    adjustTextareaHeight();
                  }}
                  onPaste={(event) => {
                    const pastedFiles = event.clipboardData?.files;
                    if (pastedFiles && pastedFiles.length > 0) {
                      event.preventDefault();
                      void handleFilesSelected(pastedFiles);
                    }
                  }}
                  onKeyDown={(event) => {
                    if (
                      (event.ctrlKey || event.metaKey) &&
                      event.key.toLowerCase() === "u"
                    ) {
                      event.preventDefault();
                      if ((connected || canTakeControl) && !busy) {
                        fileInputRef.current?.click();
                      }
                      return;
                    }
                    const isComposing =
                      (event.nativeEvent as KeyboardEvent).isComposing ||
                      event.keyCode === 229;
                    // 移动端不通过回车触发发送；桌面端双重互锁防输入法选词误发
                    if (
                      !isMobile &&
                      event.key === "Enter" &&
                      !event.shiftKey &&
                      !isComposing
                    ) {
                      event.preventDefault();
                      sendCurrentPrompt();
                    }
                  }}
                  placeholder={
                    !session
                      ? "等待电脑确认会话…"
                      : !session.online
                        ? "电脑离线"
                        : !connected
                          ? canTakeControl
                            ? "输入消息，发送时接管会话"
                            : "等待会话可用"
                          : isTurnExecuting
                            ? isMobile
                              ? '输入下一条消息，点"排队"加入待发…'
                              : "输入下一条消息，按 Enter 加入待发队列…"
                            : items.length === 0
                              ? "输入指令开始新对话…"
                              : "继续这段工作…"
                  }
                  rows={1}
                  disabled={!connected && !canTakeControl}
                />
              </div>
              <div className="remote-composer-toolbar">
                <div className="remote-composer-left">
                  <input
                    ref={fileInputRef}
                    type="file"
                    multiple
                    hidden
                    onChange={(event) => {
                      if (event.target.files) {
                        void handleFilesSelected(event.target.files);
                        event.target.value = "";
                      }
                    }}
                  />
                  <div className="remote-attach-menu-wrap" ref={attachMenuRef}>
                    <button
                      type="button"
                      className={`remote-attach-btn ${attachMenuOpen ? "open" : ""}`}
                      title="添加内容或附件"
                      aria-label="添加内容或附件"
                      aria-haspopup="menu"
                      aria-expanded={attachMenuOpen}
                      disabled={!connected || busy}
                      onClick={() => {
                        setAttachMenuOpen((prev) => {
                          if (prev) setAttachQuickPromptsOpen(false);
                          return !prev;
                        });
                      }}
                    >
                      <svg
                        width="17"
                        height="17"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        aria-hidden="true"
                      >
                        <line x1="12" y1="5" x2="12" y2="19" />
                        <line x1="5" y1="12" x2="19" y2="12" />
                      </svg>
                    </button>
                    {attachMenuOpen && (
                      <div
                        className="remote-attach-popover"
                        role="menu"
                        aria-label="添加附件或快捷内容"
                      >
                        <button
                          type="button"
                          role="menuitem"
                          className="remote-attach-menu-item"
                          onClick={() => {
                            setAttachMenuOpen(false);
                            setAttachQuickPromptsOpen(false);
                            fileInputRef.current?.click();
                          }}
                        >
                          <span
                            className="remote-attach-menu-icon"
                            aria-hidden="true"
                          >
                            <svg
                              width="15"
                              height="15"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.9"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />
                            </svg>
                          </span>
                          <span className="remote-attach-menu-label">
                            添加文件或图片
                          </span>
                          <kbd className="remote-attach-menu-kbd">Ctrl+U</kbd>
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="remote-attach-menu-item"
                          onClick={() => void handlePasteFromClipboardMenu()}
                        >
                          <span
                            className="remote-attach-menu-icon"
                            aria-hidden="true"
                          >
                            <svg
                              width="15"
                              height="15"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.9"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <rect
                                x="9"
                                y="2"
                                width="6"
                                height="4"
                                rx="1"
                                ry="1"
                              />
                              <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
                            </svg>
                          </span>
                          <span className="remote-attach-menu-label">
                            从剪贴板粘贴
                          </span>
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="remote-attach-menu-item"
                          onClick={handleInsertCodeSnippet}
                        >
                          <span
                            className="remote-attach-menu-icon"
                            aria-hidden="true"
                          >
                            <svg
                              width="15"
                              height="15"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.9"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <polyline points="16 18 22 12 16 6" />
                              <polyline points="8 6 2 12 8 18" />
                            </svg>
                          </span>
                          <span className="remote-attach-menu-label">
                            插入代码片段
                          </span>
                        </button>
                        <div className="remote-attach-menu-divider" />
                        <button
                          type="button"
                          role="menuitem"
                          aria-expanded={attachQuickPromptsOpen}
                          className={`remote-attach-menu-item ${attachQuickPromptsOpen ? "active" : ""}`}
                          onClick={() =>
                            setAttachQuickPromptsOpen((prev) => !prev)
                          }
                        >
                          <span
                            className="remote-attach-menu-icon"
                            aria-hidden="true"
                          >
                            <svg
                              width="15"
                              height="15"
                              viewBox="0 0 24 24"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.9"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            >
                              <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
                            </svg>
                          </span>
                          <span className="remote-attach-menu-label">
                            快捷指令
                          </span>
                          <ChevronDownIcon
                            size={12}
                            className={`remote-attach-chevron ${attachQuickPromptsOpen ? "open" : ""}`}
                          />
                        </button>
                        {attachQuickPromptsOpen && (
                          <div className="remote-attach-sublist">
                            {QUICK_PROMPT_SNIPPETS.map((snippet) => (
                              <button
                                key={snippet.label}
                                type="button"
                                role="menuitem"
                                className="remote-attach-subitem"
                                onClick={() => {
                                  setAttachMenuOpen(false);
                                  setAttachQuickPromptsOpen(false);
                                  setText((prev) =>
                                    prev
                                      ? `${prev}\n${snippet.text}`
                                      : snippet.text,
                                  );
                                  requestAnimationFrame(() => {
                                    adjustTextareaHeight();
                                    textareaRef.current?.focus();
                                  });
                                }}
                              >
                                {snippet.label}
                              </button>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                  <div
                    className="remote-permission-selector"
                    title="设置当前会话执行权限模式"
                  >
                    <Select
                      variant="compact"
                      placement="top"
                      label="权限模式"
                      value={permissionMode}
                      disabled={!connected || busy}
                      options={PERMISSION_MODE_OPTIONS}
                      onChange={(next) => {
                        setPermissionMode(next as RemotePermissionMode);
                        setPermissionModeTouched(true);
                      }}
                    />
                  </div>
                </div>
                <div className="remote-composer-actions">
                  <div
                    className="remote-model-selector"
                    title="选择本轮对话模型"
                  >
                    <Select
                      variant="compact"
                      placement="top"
                      label="会话模型"
                      value={sessionModel}
                      disabled={!connected || busy}
                      options={sessionModelOptions}
                      onChange={setSessionModel}
                    />
                  </div>
                  <div
                    className="remote-effort-selector"
                    title="设置本轮思考推理深度"
                  >
                    <Select
                      variant="compact"
                      placement="top"
                      label="推理思考深度"
                      value={effort}
                      disabled={!connected || busy}
                      options={EFFORT_OPTIONS}
                      onChange={(next) => setEffort(next as typeof effort)}
                    />
                  </div>
                  <RemoteContextCapacity
                    events={events}
                    candidate={
                      session
                        ? discovered.get(
                            `${session.runner_id}/${session.binding.provider_session_id}`,
                          )
                        : undefined
                    }
                  />
                  {/* One control for the send slot, not two buttons side by
                      side: while a turn is running the slot *becomes* stop.
                      This is the only stop control in the view — the header
                      and the live banner used to carry one each as well. */}
                  {isTurnExecuting ? (
                    <div className="remote-live-action-pair">
                      {(text.trim() || attachments.length > 0) && (
                        <button
                          type="button"
                          className="remote-queue-btn"
                          title="加入待发队列，当前轮次完成后自动发送"
                          disabled={!connected || busy}
                          onClick={enqueueCurrentPrompt}
                        >
                          排队
                        </button>
                      )}
                      {session?.binding.current_turn_id ? (
                        <button
                          type="button"
                          className="remote-interrupt-btn"
                          title="停止这一轮"
                          disabled={!connected || busy || submittingInterrupt}
                          onClick={() => {
                            if (selected) {
                              wasInterruptedBySessionRef.current[selected] =
                                true;
                            }
                            void submit({ action: "interrupt" });
                          }}
                        >
                          <svg
                            width="12"
                            height="12"
                            viewBox="0 0 24 24"
                            fill="currentColor"
                          >
                            <rect x="4" y="4" width="16" height="16" rx="2" />
                          </svg>
                          <span>
                            {submittingInterrupt ? "停止中…" : "停止"}
                          </span>
                        </button>
                      ) : null}
                    </div>
                  ) : (
                    <button
                      type="submit"
                      className={
                        canTakeControl
                          ? "remote-send-btn remote-send-takeover"
                          : "remote-send-btn"
                      }
                      title={
                        canTakeControl
                          ? "接管并发送"
                          : !connected
                            ? "等待会话连接"
                            : text.trim() || attachments.length > 0
                              ? "发送消息 (Enter)"
                              : "请输入消息内容"
                      }
                      aria-label={
                        canTakeControl
                          ? "接管并发送"
                          : text.trim() || attachments.length > 0
                            ? "发送消息"
                            : "请输入消息内容"
                      }
                      disabled={
                        !canSend ||
                        busy ||
                        (!text.trim() && attachments.length === 0)
                      }
                    >
                      {canTakeControl && <span>接管并发送</span>}
                      <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <line x1="12" y1="19" x2="12" y2="5" />
                        <polyline points="5 12 12 5 19 12" />
                      </svg>
                    </button>
                  )}
                </div>
              </div>
            </form>
          </div>
        )}
      </main>
      <RemoteLaunchModal
        open={launchModalOpen}
        busy={busy}
        runner={runner}
        workspace={workspace}
        nativeId={nativeId}
        agentType={agentType}
        model={model}
        sandbox={sandbox}
        launchPermissionMode={launchPermissionMode}
        onlineDeviceOptions={onlineDeviceOptions}
        launchWorkspaceOptions={launchWorkspaceOptions}
        launchSessionOptions={launchSessionOptions}
        launchAgentOptions={launchAgentOptions}
        launchModelOptions={launchModelOptions}
        sandboxOptions={SANDBOX_OPTIONS}
        permissionModeOptions={PERMISSION_MODE_OPTIONS}
        onClose={() => setLaunchModalOpen(false)}
        onSubmit={() => void launch()}
        onRunnerChange={(next) => {
          setRunner(next);
          setWorkspace(
            runners.find((item) => item.runner_id === next)?.hello
              ?.workspaces[0] ?? "",
          );
          setNativeId("");
          setModel("");
        }}
        onWorkspaceChange={(next) => {
          setWorkspace(next);
          setNativeId("");
        }}
        onNativeIdChange={setNativeId}
        onAgentTypeChange={(next) => {
          setAgentType(next);
          setModel("");
        }}
        onModelChange={setModel}
        onSandboxChange={setSandbox}
        onPermissionModeChange={setLaunchPermissionMode}
      />
      {settingsOpen && panelConfig !== undefined && (
        <Suspense fallback={null}>
          <SettingsPage
            account={account ?? undefined}
            config={panelConfig}
            notificationPresentedAt={notificationPresentedAt}
            onClose={() => setSettingsOpen(false)}
            onLoggedOut={() => {
              setSettingsOpen(false);
              onLoggedOut?.();
            }}
            onSave={(next) => {
              saveConfig(window.localStorage, next);
              setPanelConfig(next);
              setSettingsOpen(false);
              refresh.current();
            }}
            onWorkspaceChanged={() => refresh.current()}
          />
        </Suspense>
      )}
    </div>
  );
}
