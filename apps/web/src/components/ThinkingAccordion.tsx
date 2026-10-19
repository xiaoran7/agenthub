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
    <div
      className={`cds-thinking-accordion my-2.5 rounded-xl border border-[var(--cds-border)] bg-[var(--cds-surface-1)] transition-colors hover:border-[var(--cds-border-strong)] overflow-hidden text-xs ${className}`}
      style={{
        boxShadow: "var(--cds-shadow-sm, 0 1px 2px rgba(0,0,0,0.04))",
      }}
    >
      {/* 头部状态条 */}
      <div
        onClick={handleToggle}
        className={`flex items-center justify-between px-3.5 py-2 select-none ${
          isThinking ? "cursor-default" : "cursor-pointer"
        }`}
      >
        <div className="flex items-center gap-2">
          {isThinking ? (
            <div className="inline-flex items-center gap-1">
              <span
                className="w-1.5 h-1.5 rounded-full bg-[var(--cds-clay,#d97757)]"
                style={{ animation: "timeline-quiet-dot 1.4s infinite 0s" }}
              />
              <span
                className="w-1.5 h-1.5 rounded-full bg-[var(--cds-clay,#d97757)]"
                style={{ animation: "timeline-quiet-dot 1.4s infinite 0.18s" }}
              />
              <span
                className="w-1.5 h-1.5 rounded-full bg-[var(--cds-clay,#d97757)]"
                style={{ animation: "timeline-quiet-dot 1.4s infinite 0.36s" }}
              />
            </div>
          ) : (
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--cds-text-muted,#8a8a8a)]" />
          )}

          <span className="font-semibold text-[var(--cds-text-primary,#151515)]">
            {isThinking ? "Thinking…" : "Thought process"}
          </span>

          {elapsedMs > 0 && (
            <span className="font-mono text-[11px] text-[var(--cds-text-muted,#8a8a8a)]">
              ({(elapsedMs / 1000).toFixed(1)}s)
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {thoughtText.trim() && (
            <button
              type="button"
              onClick={handleCopy}
              className="flex items-center gap-1 rounded px-2 py-0.5 text-[11px] font-medium text-[var(--cds-text-secondary,#525252)] hover:bg-[var(--cds-fill-ghost-hover)] hover:text-[var(--cds-text-primary)] transition-colors"
              title="复制为引用格式"
            >
              {didCopy ? (
                <span className="inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400 font-semibold">
                  <CheckIcon size={12} />
                  <span>已复制</span>
                </span>
              ) : (
                <span>复制思考</span>
              )}
            </button>
          )}

          {!isThinking && (
            <ChevronDownIcon
              size={13}
              className={`text-[var(--cds-text-muted)] transition-transform duration-200 ${
                isOpen ? "rotate-180" : ""
              }`}
            />
          )}
        </div>
      </div>

      {/* 内容区域：纯 CSS Grid 0fr -> 1fr 弹性展开容器 */}
      <div
        className="grid transition-[grid-template-rows] duration-200 ease-[cubic-bezier(.16,1,.3,1)]"
        style={{
          gridTemplateRows: isOpen ? "1fr" : "0fr",
        }}
      >
        <div className="min-h-0 overflow-hidden">
          <div className="px-3.5 pb-3 pt-2 text-[12px] leading-relaxed text-[var(--cds-text-secondary,#525252)] font-mono whitespace-pre-wrap break-words border-t border-[var(--cds-border)] bg-[var(--cds-surface-0)]/50">
            {thoughtText}
          </div>
        </div>
      </div>
    </div>
  );
}
