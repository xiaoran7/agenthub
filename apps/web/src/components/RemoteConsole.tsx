"use client";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import type { AccountView } from "../lib/account";
import { safeRandomUUID } from "../lib/uuid";
import {
  conversationItems,
  groupConversationItems,
  isRedactedPathOnly,
  latestTurnFailure,
  pendingApprovals,
  pendingQuestions,
  readableRemoteText,
  remoteError,
  remoteRequest,
  type RemoteRequest,
  type RemoteSnapshot,
  type RemoteRunner,
  type RemoteSession,
  type RemoteEvent,
  type RemoteCommandRecord,
  type RemoteSessionDetail,
  type ConversationItem,
  type DiscoveryCandidate,
} from "../lib/remote";
import { MarkdownView } from "./MarkdownView";
import { RemoteContextCapacity } from "./RemoteContextCapacity";
import { SettingsPage } from "./SettingsPage";
import { parseNotificationPresentedAt } from "../lib/notifications";
import type { PanelConfig } from "../lib/config";
import { saveConfig } from "../lib/config";
import {
  buildSessionTree,
  cleanPath,
  getProviderInfo,
  workspaceLabel,
  workspaceTooltip,
  type ProviderKind,
} from "../lib/session-tree";
import "./remote.css";
import "./remote-desktop.css";
import {
  getStoredTheme,
  setTheme,
  initThemeListener,
  type ThemeMode,
} from "../lib/theme";

const empty: RemoteSnapshot = { cursor: 0, runners: [], sessions: [] };

/**
 * Session-detail refresh interval. The detail route already returns the event
 * cursor, so this only needs to be fast enough to feel live while typing; a
 * 200 ms tick made every open console fire 10 requests per second, doubled by
 * the event fetch, against a Gateway that is already polling its runners.
 */
const SESSION_POLL_MS = 1000;

/** Human label for a session's execution state, shared by the tree and header. */
const states: Record<string, string> = {
  idle: "就绪",
  running: "正在工作",
  awaiting_permission: "等待审批",
  awaiting_input: "等待输入",
  unknown: "状态待确认",
  error: "执行出错",
  completed: "已完成",
  terminated: "已结束",
};
const key = (session: RemoteSession) =>
  `${session.runner_id}/${session.binding.hub_session_id}`;

/**
 * The shapes a tool call actually takes in a transcript. A wall of identical
 * pencils is unreadable, so each shape gets its own glyph and its own accent.
 */
type ToolKind = "command" | "read" | "search" | "edit" | "web" | "tool";

/**
 * Matches only on the tool name, never on the MCP server prefix.
 *
 * `conversationItems` builds toolCall titles as `server · tool` ("shell · Read",
 * "mcp · fetch"), so matching the whole string would let the *server* decide the
 * glyph: every tool from the `shell` server would come back as a command no
 * matter what it actually did. The name is the last segment.
 */
function toolKind(item: ConversationItem): ToolKind {
  const title = item.title ?? "";
  const name = (title.split("·").pop() ?? title).toLowerCase().trim();
  const has = (...needles: string[]) => needles.some((n) => name.includes(n));
  if (
    has(
      "bash",
      "shell",
      "zsh",
      "powershell",
      "command",
      "exec",
      "terminal",
      "命令",
      "执行",
    )
  )
    return "command";
  if (has("web", "browse", "fetch", "http", "url", "联网", "网络"))
    return "web";
  if (
    has(
      "write",
      "edit",
      "create",
      "patch",
      "delete",
      "remove",
      "rename",
      "move",
      "写入",
      "编辑",
      "修改",
    )
  )
    return "edit";
  if (has("grep", "search", "glob", "find", "list", "ls", "搜索", "查找"))
    return "search";
  if (has("read", "open", "view", "notebook", "cat", "读取", "查看"))
    return "read";
  // No name signal: a command with no path in it is a shell invocation,
  // whatever it ended up being called.
  if (item.command && !/[/\\]/u.test(item.command)) return "command";
  return "tool";
}

/** Stroke-drawn glyph per tool shape; the accent colour comes from data-tool. */
const TOOL_PATHS: Record<ToolKind, React.ReactNode> = {
  command: (
    <>
      <polyline points="4 7 4 4 8 4" />
      <polyline points="16 4 20 4 20 7" />
      <polyline points="20 17 20 20 16 20" />
      <polyline points="8 20 4 20 4 17" />
      <line x1="7" y1="8" x2="17" y2="16" />
      <line x1="17" y1="8" x2="7" y2="16" />
    </>
  ),
  read: (
    <>
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <polyline points="14 3 14 8 19 8" />
      <line x1="9" y1="13" x2="15" y2="13" />
      <line x1="9" y1="17" x2="13" y2="17" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <line x1="20" y1="20" x2="16.4" y2="16.4" />
    </>
  ),
  edit: (
    <>
      <path d="M12 20h8" />
      <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L8 18l-4 1 1-4z" />
    </>
  ),
  web: (
    <>
      <circle cx="12" cy="12" r="9" />
      <line x1="3" y1="12" x2="21" y2="12" />
      <path d="M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z" />
    </>
  ),
  tool: (
    <path d="m14.7 6.3 3 3M4 20l5.3-1.3L20 8a2.1 2.1 0 0 0-3-3L6.3 15.7 5 21" />
  ),
};

function ToolIcon({ kind }: { kind: ToolKind }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {TOOL_PATHS[kind]}
    </svg>
  );
}

interface DiffStats {
  additions: number;
  deletions: number;
  oldText?: string | undefined;
  newText?: string | undefined;
}

function parseDiffStats(content?: string): DiffStats {
  if (!content) return { additions: 0, deletions: 0 };
  let additions = 0;
  let deletions = 0;
  let oldText: string | undefined;
  let newText: string | undefined;

  const trimmed = content.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const parsed = JSON.parse(trimmed);
      const oldStr = parsed.old ?? parsed.old_string;
      const newStr = parsed.new ?? parsed.new_string;
      if (typeof oldStr === "string" || typeof newStr === "string") {
        if (typeof oldStr === "string") {
          oldText = oldStr;
          deletions = oldStr
            .split("\n")
            .filter((l: string) => l.trim() !== "").length;
        }
        if (typeof newStr === "string") {
          newText = newStr;
          additions = newStr
            .split("\n")
            .filter((l: string) => l.trim() !== "").length;
        }
        return { additions, deletions, oldText, newText };
      }
    } catch {
      // not simple json
    }
  }

  const lines = content.split("\n");
  for (const line of lines) {
    if (
      line.startsWith("+++") ||
      line.startsWith("---") ||
      line.startsWith("@@")
    ) {
      continue;
    }
    if (line.startsWith("+")) additions++;
    else if (line.startsWith("-")) deletions++;
  }
  return { additions, deletions };
}

interface QuestionItem {
  question: string;
  header?: string | undefined;
  options?:
    | Array<{
        label: string;
        description?: string | undefined;
        preview?: string | undefined;
      }>
    | undefined;
  multiSelect?: boolean | undefined;
}

function parseQuestionData(content?: string): QuestionItem[] | null {
  if (!content) return null;
  const trimmed = content.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  try {
    const parsed = JSON.parse(trimmed);
    if (Array.isArray(parsed.questions) && parsed.questions.length > 0) {
      return parsed.questions as QuestionItem[];
    }
  } catch {
    // not json
  }
  return null;
}

const RemoteToolCard = memo(function RemoteToolCard({
  item,
}: {
  item: ConversationItem;
}) {
  const status = item.status ?? "completed";
  const kind = toolKind(item);
  const diffStats = parseDiffStats(item.command);
  const questionItems = parseQuestionData(item.command);
  const hasDiff = diffStats.additions > 0 || diffStats.deletions > 0;

  return (
    <details className={`remote-tool ${status}`} data-tool={kind}>
      <summary className="remote-tool-summary">
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
        <span className="remote-tool-header-left">
          <span className="remote-tool-glyph">
            <ToolIcon kind={kind} />
          </span>
          <span className="remote-tool-title">{item.title ?? "工具调用"}</span>
          {hasDiff && (
            <span
              className="remote-diff-badge"
              aria-label={`+${diffStats.additions} -${diffStats.deletions}`}
            >
              {diffStats.additions > 0 && (
                <span className="diff-add">+{diffStats.additions}</span>
              )}
              {diffStats.deletions > 0 && (
                <span className="diff-del">-{diffStats.deletions}</span>
              )}
            </span>
          )}
        </span>
        <span className={`remote-tool-status ${status}`}>
          {status === "running" && (
            <span className="remote-tool-spinner" aria-hidden="true" />
          )}
          {status === "running"
            ? "执行中"
            : status === "failed"
              ? "失败"
              : "完成"}
        </span>
      </summary>
      {(item.description || item.command || item.output) && (
        <div className="remote-tool-content">
          {item.description && (
            <p className="remote-tool-description">
              {readableRemoteText(item.description)}
            </p>
          )}
          {questionItems ? (
            <div className="remote-question-card">
              {questionItems.map((q, qIdx) => (
                <div key={qIdx} className="remote-question-item">
                  <div className="remote-question-header">
                    {q.header && (
                      <span className="remote-question-tag">{q.header}</span>
                    )}
                    <span className="remote-question-text">{q.question}</span>
                  </div>
                  {q.options && q.options.length > 0 && (
                    <div className="remote-options-list">
                      {q.options.map((opt, optIdx) => {
                        const isSelected =
                          item.output?.includes(opt.label) ?? false;
                        return (
                          <div
                            key={optIdx}
                            className={`remote-option-item ${isSelected ? "selected" : ""}`}
                          >
                            <div className="remote-option-label">
                              <span>{opt.label}</span>
                              {isSelected && (
                                <span className="remote-option-badge">
                                  已选择
                                </span>
                              )}
                            </div>
                            {opt.description && (
                              <div className="remote-option-desc">
                                {opt.description}
                              </div>
                            )}
                            {opt.preview && (
                              <pre className="remote-option-preview">
                                {opt.preview}
                              </pre>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ) : diffStats.oldText || diffStats.newText ? (
            <div className="remote-diff-viewer">
              {diffStats.oldText && (
                <div className="remote-diff-chunk del">
                  <div className="remote-diff-tag">- 变更前</div>
                  <pre className="remote-diff-pre">{diffStats.oldText}</pre>
                </div>
              )}
              {diffStats.newText && (
                <div className="remote-diff-chunk add">
                  <div className="remote-diff-tag">+ 变更后</div>
                  <pre className="remote-diff-pre">{diffStats.newText}</pre>
                </div>
              )}
            </div>
          ) : item.command ? (
            <div>
              <div className="remote-tool-subhead">命令 / 参数</div>
              <pre className="remote-tool-cmd">
                {readableRemoteText(item.command)}
              </pre>
            </div>
          ) : null}
          {item.output && !questionItems && (
            <div>
              <div className="remote-tool-subhead">执行结果</div>
              <pre className="remote-tool-output">
                {readableRemoteText(item.output)}
              </pre>
            </div>
          )}
        </div>
      )}
    </details>
  );
});

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
  return (
    <details className="remote-tool-group">
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
        <span>已执行 {items.length} 个工具与操作</span>
        <span className="remote-tool-group-preview">
          {items
            .slice(0, 3)
            .map((item) => {
              const detail = item.description ?? item.command;
              return detail && !detail.includes("[local_path_redacted]")
                ? `${item.title ?? "工具调用"} · ${detail.slice(0, 34)}`
                : (item.title ?? "工具调用");
            })
            .join(" · ")}
        </span>
        <span className={`remote-tool-status ${status}`}>
          {status === "running"
            ? "执行中"
            : status === "failed"
              ? "异常"
              : "完成"}
        </span>
      </summary>
      <div className="remote-tool-group-body">
        {items.map((item) => (
          <RemoteToolCard key={item.id} item={item} />
        ))}
      </div>
    </details>
  );
});

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
  const [events, setEvents] = useState<RemoteEvent[]>([]);
  const [commands, setCommands] = useState<RemoteCommandRecord[]>([]);
  const [error, setError] = useState("");
  const [text, setText] = useState("");
  const [questionAnswers, setQuestionAnswers] = useState<
    Record<string, string>
  >({});
  const [submittingApproval, setSubmittingApproval] = useState<{
    id: string;
    action: "approve" | "decline";
  } | null>(null);
  const [copiedApprovalId, setCopiedApprovalId] = useState<string | null>(null);
  const [submittingQuestionId, setSubmittingQuestionId] = useState<
    string | null
  >(null);
  const [selectedOptions, setSelectedOptions] = useState<
    Record<string, string[]>
  >({});
  const [effort, setEffort] = useState<
    "default" | "low" | "medium" | "high" | "max"
  >("default");
  const [submittingInterrupt, setSubmittingInterrupt] = useState(false);
  const [busy, setBusy] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [notificationPresentedAt, setNotificationPresentedAt] = useState<
    number | undefined
  >(undefined);
  const [followBottom, setFollowBottom] = useState(true);
  // A dismissed failure stays dismissed until the session changes or a new
  // failure arrives, so it cannot reappear on every poll.
  const [dismissedFailure, setDismissedFailure] = useState<string | null>(null);
  // Keep an uncertain POST's identity when the user retries the same operation.
  const attempts = useRef(new Map<string, RemoteCommand>());
  const autoResumedRef = useRef<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);

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
        const targetSession = event.data.session;
        if (typeof targetSession === "string") {
          setSelected(targetSession);
        } else if (targetSession && typeof targetSession === "object") {
          const found = snapshotRef.current.sessions.find(
            (s) =>
              s.runner_id === targetSession.device_id &&
              s.binding.provider_session_id === targetSession.agent_session_id,
          );
          if (found) {
            setSelected(`${found.runner_id}/${found.binding.hub_session_id}`);
          }
        }
      }
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
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
  const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [currentTheme, setCurrentTheme] = useState<ThemeMode>(() =>
    getStoredTheme(),
  );

  useEffect(() => {
    return initThemeListener((mode) => {
      setCurrentTheme(mode);
    });
  }, []);

  useEffect(() => {
    const checkMobile = () => {
      setIsMobile(window.innerWidth <= 700);
    };
    checkMobile();
    window.addEventListener("resize", checkMobile);
    return () => window.removeEventListener("resize", checkMobile);
  }, []);
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

  const [providerFilter, setProviderFilter] = useState<ProviderKind | "all">(
    "all",
  );
  const [runner, setRunner] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [agentType, setAgentType] = useState<"zcode" | "codex" | "claude_code">(
    "zcode",
  );
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [nativeId, setNativeId] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const timeline = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const refresh = useRef(() => {});
  const session = snapshot.sessions.find((item) => key(item) === selected);
  const runnerInfo = snapshot.runners.find((item) => item.runner_id === runner);
  const connected = !!session?.online && !!session.binding.control_connected;
  const canSend =
    connected &&
    ["idle", "completed"].includes(session?.binding.execution_state ?? "");

  const adjustTextareaHeight = () => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(Math.max(el.scrollHeight, 28), 220)}px`;
  };
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    let inFlight = false;
    const poll = async () => {
      if (stopped || inFlight) return;
      inFlight = true;
      try {
        const value = await request<RemoteSnapshot>("/sessions");
        if (!stopped) {
          setSnapshot(value);
          setLoading(false);
        }
      } catch (err) {
        if (!stopped) {
          setError(remoteError(err));
          setLoading(false);
          if (err instanceof Error && err.message === "authentication_required")
            onAuthenticationRequired?.();
        }
      } finally {
        inFlight = false;
        if (!stopped) timer = setTimeout(() => void poll(), 1500);
      }
    };
    refresh.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [request, onAuthenticationRequired]);
  useEffect(() => {
    setEvents([]);
    setCommands([]);
    setText("");
    setQuestionAnswers({});
    setSelectedOptions({});
    setSubmittingApproval(null);
    setSubmittingQuestionId(null);
    setFollowBottom(true);
    setDismissedFailure(null);
    if (!selected) return;
    const [runnerId, hub] = selected.split("/");
    if (!runnerId || !hub) return;
    let stopped = false;
    let inFlight = false;
    let cursor = 0;
    let accumulated: RemoteEvent[] = [];
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (stopped || inFlight) return;
      inFlight = true;
      try {
        // One request, not two. The session detail route already carries the
        // binding, the event cursor and the command history, so the event fetch
        // below only runs when the cursor has actually moved.
        const detail = await request<RemoteSessionDetail>(
          `/sessions/${encodeURIComponent(runnerId)}/${encodeURIComponent(hub)}`,
        );
        if (stopped) return;
        if (detail.cursor > cursor) {
          const batch = await request<{ events: RemoteEvent[] }>(
            `/events?runner=${encodeURIComponent(runnerId)}&hub=${encodeURIComponent(hub)}&after=${cursor}`,
          );
          if (stopped) return;
          cursor = batch.events.length
            ? batch.events[batch.events.length - 1]!.cursor
            : detail.cursor;
          accumulated = [...accumulated, ...batch.events];
          setEvents([...accumulated]);
        }
        // `session` is derived from the 1.5 s snapshot poll, which is where
        // `control_connected` is refreshed; the copy on `detail` would be a
        // second source of truth for the same field.
        setCommands(detail.commands);
      } catch (err) {
        if (!stopped) setError(remoteError(err));
      } finally {
        inFlight = false;
        if (!stopped) timer = setTimeout(() => void poll(), SESSION_POLL_MS);
      }
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [request, selected]);
  const items = useMemo(
    () => conversationItems(events, commands),
    [events, commands],
  );
  const itemGroups = useMemo(() => groupConversationItems(items), [items]);
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
  }, [items, followBottom]);

  // 核心特性：选定会话时全量自动开始恢复接入，无需手动点击恢复
  useEffect(() => {
    if (
      !session ||
      !session.online ||
      session.binding.control_connected ||
      busy
    )
      return;
    const sessionKey = `${key(session)}/${session.binding.host_generation}`;
    if (autoResumedRef.current.has(sessionKey)) return;
    autoResumedRef.current.add(sessionKey);
    void submit(
      {
        action: "connect",
        mode: "resume",
        workspace_path: session.binding.workspace_path,
        provider_session_id: session.binding.provider_session_id,
      },
      session,
    );
  }, [
    selected,
    session?.online,
    session?.binding.control_connected,
    session?.binding.host_generation,
    busy,
  ]);

  const latestFailure = [...commands]
    .reverse()
    .find((item) =>
      ["rejected", "indeterminate"].includes(item.receipt?.status ?? ""),
    );
  const handleCopyText = (id: string, textToCopy: string) => {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(textToCopy);
      setCopiedApprovalId(id);
      setTimeout(() => {
        setCopiedApprovalId((current) => (current === id ? null : current));
      }, 2000);
    }
  };

  const submit = async (action: RemoteCommand["command"], target = session) => {
    if (!target) return;
    setBusy(true);
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
    const attemptKey = JSON.stringify([
      target.runner_id,
      target.binding.hub_session_id,
      action,
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id: target.binding.hub_session_id,
        host_generation: target.binding.host_generation,
        command: action,
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(
        `/runners/${encodeURIComponent(target.runner_id)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      if (action.action === "send_prompt") {
        setText("");
        if (textareaRef.current) textareaRef.current.style.height = "auto";
      }
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
      setSubmittingApproval(null);
      setSubmittingQuestionId(null);
      setSubmittingInterrupt(false);
    }
  };
  const launch = async () => {
    const launchRunner = snapshot.runners.find(
      (item) => item.runner_id === runner,
    );
    const generation = launchRunner?.hello?.host_generation;
    if (!runner || !workspace || !generation) return;
    setBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      runner,
      workspace,
      nativeId,
      agentType,
      sandbox,
      model,
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id: safeRandomUUID(),
        host_generation: generation,
        command: {
          action: "connect",
          mode: nativeId ? "resume" : "create",
          workspace_path: workspace,
          sandbox,
          approval_policy: "untrusted",
          ...(model ? { model } : {}),
          ...(nativeId
            ? { provider_session_id: nativeId }
            : { agent_type: agentType }),
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(`/runners/${encodeURIComponent(runner)}/commands`, command);
      attempts.current.delete(attemptKey);
      setSelected(`${runner}/${command.hub_session_id}`);
      dialog.current?.close();
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
    }
  };
  const resumeCandidate = async (
    targetRunnerId: string,
    targetWorkspace: string,
    providerSessionId: string,
  ) => {
    const runnerObj = snapshot.runners.find(
      (item) => item.runner_id === targetRunnerId,
    );
    const generation = runnerObj?.hello?.host_generation;
    if (!runnerObj || !generation || busy) return;
    setBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      targetRunnerId,
      targetWorkspace,
      providerSessionId,
      "read-only",
      "",
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id: safeRandomUUID(),
        host_generation: generation,
        command: {
          action: "connect",
          mode: "resume",
          workspace_path: targetWorkspace,
          sandbox: "read-only",
          approval_policy: "untrusted",
          provider_session_id: providerSessionId,
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(
        `/runners/${encodeURIComponent(targetRunnerId)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelected(`${targetRunnerId}/${command.hub_session_id}`);
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
    }
  };

  // Each tree level collapses independently, so the open path down to the
  // selected session survives switching agents or devices.
  const [collapsedDevices, setCollapsedDevices] = useState<
    Record<string, boolean>
  >({});
  const [collapsedAgents, setCollapsedAgents] = useState<
    Record<string, boolean>
  >({});
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<
    Record<string, boolean>
  >({});

  const toggleNode = (
    setter: React.Dispatch<React.SetStateAction<Record<string, boolean>>>,
    nodeKey: string,
  ) => setter((prev) => ({ ...prev, [nodeKey]: !prev[nodeKey] }));

  const discovered = useMemo(() => {
    const candidates = new Map<string, DiscoveryCandidate>();
    for (const r of snapshot.runners)
      for (const candidate of r.hello?.discovery?.candidates ?? [])
        candidates.set(
          `${r.runner_id}/${candidate.provider_session_id}`,
          candidate,
        );
    return candidates;
  }, [snapshot.runners]);

  const deviceGroups = useMemo(
    () =>
      buildSessionTree({
        discovered,
        devices: snapshot.runners.map((runner) => ({
          runner_id: runner.runner_id,
          online: runner.online,
          hello: runner.hello as {
            hostname?: unknown;
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
          online: item.online,
          control_connected: item.binding.control_connected,
          execution_state: item.binding.execution_state,
        })),
        candidates: snapshot.runners.flatMap((runner) =>
          (runner.hello?.discovery?.candidates ?? []).map((candidate) => ({
            runner_id: runner.runner_id,
            provider_session_id: candidate.provider_session_id,
            workspace_path: candidate.workspace_path,
            title: candidate.title,
            provider: candidate.provider,
            resumable: candidate.resumable,
          })),
        ),
        providerFilter,
      }),
    [snapshot.sessions, snapshot.runners, providerFilter, discovered],
  );

  /** Agent folders that actually exist, for the filter row. */
  const availableProviders = useMemo(() => {
    const seen = new Map<ProviderKind, string>();
    for (const device of deviceGroups)
      for (const agent of device.workspaces)
        seen.set(agent.provider, agent.label);
    return seen;
  }, [deviceGroups]);

  const pendingApprovalsBySessionKey = useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of snapshot.runners) {
      if (r.hello?.pending_approvals) {
        for (const pa of r.hello.pending_approvals) {
          const k = `${r.runner_id}/${pa.hub_session_id}`;
          counts.set(k, (counts.get(k) ?? 0) + 1);
        }
      }
    }
    return counts;
  }, [snapshot.runners]);

  /**
   * The folder path to the selected session, so the user can always see where
   * they are in a four-level tree and step back out of it.
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
              { key: device.runnerId, label: device.label },
              {
                key: `${device.runnerId}/${agent.provider}`,
                label: agent.label,
              },
              {
                key: `${device.runnerId}/${agent.provider}/${workspace.id}`,
                label: workspace.label,
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

  return (
    <div className="remote-console">
      {/* 移动端抽屉遮罩 */}
      {isMobile && (
        <div
          className={`remote-drawer-backdrop ${mobileDrawerOpen ? "open" : ""}`}
          onClick={() => setMobileDrawerOpen(false)}
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
          <button
            type="button"
            className="remote-icon-btn"
            title={isMobile ? "关闭菜单" : "收起侧边栏"}
            aria-label={isMobile ? "关闭菜单" : "收起侧边栏"}
            onClick={
              isMobile ? () => setMobileDrawerOpen(false) : toggleSidebar
            }
          >
            {isMobile ? (
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            ) : (
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
                <path d="M15 9l-3 3 3 3" />
              </svg>
            )}
          </button>
        </div>
        <button
          className="remote-primary"
          onClick={() => {
            const available = snapshot.runners.find(
              (item) => item.online && item.hello?.workspaces.length,
            );
            setRunner(available?.runner_id ?? "");
            setWorkspace(available?.hello?.workspaces[0] ?? "");
            setNativeId("");
            setModel("");
            dialog.current?.showModal();
          }}
        >
          ＋ 新建会话
        </button>
        <nav className="remote-breadcrumb" aria-label="会话位置">
          <button
            type="button"
            className="remote-breadcrumb-root"
            data-current={selectedSessionPath === undefined}
            onClick={() => {
              setSelected("");
              if (isMobile) setMobileDrawerOpen(false);
            }}
          >
            全部会话
          </button>
          {selectedSessionPath?.map((crumb, index) => {
            const isLast = index === selectedSessionPath.length - 1;
            return (
              <span className="remote-breadcrumb-item" key={crumb.key}>
                <span className="remote-breadcrumb-sep" aria-hidden="true">
                  /
                </span>
                <button
                  type="button"
                  className="remote-breadcrumb-crumb"
                  data-current={isLast}
                  onClick={() => {
                    if (isLast) return;
                    if (isMobile) setMobileDrawerOpen(false);
                    if (crumb.sessionKey !== undefined) {
                      setSelected(crumb.sessionKey);
                    } else {
                      // Backing out to a folder: collapse everything below it
                      // so the tree returns to that level instead of leaving
                      // a deeper level open with nothing highlighted.
                      setCollapsedDevices((prev) => ({
                        ...prev,
                        [crumb.key]: true,
                      }));
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
        <div className="remote-filter-pill-row">
          <button
            type="button"
            className={`remote-filter-pill ${providerFilter === "all" ? "active" : ""}`}
            onClick={() => setProviderFilter("all")}
          >
            全部
          </button>
          {[...availableProviders].map(([kind, label]) => (
            <button
              type="button"
              className={`remote-filter-pill ${providerFilter === kind ? "active" : ""}`}
              key={kind}
              onClick={() => setProviderFilter(kind)}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="remote-nav-label">设备 / Agent / 工作区</div>
        <div className="remote-session-tree">
          {deviceGroups.map((dev) => {
            const isDevCollapsed = collapsedDevices[dev.runnerId] === true;
            return (
              <section className="remote-device-group" key={dev.runnerId}>
                <button
                  type="button"
                  className="remote-device-header"
                  onClick={() => toggleNode(setCollapsedDevices, dev.runnerId)}
                  aria-expanded={!isDevCollapsed}
                  title={dev.online ? dev.label : `${dev.label}（离线）`}
                >
                  <span
                    className={`remote-tree-caret ${
                      isDevCollapsed ? "collapsed" : ""
                    }`}
                    aria-hidden="true"
                  >
                    ▾
                  </span>
                  <span className="remote-device-title">{dev.label}</span>
                  <span
                    className={`remote-status-dot ${dev.online ? "online" : "offline"}`}
                    title={dev.online ? "设备在线" : "设备离线"}
                  />
                  <span className="remote-node-count">{dev.totalCount}</span>
                </button>

                {!isDevCollapsed && (
                  <div className="remote-node-children">
                    {dev.workspaces.length === 0 ? (
                      <p className="remote-tree-empty">
                        {dev.online ? "暂无会话" : "电脑离线"}
                      </p>
                    ) : (
                      dev.workspaces.map((agentGroup) => {
                        const agentKey = `${dev.runnerId}/${agentGroup.provider}`;
                        const isAgentCollapsed =
                          collapsedAgents[agentKey] === true;
                        return (
                          <div className="remote-agent-group" key={agentKey}>
                            <button
                              type="button"
                              className="remote-node-header"
                              data-level="agent"
                              onClick={() =>
                                toggleNode(setCollapsedAgents, agentKey)
                              }
                              aria-expanded={!isAgentCollapsed}
                              title={`${agentGroup.label} · ${agentGroup.totalCount} 个会话`}
                            >
                              <span
                                className={`remote-tree-caret ${
                                  isAgentCollapsed ? "collapsed" : ""
                                }`}
                                aria-hidden="true"
                              >
                                ▾
                              </span>
                              <span
                                className={`remote-agent-badge ${agentGroup.badgeClass}`}
                              >
                                {agentGroup.label}
                              </span>
                              <span className="remote-node-count">
                                {agentGroup.totalCount}
                              </span>
                            </button>

                            {!isAgentCollapsed && (
                              <div className="remote-node-children">
                                {agentGroup.workspaces.map((workspaceGroup) => {
                                  const workspaceKey = `${agentKey}/${workspaceGroup.id}`;
                                  const isWorkspaceCollapsed =
                                    collapsedWorkspaces[workspaceKey] === true;
                                  return (
                                    <div
                                      className="remote-workspace-group"
                                      key={workspaceKey}
                                    >
                                      <button
                                        type="button"
                                        className="remote-node-header"
                                        data-level="workspace"
                                        onClick={() =>
                                          toggleNode(
                                            setCollapsedWorkspaces,
                                            workspaceKey,
                                          )
                                        }
                                        aria-expanded={!isWorkspaceCollapsed}
                                        title={workspaceTooltip(
                                          workspaceGroup.id,
                                        )}
                                      >
                                        <span
                                          className={`remote-tree-caret ${
                                            isWorkspaceCollapsed
                                              ? "collapsed"
                                              : ""
                                          }`}
                                          aria-hidden="true"
                                        >
                                          ▾
                                        </span>
                                        <svg
                                          className="remote-workspace-icon"
                                          width="14"
                                          height="14"
                                          viewBox="0 0 24 24"
                                          fill="none"
                                          stroke="currentColor"
                                          strokeWidth="1.8"
                                          aria-hidden="true"
                                        >
                                          <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                                        </svg>
                                        <span className="remote-workspace-name">
                                          {workspaceGroup.label}
                                        </span>
                                        <span className="remote-node-count">
                                          {workspaceGroup.items.length}
                                        </span>
                                      </button>

                                      {!isWorkspaceCollapsed && (
                                        <div className="remote-node-sessions">
                                          {workspaceGroup.items.map((item) => {
                                            if (item.kind === "active") {
                                              const activeSession =
                                                snapshot.sessions.find(
                                                  (entry) =>
                                                    key(entry) ===
                                                    item.sessionKey,
                                                );
                                              const isSelected =
                                                item.sessionKey === selected;
                                              const pendingCount =
                                                pendingApprovalsBySessionKey.get(
                                                  item.sessionKey,
                                                ) ?? 0;
                                              return (
                                                <button
                                                  className={`remote-session ${isSelected ? "selected" : ""}`}
                                                  key={item.key}
                                                  onClick={() => {
                                                    if (
                                                      activeSession ===
                                                      undefined
                                                    )
                                                      return;
                                                    if (isSelected) {
                                                      if (isMobile)
                                                        setMobileDrawerOpen(
                                                          false,
                                                        );
                                                      return;
                                                    }
                                                    setSelected(
                                                      item.sessionKey,
                                                    );
                                                    setError("");
                                                    if (isMobile)
                                                      setMobileDrawerOpen(
                                                        false,
                                                      );
                                                    if (
                                                      activeSession.online &&
                                                      !activeSession.binding
                                                        .control_connected
                                                    ) {
                                                      void submit(
                                                        {
                                                          action: "connect",
                                                          mode: "resume",
                                                          workspace_path:
                                                            activeSession
                                                              .binding
                                                              .workspace_path,
                                                          provider_session_id:
                                                            activeSession
                                                              .binding
                                                              .provider_session_id,
                                                        },
                                                        activeSession,
                                                      );
                                                    }
                                                  }}
                                                >
                                                  <span className="remote-session-title">
                                                    {item.title}
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
                                                  </span>
                                                  <span className="remote-session-meta">
                                                    <span
                                                      className={`remote-state-dot ${
                                                        item.session
                                                          .execution_state ===
                                                        "running"
                                                          ? "running"
                                                          : item.session
                                                                .execution_state ===
                                                                "awaiting_permission" ||
                                                              item.session
                                                                .execution_state ===
                                                                "awaiting_input"
                                                            ? "waiting"
                                                            : "idle"
                                                      }`}
                                                      aria-hidden="true"
                                                    />
                                                    {states[
                                                      item.session
                                                        .execution_state
                                                    ] ?? "状态待确认"}
                                                  </span>
                                                </button>
                                              );
                                            }

                                            return (
                                              <button
                                                className="remote-session candidate"
                                                key={item.key}
                                                title={
                                                  item.candidate.resumable ===
                                                  false
                                                    ? "电脑端目前仅发现会话元数据，尚不能远程恢复"
                                                    : `点击恢复：${item.title}`
                                                }
                                                disabled={
                                                  busy ||
                                                  item.candidate.resumable ===
                                                    false
                                                }
                                                onClick={() => {
                                                  if (isMobile)
                                                    setMobileDrawerOpen(false);
                                                  void resumeCandidate(
                                                    item.runnerId,
                                                    item.workspaceId,
                                                    item.providerSessionId,
                                                  );
                                                }}
                                              >
                                                <span className="remote-session-title">
                                                  {item.title}
                                                </span>
                                                <span className="remote-session-meta">
                                                  {item.candidate.resumable ===
                                                  false
                                                    ? "仅发现"
                                                    : "点击恢复"}
                                                </span>
                                              </button>
                                            );
                                          })}
                                        </div>
                                      )}
                                    </div>
                                  );
                                })}
                              </div>
                            )}
                          </div>
                        );
                      })
                    )}
                  </div>
                )}
              </section>
            );
          })}
        </div>
        {!deviceGroups.length && (
          <p className="remote-muted">连接电脑后，在工作区中开始会话。</p>
        )}
        <div className="remote-sidebar-footer">
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              padding: "2px 4px 6px",
              borderBottom: "1px solid var(--remote-line)",
              marginBottom: "4px",
            }}
          >
            <span
              style={{
                fontSize: "11px",
                color: "var(--remote-muted)",
                fontFamily: "var(--font-mono)",
                letterSpacing: "0.04em",
              }}
            >
              THEME
            </span>
            <div style={{ display: "inline-flex", gap: "3px" }}>
              {(
                [
                  {
                    mode: "light",
                    label: "浅色",
                    icon: "☀️",
                    title: "温润纸张 (Warm Paper)",
                  },
                  {
                    mode: "dark",
                    label: "深色",
                    icon: "🌙",
                    title: "黑曜石暖炭 (Obsidian Charcoal)",
                  },
                  {
                    mode: "system",
                    label: "系统",
                    icon: "🖥️",
                    title: "跟随系统设置",
                  },
                ] as const
              ).map((t) => (
                <button
                  key={t.mode}
                  type="button"
                  onClick={() => {
                    setTheme(t.mode);
                    setCurrentTheme(t.mode);
                  }}
                  title={t.title}
                  style={{
                    padding: "2px 6px",
                    fontSize: "11px",
                    border: "none",
                    borderRadius: "5px",
                    background:
                      currentTheme === t.mode
                        ? "var(--remote-selected)"
                        : "transparent",
                    color:
                      currentTheme === t.mode
                        ? "var(--remote-terracotta)"
                        : "var(--remote-muted)",
                    fontWeight: currentTheme === t.mode ? 600 : 400,
                    cursor: "pointer",
                    lineHeight: "1.4",
                  }}
                >
                  <span style={{ marginRight: "2px", fontSize: "10px" }}>
                    {t.icon}
                  </span>
                  {t.label}
                </button>
              ))}
            </div>
          </div>
          <div className="remote-runner-status">
            <span
              className={`remote-status-dot ${snapshot.runners.some((item) => item.online) ? "online" : "offline"}`}
            />
            <span>
              {snapshot.runners.filter((item) => item.online).length} 台电脑在线
            </span>
          </div>
          {account?.user && (
            <div className="remote-account-bar">
              <span className="remote-account-name" title={account.user.email}>
                {account.user.display_name || account.user.email}
              </span>
              <button
                className="remote-logout-btn"
                onClick={() => setSettingsOpen(true)}
                type="button"
                title="设置"
              >
                设置
              </button>
              {onLogout && (
                <button
                  className="remote-logout-btn"
                  onClick={onLogout}
                  type="button"
                >
                  退出
                </button>
              )}
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
      <main className="remote-main">
        <header>
          <div className="remote-header-left">
            {isMobile ? (
              <button
                type="button"
                className="remote-icon-btn mobile-menu-btn"
                title="打开菜单"
                aria-label="打开菜单"
                onClick={() => setMobileDrawerOpen(true)}
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
              <h1>
                {session
                  ? workspaceLabel(session.binding.workspace_path)
                  : "随时接着做"}
              </h1>
              <p>
                {session
                  ? `${getProviderInfo(session.binding.provider_session_id, discovered.get(`${session.runner_id}/${session.binding.provider_session_id}`)?.provider).name} · ${session.online ? (connected ? "会话已连接" : "会话未连接") : "电脑离线"}`
                  : "电脑执行，随处继续。"}
              </p>
            </div>
          </div>
          {session?.binding.current_turn_id && (
            <button
              disabled={!connected || busy}
              onClick={() => void submit({ action: "interrupt" })}
            >
              停止任务
            </button>
          )}
        </header>
        {error && (
          <div role="alert" className="remote-notice">
            {error}
            <button aria-label="关闭提示" onClick={() => setError("")}>
              ×
            </button>
          </div>
        )}
        {latestFailure && (
          <div className="remote-notice" role="status">
            {remoteError(new Error(latestFailure.receipt?.code))}
          </div>
        )}
        {turnFailure &&
          !turnFailure.retrying &&
          turnFailure.message !== dismissedFailure && (
            <div className="remote-notice remote-notice-error" role="alert">
              <div className="remote-notice-error-body">
                <strong>这一轮没有完成</strong>
                <span>{readableRemoteText(turnFailure.message)}</span>
              </div>
              <button
                aria-label="关闭提示"
                onClick={() => setDismissedFailure(turnFailure.message)}
              >
                ×
              </button>
            </div>
          )}
        {!selected ? (
          <div className="remote-welcome">
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
                  action: () => {
                    const available = snapshot.runners.find(
                      (item) => item.online && item.hello?.workspaces?.length,
                    );
                    setRunner(
                      available?.runner_id ??
                        snapshot.runners[0]?.runner_id ??
                        "",
                    );
                    setWorkspace(available?.hello?.workspaces[0] ?? "");
                    setNativeId("");
                    setModel("");
                    dialog.current?.showModal();
                  },
                },
                {
                  title: "外观设置",
                  desc: "在温润天然纸张与黑曜石暖炭间切换",
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
          <>
            {!session && <p className="remote-muted">正在等待电脑确认会话…</p>}
            {session && !connected && (
              <div className="remote-notice restoring">
                <span className="remote-notice-spinner" />
                <span>
                  {session.online
                    ? "正在全量恢复连接原生 Agent 会话…"
                    : "电脑处于离线状态，上线后将自动恢复接入。"}
                </span>
                {session.online && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      void submit({
                        action: "connect",
                        mode: "resume",
                        workspace_path: session.binding.workspace_path,
                        provider_session_id:
                          session.binding.provider_session_id,
                      })
                    }
                  >
                    重试连接
                  </button>
                )}
              </div>
            )}
            <div
              className="remote-timeline"
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
                if (
                  item.kind === "assistant" &&
                  isRedactedPathOnly(item.text)
                ) {
                  return (
                    <p className="remote-private-note" key={item.id}>
                      本机路径已隐藏
                    </p>
                  );
                }
                return (
                  <article
                    className={`remote-message ${item.kind}`}
                    key={item.id}
                  >
                    {item.kind !== "user" && (
                      <div className="remote-turn-tag">
                        <span
                          className="remote-turn-marker"
                          aria-hidden="true"
                        />
                        <small>
                          {
                            getProviderInfo(
                              session?.binding.provider_session_id ?? "",
                            ).name
                          }
                        </small>
                      </div>
                    )}
                    <div className="remote-message-body">
                      <MarkdownView
                        content={readableRemoteText(item.text)}
                        streaming={Boolean(
                          item.kind === "assistant" &&
                          session?.binding.current_turn_id &&
                          item === items[items.length - 1],
                        )}
                      />
                    </div>
                  </article>
                );
              })}
              {!items.length && session && (
                <p className="remote-muted remote-timeline-empty">
                  会话已建立，可以发送第一条消息。
                </p>
              )}
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
                    className="remote-approval"
                    key={approval.approval_id}
                  >
                    <div className="remote-approval-header">
                      <div className="remote-approval-title-wrap">
                        <h3>需要你的确认</h3>
                        <span className="remote-approval-tool-badge">
                          {approval.tool_name ?? "系统工具"}
                        </span>
                      </div>
                      {approval.risk_level && (
                        <span
                          className={`remote-approval-badge ${approval.risk_level}`}
                        >
                          {approval.risk_level === "high"
                            ? "高风险"
                            : approval.risk_level === "medium"
                              ? "中风险"
                              : "低风险"}
                        </span>
                      )}
                    </div>
                    <div className="remote-approval-desc">
                      <strong>
                        {
                          getProviderInfo(
                            session?.binding.provider_session_id ?? "",
                          ).name
                        }
                      </strong>{" "}
                      请求执行【{approval.tool_name ?? "系统工具"}】
                      {approval.description && `：${approval.description}`}
                    </div>
                    {approval.command_preview && (
                      <div className="remote-approval-box-wrapper">
                        <div className="remote-approval-box-bar">
                          <span className="remote-approval-box-label">
                            待执行命令
                          </span>
                          <button
                            type="button"
                            className="remote-approval-copy-btn"
                            onClick={() =>
                              handleCopyText(
                                approval.approval_id,
                                approval.command_preview!,
                              )
                            }
                            title="复制命令"
                          >
                            {isCopied ? "✓ 已复制" : "复制命令"}
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
                            涉及路径
                          </span>
                          <button
                            type="button"
                            className="remote-approval-copy-btn"
                            onClick={() =>
                              handleCopyText(
                                approval.approval_id,
                                approval.path!,
                              )
                            }
                            title="复制路径"
                          >
                            {isCopied ? "✓ 已复制" : "复制路径"}
                          </button>
                        </div>
                        <div className="remote-approval-command-box">
                          {approval.path}
                        </div>
                      </div>
                    )}
                    <div className="remote-approval-actions">
                      <button
                        type="button"
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
                        {isDeclining ? "处理中…" : "拒绝"}
                      </button>
                      <button
                        type="button"
                        className="remote-primary"
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
                        {isApproving ? "处理中…" : "允许此次操作"}
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
                  <section className="remote-question" key={q.question_id}>
                    <div className="remote-question-header">
                      <span>{q.header ?? "Agent 向你提问"}</span>
                      {isMulti && (
                        <span className="remote-question-type-badge">
                          可多选
                        </span>
                      )}
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
                                    {isOptionSelected ? "✓" : ""}
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
                if (canSend && text.trim()) {
                  void submit({
                    action: "send_prompt",
                    text,
                    ...(effort !== "default" ? { effort } : {}),
                  });
                  if (textareaRef.current)
                    textareaRef.current.style.height = "auto";
                }
              }}
            >
              <div className="remote-composer-input-wrapper">
                <textarea
                  ref={textareaRef}
                  id="remote-prompt"
                  value={text}
                  onChange={(event) => {
                    setText(event.target.value);
                    adjustTextareaHeight();
                  }}
                  onKeyDown={(event) => {
                    // 移动端不通过回车触发发送，防止输入法确认词时误触发送
                    if (!isMobile && event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      if (canSend && text.trim()) {
                        void submit({
                          action: "send_prompt",
                          text,
                          ...(effort !== "default" ? { effort } : {}),
                        });
                        if (textareaRef.current)
                          textareaRef.current.style.height = "auto";
                      }
                    }
                  }}
                  placeholder={
                    !session
                      ? "等待电脑确认会话…"
                      : !session.online
                        ? "电脑离线，无法发送消息"
                        : !connected
                          ? "会话正在自动恢复接入中…"
                          : canSend
                            ? isMobile
                              ? "继续这段工作…"
                              : "继续这段工作… (Enter 发送, Shift+Enter 换行)"
                            : "Agent 正在执行中…"
                  }
                  rows={1}
                  disabled={!connected}
                />
              </div>
              <div className="remote-composer-toolbar">
                <div className="remote-composer-status">
                  <span
                    className={`remote-status-pill ${
                      session?.online
                        ? connected
                          ? session?.binding.execution_state === "running"
                            ? "running"
                            : "ready"
                          : "connecting"
                        : "offline"
                    }`}
                  >
                    <span className="remote-pill-dot" />
                    {session
                      ? !session.online
                        ? "电脑离线"
                        : !connected
                          ? "正在恢复…"
                          : (states[session.binding.execution_state] ?? "就绪")
                      : "正在连接"}
                  </span>
                  <span className="remote-composer-hint">
                    {canSend
                      ? "Enter 发送 · Shift+Enter 换行"
                      : "Hub 双向安全同步"}
                  </span>
                </div>
                <div className="remote-composer-actions">
                  <div
                    className="remote-effort-selector"
                    title="设置本轮思考推理深度 (Claude Code / Codex)"
                  >
                    <select
                      value={effort}
                      onChange={(e) =>
                        setEffort(e.target.value as typeof effort)
                      }
                      className="remote-effort-select"
                      disabled={!connected || busy}
                      aria-label="推理思考深度"
                    >
                      <option value="default">默认思考</option>
                      <option value="low">低推理 (快速)</option>
                      <option value="medium">中推理 (标准)</option>
                      <option value="high">高推理 (深度)</option>
                      <option value="max">超高推理 (最大)</option>
                    </select>
                  </div>
                  <RemoteContextCapacity events={events} />
                  {session?.binding.current_turn_id && (
                    <button
                      type="button"
                      className="remote-interrupt-btn"
                      title="停止任务"
                      disabled={!connected || busy || submittingInterrupt}
                      onClick={() => void submit({ action: "interrupt" })}
                    >
                      <svg
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                      >
                        <rect x="4" y="4" width="16" height="16" rx="2" />
                      </svg>
                      <span>{submittingInterrupt ? "正在停止…" : "停止"}</span>
                    </button>
                  )}
                  <button
                    type="submit"
                    className="remote-send-btn"
                    title={canSend && text.trim() ? "发送" : "等待输入就绪"}
                    disabled={!canSend || busy || !text.trim()}
                  >
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
                </div>
              </div>
            </form>
          </>
        )}
      </main>
      <dialog ref={dialog} className="remote-launch">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void launch();
          }}
        >
          <header>
            <h2>在电脑上开始</h2>
            <button
              type="button"
              aria-label="关闭"
              onClick={() => dialog.current?.close()}
            >
              ×
            </button>
          </header>
          <label>
            电脑
            <select
              value={runner}
              onChange={(event) => {
                setRunner(event.target.value);
                setWorkspace(
                  snapshot.runners.find(
                    (item) => item.runner_id === event.target.value,
                  )?.hello?.workspaces[0] ?? "",
                );
                setNativeId("");
              }}
            >
              <option value="">选择在线电脑</option>
              {snapshot.runners
                .filter((item) => item.online)
                .map((item) => (
                  <option value={item.runner_id} key={item.runner_id}>
                    {item.runner_id}
                  </option>
                ))}
            </select>
          </label>
          <label>
            工作区
            <select
              value={workspace}
              onChange={(event) => {
                setWorkspace(event.target.value);
                setNativeId("");
              }}
            >
              <option value="">选择已授权目录</option>
              {runnerInfo?.hello?.workspaces.map((path) => (
                <option value={path} key={path}>
                  {cleanPath(path)}
                </option>
              ))}
            </select>
          </label>
          <label>
            会话
            <select
              value={nativeId}
              onChange={(event) => setNativeId(event.target.value)}
            >
              <option value="">创建新会话</option>
              {runnerInfo?.hello?.discovery?.candidates
                .filter(
                  (item) =>
                    item.workspace_path
                      .replace(/^\\\\\?\\/u, "")
                      .replaceAll("\\", "/")
                      .toLowerCase() ===
                    workspace
                      .replace(/^\\\\\?\\/u, "")
                      .replaceAll("\\", "/")
                      .toLowerCase(),
                )
                .map((item) => {
                  const provider = getProviderInfo(item.provider_session_id);
                  return (
                    <option
                      key={item.provider_session_id}
                      value={item.provider_session_id}
                    >
                      [{provider.name}] 接管：
                      {item.title ?? item.provider_session_id}
                    </option>
                  );
                })}
            </select>
          </label>
          {!nativeId && (
            <label>
              Agent 引擎
              <select
                value={agentType}
                onChange={(event) =>
                  setAgentType(event.target.value as typeof agentType)
                }
              >
                <option value="zcode">ZCode (Gemini 3.8 / 全功能)</option>
                <option value="codex">Codex</option>
                <option value="claude_code">Claude Code</option>
              </select>
            </label>
          )}
          <label>
            模型
            <select
              value={model}
              onChange={(event) => setModel(event.target.value)}
            >
              <option value="">沿用原生设置</option>
              {runnerInfo?.hello?.models?.data.map((item) => (
                <option key={item.id} value={item.model ?? item.id}>
                  {item.displayName}
                </option>
              ))}
            </select>
          </label>
          <label>
            文件权限
            <select
              value={sandbox}
              onChange={(event) =>
                setSandbox(event.target.value as typeof sandbox)
              }
            >
              <option value="read-only">只读</option>
              <option value="workspace-write">允许修改工作区</option>
            </select>
          </label>
          <p>
            执行发生在所选电脑。会话内容将同步到你的 Hub，供其他设备查看和继续。
          </p>
          <button
            className="remote-primary"
            disabled={busy || !workspace || !runner}
          >
            {nativeId ? "恢复会话" : "创建会话"}
          </button>
        </form>
      </dialog>
      {settingsOpen && config !== undefined && (
        <SettingsPage
          account={account ?? undefined}
          config={config}
          notificationPresentedAt={notificationPresentedAt}
          onClose={() => setSettingsOpen(false)}
          onLoggedOut={() => {
            setSettingsOpen(false);
            onLoggedOut?.();
          }}
          onSave={(next) => {
            saveConfig(window.localStorage, next);
            setSettingsOpen(false);
            refresh.current();
          }}
          onWorkspaceChanged={() => refresh.current()}
        />
      )}
    </div>
  );
}
