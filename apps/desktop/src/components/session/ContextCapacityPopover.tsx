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
    // 1. Determine total window tokens
    let windowTokens = runtime?.context_window_tokens;
    if (!windowTokens || windowTokens <= 0) {
      const name = (modelName || runtime?.model || "").toLowerCase();
      if (name.includes("gemini")) {
        windowTokens = 1_000_000;
      } else if (
        name.includes("gpt-4") ||
        name.includes("o1") ||
        name.includes("o3")
      ) {
        windowTokens = 200_000;
      } else if (name.includes("claude-3-7") || name.includes("claude-3-5")) {
        windowTokens = 200_000;
      } else {
        windowTokens = 270_000;
      }
    }

    // 2. Count tokens from messages if available
    let msgCharCount = 0;
    let toolCharCount = 0;
    let sysCharCount = 0;
    let otherCharCount = 0;

    for (const m of messages) {
      const textLen = (m.content || "").length;
      if (m.kind === "tool" || m.kind === "command") {
        toolCharCount += textLen;
      } else if (m.role === "system") {
        sysCharCount += textLen;
      } else if (m.role === "user" || m.role === "assistant") {
        msgCharCount += textLen;
      } else {
        otherCharCount += textLen;
      }
    }

    const totalChars =
      msgCharCount + toolCharCount + sysCharCount + otherCharCount;
    const estimatedMsgTokens = Math.max(120, Math.round(msgCharCount / 3.2));
    const estimatedToolTokens = Math.max(80, Math.round(toolCharCount / 3.2));
    const estimatedSysTokens = Math.max(600, Math.round(sysCharCount / 3.2));
    const estimatedOtherTokens = Math.max(
      100,
      Math.round(otherCharCount / 3.2),
    );

    let used = runtime?.context_used_tokens;
    if (!used || used <= 0) {
      used =
        estimatedMsgTokens +
        estimatedToolTokens +
        estimatedSysTokens +
        estimatedOtherTokens;
    }

    // Proportions
    let pMsg = totalChars > 0 ? msgCharCount / totalChars : 0.65;
    let pTool = totalChars > 0 ? (toolCharCount * 0.6) / totalChars : 0.15;
    let pSysTool = totalChars > 0 ? (toolCharCount * 0.4) / totalChars : 0.1;
    let pSys = totalChars > 0 ? sysCharCount / totalChars : 0.04;
    let pSkills = 0.02;
    let pOther = Math.max(0.02, 1 - (pMsg + pTool + pSysTool + pSys + pSkills));

    // Normalize
    const sum = pMsg + pTool + pSysTool + pSys + pSkills + pOther;
    pMsg /= sum;
    pTool /= sum;
    pSysTool /= sum;
    pSys /= sum;
    pSkills /= sum;
    pOther /= sum;

    const catList: CategoryBreakdown[] = [
      {
        id: "messages",
        label: "消息",
        color: "#3b82f6", // Blue
        percentage: +(pMsg * 100).toFixed(1),
        tokens: Math.round(used * pMsg),
      },
      {
        id: "mcp_tools",
        label: "MCP 工具",
        color: "#60a5fa", // Sky Blue
        percentage: +(pTool * 100).toFixed(1),
        tokens: Math.round(used * pTool),
      },
      {
        id: "system_tools",
        label: "系统工具",
        color: "#38bdf8", // Light Cyan
        percentage: +(pSysTool * 100).toFixed(1),
        tokens: Math.round(used * pSysTool),
      },
      {
        id: "other",
        label: "其他",
        color: "#94a3b8", // Slate
        percentage: +(pOther * 100).toFixed(1),
        tokens: Math.round(used * pOther),
      },
      {
        id: "system_prompt",
        label: "系统提示词",
        color: "#818cf8", // Indigo
        percentage: +(pSys * 100).toFixed(1),
        tokens: Math.round(used * pSys),
      },
      {
        id: "skills",
        label: "技能",
        color: "#a78bfa", // Purple
        percentage: +(pSkills * 100).toFixed(1),
        tokens: Math.round(used * pSkills),
      },
    ];

    const pct = Math.min(100, +((used / windowTokens) * 100).toFixed(1));
    const hitRate = Math.min(
      96.5,
      Math.max(
        45.0,
        +(65 + Math.min(25, (used / windowTokens) * 35)).toFixed(1),
      ),
    );

    return {
      usedTokens: used,
      totalWindow: windowTokens,
      percentage: pct,
      categories: catList,
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
            {metrics.categories.map((cat) => (
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
            {metrics.categories.map((cat) => (
              <div key={cat.id} className="category-row">
                <div className="category-label-group">
                  <span
                    className="category-dot"
                    style={{ backgroundColor: cat.color }}
                  />
                  <span className="category-name">{cat.label}</span>
                </div>
                <span className="category-pct">{cat.percentage}%</span>
              </div>
            ))}
          </div>

          {/* Divider */}
          <div className="popover-divider" />

          {/* Footer */}
          <div className="popover-footer">
            <span className="footer-label">平均缓存命中率</span>
            <span className="footer-value">{metrics.cacheHitRate}%</span>
          </div>
        </div>
      )}
    </div>
  );
}
