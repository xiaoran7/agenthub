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
  type RemoteAttachment,
} from "../lib/remote";
import { MarkdownView } from "./MarkdownView";
import { Select } from "./Select";
import { RemoteContextCapacity } from "./RemoteContextCapacity";
import { SettingsPage } from "./SettingsPage";
import { ThinkingAccordion } from "./ThinkingAccordion";
import { parseNotificationPresentedAt } from "../lib/notifications";
import type { PanelConfig } from "../lib/config";
import { loadConfig, saveConfig } from "../lib/config";
import {
  buildSessionTree,
  formatFallbackTitle,
  getDeviceLabel,
  getProviderInfo,
  workspaceLabel,
  workspaceTooltip,
  type ProviderKind,
  type TreeSessionItem,
} from "../lib/session-tree";
import "./remote.css";
import "./remote-desktop.css";

const empty: RemoteSnapshot = { cursor: 0, runners: [], sessions: [] };

/**
 * Session-detail refresh interval. The detail route already returns the event
 * cursor, so this only needs to be fast enough to feel live while typing; a
 * 200 ms tick made every open console fire 10 requests per second, doubled by
 * the event fetch, against a Gateway that is already polling its runners.
 */
const SESSION_POLL_MS = 1000;

const key = (session: RemoteSession) =>
  `${session.runner_id}/${session.binding.hub_session_id}`;

/** The launch dialog's fixed choices. Everything else is derived from the Host. */
const EFFORT_OPTIONS = [
  { value: "default", label: "默认思考" },
  { value: "low", label: "低推理 (快速)" },
  { value: "medium", label: "中推理 (标准)" },
  { value: "high", label: "高推理 (深度)" },
  { value: "max", label: "超高推理 (最大)" },
] as const;

const SANDBOX_OPTIONS = [
  { value: "read-only", label: "只读" },
  { value: "workspace-write", label: "允许修改工作区" },
] as const;

/**
 * The engines the Runner can launch, in the order the console offers them.
 *
 * `catalog` names the key the Host's `models.agents` map uses for that engine;
 * the model list is read from there rather than guessed.
 */
const LAUNCH_AGENTS = [
  { value: "zcode", label: "ZCode", catalog: ["zcode"] },
  { value: "codex", label: "Codex", catalog: ["codex"] },
  {
    value: "claude_code",
    label: "Coding Assistant",
    catalog: ["claude_code", "claude"],
  },
  { value: "antigravity", label: "Antigravity", catalog: [] },
  { value: "grok_build", label: "Grok", catalog: [] },
] as const;

const LAUNCH_AGENT_OPTIONS = LAUNCH_AGENTS.map(({ value, label }) => ({
  value,
  label,
}));

function formatSessionTokens(tokens?: number | null): string | null {
  if (tokens === null || tokens === undefined || tokens <= 0) return null;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M tok`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k tok`;
  return `${tokens} tok`;
}

/**
 * Extracts <thinking> or <thought> blocks from message content for CDS ThinkingAccordion.
 */
function splitThinkingContent(
  content: string,
  isStreaming = false,
): {
  thinking: string | null;
  cleanText: string;
  isThinkingLive: boolean;
} {
  const match = content.match(
    /<(?:thinking|thought)>([\s\S]*?)(?:<\/(?:thinking|thought)>|$)/i,
  );
  if (!match)
    return { thinking: null, cleanText: content, isThinkingLive: false };

  const thinking = match[1]?.trim() ?? "";
  const isThinkingLive =
    isStreaming &&
    !content.includes("</thinking>") &&
    !content.includes("</thought>");
  const cleanText = content
    .replace(
      /<(?:thinking|thought)>[\s\S]*?(?:<\/(?:thinking|thought)>|$)/i,
      "",
    )
    .trim();

  return {
    thinking: thinking || null,
    cleanText,
    isThinkingLive,
  };
}

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

function ChevronDownIcon({
  size = 12,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}

function CheckIcon({
  size = 12,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function CopyIcon({
  size = 12,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </svg>
  );
}

function SettingsIcon({
  size = 14,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
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

  if (!oldText && !newText && (additions > 0 || deletions > 0)) {
    const oldLines: string[] = [];
    const newLines: string[] = [];
    for (const line of lines) {
      if (
        line.startsWith("+++") ||
        line.startsWith("---") ||
        line.startsWith("@@")
      ) {
        continue;
      }
      if (line.startsWith("-")) {
        oldLines.push(line.slice(1));
      } else if (line.startsWith("+")) {
        newLines.push(line.slice(1));
      } else {
        const ctx = line.startsWith(" ") ? line.slice(1) : line;
        oldLines.push(ctx);
        newLines.push(ctx);
      }
    }
    if (oldLines.length > 0) oldText = oldLines.join("\n");
    if (newLines.length > 0) newText = newLines.join("\n");
  }

  return { additions, deletions, oldText, newText };
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
  const [copiedSection, setCopiedSection] = useState<string | null>(null);

  const handleCopy = (section: string, text: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(text);
      setCopiedSection(section);
      setTimeout(() => {
        setCopiedSection((cur) => (cur === section ? null : cur));
      }, 1800);
    }
  };

  return (
    <details
      className={`remote-tool mcp-tool-decision-card state-${status}`}
      data-tool={kind}
    >
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
            <span className="remote-tool-pulse-dots" aria-hidden="true">
              <span className="pulse-dot" />
              <span className="pulse-dot" />
              <span className="pulse-dot" />
            </span>
          )}
          {status === "running" ? (
            <span className="status-label">执行中…</span>
          ) : status === "failed" ? (
            <span className="status-label">执行失败</span>
          ) : (
            <span className="status-label">已完成</span>
          )}
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
                  <div className="remote-question-item-header">
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
                  <div className="remote-diff-bar">
                    <span className="remote-diff-tag">- 变更前</span>
                    <button
                      type="button"
                      className="remote-card-mini-copy"
                      onClick={(e) =>
                        handleCopy("diff-del", diffStats.oldText!, e)
                      }
                    >
                      {copiedSection === "diff-del" ? (
                        <span className="remote-copied-badge">
                          <CheckIcon size={12} />
                          <span>已复制</span>
                        </span>
                      ) : (
                        <span className="remote-copy-badge">
                          <CopyIcon size={12} />
                          <span>复制</span>
                        </span>
                      )}
                    </button>
                  </div>
                  <pre className="remote-diff-pre">{diffStats.oldText}</pre>
                </div>
              )}
              {diffStats.newText && (
                <div className="remote-diff-chunk add">
                  <div className="remote-diff-bar">
                    <span className="remote-diff-tag">+ 变更后</span>
                    <button
                      type="button"
                      className="remote-card-mini-copy"
                      onClick={(e) =>
                        handleCopy("diff-add", diffStats.newText!, e)
                      }
                    >
                      {copiedSection === "diff-add" ? (
                        <span className="remote-copied-badge">
                          <CheckIcon size={12} />
                          <span>已复制</span>
                        </span>
                      ) : (
                        <span className="remote-copy-badge">
                          <CopyIcon size={12} />
                          <span>复制</span>
                        </span>
                      )}
                    </button>
                  </div>
                  <pre className="remote-diff-pre">{diffStats.newText}</pre>
                </div>
              )}
            </div>
          ) : item.command ? (
            <div className="remote-tool-section">
              <div className="remote-tool-subhead-bar">
                <span className="remote-tool-subhead">命令 / 参数输入</span>
                <button
                  type="button"
                  className="remote-card-mini-copy"
                  onClick={(e) => handleCopy("cmd", item.command!, e)}
                >
                  {copiedSection === "cmd" ? (
                    <span className="remote-copied-badge">
                      <CheckIcon size={12} />
                      <span>已复制</span>
                    </span>
                  ) : (
                    <span className="remote-copy-badge">
                      <CopyIcon size={12} />
                      <span>复制</span>
                    </span>
                  )}
                </button>
              </div>
              <pre className="remote-tool-cmd">
                {readableRemoteText(item.command)}
              </pre>
            </div>
          ) : null}
          {item.output && !questionItems && (
            <div className="remote-tool-section">
              <div className="remote-tool-subhead-bar">
                <span className="remote-tool-subhead">执行结算输出</span>
                <button
                  type="button"
                  className="remote-card-mini-copy"
                  onClick={(e) => handleCopy("output", item.output!, e)}
                >
                  {copiedSection === "output" ? (
                    <span className="remote-copied-badge">
                      <CheckIcon size={12} />
                      <span>已复制</span>
                    </span>
                  ) : (
                    <span className="remote-copy-badge">
                      <CopyIcon size={12} />
                      <span>复制</span>
                    </span>
                  )}
                </button>
              </div>
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
  const [sessionModel, setSessionModel] = useState("");
  const [attachments, setAttachments] = useState<RemoteAttachment[]>([]);
  const [sessionLoading, setSessionLoading] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [submittingInterrupt, setSubmittingInterrupt] = useState(false);
  const [busy, setBusy] = useState(false);
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
  const autoResumedRef = useRef<Set<string>>(new Set());
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
    () => (path, body) =>
      request(path, body, 12000, account?.user.user_id ?? config?.userId),
    [request, account?.user.user_id, config?.userId],
  );

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

  useEffect(() => {
    const checkMobile = () => {
      setIsMobile(window.innerWidth <= 700);
    };
    checkMobile();
    window.addEventListener("resize", checkMobile);
    return () => window.removeEventListener("resize", checkMobile);
  }, []);

  // The mobile drawer covers the console, so Escape has to close it and the
  // keyboard must not wander into the content behind it.
  const mobileDrawerRef = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!isMobile || !mobileDrawerOpen) return;
    const drawer = mobileDrawerRef.current;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMobileDrawerOpen(false);
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
  }, [isMobile, mobileDrawerOpen]);
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

  const [providerFilter, setProviderFilter] = useState<ProviderKind>("claude");
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
  const [agentType, setAgentType] = useState<
    "zcode" | "codex" | "claude_code" | "antigravity" | "grok_build"
  >("zcode");
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [nativeId, setNativeId] = useState("");
  const [launchModalOpen, setLaunchModalOpen] = useState(false);
  const timeline = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const refresh = useRef(() => {});
  const refreshSessionDetail = useRef(() => {});
  const session = snapshot.sessions.find((item) => key(item) === selected);
  const runnerInfo = snapshot.runners.find((item) => item.runner_id === runner);
  const connected = !!session?.online && !!session.binding.control_connected;

  const discovered = useMemo(() => {
    const candidates = new Map<string, DiscoveryCandidate>();
    for (const r of snapshot.runners) {
      if (r.revoked) continue;
      for (const candidate of r.hello?.discovery?.candidates ?? [])
        candidates.set(
          `${r.runner_id}/${candidate.provider_session_id}`,
          candidate,
        );
    }
    return candidates;
  }, [snapshot.runners]);

  /** Every online computer, for the launch dialog's first field. */
  const onlineDeviceOptions = useMemo(
    () =>
      snapshot.runners
        .filter((item) => !item.revoked && item.online)
        .map((item) => ({
          value: item.runner_id,
          label: getDeviceLabel(item.runner_id, item.hello),
        })),
    [snapshot.runners],
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
    const engine = LAUNCH_AGENTS.find((item) => item.value === agentType);
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
    for (const key of engine?.catalog ?? []) {
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
    const targetProvider =
      agentType === "claude_code"
        ? "claude"
        : agentType === "grok_build"
          ? "grok"
          : agentType;
    for (const cand of runnerInfo?.hello?.discovery?.candidates ?? []) {
      const pInfo = getProviderInfo(cand.provider_session_id, cand.provider);
      if (pInfo.type === targetProvider && cand.model) {
        pushModel(cand.model, cand.model);
      }
    }
    if (models.length === 0 && agentType === "claude_code") {
      pushModel("claude-opus-4-6", "Claude Opus 4.6");
      pushModel("claude-sonnet-4-6", "Claude Sonnet 4.6");
      pushModel("claude-haiku-4-5-20251001", "Claude Haiku 4.5");
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
    const sessionRunner = snapshot.runners.find(
      (item) => item.runner_id === session.runner_id,
    );
    const hostModels = sessionRunner?.hello?.models;
    const candidate = discovered.get(
      `${session.runner_id}/${session.binding.provider_session_id}`,
    );
    const providerType = getProviderInfo(
      session.binding.provider_session_id,
      candidate?.provider,
    ).type;
    const catalogKeys =
      providerType === "claude"
        ? ["claude_code", "claude_desktop", "claude"]
        : providerType === "codex"
          ? ["codex"]
          : providerType === "zcode"
            ? ["zcode"]
            : providerType === "antigravity"
              ? ["antigravity"]
              : ["grok_build", "grok"];
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
    if (candidate?.model) {
      pushModel(candidate.model, candidate.model);
    }
    for (const k of catalogKeys) {
      for (const item of hostModels?.agents?.[k] ?? []) {
        pushModel(item.id, item.display_name);
      }
    }
    if (providerType === "codex" || models.length === 0) {
      for (const item of hostModels?.data ?? []) {
        pushModel(
          item.id,
          item.display_name ?? item.displayName ?? item.name ?? item.id,
        );
      }
    }
    for (const cand of sessionRunner?.hello?.discovery?.candidates ?? []) {
      const pInfo = getProviderInfo(cand.provider_session_id, cand.provider);
      if (pInfo.type === providerType && cand.model) {
        pushModel(cand.model, cand.model);
      }
    }
    if (models.length === 0 && providerType === "claude") {
      pushModel("claude-opus-4-6", "Claude Opus 4.6");
      pushModel("claude-sonnet-4-6", "Claude Sonnet 4.6");
      pushModel("claude-haiku-4-5-20251001", "Claude Haiku 4.5");
    }
    const defaultLabel = candidate?.model
      ? candidate.model.replace(/^models\//u, "")
      : "默认模型";
    return [
      { value: "", label: defaultLabel },
      ...models
        .filter(
          (item) =>
            !candidate?.model ||
            item.id.toLowerCase() !== candidate.model.toLowerCase(),
        )
        .map((item) => ({
          value: item.id,
          label: item.display_name ?? item.id,
        })),
    ];
  }, [session, snapshot.runners, discovered]);

  const handleFilesSelected = async (fileList: FileList | File[]) => {
    const files = Array.from(fileList);
    if (files.length === 0) return;
    const TEXT_EXT_RE =
      /\.(txt|md|markdown|json|jsonl|yaml|yml|toml|xml|csv|tsv|py|ts|tsx|js|jsx|mjs|cjs|rs|go|java|kt|c|cpp|h|hpp|cs|rb|php|swift|sh|bash|zsh|ps1|bat|cmd|sql|html|css|scss|less|vue|svelte|ini|conf|cfg|env|log)$/iu;
    const loaded: RemoteAttachment[] = [];
    for (const file of files.slice(0, 16)) {
      if (file.size > 6 * 1024 * 1024) {
        setError(`附件 ${file.name} 超过 6MB 大小限制`);
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
        const value = await requestWithIdentity<RemoteSnapshot>("/sessions");
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
  }, [requestWithIdentity, onAuthenticationRequired]);
  useEffect(() => {
    setEvents([]);
    setCommands([]);
    setText("");
    setSessionModel("");
    setAttachments([]);
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
    setSessionLoading(true);
    let stopped = false;
    let inFlight = false;
    let cursor = 0;
    let lastPolledGlobalCursor = -1;
    let accumulated: RemoteEvent[] = [];
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (stopped || inFlight) return;
      inFlight = true;
      let latestDetail: RemoteSessionDetail | null = null;
      try {
        // One request, not two. The session detail route already carries the
        // binding, the event cursor and the command history, so the event fetch
        // below only runs when the cursor has actually moved.
        const detail = await requestWithIdentity<RemoteSessionDetail>(
          `/sessions/${encodeURIComponent(runnerId)}/${encodeURIComponent(hub)}`,
        );
        if (stopped) return;
        latestDetail = detail;
        if (detail.cursor > lastPolledGlobalCursor) {
          while (true) {
            const batch = await requestWithIdentity<{ events: RemoteEvent[] }>(
              `/events?runner=${encodeURIComponent(runnerId)}&hub=${encodeURIComponent(hub)}&after=${cursor}`,
            );
            if (stopped) return;
            if (batch.events.length > 0) {
              cursor = batch.events[batch.events.length - 1]!.cursor;
              accumulated = [...accumulated, ...batch.events];
              setEvents([...accumulated]);
            }
            if (batch.events.length < 256) break;
          }
          lastPolledGlobalCursor = detail.cursor;
        }
        setCommands(detail.commands);
        setSessionLoading(false);
        const resolvedBinding = detail.binding;
        if (resolvedBinding) {
          setSnapshot((prev) => {
            const idx = prev.sessions.findIndex(
              (s) =>
                s.runner_id === runnerId && s.binding.hub_session_id === hub,
            );
            if (
              idx >= 0 &&
              prev.sessions[idx]?.binding.control_connected !==
                resolvedBinding.control_connected
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
        if (!stopped) {
          setError(remoteError(err));
          setSessionLoading(false);
        }
      } finally {
        inFlight = false;
        const hasPending = latestDetail?.commands?.some((c) => !c.receipt);
        const nextDelay = hasPending ? 200 : SESSION_POLL_MS;
        if (!stopped) timer = setTimeout(() => void poll(), nextDelay);
      }
    };
    refreshSessionDetail.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
      refreshSessionDetail.current = () => {};
    };
  }, [requestWithIdentity, selected]);
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

  // 核心特性：选定会话时全量自动开始恢复接入（若已标记连接但尚未拉取到历史快照，也自动触发一次幂等同步）
  useEffect(() => {
    if (!session || !session.online || busy || sessionLoading) return;
    const needsHistorySync =
      items.length === 0 &&
      discovered.has(
        `${session.runner_id}/${session.binding.provider_session_id}`,
      );
    if (session.binding.control_connected && !needsHistorySync) return;
    const targetRunner = snapshot.runners.find(
      (r) => r.runner_id === session.runner_id,
    );
    const activeGeneration =
      targetRunner?.hello?.host_generation ?? session.binding.host_generation;
    const sessionKey = `${key(session)}/${activeGeneration}`;
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
    snapshot.runners,
    busy,
    sessionLoading,
    items.length,
    discovered,
  ]);

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
    const targetRunner = snapshot.runners.find(
      (item) => item.runner_id === target.runner_id,
    );
    const activeGeneration =
      targetRunner?.hello?.host_generation ?? target.binding.host_generation;
    const attemptKey = JSON.stringify([
      target.runner_id,
      target.binding.hub_session_id,
      action,
      activeGeneration,
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id: target.binding.hub_session_id,
        host_generation: activeGeneration,
        command: action,
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(target.runner_id)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      if (action.action === "send_prompt") {
        setText("");
        setAttachments([]);
        if (textareaRef.current) textareaRef.current.style.height = "auto";
      }
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
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
      await requestWithIdentity(
        `/runners/${encodeURIComponent(runner)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelected(`${runner}/${command.hub_session_id}`);
      setLaunchModalOpen(false);
      refresh.current();
      refreshSessionDetail.current();
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
          existingSession?.binding.hub_session_id ?? safeRandomUUID(),
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
      setBusy(false);
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
        devices: snapshot.runners
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
          online: item.online,
          control_connected: item.binding.control_connected,
          execution_state: item.binding.execution_state,
        })),
        candidates: snapshot.runners
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
      }),
    [snapshot.sessions, snapshot.runners, providerFilter, discovered],
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
    if (!launchModalOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setLaunchModalOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [launchModalOpen]);

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
    return snapshot.runners
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
  }, [snapshot.runners]);

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
        disc?.provider,
      );
      const existing = map.get(info.type);
      map.set(info.type, {
        type: info.type,
        name: info.name,
        count: (existing?.count ?? 0) + 1,
      });
    }

    // 2. 该电脑 Runner runtime 探测上报的 candidates
    const activeRunner = snapshot.runners.find(
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
  }, [activeRunnerId, snapshot.runners, snapshot.sessions, discovered]);

  const currentAgentInfo = useMemo(() => {
    if (!activeRunnerId) {
      return {
        type: "" as ProviderKind,
        name: "请先选择电脑",
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
  }, [activeRunnerId, availableAgentsForCurrentDevice, providerFilter]);

  // 级联规则：只有选择电脑后才响应 Agent；若选中的 Agent 在当前电脑上不存在，自动切换为该电脑支持的第 1 个 Agent
  useEffect(() => {
    if (!activeRunnerId || availableAgentsForCurrentDevice.length === 0) {
      return;
    }
    const exists = availableAgentsForCurrentDevice.some(
      (a) => a.type === providerFilter,
    );
    if (!exists) {
      const first = availableAgentsForCurrentDevice[0];
      if (first) {
        setProviderFilter(first.type);
      }
    }
  }, [activeRunnerId, availableAgentsForCurrentDevice, providerFilter]);

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
    window.addEventListener("mousedown", onClickOutside);
    return () => window.removeEventListener("mousedown", onClickOutside);
  }, [deviceMenuOpen, agentMenuOpen]);

  // Escape closes any open menu and hands the focus back to the button
  // that opened it, so the keyboard never ends up on a closed popup.
  useEffect(() => {
    if (!deviceMenuOpen && !agentMenuOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
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
        disc?.provider,
      );
      if (info.type) {
        setProviderFilter(info.type);
      }
    }
  }, [selected, snapshot.sessions, selectedRunnerId, discovered]);

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
        ref={mobileDrawerRef}
        aria-hidden={isMobile && !mobileDrawerOpen ? true : undefined}
        // `inert` also takes the closed drawer's controls out of the tab
        // order, which `aria-hidden` alone does not do.
        inert={isMobile && !mobileDrawerOpen ? true : undefined}
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
              title={searchOpen ? "收起搜索" : "搜索会话"}
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
              onClick={() => {
                const available = snapshot.runners.find(
                  (item) =>
                    !item.revoked &&
                    item.online &&
                    item.hello?.workspaces.length,
                );
                setRunner(activeRunnerId || available?.runner_id || "");
                const rInfo = snapshot.runners.find(
                  (r) =>
                    !r.revoked &&
                    r.runner_id === (activeRunnerId || available?.runner_id),
                );
                setWorkspace(rInfo?.hello?.workspaces[0] ?? "");
                setNativeId("");
                setModel("");
                setLaunchModalOpen(true);
              }}
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
              type="text"
              className="remote-sidebar-search-input"
              placeholder="搜索会话或工作区..."
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
                !activeRunnerId || availableAgentsForCurrentDevice.length === 0;
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

            {agentMenuOpen && activeRunnerId && (
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
                  onClick={() => {
                    setRunner(activeRunnerId);
                    setWorkspace(activeDevice?.workspaces[0] ?? "");
                    setNativeId("");
                    setModel("");
                    setLaunchModalOpen(true);
                  }}
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
                        setRunner(activeRunnerId);
                        setWorkspace(workspaceGroup.id);
                        setNativeId("");
                        setModel("");
                        setLaunchModalOpen(true);
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
                                pendingApprovalsBySessionKey.get(
                                  item.sessionKey,
                                ) ?? 0;
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
                                      if (isSelected) {
                                        if (isMobile)
                                          setMobileDrawerOpen(false);
                                        if (
                                          activeSession.online &&
                                          !activeSession.binding
                                            .control_connected &&
                                          !busy
                                        ) {
                                          const targetRunner =
                                            snapshot.runners.find(
                                              (r) =>
                                                r.runner_id ===
                                                activeSession.runner_id,
                                            );
                                          const activeGen =
                                            targetRunner?.hello
                                              ?.host_generation ??
                                            activeSession.binding
                                              .host_generation;
                                          autoResumedRef.current.delete(
                                            `${key(activeSession)}/${activeSession.binding.host_generation}`,
                                          );
                                          autoResumedRef.current.delete(
                                            `${key(activeSession)}/${activeGen}`,
                                          );
                                          void submit(
                                            {
                                              action: "connect",
                                              mode: "resume",
                                              workspace_path:
                                                activeSession.binding
                                                  .workspace_path,
                                              provider_session_id:
                                                activeSession.binding
                                                  .provider_session_id,
                                            },
                                            activeSession,
                                          );
                                        }
                                        return;
                                      }
                                      setSelected(item.sessionKey);
                                      setError("");
                                      if (isMobile) setMobileDrawerOpen(false);
                                      if (
                                        activeSession.online &&
                                        !activeSession.binding.control_connected
                                      ) {
                                        const targetRunner =
                                          snapshot.runners.find(
                                            (r) =>
                                              r.runner_id ===
                                              activeSession.runner_id,
                                          );
                                        const activeGen =
                                          targetRunner?.hello
                                            ?.host_generation ??
                                          activeSession.binding.host_generation;
                                        autoResumedRef.current.delete(
                                          `${key(activeSession)}/${activeSession.binding.host_generation}`,
                                        );
                                        autoResumedRef.current.delete(
                                          `${key(activeSession)}/${activeGen}`,
                                        );
                                        void submit(
                                          {
                                            action: "connect",
                                            mode: "resume",
                                            workspace_path:
                                              activeSession.binding
                                                .workspace_path,
                                            provider_session_id:
                                              activeSession.binding
                                                .provider_session_id,
                                          },
                                          activeSession,
                                        );
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
                                          {busy && isSelected
                                            ? "正在恢复..."
                                            : "未连接"}
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
                                        <CheckIcon size={12} />
                                      ) : (
                                        <CopyIcon size={12} />
                                      )}
                                    </button>
                                  </div>
                                </div>
                              );
                            }

                            return (
                              <div
                                className="remote-session candidate"
                                key={item.key}
                              >
                                <button
                                  type="button"
                                  className="remote-session-main"
                                  title={
                                    item.candidate.resumable === false
                                      ? "电脑端目前仅发现会话元数据，尚不能远程恢复"
                                      : `点击恢复：${item.title}`
                                  }
                                  disabled={
                                    busy || item.candidate.resumable === false
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
                                      className="remote-session-bullet candidate"
                                      aria-hidden="true"
                                    />
                                    <span className="remote-session-title">
                                      {item.title}
                                    </span>
                                  </div>
                                  <span className="remote-session-meta">
                                    <span className="remote-session-badge candidate">
                                      可接管
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
                                      <CheckIcon size={12} />
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
              className={`remote-status-dot ${snapshot.runners.some((item) => !item.revoked && item.online) ? "online" : "offline"}`}
            />
            <span>
              {
                snapshot.runners.filter((item) => !item.revoked && item.online)
                  .length
              }{" "}
              台电脑在线
            </span>
          </div>
          {account?.user ? (
            <div className="remote-account-card">
              <div className="remote-account-avatar" aria-hidden="true">
                {account.user.avatar_data_url ? (
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
                  onClick={() => setSettingsOpen(true)}
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
                onClick={() => setSettingsOpen(true)}
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
                        )?.provider,
                      ).name;
                      if (!session.online) return `${providerName} · 电脑离线`;
                      if (!connected || (sessionLoading && items.length === 0))
                        return `${providerName} · 正在连接…`;
                      return providerName;
                    })()
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
            {remoteError(new Error(latestFailure.receipt?.code ?? ""))}
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
                    setLaunchModalOpen(true);
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
          <div key={selected} className="remote-session-view remote-view-enter">
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
                const isStreamingThis = Boolean(
                  item.kind === "assistant" &&
                  session?.binding.current_turn_id &&
                  item === items[items.length - 1],
                );
                const rawText = readableRemoteText(item.text);
                const { thinking, cleanText, isThinkingLive } =
                  item.kind === "assistant"
                    ? splitThinkingContent(rawText, isStreamingThis)
                    : {
                        thinking: null,
                        cleanText: rawText,
                        isThinkingLive: false,
                      };

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
                      {thinking && (
                        <ThinkingAccordion
                          thoughtText={thinking}
                          isThinking={isThinkingLive}
                        />
                      )}
                      {(cleanText || !thinking) && (
                        <MarkdownView
                          content={cleanText}
                          streaming={isStreamingThis}
                        />
                      )}
                    </div>
                  </article>
                );
              })}
              {!session ||
              (sessionLoading && items.length === 0) ||
              (!connected && session.online && items.length === 0) ||
              (busy && items.length === 0) ? (
                <div className="remote-timeline-empty">
                  <span className="remote-notice-spinner" />
                  <p className="remote-timeline-empty-desc">
                    正在加载会话内容…
                  </p>
                </div>
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
                  {!session.online && (
                    <p className="remote-timeline-empty-desc">电脑离线</p>
                  )}
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
                          ).name
                        }
                      </strong>{" "}
                      请求在当前工作区执行【{approval.tool_name ?? "系统工具"}】
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
                if (canSend && (text.trim() || attachments.length > 0)) {
                  const promptText =
                    text.trim() ||
                    `请查看附件：${attachments.map((a) => a.name).join("、")}`;
                  const fallbackModel = session
                    ? discovered.get(
                        `${session.runner_id}/${session.binding.provider_session_id}`,
                      )?.model
                    : undefined;
                  const effectiveModel = sessionModel || fallbackModel || "";
                  void submit({
                    action: "send_prompt",
                    text: promptText,
                    ...(effort !== "default" ? { effort } : {}),
                    ...(effectiveModel ? { model: effectiveModel } : {}),
                    ...(attachments.length > 0 ? { attachments } : {}),
                  });
                  if (textareaRef.current)
                    textareaRef.current.style.height = "auto";
                }
              }}
            >
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
                      if (canSend && (text.trim() || attachments.length > 0)) {
                        const promptText =
                          text.trim() ||
                          `请查看附件：${attachments.map((a) => a.name).join("、")}`;
                        const fallbackModel = session
                          ? discovered.get(
                              `${session.runner_id}/${session.binding.provider_session_id}`,
                            )?.model
                          : undefined;
                        const effectiveModel =
                          sessionModel || fallbackModel || "";
                        void submit({
                          action: "send_prompt",
                          text: promptText,
                          ...(effort !== "default" ? { effort } : {}),
                          ...(effectiveModel ? { model: effectiveModel } : {}),
                          ...(attachments.length > 0 ? { attachments } : {}),
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
                        ? "电脑离线"
                        : !connected
                          ? "正在连接…"
                          : canSend
                            ? "继续这段工作…"
                            : "正在执行…"
                  }
                  rows={1}
                  disabled={!connected}
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
                  <button
                    type="button"
                    className="remote-attach-btn"
                    title="添加附件"
                    aria-label="添加附件"
                    disabled={!connected || busy}
                    onClick={() => fileInputRef.current?.click()}
                  >
                    <svg
                      width="16"
                      height="16"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      aria-hidden="true"
                    >
                      <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />
                    </svg>
                  </button>
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
                    title={
                      canSend && (text.trim() || attachments.length > 0)
                        ? "发送"
                        : "等待输入就绪"
                    }
                    disabled={
                      !canSend ||
                      busy ||
                      (!text.trim() && attachments.length === 0)
                    }
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
          </div>
        )}
      </main>
      {launchModalOpen && (
        <div
          className="remote-modal-backdrop"
          role="presentation"
          onClick={() => setLaunchModalOpen(false)}
        >
          <div
            className="remote-launch"
            role="dialog"
            aria-modal="true"
            aria-labelledby="remote-launch-title"
            onClick={(event) => event.stopPropagation()}
          >
            <form
              onSubmit={(event) => {
                event.preventDefault();
                void launch();
              }}
            >
              <header>
                <h2 id="remote-launch-title">在电脑上开始</h2>
                <button
                  type="button"
                  className="remote-modal-close"
                  aria-label="关闭"
                  onClick={() => setLaunchModalOpen(false)}
                >
                  ×
                </button>
              </header>
              <div className="remote-launch-field">
                <span className="remote-launch-label">电脑</span>
                <Select
                  label="电脑"
                  value={runner}
                  placeholder="选择在线电脑"
                  options={onlineDeviceOptions}
                  onChange={(next) => {
                    setRunner(next);
                    setWorkspace(
                      snapshot.runners.find((item) => item.runner_id === next)
                        ?.hello?.workspaces[0] ?? "",
                    );
                    setNativeId("");
                    setModel("");
                  }}
                />
              </div>
              <div className="remote-launch-field">
                <span className="remote-launch-label">工作区</span>
                <Select
                  label="工作区"
                  value={workspace}
                  placeholder="选择已授权目录"
                  options={launchWorkspaceOptions}
                  onChange={(next) => {
                    setWorkspace(next);
                    setNativeId("");
                  }}
                />
              </div>
              <div className="remote-launch-field">
                <span className="remote-launch-label">会话</span>
                <Select
                  label="会话"
                  value={nativeId}
                  options={launchSessionOptions}
                  onChange={setNativeId}
                />
              </div>
              {!nativeId && (
                <div className="remote-launch-field">
                  <span className="remote-launch-label">Agent 引擎</span>
                  <Select
                    label="Agent 引擎"
                    value={agentType}
                    options={LAUNCH_AGENT_OPTIONS}
                    onChange={(next) => {
                      setAgentType(next as typeof agentType);
                      // A model id belongs to one engine; carrying it across a
                      // switch would hand the new engine an id it cannot run.
                      setModel("");
                    }}
                  />
                </div>
              )}
              <div className="remote-launch-field">
                <span className="remote-launch-label">模型</span>
                <Select
                  label="模型"
                  value={model}
                  options={launchModelOptions}
                  onChange={setModel}
                />
                {launchModelOptions.length <= 1 && (
                  <span className="remote-launch-hint">
                    所选电脑没有上报这个引擎的模型清单，将沿用原生设置。
                  </span>
                )}
              </div>
              <div className="remote-launch-field">
                <span className="remote-launch-label">文件权限</span>
                <Select
                  label="文件权限"
                  value={sandbox}
                  options={SANDBOX_OPTIONS}
                  onChange={(next) => setSandbox(next as typeof sandbox)}
                />
              </div>
              <p className="remote-launch-note">
                执行发生在所选电脑。会话内容将同步到你的
                Hub，供其他设备查看和继续。
              </p>
              <div className="remote-modal-actions">
                <button
                  type="submit"
                  className="remote-primary remote-modal-submit"
                  disabled={busy || !workspace || !runner}
                >
                  {busy ? "正在启动…" : nativeId ? "恢复会话" : "创建会话"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
      {settingsOpen && panelConfig !== undefined && (
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
      )}
    </div>
  );
}
