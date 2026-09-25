"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { latestContextUsage, type RemoteEvent } from "../lib/remote";

export function RemoteContextCapacity({ events }: { events: RemoteEvent[] }) {
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const usage = useMemo(() => latestContextUsage(events), [events]);
  const percentage =
    usage && usage.window > 0
      ? Math.min(100, (usage.used / usage.window) * 100)
      : 0;
  const radius = 7.5;
  const circumference = 2 * Math.PI * radius;

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: MouseEvent) => {
      if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", closeOutside);
    document.addEventListener("keydown", closeEscape);
    return () => {
      document.removeEventListener("mousedown", closeOutside);
      document.removeEventListener("keydown", closeEscape);
    };
  }, [open]);

  return (
    <div className="remote-context" ref={container}>
      <button
        type="button"
        className="remote-context-button"
        aria-label="查看上下文容量"
        aria-expanded={open}
        title="上下文容量"
        onClick={() => setOpen((previous) => !previous)}
      >
        <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true">
          <circle
            cx="10"
            cy="10"
            r={radius}
            fill="none"
            stroke="currentColor"
            strokeOpacity="0.22"
            strokeWidth="2.2"
          />
          <circle
            cx="10"
            cy="10"
            r={radius}
            fill="none"
            stroke={
              percentage >= 90
                ? "#ffaaa4"
                : percentage >= 70
                  ? "#e7b35d"
                  : "#93c5fd"
            }
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - percentage / 100)}
            transform="rotate(-90 10 10)"
          />
        </svg>
      </button>
      {open && (
        <div
          className="remote-context-popover"
          role="dialog"
          aria-label="上下文容量"
        >
          <div className="remote-context-heading">
            <strong>上下文容量</strong>
            {usage?.window ? <span>{percentage.toFixed(1)}%</span> : null}
          </div>
          {usage ? (
            <>
              <div className="remote-context-count">
                {usage.used.toLocaleString("zh-CN")}
                <span>
                  {usage.window
                    ? ` / ${usage.window.toLocaleString("zh-CN")} tokens`
                    : " tokens（容量未上报）"}
                </span>
              </div>
              <div className="remote-context-track">
                <span style={{ width: `${percentage}%` }} />
              </div>
              <p>来自电脑端最近一次上报的上下文用量。</p>
            </>
          ) : (
            <p>电脑端尚未上报这段会话的上下文用量。</p>
          )}
        </div>
      )}
    </div>
  );
}
