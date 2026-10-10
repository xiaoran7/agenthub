"use client";

import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface MarkdownViewProps {
  content: string;
  /** Appends a caret to the last block while the Agent is still writing. */
  streaming?: boolean;
  className?: string;
}

/**
 * A copy button that confirms in place. Falls back to a hidden textarea when
 * the async clipboard API is unavailable (older browsers, non-secure origins).
 */
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => () => clearTimeout(timer.current), []);

  const handleCopy = useCallback(async () => {
    try {
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = text;
        textarea.style.position = "fixed";
        textarea.style.opacity = "0";
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        document.body.removeChild(textarea);
      }
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard access can be denied; leaving the label unchanged is
      // clearer than claiming a copy that did not happen.
    }
  }, [text]);

  return (
    <button
      type="button"
      className={`remote-code-copy-btn ${copied ? "copied" : ""}`}
      onClick={() => void handleCopy()}
      title={copied ? "已复制到剪贴板" : "复制代码"}
      aria-label={copied ? "已复制" : "复制代码"}
    >
      {copied ? (
        <>
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            aria-hidden="true"
          >
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>已复制</span>
        </>
      ) : (
        <>
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            aria-hidden="true"
          >
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
          <span>复制</span>
        </>
      )}
    </button>
  );
}

function CodeBlock({ language, value }: { language: string; value: string }) {
  return (
    <div className="remote-code-block">
      <div className="remote-code-header">
        <span className="remote-code-lang">{language}</span>
        <CopyButton text={value} />
      </div>
      <pre className="remote-code-pre">
        <code>{value}</code>
      </pre>
    </div>
  );
}

const REMARK_PLUGINS = [remarkGfm];

/** Renders the caret inline, so the streaming state never reflows the block. */
function Caret() {
  return <span className="remote-typing-cursor" aria-hidden="true" />;
}

const MARKDOWN_COMPONENTS: React.ComponentProps<
  typeof ReactMarkdown
>["components"] = {
  pre({ children }) {
    // The <pre> is what identifies a fenced code block; the language
    // rides on the inner <code> as a `language-*` class.
    const child = React.Children.toArray(children)[0];
    if (!React.isValidElement<React.ComponentProps<"code">>(child)) {
      return <pre>{children}</pre>;
    }
    const language =
      /(?:^|\s)language-(\S+)/.exec(child.props.className ?? "")?.[1] ?? "text";
    const value = String(child.props.children ?? "").replace(/\n$/, "");
    return <CodeBlock language={language} value={value} />;
  },
  a({ children, href }) {
    return (
      <a
        className="remote-md-link"
        href={href}
        target="_blank"
        rel="noopener noreferrer"
      >
        {children}
      </a>
    );
  },
  table({ children }) {
    return (
      <div className="remote-md-table-wrapper">
        <table className="remote-md-table">{children}</table>
      </div>
    );
  },
};

/**
 * Markdown rendering for Agent output.
 *
 * Built on react-markdown + remark-gfm so the grammar matches CommonMark and
 * the GFM extensions: ordered lists keep their numbers, nested lists keep their
 * depth, tables and task lists render structurally, and images are real images.
 * The caret is appended to the last child of the last block rather than
 * injected into the AST, which keeps the streamed text byte-identical to what
 * the Provider sent.
 */
export function MarkdownView({
  content,
  streaming = false,
  className = "",
}: MarkdownViewProps) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const [showCaret, setShowCaret] = useState(streaming);

  // The caret belongs to whatever block currently ends the message. Re-applying
  // it on every content change keeps it correct as blocks are appended, and
  // dropping it once the turn finishes keeps the final text clean.
  useEffect(() => {
    if (!streaming) {
      setShowCaret(false);
      return;
    }
    const body = bodyRef.current;
    if (body === null) {
      setShowCaret(true);
      return;
    }
    // Walk to the last rendered block; if it is a code block the caret would
    // sit inside the <pre>, so fall back to after the whole body.
    const blocks = body.children;
    const last = blocks.item(blocks.length - 1);
    setShowCaret(last !== null && last.tagName !== "PRE");
  }, [content, streaming]);

  return (
    <div
      className={`remote-md-content ${className}`}
      ref={bodyRef}
      data-streaming={streaming ? "true" : undefined}
    >
      <ReactMarkdown
        remarkPlugins={REMARK_PLUGINS}
        components={MARKDOWN_COMPONENTS}
      >
        {content}
      </ReactMarkdown>
      {showCaret && <Caret />}
    </div>
  );
}
