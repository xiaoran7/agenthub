import { memo, useEffect, useRef, useState } from "react";
import { copyTextToClipboard } from "../lib/clipboard";
import {
  isRedactedPathOnly,
  readableRemoteText,
  type ConversationItem,
} from "../lib/remote";
import { MarkdownView } from "./MarkdownView";
import { ThinkingAccordion } from "./ThinkingAccordion";

export function CheckIcon({
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

export function CopyIcon({
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

const FENCED_CODE_SEGMENT_RE =
  /(^|\n)(`{3,}|~{3,})[^\n]*\n[\s\S]*?(?:\n\2[ \t]*(?=\n|$)|$)/g;
const THINKING_BLOCK_GLOBAL_RE = /<(thinking|thought)>([\s\S]*?)(?:<\/\1>|$)/gi;

/**
 * Extracts <thinking> or <thought> blocks outside fenced Markdown code blocks
 * for CDS ThinkingAccordion, supporting multiple blocks across a turn.
 */
export function splitThinkingContent(
  content: string,
  isStreaming = false,
): {
  thinking: string | null;
  cleanText: string;
  isThinkingLive: boolean;
} {
  const collected: string[] = [];
  let isThinkingLive = false;
  let cleanBuffer = "";
  let cursor = 0;

  const extractProse = (prose: string) => {
    THINKING_BLOCK_GLOBAL_RE.lastIndex = 0;
    let lastPos = 0;
    let match: RegExpExecArray | null;
    while ((match = THINKING_BLOCK_GLOBAL_RE.exec(prose)) !== null) {
      cleanBuffer += prose.slice(lastPos, match.index);
      const inner = (match[2] ?? "").trim();
      if (inner) collected.push(inner);
      const full = match[0];
      const tag = (match[1] ?? "thinking").toLowerCase();
      if (isStreaming && !new RegExp(`</${tag}>\\s*$`, "i").test(full)) {
        isThinkingLive = true;
      }
      lastPos = match.index + full.length;
    }
    cleanBuffer += prose.slice(lastPos);
  };

  FENCED_CODE_SEGMENT_RE.lastIndex = 0;
  let fenceMatch: RegExpExecArray | null;
  while ((fenceMatch = FENCED_CODE_SEGMENT_RE.exec(content)) !== null) {
    extractProse(content.slice(cursor, fenceMatch.index));
    cleanBuffer += fenceMatch[0];
    cursor = fenceMatch.index + fenceMatch[0].length;
  }
  extractProse(content.slice(cursor));

  const thinking = collected.length > 0 ? collected.join("\n\n") : null;
  return {
    thinking,
    cleanText: cleanBuffer.trim(),
    isThinkingLive,
  };
}

export interface RemoteMessageCardProps {
  item: ConversationItem;
  isStreamingThis: boolean;
  providerName?: string | undefined;
}

export const RemoteMessageCard = memo(function RemoteMessageCard({
  item,
  isStreamingThis,
  providerName,
}: RemoteMessageCardProps) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      clearTimeout(copyTimerRef.current);
    };
  }, []);

  if (isRedactedPathOnly(item.text)) {
    return null;
  }
  const rawText = readableRemoteText(item.text);
  const { thinking, cleanText, isThinkingLive } =
    item.kind === "assistant"
      ? splitThinkingContent(rawText, isStreamingThis)
      : {
          thinking: null,
          cleanText: rawText,
          isThinkingLive: false,
        };

  const isSending = item.kind === "user" && item.status === "running";
  const canCopyAssistant =
    item.kind === "assistant" &&
    !isStreamingThis &&
    cleanText.trim().length > 0;

  const copyMessage = () => {
    if (!cleanText) return;
    void copyTextToClipboard(cleanText).then((ok) => {
      if (!ok || !mountedRef.current) return;
      setCopied(true);
      clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        if (mountedRef.current) setCopied(false);
      }, 1800);
    });
  };

  return (
    <article
      className={`remote-message ${item.kind} ${isSending ? "sending optimistic" : ""}`}
    >
      {item.kind !== "user" && (
        <div className="remote-turn-tag">
          <span className="remote-turn-marker" aria-hidden="true" />
          <small>{providerName}</small>
          {canCopyAssistant && (
            <button
              type="button"
              className={`remote-msg-copy-btn ${copied ? "copied" : ""}`}
              onClick={copyMessage}
              title={copied ? "已复制回复内容" : "复制回复内容"}
              aria-label={copied ? "已复制回复内容" : "复制回复内容"}
            >
              {copied ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
              <span>{copied ? "已复制" : "复制"}</span>
            </button>
          )}
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
          <MarkdownView content={cleanText} streaming={isStreamingThis} />
        )}
        {isSending && (
          <div className="remote-sending-indicator" aria-label="正在发送">
            <span className="remote-sending-dot" />
            <span>发送中…</span>
          </div>
        )}
      </div>
    </article>
  );
});
