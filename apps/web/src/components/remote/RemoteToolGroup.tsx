import { memo, useState } from "react";
import type { ConversationItem } from "../../lib/remote-types";
import { RemoteToolCard } from "../RemoteToolCard";

/**
 * A one-line preview of what a tool group actually did.
 *
 * Only distinct command snippets are listed: repeating one label three times
 * (`exec · exec · exec`) or echoing an empty detail buries the single bit that
 * tells a reader what the turn is working on.
 */
function groupPreview(items: ConversationItem[]): string {
  const seen = new Set<string>();
  const parts: string[] = [];
  for (const item of items) {
    const detail = (item.description ?? item.command ?? "").trim();
    const line = detail.split("\n").find((part) => part.trim().length > 0);
    if (!line || line.includes("[local_path_redacted]")) continue;
    const snippet = line.trim().slice(0, 40);
    if (seen.has(snippet)) continue;
    seen.add(snippet);
    parts.push(`${item.title ?? "工具"} ${snippet}`);
    if (parts.length === 2) break;
  }
  return parts.join(" · ");
}

function sameConversationItemArray(
  prev: { items: ConversationItem[] },
  next: { items: ConversationItem[] },
): boolean {
  if (prev.items === next.items) return true;
  if (prev.items.length !== next.items.length) return false;
  for (let i = 0; i < prev.items.length; i++) {
    if (prev.items[i] !== next.items[i]) return false;
  }
  return true;
}

export const RemoteToolGroup = memo(function RemoteToolGroup({
  items,
}: {
  items: ConversationItem[];
}) {
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const status = items.some((item) => item.status === "failed")
    ? "failed"
    : items.some((item) => item.status === "running")
      ? "running"
      : "completed";
  const isRunning = status === "running";
  const open = userOpen ?? isRunning;
  return (
    <details
      className={`remote-tool-group state-${status}`}
      open={open}
      onToggle={(event) => {
        const nextOpen = event.currentTarget.open;
        setUserOpen((prev) =>
          prev === null && nextOpen === isRunning ? prev : nextOpen,
        );
      }}
    >
      <summary className="remote-tool-group-summary">
        <span className="remote-disclosure" aria-hidden="true">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="9 6 15 12 9 18" />
          </svg>
        </span>
        <span className="remote-tool-group-icon" aria-hidden="true">
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
          </svg>
        </span>
        <span className="remote-tool-group-title">
          执行了 {items.length} 项操作
        </span>
        <span className="remote-tool-group-preview">{groupPreview(items)}</span>
        <span className={`remote-tool-status ${status}`}>
          {status === "running" && (
            <span className="remote-tool-pulse-dots" aria-hidden="true">
              <span className="pulse-dot" />
              <span className="pulse-dot" />
              <span className="pulse-dot" />
            </span>
          )}
          <span className="status-label">
            {status === "running"
              ? "执行中…"
              : status === "failed"
                ? "异常"
                : "已完成"}
          </span>
        </span>
      </summary>
      <div className="remote-tool-group-body">
        {items.map((item) => (
          <RemoteToolCard key={item.id} item={item} />
        ))}
      </div>
    </details>
  );
}, sameConversationItemArray);
