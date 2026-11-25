import { useEffect, useRef, useState } from "react";
import type { RemotePendingApproval } from "@agent-pulse/contracts/remote";
import { copyTextToClipboard } from "../../lib/clipboard";
import { ChevronDownIcon } from "../remote-console-constants";
import { CheckIcon, CopyIcon } from "../RemoteMessageCard";
import { parseDiffStats } from "../RemoteToolCard";

export interface RemoteApprovalCardProps {
  approval: RemotePendingApproval;
  providerName: string;
  connected: boolean;
  busy: boolean;
  submittingApproval: { id: string; action: "approve" | "decline" } | null;
  onResolve: (approvalId: string, approved: boolean) => void;
}

export function RemoteApprovalCard({
  approval,
  providerName,
  connected,
  busy,
  submittingApproval,
  onResolve,
}: RemoteApprovalCardProps) {
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [diffCollapsed, setDiffCollapsed] = useState(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
    };
  }, []);

  const handleCopyText = (id: string, textToCopy: string) => {
    void copyTextToClipboard(textToCopy).then((ok) => {
      if (!ok || !mountedRef.current) return;
      setCopiedId(id);
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        if (mountedRef.current) {
          setCopiedId((current) => (current === id ? null : current));
        }
      }, 2000);
    });
  };

  const isSubmittingThis = submittingApproval?.id === approval.approval_id;
  const isApproving =
    isSubmittingThis && submittingApproval?.action === "approve";
  const isDeclining =
    isSubmittingThis && submittingApproval?.action === "decline";
  const isCopied = copiedId === approval.approval_id;
  const diffStats = approval.diff ? parseDiffStats(approval.diff) : null;
  const isDiffCopied = copiedId === `${approval.approval_id}-diff`;

  return (
    <section className="remote-approval mcp-tool-decision-card state-pending-approval">
      <div className="remote-approval-header">
        <div className="remote-approval-title-wrap">
          <span className="remote-approval-shield-icon" aria-hidden="true">
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
            </svg>
          </span>
          <h3>权限与工具执行审批</h3>
          <span className="remote-approval-tool-badge">
            {approval.tool_name ?? "系统工具"}
          </span>
        </div>
        <div className="remote-approval-meta-right">
          {approval.risk_level && (
            <span className={`remote-approval-badge ${approval.risk_level}`}>
              {approval.risk_level === "high"
                ? "高风险拦截"
                : approval.risk_level === "medium"
                  ? "中风险操作"
                  : "受控操作"}
            </span>
          )}
          <span className="remote-approval-awaiting-pill">
            <span className="awaiting-dot" />
            等待决策
          </span>
        </div>
      </div>

      <div className="remote-approval-desc">
        <strong>{providerName}</strong> 请求在当前工作区执行 "
        {approval.tool_name ?? "系统工具"}"
        {approval.description && `：${approval.description}`}
      </div>

      {approval.command_preview && (
        <div className="remote-approval-box-wrapper">
          <div className="remote-approval-box-bar">
            <span className="remote-approval-box-label">待执行命令参数</span>
            <button
              type="button"
              className="remote-card-mini-copy"
              onClick={() =>
                handleCopyText(approval.approval_id, approval.command_preview!)
              }
              title="复制完整命令"
            >
              {isCopied ? (
                <span className="remote-copied-badge">
                  <CheckIcon size={12} />
                  <span>已复制</span>
                </span>
              ) : (
                <span className="remote-copy-action">
                  <CopyIcon size={12} />
                  <span>复制命令</span>
                </span>
              )}
            </button>
          </div>
          <div className="remote-approval-command-box">
            {approval.command_preview}
          </div>
        </div>
      )}

      {approval.path && !approval.command_preview && (
        <div className="remote-approval-box-wrapper">
          <div className="remote-approval-box-bar">
            <span className="remote-approval-box-label">涉及目标文件路径</span>
            <button
              type="button"
              className="remote-card-mini-copy"
              onClick={() =>
                handleCopyText(approval.approval_id, approval.path!)
              }
              title="复制文件路径"
            >
              {isCopied ? (
                <span className="remote-copied-badge">
                  <CheckIcon size={12} />
                  <span>已复制</span>
                </span>
              ) : (
                <span className="remote-copy-action">
                  <CopyIcon size={12} />
                  <span>复制路径</span>
                </span>
              )}
            </button>
          </div>
          <div className="remote-approval-command-box">{approval.path}</div>
        </div>
      )}

      {approval.diff && diffStats && (
        <div className="remote-approval-diff-section">
          <div className="remote-approval-diff-header">
            <div className="remote-approval-diff-title-wrap">
              <span className="remote-approval-box-label">
                拟修改代码变更 (Diff)
              </span>
              {(diffStats.additions > 0 || diffStats.deletions > 0) && (
                <span className="remote-approval-diff-badge">
                  {diffStats.additions > 0 && (
                    <span className="remote-diff-stat-add">
                      +{diffStats.additions}
                    </span>
                  )}
                  {diffStats.deletions > 0 && (
                    <span className="remote-diff-stat-del">
                      -{diffStats.deletions}
                    </span>
                  )}
                </span>
              )}
            </div>
            <div className="remote-approval-diff-actions">
              <button
                type="button"
                className="remote-card-mini-copy"
                onClick={() =>
                  handleCopyText(`${approval.approval_id}-diff`, approval.diff!)
                }
                title="复制完整 Diff 内容"
              >
                {isDiffCopied ? (
                  <span className="remote-copied-badge">
                    <CheckIcon size={12} />
                    <span>已复制</span>
                  </span>
                ) : (
                  <span className="remote-copy-action">
                    <CopyIcon size={12} />
                    <span>复制变更</span>
                  </span>
                )}
              </button>
              <button
                type="button"
                className="remote-approval-diff-toggle-btn"
                onClick={() => setDiffCollapsed((prev) => !prev)}
                title={diffCollapsed ? "展开比对" : "折叠比对"}
              >
                <span>{diffCollapsed ? "展开比对" : "折叠比对"}</span>
                <ChevronDownIcon
                  size={12}
                  className={`remote-diff-chevron ${diffCollapsed ? "" : "open"}`}
                />
              </button>
            </div>
          </div>
          {!diffCollapsed && (
            <div className="remote-approval-diff-body">
              {diffStats.oldText || diffStats.newText ? (
                <div className="remote-diff-viewer">
                  {diffStats.oldText && (
                    <div className="remote-diff-chunk del">
                      <div className="remote-diff-bar">
                        <span className="remote-diff-tag">- 变更前代码</span>
                        <button
                          type="button"
                          className="remote-card-mini-copy"
                          onClick={() =>
                            handleCopyText(
                              `${approval.approval_id}-diff-del`,
                              diffStats.oldText!,
                            )
                          }
                        >
                          {copiedId === `${approval.approval_id}-diff-del` ? (
                            <span className="remote-copied-badge">
                              <CheckIcon size={12} />
                              <span>已复制</span>
                            </span>
                          ) : (
                            <span className="remote-copy-action">
                              <CopyIcon size={12} />
                              <span>复制</span>
                            </span>
                          )}
                        </button>
                      </div>
                      <pre className="remote-diff-pre">{diffStats.oldText}</pre>
                    </div>
                  )}
                  {diffStats.newText && (
                    <div className="remote-diff-chunk add">
                      <div className="remote-diff-bar">
                        <span className="remote-diff-tag">+ 变更后代码</span>
                        <button
                          type="button"
                          className="remote-card-mini-copy"
                          onClick={() =>
                            handleCopyText(
                              `${approval.approval_id}-diff-add`,
                              diffStats.newText!,
                            )
                          }
                        >
                          {copiedId === `${approval.approval_id}-diff-add` ? (
                            <span className="remote-copied-badge">
                              <CheckIcon size={12} />
                              <span>已复制</span>
                            </span>
                          ) : (
                            <span className="remote-copy-action">
                              <CopyIcon size={12} />
                              <span>复制</span>
                            </span>
                          )}
                        </button>
                      </div>
                      <pre className="remote-diff-pre">{diffStats.newText}</pre>
                    </div>
                  )}
                </div>
              ) : (
                <pre className="remote-diff-pre remote-approval-raw-diff">
                  {approval.diff}
                </pre>
              )}
            </div>
          )}
        </div>
      )}

      <div className="remote-approval-actions">
        <button
          type="button"
          className="remote-approval-btn-decline"
          disabled={
            !connected || busy || !approval.can_decline || isSubmittingThis
          }
          onClick={() => onResolve(approval.approval_id, false)}
        >
          {isDeclining ? "正在拒绝…" : "拒绝授权"}
        </button>
        <button
          type="button"
          className="remote-primary remote-approval-btn-approve"
          disabled={
            !connected || busy || !approval.can_approve || isSubmittingThis
          }
          onClick={() => onResolve(approval.approval_id, true)}
        >
          {isApproving ? "正在授权…" : "允许此次执行"}
        </button>
      </div>
    </section>
  );
}
