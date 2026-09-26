import React, { useMemo, useRef, useEffect, useState } from "react";
import type { SessionRuntimeContext } from "../../types";
import type { ConversationMessage } from "../../turn-stream";

export interface ContextCapacityProps {
  runtime?: SessionRuntimeContext | null;
  messages?: ConversationMessage[];
  modelName?: string;
  className?: string;
}

interface CategoryBreakdown {
  id: string;
  label: string;
  color: string;
  /** Share of the whole context window, which is what the bar renders. */
  windowPct: number;
  /** Share of the tokens actually used, shown next to the count. */
  usedPct: number;
  tokens: number;
  /** Optional drill-down: individual entries behind this category. */
  details?: Array<{ label: string; tokens: number; count: number }>;
}

interface ContextMetrics {
  usedTokens: number;
  totalWindow: number;
  percentage: number;
  freeTokens: number;
  freePct: number;
  categories: CategoryBreakdown[];
  cacheHitRate: number;
  /** True when the breakdown is measured rather than estimated. */
  measured: boolean;
}

function formatTokens(count: number): string {
  if (count >= 10_000) {
    return `${(count / 10_000).toFixed(1)}万`;
  }
  return Math.round(count).toLocaleString();
}

function pct(part: number, whole: number): number {
  if (whole <= 0) return 0;
  return +((part / whole) * 100).toFixed(1);
}

/** Fallback window size when the provider does not report one. */
function inferWindowTokens(
  runtime?: SessionRuntimeContext | null,
  modelName?: string,
): number {
  if (runtime?.context_window_tokens && runtime.context_window_tokens > 0) {
    return runtime.context_window_tokens;
  }
  const name = (modelName || runtime?.model || "").toLowerCase();
  if (name.includes("gemini")) return 1_000_000;
  if (name.includes("grok")) return 128_000;
  if (name.includes("glm")) return 128_000;
  return 200_000;
}

export function useContextMetrics(
  runtime?: SessionRuntimeContext | null,
  messages: ConversationMessage[] = [],
  modelName?: string,
): ContextMetrics {
  return useMemo(() => {
    const windowTokens = inferWindowTokens(runtime, modelName);
    const used = runtime?.context_used_tokens ?? runtime?.input_tokens ?? 0;
    const safeWindow = windowTokens > 0 ? windowTokens : 1;
    const safeUsed = used > 0 ? used : 1;

    // Count transcript entries per kind so categories can report real item
    // counts alongside real token totals.
    let toolEntryCount = 0;
    let messageEntryCount = 0;
    let toolChars = 0;
    let messageChars = 0;
    for (const message of messages) {
      const textLength = (message.content || "").length;
      if (message.kind === "tool" || message.kind === "command") {
        toolChars += textLength;
        toolEntryCount += 1;
      } else {
        messageChars += textLength;
        messageEntryCount += 1;
      }
    }

    const cacheRead = runtime?.cache_read_tokens ?? 0;
    const cacheWrite = runtime?.cache_write_tokens ?? 0;
    const inputTotal = runtime?.input_tokens ?? used;
    const freshInput = Math.max(0, inputTotal - cacheRead - cacheWrite);
    const output = runtime?.output_tokens ?? 0;
    const reasoning = runtime?.reasoning_tokens ?? 0;
    const hasDetailed =
      runtime?.input_tokens != null ||
      runtime?.cache_read_tokens != null ||
      runtime?.output_tokens != null;

    const categories: CategoryBreakdown[] = [];
    const push = (
      id: string,
      label: string,
      color: string,
      tokens: number,
      details?: CategoryBreakdown["details"],
    ) => {
      if (tokens <= 0) return;
      categories.push({
        id,
        label,
        color,
        tokens,
        windowPct: pct(tokens, safeWindow),
        usedPct: pct(tokens, safeUsed),
        details,
      });
    };

    if (hasDetailed) {
      // Measured breakdown: every category comes from a reported token count.
      push("cache_read", "缓存读取", "var(--status-success)", cacheRead);
      push("prompt_input", "新输入", "var(--status-info)", freshInput);
      if (reasoning > 0) {
        push("reasoning", "思考输出", "var(--brand-accent)", reasoning);
      }
      if (cacheWrite > 0) {
        push("cache_write", "缓存写入", "var(--status-warning)", cacheWrite);
      }
      push("output", "生成输出", "var(--status-warning)", output);
    } else {
      // No provider breakdown: attribute the reported total across the
      // transcript by character weight so the split is at least grounded in
      // what the conversation actually contains.
      const totalChars = toolChars + messageChars;
      if (totalChars > 0 && used > 0) {
        const toolTokens = Math.round((used * toolChars) / totalChars);
        const messageTokens = Math.max(0, used - toolTokens);
        push(
          "tools",
          "工具调用与结果",
          "var(--status-success)",
          toolTokens,
          toolEntryCount > 0
            ? [{ label: "工具条目", tokens: toolTokens, count: toolEntryCount }]
            : undefined,
        );
        push(
          "messages",
          "对话消息",
          "var(--status-info)",
          messageTokens,
          messageEntryCount > 0
            ? [
                {
                  label: "消息条目",
                  tokens: messageTokens,
                  count: messageEntryCount,
                },
              ]
            : undefined,
        );
      } else {
        push("messages", "对话上下文", "var(--status-info)", used);
      }
    }

    const freeTokens = Math.max(0, safeWindow - used);
    const cacheHitRate =
      inputTotal > 0 && cacheRead > 0
        ? Math.min(100, +((cacheRead / inputTotal) * 100).toFixed(1))
        : 0;

    return {
      usedTokens: used,
      totalWindow: windowTokens,
      percentage: Math.min(100, pct(used, safeWindow)),
      freeTokens,
      freePct: pct(freeTokens, safeWindow),
      categories,
      cacheHitRate,
      measured: hasDetailed,
    };
  }, [runtime, messages, modelName]);
}

export function ContextCapacityRing({
  runtime,
  messages = [],
  modelName,
  className = "",
}: ContextCapacityProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const metrics = useContextMetrics(runtime, messages, modelName);

  const { percentage, usedTokens, totalWindow } = metrics;

  const radius = 7.5;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - (percentage / 100) * circumference;

  let ringColor = "var(--status-info)";
  if (percentage >= 90) {
    ringColor = "var(--status-danger)";
  } else if (percentage >= 70) {
    ringColor = "var(--status-warning)";
  }

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
        setExpanded(null);
      }
    }
    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => {
        document.removeEventListener("mousedown", handleClickOutside);
      };
    }
  }, [isOpen]);

  const tooltip = `上下文容量: ${formatTokens(usedTokens)} / ${formatTokens(totalWindow)} (${percentage}%)`;

  return (
    <div className={`composer-context-wrapper ${className}`} ref={containerRef}>
      <button
        type="button"
        className={`composer-context-ring-btn ${isOpen ? "active" : ""}`}
        onClick={() => setIsOpen((prev) => !prev)}
        title={tooltip}
        aria-label={tooltip}
        aria-expanded={isOpen}
      >
        <svg
          className="context-ring-svg"
          width="20"
          height="20"
          viewBox="0 0 20 20"
        >
          <circle
            cx="10"
            cy="10"
            r={radius}
            fill="none"
            stroke="var(--border-subtle)"
            strokeWidth="2.2"
          />
          <circle
            cx="10"
            cy="10"
            r={radius}
            fill="none"
            stroke={ringColor}
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={strokeDashoffset}
            transform="rotate(-90 10 10)"
          />
        </svg>
      </button>

      {isOpen && (
        <div
          className="context-capacity-popover"
          role="dialog"
          aria-label="上下文容量"
        >
          <div className="popover-header">
            <span className="popover-title">上下文窗口</span>
            <span className="popover-stats">
              <strong>{formatTokens(usedTokens)}</strong>
              {" / "}
              <span>{formatTokens(totalWindow)}</span>
              <span className="popover-percent"> ({percentage}%)</span>
            </span>
          </div>

          <div className="segmented-progress-bar">
            {metrics.categories.map((cat) => (
              <div
                key={cat.id}
                className="progress-segment"
                style={{
                  width: `${cat.windowPct}%`,
                  backgroundColor: cat.color,
                }}
                title={`${cat.label}: ${formatTokens(cat.tokens)} (占窗口 ${cat.windowPct}%)`}
              />
            ))}
            <div
              className="progress-segment free"
              style={{ width: `${metrics.freePct}%` }}
              title={`剩余空间: ${formatTokens(metrics.freeTokens)} (占窗口 ${metrics.freePct}%)`}
            />
          </div>

          <div className="popover-categories">
            {metrics.categories.map((cat) => {
              const hasDetails = (cat.details?.length ?? 0) > 0;
              const isExpanded = expanded === cat.id;
              return (
                <div key={cat.id} className="category-block">
                  <div
                    className={`category-row ${hasDetails ? "expandable" : ""}`}
                    role={hasDetails ? "button" : undefined}
                    tabIndex={hasDetails ? 0 : undefined}
                    aria-expanded={hasDetails ? isExpanded : undefined}
                    onClick={() =>
                      hasDetails &&
                      setExpanded((prev) => (prev === cat.id ? null : cat.id))
                    }
                    onKeyDown={(event) => {
                      if (
                        hasDetails &&
                        (event.key === "Enter" || event.key === " ")
                      ) {
                        event.preventDefault();
                        setExpanded((prev) =>
                          prev === cat.id ? null : cat.id,
                        );
                      }
                    }}
                    title={`${cat.label}: ${formatTokens(cat.tokens)} (占窗口 ${cat.windowPct}% · 占已用 ${cat.usedPct}%)`}
                  >
                    <div className="category-label-group">
                      {hasDetails && (
                        <span
                          className={`category-caret ${isExpanded ? "open" : ""}`}
                          aria-hidden="true"
                        >
                          ›
                        </span>
                      )}
                      <span
                        className="category-dot"
                        style={{ backgroundColor: cat.color }}
                      />
                      <span className="category-name">{cat.label}</span>
                    </div>
                    <span className="category-pct">
                      {formatTokens(cat.tokens)}
                      <small className="category-sub-pct">
                        {" "}
                        {cat.usedPct}%
                      </small>
                    </span>
                  </div>
                  {isExpanded &&
                    cat.details?.map((detail) => (
                      <div className="category-detail" key={detail.label}>
                        <span className="category-detail-name">
                          {detail.label}
                        </span>
                        <span className="category-detail-value">
                          {detail.tokens > 0
                            ? formatTokens(detail.tokens)
                            : "—"}
                          <small className="category-detail-count">
                            {detail.count}
                          </small>
                        </span>
                      </div>
                    ))}
                </div>
              );
            })}
            <div className="category-row free-row">
              <div className="category-label-group">
                <span className="category-dot free" />
                <span className="category-name">剩余空间</span>
              </div>
              <span className="category-pct">
                {formatTokens(metrics.freeTokens)}
                <small className="category-sub-pct"> {metrics.freePct}%</small>
              </span>
            </div>
          </div>

          <div className="popover-divider" />

          <div className="popover-footer">
            <span className="footer-label">
              {metrics.measured ? "本轮缓存读取率" : "缓存读取率（未上报）"}
            </span>
            <span className="footer-value">
              {metrics.measured ? `${metrics.cacheHitRate}%` : "—"}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
