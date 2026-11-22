import { useState } from "react";
import type { RemotePendingApproval } from "@agent-pulse/contracts/remote";
import type { AttentionEntry } from "../lib/session-tree";

/**
 * The strip above the sidebar's device selector that lists sessions blocked on
 * a person, across every computer.
 *
 * The sidebar below it stays a drill-down — pick a computer, then an agent,
 * then a workspace. That shape is right for browsing and this component does
 * not change it. What it adds is the one thing a drill-down cannot show: a
 * session on a machine you are not looking at, sitting in `awaiting_permission`
 * since this morning. The strip reads from every device regardless of which
 * one is selected, because filtering it by the selector would put the waiting
 * session back behind the dropdown it is trying to escape.
 *
 * Collapsed it shows the newest three sessions. Expanded it shows every one.
 * It renders nothing at all when nobody is waiting — a standing banner that is
 * usually empty trains people to ignore it.
 *
 * Visual language follows the sidebar it sits in rather than inventing a
 * second one: same 13.5px title weight as `.remote-session-title`, same 10px
 * row radius, same hover lift and press rebound, same hairline borders. The
 * accent colour is confined to the count and the state dot; an accent-coloured
 * frame around the whole block read as an error banner.
 */

export interface AttentionQueueProps {
  entries: AttentionEntry[];
  /** Every pending approval, keyed `${runnerId}/${hubSessionId}`. */
  approvalsBySession: ReadonlyMap<string, RemotePendingApproval[]>;
  /** The approval this page is currently submitting, if any. */
  submitting: { id: string; action: "approve" | "decline" } | null;
  onOpen: (entry: AttentionEntry) => void;
  onResolve: (
    entry: AttentionEntry,
    approval: RemotePendingApproval,
    approved: boolean,
  ) => void;
}

/** Sessions shown before the strip is expanded. */
const COLLAPSED_ROW_LIMIT = 3;

const STATE_LABEL: Record<string, string> = {
  awaiting_permission: "等待授权",
  awaiting_input: "等待输入",
};

function stateLabel(state: string): string {
  return STATE_LABEL[state] ?? "等待处理";
}

/**
 * One line describing what the approval is for.
 *
 * A tool name plus its target reads best; the rest are fallbacks for the
 * providers that send only some of the fields. The target outranks the bare
 * tool name on purpose: "Bash" alone says nothing about what is about to run,
 * and the strip is too narrow to show the full card.
 */
function approvalSummary(approval: RemotePendingApproval): string {
  if (approval.tool_name && approval.command_preview) {
    return `${approval.tool_name} · ${approval.command_preview}`;
  }
  if (approval.tool_name && approval.path) {
    return `${approval.tool_name} · ${approval.path}`;
  }
  if (approval.command_preview) return approval.command_preview;
  if (approval.path) return approval.path;
  if (approval.tool_name) return approval.tool_name;
  return approval.description ?? "工具调用";
}

export function AttentionQueue({
  entries,
  approvalsBySession,
  submitting,
  onOpen,
  onResolve,
}: AttentionQueueProps) {
  const [expanded, setExpanded] = useState(false);

  if (entries.length === 0) return null;

  const visible = expanded ? entries : entries.slice(0, COLLAPSED_ROW_LIMIT);
  const hidden = entries.length - visible.length;

  return (
    <section className="remote-attention" aria-label="要你处理">
      <header className="remote-attention-header">
        <span className="remote-attention-title">要你处理</span>
        <span className="remote-attention-count" aria-hidden="true">
          {entries.length}
        </span>
        {entries.length > COLLAPSED_ROW_LIMIT && (
          <button
            type="button"
            className="remote-attention-toggle"
            onClick={() => setExpanded((prev) => !prev)}
          >
            {expanded ? "收起" : "全部"}
          </button>
        )}
      </header>

      <ul className="remote-attention-list">
        {visible.map((entry) => {
          const approvals = approvalsBySession.get(entry.sessionKey) ?? [];
          return (
            <li className="remote-attention-item" key={entry.sessionKey}>
              <button
                type="button"
                className="remote-attention-row"
                onClick={() => onOpen(entry)}
              >
                <span
                  className="remote-attention-dot"
                  data-state={entry.executionState}
                  aria-hidden="true"
                />
                <span className="remote-attention-body">
                  <span className="remote-attention-session">
                    {entry.title}
                  </span>
                  <span className="remote-attention-meta">
                    {entry.deviceLabel} · {entry.agentLabel}
                  </span>
                </span>
                <span
                  className="remote-attention-state"
                  data-state={entry.executionState}
                >
                  {stateLabel(entry.executionState)}
                </span>
              </button>

              {approvals.length > 0 && (
                <div className="remote-attention-approvals">
                  {approvals.map((approval) => {
                    const busy = submitting?.id === approval.approval_id;
                    return (
                      <div
                        className="remote-attention-approval"
                        key={approval.approval_id}
                      >
                        <code
                          className="remote-attention-approval-text"
                          title={approvalSummary(approval)}
                        >
                          {approvalSummary(approval)}
                        </code>
                        <span className="remote-attention-approval-actions">
                          <button
                            type="button"
                            className="remote-attention-approve"
                            disabled={!approval.can_approve || busy}
                            onClick={() => onResolve(entry, approval, true)}
                          >
                            {busy && submitting?.action === "approve"
                              ? "批准中"
                              : "批准"}
                          </button>
                          <button
                            type="button"
                            className="remote-attention-decline"
                            disabled={!approval.can_decline || busy}
                            onClick={() => onResolve(entry, approval, false)}
                          >
                            {busy && submitting?.action === "decline"
                              ? "拒绝中"
                              : "拒绝"}
                          </button>
                        </span>
                      </div>
                    );
                  })}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {!expanded && hidden > 0 && (
        <button
          type="button"
          className="remote-attention-more"
          onClick={() => setExpanded(true)}
        >
          还有 {hidden} 条
        </button>
      )}
    </section>
  );
}
