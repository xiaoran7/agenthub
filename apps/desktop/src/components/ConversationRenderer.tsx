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
} from "lucide-react";
import type { ConversationMessage } from "../turn-stream";

interface ConversationRendererProps {
  message: ConversationMessage;
  index: number;
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
}: {
  messages: ConversationMessage[];
}) {
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
            <ToolCallBlock key={idx} message={item} />
          ))}
        </div>
      )}
    </div>
  );
});

export const ConversationItem = memo(function ConversationItem({
  message,
}: ConversationRendererProps) {
  if (
    message.content === "" &&
    message.kind !== "tool" &&
    message.kind !== "thinking"
  ) {
    return null;
  }

  // 1. Dedicated thinking process item
  if (message.kind === "thinking") {
    return <ThinkingBlock content={message.content} defaultOpen={false} />;
  }

  // 2. Dedicated tool call / command / status item (collapsed one-liner)
  if (
    message.kind === "tool" ||
    message.kind === "command" ||
    message.kind === "status"
  ) {
    return <ToolCallBlock message={message} />;
  }

  // 3. User Message -> Rounded Dark Bubble
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

  // 4. Assistant / System Message -> Natural Fluid Markdown
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
function splitToolContent(content: string): { args: string; result: string } {
  const at = content.indexOf(RESULT_SEPARATOR);
  if (at === -1) return { args: content, result: "" };
  return {
    args: content.slice(0, at).trim(),
    result: content.slice(at + RESULT_SEPARATOR.length).trim(),
  };
}

function ToolCallBlock({ message }: { message: ConversationMessage }) {
  const [open, setOpen] = useState(false);
  const failed = message.status === "failed";
  const running = message.status === "running";
  const isCommand = message.kind === "command";
  const isStatus = message.kind === "status";
  const { args, result } = splitToolContent(message.content);
  const effectiveStatus = running ? "running" : failed ? "failed" : "completed";

  return (
    <details
      className={`tool-call-accordion ${failed ? "failed" : ""} ${open ? "open" : ""}`}
      open={open}
      onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className="tool-call-summary">
        <span className="tool-icon-badge">
          {isCommand ? (
            <Terminal size={12} />
          ) : isStatus ? (
            <Activity size={12} />
          ) : (
            <Wrench size={12} />
          )}
        </span>
        <strong className="tool-name">
          {message.label ||
            (isCommand ? "命令执行" : isStatus ? "状态更新" : "工具调用")}
        </strong>
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
        {args.trim() && <pre className="tool-content-pre">{args}</pre>}
        {result.trim() && (
          <div className="tool-result-markdown">
            <span className="tool-result-caption">执行结果</span>
            <MarkdownSection content={result} />
          </div>
        )}
      </div>
    </details>
  );
}

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
