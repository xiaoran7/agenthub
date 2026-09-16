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
} from "lucide-react";
import type { ConversationMessage } from "../turn-stream";

interface ConversationRendererProps {
  message: ConversationMessage;
  index: number;
}

const RESULT_SEPARATOR = "〔结果〕";

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
    return (
      <div className="message-row user-row">
        <div className="user-bubble-pill">
          <div className="user-bubble-content">
            <ReactMarkdown remarkPlugins={REMARK_PLUGINS}>
              {message.content}
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
          code({ className, children, ...props }: any) {
            const match = /language-(\w+)/.exec(className || "");
            const codeString = String(children).replace(/\n$/, "");
            if (!props.inline && (match || codeString.includes("\n"))) {
              return (
                <CodeBlock
                  language={match ? match[1] : "text"}
                  value={codeString}
                />
              );
            }
            return (
              <code className={className} {...props}>
                {children}
              </code>
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
