"use client";

import React, { useState, useEffect } from "react";

/**
 * Claude Design System (CDS) - ThinkingAccordion
 *
 * 逆向还原自 Claude Desktop: 思考过程折叠互斥锁与状态机
 * 1. 生成中 (isThinking = true) 强制展开，剥夺折叠权限，防止视口抖动
 * 2. 思考完成后交还折叠控制权，支持 CSS Grid 0fr -> 1fr 弹性伸缩
 * 3. 陶土橙 (#d97757) 3 点波浪跳跃微动效 (timeline-quiet-dot)
 * 4. 一键引用复制 (Copy as quote)
 */
export interface ThinkingAccordionProps {
  thoughtText: string;
  isThinking?: boolean;
  elapsedMs?: number;
  className?: string;
}

function CheckIcon({ size = 12 }: { size?: number }) {
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
      aria-hidden="true"
    >
      <polyline points="20 6 9 17 4 12" />
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

export function ThinkingAccordion({
  thoughtText,
  isThinking = false,
  elapsedMs = 0,
  className = "",
}: ThinkingAccordionProps) {
  const [userExpanded, setUserExpanded] = useState(false);
  const [didCopy, setDidCopy] = useState(false);

  // 核心互斥物理：思考进行中必定展开
  const isOpen = isThinking || userExpanded;

  const handleToggle = () => {
    if (isThinking) return;
    setUserExpanded((prev) => !prev);
  };

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    const quoted = thoughtText
      .split("\n")
      .map((line) => `> ${line}`)
      .join("\n");
    void navigator.clipboard.writeText(quoted);
    setDidCopy(true);
    setTimeout(() => setDidCopy(false), 2000);
  };

  if (!thoughtText.trim() && !isThinking) return null;

  return (
    <div className={`cds-thinking ${className}`}>
      {/* 头部状态条 */}
      <div className="cds-thinking-header">
        <button
          type="button"
          onClick={handleToggle}
          className="cds-thinking-toggle"
          aria-expanded={isOpen}
          disabled={isThinking}
        >
          <span className="cds-thinking-lead">
            {isThinking ? (
              <span className="cds-thinking-dots">
                <span
                  className="cds-thinking-dot"
                  style={{ animation: "timeline-quiet-dot 1.4s infinite 0s" }}
                />
                <span
                  className="cds-thinking-dot"
                  style={{
                    animation: "timeline-quiet-dot 1.4s infinite 0.18s",
                  }}
                />
                <span
                  className="cds-thinking-dot"
                  style={{
                    animation: "timeline-quiet-dot 1.4s infinite 0.36s",
                  }}
                />
              </span>
            ) : (
              <span className="cds-thinking-dot cds-thinking-dot-idle" />
            )}

            <span className="cds-thinking-title">
              {isThinking ? "正在思考…" : "思考过程"}
            </span>

            {elapsedMs > 0 && (
              <span className="cds-thinking-elapsed">
                ({(elapsedMs / 1000).toFixed(1)}s)
              </span>
            )}
          </span>

          {!isThinking && (
            <ChevronDownIcon
              size={13}
              className={`cds-thinking-chevron${isOpen ? " open" : ""}`}
            />
          )}
        </button>

        <span className="cds-thinking-actions">
          {thoughtText.trim() && (
            <button
              type="button"
              onClick={handleCopy}
              className="cds-thinking-copy"
              title="复制为引用格式"
            >
              {didCopy ? (
                <span className="cds-thinking-copied">
                  <CheckIcon size={12} />
                  <span>已复制</span>
                </span>
              ) : (
                <span>复制思考</span>
              )}
            </button>
          )}
        </span>
      </div>

      {/* 内容区域：纯 CSS Grid 0fr -> 1fr 弹性展开容器 */}
      <div
        className="cds-thinking-body"
        style={{
          gridTemplateRows: isOpen ? "1fr" : "0fr",
        }}
      >
        <div className="cds-thinking-body-inner">
          <div className="cds-thinking-text">{thoughtText}</div>
        </div>
      </div>
    </div>
  );
}
