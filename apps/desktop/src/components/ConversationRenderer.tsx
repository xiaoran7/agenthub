import React, { useState, memo, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Brain,
  ChevronRight,
  Copy,
  Check,
  Terminal,
  Activity,
  Wrench,
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  FileCode,
  FileText,
  Search,
  HelpCircle,
  Globe,
  ExternalLink,
} from "lucide-react";
import type { ConversationMessage } from "../turn-stream";

export type OnAnswerQuestionHandler = (
  question: string,
  option: string,
) => void;

export interface ConversationRendererProps {
  message: ConversationMessage;
  index: number;
  onAnswerQuestion?: OnAnswerQuestionHandler;
}

export interface ToolCallGroupBlockProps {
  messages: ConversationMessage[];
  onAnswerQuestion?: OnAnswerQuestionHandler;
}

export interface ToolCallBlockProps {
  message: ConversationMessage;
  onAnswerQuestion?: OnAnswerQuestionHandler;
}

const RESULT_SEPARATOR = "〔结果〕";

/** Clean up user messages by stripping injected system prompts, environment tags, and agent instructions. */
export function cleanClientUserContent(raw: string): string {
  if (!raw) return "";
  let text = raw
    .replace(/<recommended_plugins>[\s\S]*?<\/recommended_plugins>/gi, "")
    .replace(/<environment_context>[\s\S]*?<\/environment_context>/gi, "")
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/gi, "")
    .replace(/<INSTRUCTIONS>[\s\S]*?<\/INSTRUCTIONS>/gi, "")
    .replace(/# AGENTS\.md instructions for[^\n]*\n+/gi, "")
    .replace(/<USER_REQUEST>/gi, "")
    .replace(/<\/USER_REQUEST>/gi, "")
    .trim();

  // If the message contains solely system-level traces or reminders
  if (
    text.startsWith("The TodoWrite tool") ||
    text.startsWith("The following skills are available") ||
    text.startsWith("Called the Read tool with the following input:") ||
    text.startsWith("Called the Bash tool with the following input:") ||
    text.startsWith("Called the Edit tool with the following input:") ||
    text.startsWith("<local-command-caveat>")
  ) {
    return "";
  }
  return text;
}

/**
 * Group consecutive tool / status / command messages together so that long tool
 * call runs (e.g. 5+ commands or tools) collapse neatly into a single compact block.
 */
export type RenderMessageGroup =
  | { type: "single"; message: ConversationMessage; index: number }
  | { type: "tool_group"; messages: ConversationMessage[]; startIndex: number };

export function groupConversationMessages(
  messages: ConversationMessage[],
): RenderMessageGroup[] {
  const groups: RenderMessageGroup[] = [];
  let currentToolBatch: ConversationMessage[] = [];
  let batchStartIndex = 0;

  const flushToolBatch = () => {
    if (currentToolBatch.length === 0) return;
    if (currentToolBatch.length === 1) {
      groups.push({
        type: "single",
        message: currentToolBatch[0],
        index: batchStartIndex,
      });
    } else {
      groups.push({
        type: "tool_group",
        messages: [...currentToolBatch],
        startIndex: batchStartIndex,
      });
    }
    currentToolBatch = [];
  };

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i];
    const isToolLike =
      msg.kind === "tool" || msg.kind === "command" || msg.kind === "status";

    if (isToolLike) {
      if (currentToolBatch.length === 0) {
        batchStartIndex = i;
      }
      currentToolBatch.push(msg);
    } else {
      flushToolBatch();
      groups.push({ type: "single", message: msg, index: i });
    }
  }
  flushToolBatch();

  return groups;
}

/**
 * Collapsed Tool Call Group for 2+ consecutive tool executions
 */
export const ToolCallGroupBlock = memo(function ToolCallGroupBlock({
  messages,
  onAnswerQuestion,
}: ToolCallGroupBlockProps) {
  const [open, setOpen] = useState(false);
  const count = messages.length;
  const hasFailed = messages.some((m) => m.status === "failed");
  const hasRunning = messages.some((m) => m.status === "running");

  // Summary labels preview
  const previewSummary = useMemo(() => {
    const names = messages
      .slice(0, 3)
      .map(
        (m) =>
          m.label ||
          (m.kind === "command"
            ? "命令执行"
            : m.kind === "status"
              ? "状态更新"
              : "工具调用"),
      );
    if (count > 3) {
      return `${names.join(" · ")} 等 ${count} 个调用`;
    }
    return names.join(" · ");
  }, [messages, count]);

  return (
    <div className={`tool-call-group-container ${open ? "open" : ""}`}>
      <div
        className="tool-call-group-header"
        onClick={() => setOpen((prev) => !prev)}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            setOpen((prev) => !prev);
          }
        }}
      >
        <span className="tool-group-icon">
          <Wrench size={13} />
        </span>
        <strong className="tool-group-title">
          已执行 {count} 个工具与操作
        </strong>
        <span className="tool-group-preview">{previewSummary}</span>
        <div className="tool-group-right">
          <span
            className={`tool-status-pill ${hasFailed ? "failed" : hasRunning ? "running" : "completed"}`}
          >
            {hasFailed ? (
              <>
                <AlertCircle size={10} /> 异常
              </>
            ) : hasRunning ? (
              <>
                <Activity size={10} className="spin" /> 运行中
              </>
            ) : (
              <>
                <CheckCircle2 size={10} /> 完成
              </>
            )}
          </span>
          <span className="tool-group-chevron">
            {open ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
          </span>
        </div>
      </div>

      {open && (
        <div className="tool-call-group-body">
          {messages.map((item, idx) => (
            <ToolCallBlock
              key={idx}
              message={item}
              onAnswerQuestion={onAnswerQuestion}
            />
          ))}
        </div>
      )}
    </div>
  );
});

export const ConversationItem = memo(function ConversationItem({
  message,
  onAnswerQuestion,
}: ConversationRendererProps) {
  if (
    message.content === "" &&
    message.kind !== "tool" &&
    message.kind !== "thinking"
  ) {
    return null;
  }

  // 1. Explicit Error Banner for failed / synthetic error messages
  if (
    message.status === "failed" ||
    message.role === "system" ||
    message.content.startsWith("历史请求异常:") ||
    message.content.startsWith("API Error:")
  ) {
    return (
      <div className="message-row error-banner-row">
        <div className="system-error-banner" role="alert">
          <span className="system-error-banner-icon">
            <AlertCircle size={15} />
          </span>
          <div className="system-error-banner-text">
            <span className="system-error-banner-tag">
              {message.label || "历史异常"}
            </span>
            {message.content}
          </div>
        </div>
      </div>
    );
  }

  // 2. Dedicated thinking process item
  if (message.kind === "thinking") {
    return <ThinkingBlock content={message.content} defaultOpen={false} />;
  }

  // 3. Dedicated tool call / command / status item (collapsed one-liner)
  if (
    message.kind === "tool" ||
    message.kind === "command" ||
    message.kind === "status"
  ) {
    return (
      <ToolCallBlock
        message={message}
        onAnswerQuestion={onAnswerQuestion}
      />
    );
  }

  // 4. User Message -> Rounded Dark Bubble
  if (message.role === "user") {
    const cleanContent = cleanClientUserContent(message.content);
    if (!cleanContent) return null;

    return (
      <div className="message-row user-row">
        <div className="user-bubble-pill">
          <div className="user-bubble-content">
            <ReactMarkdown remarkPlugins={REMARK_PLUGINS}>
              {cleanContent}
            </ReactMarkdown>
          </div>
        </div>
      </div>
    );
  }

  // 5. Assistant Message -> Natural Fluid Markdown
  return (
    <div className="message-row assistant-row">
      <div className="assistant-flow-content">
        <ParsedAssistantContent text={message.content} />
      </div>
    </div>
  );
});

function ParsedAssistantContent({ text }: { text: string }) {
  // Check for embedded <think>...</think> or <thinking>...</thinking> or <thought>...</thought> tags
  const parts = useMemo(() => {
    const thinkRegex =
      /<(?:think|thinking|thought)>([\s\S]*?)<\/(?:think|thinking|thought)>/gi;
    const items: React.ReactNode[] = [];
    let lastIndex = 0;
    let match: RegExpExecArray | null;

    while ((match = thinkRegex.exec(text)) !== null) {
      const preText = text.slice(lastIndex, match.index);
      if (preText.trim()) {
        items.push(
          <MarkdownSection key={`md-${lastIndex}`} content={preText} />,
        );
      }
      const thinkContent = match[1];
      if (thinkContent.trim()) {
        items.push(
          <ThinkingBlock
            key={`think-${match.index}`}
            content={thinkContent.trim()}
            defaultOpen={false}
          />,
        );
      }
      lastIndex = match.index + match[0].length;
    }

    const remaining = text.slice(lastIndex);
    if (remaining.trim() || items.length === 0) {
      items.push(
        <MarkdownSection key={`md-${lastIndex}`} content={remaining} />,
      );
    }
    return items;
  }, [text]);

  return <>{parts}</>;
}

function ThinkingBlock({
  content,
  defaultOpen = false,
}: {
  content: string;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const charCount = content.length;

  return (
    <details
      className={`thinking-accordion ${open ? "open" : ""}`}
      open={open}
      onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className="thinking-summary">
        <span className="thinking-icon-badge">
          <Brain size={14} className="thinking-spark" />
        </span>
        <strong className="thinking-title">思考过程</strong>
        <span className="thinking-stats">
          {charCount > 1000
            ? `${(charCount / 1000).toFixed(1)}k 字符`
            : `${charCount} 字符`}
        </span>
        <ChevronRight size={13} className="thinking-chevron" />
      </summary>
      <div className="thinking-body">
        <div className="thinking-inner-text">
          <ReactMarkdown remarkPlugins={REMARK_PLUGINS}>
            {content}
          </ReactMarkdown>
        </div>
      </div>
    </details>
  );
}

/** Splits `args 〔结果〕 output` payloads produced by the desktop normalizer. */
export function splitToolContent(content: string): {
  args: string;
  result: string;
} {
  const at = content.indexOf(RESULT_SEPARATOR);
  if (at === -1) return { args: content, result: "" };
  return {
    args: content.slice(0, at).trim(),
    result: content.slice(at + RESULT_SEPARATOR.length).trim(),
  };
}

export type ToolCategory =
  | "terminal"
  | "file_read"
  | "file_edit"
  | "interactive"
  | "web_search"
  | "web_fetch"
  | "system";

export function classifyTool(label?: string, kind?: string): ToolCategory {
  if (kind === "command") return "terminal";
  const norm = (label || "").toLowerCase();
  if (
    norm.startsWith("websearch") ||
    norm.startsWith("web_search") ||
    norm.includes("websearch") ||
    norm.includes("web_search")
  ) {
    return "web_search";
  }
  if (
    norm.startsWith("webfetch") ||
    norm.startsWith("web_fetch") ||
    norm.includes("webfetch") ||
    norm.includes("web_fetch")
  ) {
    return "web_fetch";
  }
  if (
    norm.startsWith("bash") ||
    norm.includes("shell") ||
    norm.includes("exec") ||
    norm.includes("命令")
  ) {
    return "terminal";
  }
  if (
    norm.startsWith("edit") ||
    norm.startsWith("write") ||
    norm.includes("patch") ||
    norm.includes("修改") ||
    norm.includes("变更")
  ) {
    return "file_edit";
  }
  if (
    norm.startsWith("read") ||
    norm.startsWith("grep") ||
    norm.startsWith("glob") ||
    norm.includes("search") ||
    norm.includes("查看")
  ) {
    return "file_read";
  }
  if (
    norm.includes("ask") ||
    norm.includes("plan") ||
    norm.includes("question") ||
    norm.includes("提问") ||
    norm.includes("规划")
  ) {
    return "interactive";
  }
  return "system";
}

export interface DiffStats {
  additions: number;
  deletions: number;
  oldText?: string;
  newText?: string;
}

export function parseDiffStats(content: string): DiffStats {
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
      // not simple JSON diff
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
    if (line.startsWith("+")) {
      additions++;
    } else if (line.startsWith("-")) {
      deletions++;
    }
  }

  return { additions, deletions };
}

export interface QuestionOption {
  label: string;
  description?: string;
}

export interface QuestionItem {
  question: string;
  header?: string;
  options?: QuestionOption[];
  multiSelect?: boolean;
}

export function parseQuestionData(content: string): QuestionItem[] | null {
  const trimmed = content.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  try {
    const parsed = JSON.parse(trimmed);
    if (Array.isArray(parsed.questions) && parsed.questions.length > 0) {
      return parsed.questions as QuestionItem[];
    }
  } catch {
    // not JSON questions
  }
  return null;
}

export interface WebSearchResultItem {
  title: string;
  url?: string;
  snippet?: string;
}

export interface WebSearchData {
  query: string;
  results: WebSearchResultItem[];
}

export function parseWebSearchData(
  args: string,
  result: string,
): WebSearchData | null {
  let query = "";
  try {
    const parsed = JSON.parse(args.trim());
    if (typeof parsed.query === "string") query = parsed.query;
  } catch {
    query = args.trim();
  }
  if (!query) return null;

  const results: WebSearchResultItem[] = [];
  const linkRegex = /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g;
  let match: RegExpExecArray | null;
  while ((match = linkRegex.exec(result)) !== null) {
    results.push({
      title: match[1],
      url: match[2],
    });
  }

  return { query, results };
}

export interface WebFetchData {
  url: string;
  domain: string;
  prompt?: string;
  content: string;
}

export function parseWebFetchData(
  args: string,
  result: string,
): WebFetchData | null {
  let url = "";
  let prompt: string | undefined;
  try {
    const parsed = JSON.parse(args.trim());
    if (typeof parsed.url === "string") url = parsed.url;
    if (typeof parsed.prompt === "string") prompt = parsed.prompt;
  } catch {
    url = args.trim();
  }
  if (!url) return null;

  let domain = "";
  try {
    domain = new URL(url).hostname;
  } catch {
    domain = url.replace(/^https?:\/\//, "").split("/")[0];
  }

  return { url, domain, prompt, content: result };
}

export function TruncatedOutputBlock({
  content,
  maxLines = 80,
  renderContent,
}: {
  content: string;
  maxLines?: number;
  renderContent: (text: string) => React.ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const lines = useMemo(() => content.split("\n"), [content]);
  const isLong = lines.length > maxLines;

  const displayContent = useMemo(() => {
    if (!isLong || expanded) return content;
    return lines.slice(0, maxLines).join("\n");
  }, [content, lines, isLong, expanded, maxLines]);

  return (
    <div
      className={`tool-output-wrapper ${isLong && !expanded ? "collapsed" : ""}`}
    >
      {renderContent(displayContent)}
      {isLong && (
        <button
          type="button"
          className="tool-line-collapse-btn"
          onClick={(e) => {
            e.stopPropagation();
            e.preventDefault();
            setExpanded((prev) => !prev);
          }}
        >
          {expanded ? "收起" : `展开全部 (共 ${lines.length} 行)`}
        </button>
      )}
    </div>
  );
}

export function extractToolPrimaryValue(label?: string, args?: string): string {
  if (label && label.includes(": ")) {
    return label.slice(label.indexOf(": ") + 2).trim();
  }
  if (args) {
    const trimmed = args.trim();
    if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
      try {
        const parsed = JSON.parse(trimmed);
        const val =
          parsed.command ??
          parsed.file_path ??
          parsed.path ??
          parsed.pattern ??
          parsed.query;
        if (typeof val === "string") return val;
      } catch {
        // ignore
      }
    }
  }
  return label || "";
}

export const ToolCallBlock = memo(function ToolCallBlock({
  message,
  onAnswerQuestion,
}: ToolCallBlockProps) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const failed = message.status === "failed";
  const running = message.status === "running";
  const isCommand = message.kind === "command";
  const isStatus = message.kind === "status";

  const { args, result } = useMemo(
    () => splitToolContent(message.content),
    [message.content],
  );
  const effectiveStatus = running ? "running" : failed ? "failed" : "completed";

  const category = useMemo(
    () => classifyTool(message.label, message.kind),
    [message.label, message.kind],
  );
  const diffStats = useMemo(() => parseDiffStats(args), [args]);
  const questionItems = useMemo(() => parseQuestionData(args), [args]);
  const webSearchData = useMemo(
    () => (category === "web_search" ? parseWebSearchData(args, result) : null),
    [category, args, result],
  );
  const webFetchData = useMemo(
    () => (category === "web_fetch" ? parseWebFetchData(args, result) : null),
    [category, args, result],
  );
  const hasDiff = diffStats.additions > 0 || diffStats.deletions > 0;
  const primaryValue = useMemo(
    () => extractToolPrimaryValue(message.label, args),
    [message.label, args],
  );

  const handleCopyPrimary = async (e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    if (!primaryValue) return;
    try {
      await navigator.clipboard.writeText(primaryValue);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // ignore
    }
  };

  const renderCategoryIcon = () => {
    switch (category) {
      case "terminal":
        return <Terminal size={12} className="tool-category-icon terminal" />;
      case "file_edit":
        return <FileCode size={12} className="tool-category-icon edit" />;
      case "file_read": {
        const norm = (message.label || "").toLowerCase();
        if (norm.startsWith("read") || norm.includes("查看")) {
          return <FileText size={12} className="tool-category-icon read" />;
        }
        return <Search size={12} className="tool-category-icon read" />;
      }
      case "interactive":
        return (
          <HelpCircle size={12} className="tool-category-icon interactive" />
        );
      case "web_search":
        return <Search size={12} className="tool-category-icon web_search" />;
      case "web_fetch":
        return <Globe size={12} className="tool-category-icon web_fetch" />;
      default:
        return isStatus ? (
          <Activity size={12} className="tool-category-icon status" />
        ) : (
          <Wrench size={12} className="tool-category-icon generic" />
        );
    }
  };

  return (
    <details
      className={`tool-call-accordion tool-cat-${category} ${failed ? "failed" : ""} ${open ? "open" : ""}`}
      open={open}
      onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className="tool-call-summary">
        <span className="tool-icon-badge">{renderCategoryIcon()}</span>
        <strong className="tool-name">
          {message.label ||
            (isCommand ? "命令执行" : isStatus ? "状态更新" : "工具调用")}
        </strong>

        {hasDiff && (
          <span
            className="diff-stats-badge"
            aria-label={`+${diffStats.additions} -${diffStats.deletions}`}
          >
            {diffStats.additions > 0 && (
              <span className="diff-stat-add">+{diffStats.additions}</span>
            )}
            {diffStats.deletions > 0 && (
              <span className="diff-stat-del">-{diffStats.deletions}</span>
            )}
          </span>
        )}

        {primaryValue && (
          <button
            type="button"
            className="tool-quick-copy-btn"
            onClick={handleCopyPrimary}
            title={`复制: ${primaryValue}`}
            aria-label="复制参数"
          >
            {copied ? (
              <Check size={11} className="copy-success" />
            ) : (
              <Copy size={11} />
            )}
          </button>
        )}

        <span className={`tool-status-pill ${effectiveStatus}`}>
          {failed ? (
            <>
              <AlertCircle size={10} /> 失败
            </>
          ) : running ? (
            <>
              <Activity size={10} className="spin" /> 运行中
            </>
          ) : (
            <>
              <CheckCircle2 size={10} /> 完成
            </>
          )}
        </span>
        <ChevronRight size={12} className="tool-chevron" />
      </summary>
      <div className="tool-call-body">
        {questionItems ? (
          <div className="tool-question-card">
            {questionItems.map((q, qIdx) => (
              <div key={qIdx} className="tool-question-item">
                <div className="tool-question-header">
                  {q.header && (
                    <span className="tool-question-tag">{q.header}</span>
                  )}
                  <span className="tool-question-text">{q.question}</span>
                </div>
                {q.options && q.options.length > 0 && (
                  <div className="tool-options-list">
                    {q.options.map((opt, optIdx) => {
                      const isSelected = result.includes(opt.label);
                      return (
                        <button
                          key={optIdx}
                          type="button"
                          className={`tool-option-item ${isSelected ? "selected" : ""} ${onAnswerQuestion ? "clickable" : ""}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            e.preventDefault();
                            if (onAnswerQuestion) {
                              onAnswerQuestion(q.question, opt.label);
                            }
                          }}
                          disabled={!onAnswerQuestion}
                          title={
                            onAnswerQuestion
                              ? `点击选择: ${opt.label}`
                              : undefined
                          }
                        >
                          <div className="tool-option-label">
                            <span>{opt.label}</span>
                            {isSelected && (
                              <span className="tool-option-badge">已选择</span>
                            )}
                            {!isSelected && onAnswerQuestion && (
                              <span className="tool-option-action-hint">
                                点击选择
                              </span>
                            )}
                          </div>
                          {opt.description && (
                            <div className="tool-option-desc">
                              {opt.description}
                            </div>
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>
            ))}
          </div>
        ) : webSearchData ? (
          <div className="tool-websearch-card">
            <div className="tool-web-header">
              <div className="tool-web-title-box">
                <Search size={13} className="tool-category-icon web_search" />
                <span className="tool-question-tag">网页搜索</span>
                <span className="tool-web-query-pill" title={webSearchData.query}>
                  {webSearchData.query}
                </span>
              </div>
            </div>
            {webSearchData.results.length > 0 && (
              <div className="tool-web-results-list">
                {webSearchData.results.map((res, rIdx) => (
                  <div key={rIdx} className="tool-web-result-item">
                    {res.url ? (
                      <a
                        href={res.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="tool-web-result-link"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <span>{res.title}</span>
                        <ExternalLink size={10} />
                      </a>
                    ) : (
                      <span>{res.title}</span>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : webFetchData ? (
          <div className="tool-webfetch-card">
            <div className="tool-web-header">
              <div className="tool-web-title-box">
                <Globe size={13} className="tool-category-icon web_fetch" />
                <span className="tool-question-tag">网页抓取</span>
                {webFetchData.domain && (
                  <span className="tool-domain-pill">{webFetchData.domain}</span>
                )}
              </div>
            </div>
            <a
              href={webFetchData.url}
              target="_blank"
              rel="noopener noreferrer"
              className="tool-web-external-link"
              onClick={(e) => e.stopPropagation()}
              title={webFetchData.url}
            >
              <span>{webFetchData.url}</span>
              <ExternalLink size={11} />
            </a>
          </div>
        ) : diffStats.oldText || diffStats.newText ? (
          <div className="tool-diff-viewer">
            {diffStats.oldText && (
              <div className="diff-chunk diff-chunk-del">
                <div className="diff-chunk-tag">- 变更前</div>
                <pre className="diff-chunk-pre">{diffStats.oldText}</pre>
              </div>
            )}
            {diffStats.newText && (
              <div className="diff-chunk diff-chunk-add">
                <div className="diff-chunk-tag">+ 变更后</div>
                <pre className="diff-chunk-pre">{diffStats.newText}</pre>
              </div>
            )}
          </div>
        ) : (
          args.trim() && (
            <TruncatedOutputBlock
              content={args}
              maxLines={80}
              renderContent={(text) => (
                <pre className="tool-content-pre">{text}</pre>
              )}
            />
          )
        )}
        {result.trim() &&
          !questionItems &&
          !webSearchData &&
          !webFetchData && (
            <div className="tool-result-markdown">
              <span className="tool-result-caption">执行结果</span>
              <TruncatedOutputBlock
                content={result}
                maxLines={80}
                renderContent={(text) => <MarkdownSection content={text} />}
              />
            </div>
          )}
        {result.trim() && questionItems && !result.startsWith("{") && (
          <div className="tool-result-markdown">
            <span className="tool-result-caption">决策反馈</span>
            <MarkdownSection content={result} />
          </div>
        )}
      </div>
    </details>
  );
});

const REMARK_PLUGINS = [remarkGfm];

const MarkdownSection = memo(function MarkdownSection({
  content,
}: {
  content: string;
}) {
  return (
    <div className="markdown-body custom-markdown">
      <ReactMarkdown
        remarkPlugins={REMARK_PLUGINS}
        components={{
          pre({ children }) {
            // The pre node, not newlines or a language hint, identifies a block.
            // Leave inline code to react-markdown's default renderer.
            const child = React.Children.toArray(children)[0];
            if (!React.isValidElement<React.ComponentProps<"code">>(child)) {
              return <pre>{children}</pre>;
            }
            const language =
              /(?:^|\s)language-(\S+)/.exec(child.props.className ?? "")?.[1] ??
              "text";
            return (
              <CodeBlock
                language={language}
                value={String(child.props.children ?? "").replace(/\n$/, "")}
              />
            );
          },
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
});

const CodeBlock = memo(function CodeBlock({
  language,
  value,
}: {
  language: string;
  value: string;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  };

  return (
    <div className="code-block-wrapper">
      <div className="code-block-header">
        <span className="code-language">{language || "code"}</span>
        <button
          type="button"
          className="code-copy-btn"
          onClick={handleCopy}
          aria-label="复制代码"
          title="复制代码"
        >
          {copied ? (
            <>
              <Check size={12} className="copy-success" />
              <span>已复制</span>
            </>
          ) : (
            <>
              <Copy size={12} />
              <span>复制</span>
            </>
          )}
        </button>
      </div>
      <pre className="code-block-pre">
        <code>{value}</code>
      </pre>
    </div>
  );
});
