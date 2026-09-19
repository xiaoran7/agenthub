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
  percentage: number;
  tokens: number;
}

function formatTokens(count: number): string {
  if (count >= 10_000) {
    const wan = count / 10_000;
    return `${wan.toFixed(wan >= 10 ? 1 : 1)}万`;
  }
  return count.toLocaleString();
}

export function useContextMetrics(
  runtime?: SessionRuntimeContext | null,
  messages: ConversationMessage[] = [],
  modelName?: string,
) {
  return useMemo(() => {
    // 1. Determine total window tokens dynamically based on runtime or model limits
    let windowTokens = runtime?.context_window_tokens;
    if (!windowTokens || windowTokens <= 0) {
      const name = (modelName || runtime?.model || "").toLowerCase();
      if (name.includes("gemini")) {
        windowTokens = 1_000_000;
      } else if (name.includes("claude")) {
        windowTokens = 200_000;
      } else if (
        name.includes("gpt-5") ||
        name.includes("gpt-4") ||
        name.includes("codex") ||
        name.includes("o1") ||
        name.includes("o3")
      ) {
        windowTokens = 200_000;
      } else if (name.includes("grok")) {
        windowTokens = 128_000;
      } else if (name.includes("glm")) {
        windowTokens = 128_000;
      } else {
        windowTokens = 200_000;
      }
    }

    // 2. Extract real token counts if available from runtime
    const hasDetailedTokens =
      runtime?.input_tokens != null ||
      runtime?.cache_read_tokens != null ||
      runtime?.output_tokens != null;

    let used = runtime?.context_used_tokens ?? runtime?.input_tokens ?? 0;
    let hitRate = 0;
    let allCategories: CategoryBreakdown[] = [];

    if (hasDetailedTokens) {
      const cacheRead = runtime?.cache_read_tokens ?? 0;
      const inputTotal = runtime?.input_tokens ?? used;
      const promptInput = Math.max(0, inputTotal - cacheRead);
      const outputTokens = runtime?.output_tokens ?? 0;
      const reasoningTokens = runtime?.reasoning_tokens ?? 0;

      // Real Cache Hit Rate: exact ratio of cached tokens to total input prompt tokens
      if (inputTotal > 0 && cacheRead > 0) {
        hitRate = Math.min(100, +((cacheRead / inputTotal) * 100).toFixed(1));
      } else {
        hitRate = 0;
      }

      const totalActiveTokens = used > 0 ? used : cacheRead + promptInput;
      const safeDenom = totalActiveTokens > 0 ? totalActiveTokens : 1;

      allCategories = [
        {
          id: "cache_read",
          label: "缓存读取 (Prompt Cache)",
          color: "#10b981", // Emerald
          percentage:
            totalActiveTokens > 0 && cacheRead > 0
              ? +((cacheRead / safeDenom) * 100).toFixed(1)
              : 0,
          tokens: cacheRead,
        },
        {
          id: "prompt_input",
          label: "提示输入 (New Input)",
          color: "#3b82f6", // Blue
          percentage:
            totalActiveTokens > 0 && promptInput > 0
              ? +((promptInput / safeDenom) * 100).toFixed(1)
              : 0,
          tokens: promptInput,
        },
        {
          id: "output",
          label: "生成输出 (Completion)",
          color: "#8b5cf6", // Purple
          percentage:
            totalActiveTokens > 0 && outputTokens > 0
              ? +((outputTokens / safeDenom) * 100).toFixed(1)
              : 0,
          tokens: outputTokens,
        },
      ];

      if (reasoningTokens > 0) {
        allCategories.push({
          id: "reasoning",
          label: "深度思考 (Reasoning)",
          color: "#f59e0b", // Amber
          percentage: +((reasoningTokens / safeDenom) * 100).toFixed(1),
          tokens: reasoningTokens,
        });
      }
    } else {
      // 3. Fallback when detailed token breakdown is not reported by provider
      let userMsgCharCount = 0;
      let assistantMsgCharCount = 0;
      let toolCharCount = 0;

      for (const m of messages) {
        const textLen = (m.content || "").length;
        if (m.kind === "tool" || m.kind === "command") {
          toolCharCount += textLen;
        } else if (m.role === "user") {
          userMsgCharCount += textLen;
        } else if (m.role === "assistant") {
          assistantMsgCharCount += textLen;
        }
      }

      const totalRecordedChars =
        userMsgCharCount + assistantMsgCharCount + toolCharCount;
      const safeDenom = used > 0 ? used : 1;

      if (totalRecordedChars > 0 && used > 0) {
        const msgRatio =
          (userMsgCharCount + assistantMsgCharCount) / totalRecordedChars;
        const toolRatio = toolCharCount / totalRecordedChars;

        const msgTokens = Math.round(used * msgRatio);
        const toolTokens = Math.max(0, used - msgTokens);

        allCategories = [
          {
            id: "messages",
            label: "对话消息",
            color: "#3b82f6",
            percentage: +((msgTokens / safeDenom) * 100).toFixed(1),
            tokens: msgTokens,
          },
          {
            id: "tools",
            label: "工具调用与结果",
            color: "#38bdf8",
            percentage: +((toolTokens / safeDenom) * 100).toFixed(1),
            tokens: toolTokens,
          },
        ];
      } else {
        allCategories = [
          {
            id: "messages",
            label: "对话上下文",
            color: "#3b82f6",
            percentage: 0,
            tokens: 0,
          },
        ];
      }
      hitRate = 0;
    }

    const pct =
      windowTokens > 0
        ? Math.min(100, +((used / windowTokens) * 100).toFixed(1))
        : 0;

    return {
      usedTokens: used,
      totalWindow: windowTokens,
      percentage: pct,
      categories: allCategories,
      cacheHitRate: hitRate,
    };
  }, [runtime, messages, modelName]);
}

/**
 * ZCode Native Style Circular Progress Ring (matching 图三)
 */
export function ContextCapacityRing({
  runtime,
  messages = [],
  modelName,
  className = "",
}: ContextCapacityProps) {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const metrics = useContextMetrics(runtime, messages, modelName);

  const { percentage, usedTokens, totalWindow } = metrics;

  // SVG Ring calculation (radius 7.5 on 20x20 canvas)
  const radius = 7.5;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - (percentage / 100) * circumference;

  // Adaptive ring color based on percentage usage
  let ringColor = "#3b82f6"; // Primary blue
  if (percentage >= 90) {
    ringColor = "#ef4444"; // Red
  } else if (percentage >= 70) {
    ringColor = "#f59e0b"; // Warning amber
  }

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsOpen(false);
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
      >
        <svg
          className="context-ring-svg"
          width="20"
          height="20"
          viewBox="0 0 20 20"
        >
          {/* Background Track */}
          <circle
            cx="10"
            cy="10"
            r={radius}
            fill="none"
            stroke="rgba(255, 255, 255, 0.15)"
            strokeWidth="2.2"
          />
          {/* Active Progress Arc */}
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
        <div className="context-capacity-popover">
          {/* Header */}
          <div className="popover-header">
            <span className="popover-title">上下文容量</span>
            <span className="popover-stats">
              <strong>{formatTokens(usedTokens)}</strong>
              {" / "}
              <span>{formatTokens(totalWindow)}</span>
              <span className="popover-percent"> ({percentage}%)</span>
            </span>
          </div>

          {/* Multi-color segmented progress bar */}
          <div className="segmented-progress-bar">
            {metrics.categories
              .filter((cat) => cat.percentage > 0)
              .map((cat) => (
                <div
                  key={cat.id}
                  className="progress-segment"
                  style={{
                    width: `${cat.percentage}%`,
                    backgroundColor: cat.color,
                  }}
                  title={`${cat.label}: ${cat.percentage}% (${formatTokens(cat.tokens)})`}
                />
              ))}
          </div>

          {/* Breakdown List */}
          <div className="popover-categories">
            {metrics.categories.map((cat) => {
              const isZero = cat.percentage === 0;
              return (
                <div
                  key={cat.id}
                  className={`category-row ${isZero ? "zero-usage" : ""}`}
                >
                  <div className="category-label-group">
                    <span
                      className="category-dot"
                      style={{
                        backgroundColor: isZero ? "transparent" : cat.color,
                        border: isZero
                          ? "1px solid rgba(255, 255, 255, 0.22)"
                          : "none",
                      }}
                    />
                    <span
                      className="category-name"
                      style={{ color: isZero ? "#71717a" : "#d4d4d8" }}
                    >
                      {cat.label}
                    </span>
                  </div>
                  <span
                    className="category-pct"
                    style={{ color: isZero ? "#71717a" : "#f4f4f5" }}
                  >
                    {cat.percentage}%
                  </span>
                </div>
              );
            })}
          </div>

          {/* Divider */}
          <div className="popover-divider" />

          {/* Footer */}
          <div className="popover-footer">
            <span className="footer-label">平均缓存命中率</span>
            <span className="footer-value">
              {metrics.cacheHitRate > 0
                ? `${metrics.cacheHitRate}%`
                : "0%"}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
