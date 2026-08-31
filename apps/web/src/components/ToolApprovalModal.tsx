import { useState } from "react";
import type { PanelConfig } from "../lib/config";
import { useModalDialog } from "../lib/dialog-focus";
import { resolveV2Approval, describeApiError } from "../lib/api";
import { Icon } from "./Icon";

export interface PendingToolApproval {
  approval_id: string;
  hub_session_id: string;
  tool_name: string;
  tool_params: Record<string, unknown>;
  diff_preview?: string;
  danger_level?: "low" | "medium" | "high";
}

interface ToolApprovalModalProps {
  config: PanelConfig;
  approval: PendingToolApproval;
  onResolved: (decision: "approved" | "rejected") => void;
  onClose: () => void;
}

export function ToolApprovalModal({
  config,
  approval,
  onResolved,
  onClose,
}: ToolApprovalModalProps) {
  const dialogRef = useModalDialog<HTMLDivElement>(onClose);
  const [feedback, setFeedback] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>();

  async function handleDecision(decision: "approved" | "rejected") {
    setBusy(true);
    setError(undefined);

    try {
      await resolveV2Approval(
        config,
        approval.hub_session_id,
        approval.approval_id,
        decision,
        feedback.trim() || undefined,
      );
      onResolved(decision);
      onClose();
    } catch (err: unknown) {
      setError(describeApiError(err));
      setBusy(false);
    }
  }

  const dangerColor =
    approval.danger_level === "high"
      ? "var(--red)"
      : approval.danger_level === "medium"
        ? "var(--gold)"
        : "var(--blue)";

  return (
    <div className="dialog-scrim" role="presentation">
      <div
        ref={dialogRef}
        className="dialog-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="approval-modal-title"
        style={{ maxWidth: "600px", width: "95%" }}
      >
        <header className="dialog-header">
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <span
              style={{
                display: "inline-block",
                width: "12px",
                height: "12px",
                borderRadius: "50%",
                backgroundColor: dangerColor,
              }}
            />
            <div>
              <h2 id="approval-modal-title">
                工具执行权限审批 (Tool Approval)
              </h2>
              <p className="dialog-subhead">
                Agent 正在请求执行高敏感操作，请核对参数后审批
              </p>
            </div>
          </div>
          <button
            type="button"
            className="dialog-close"
            aria-label="关闭"
            onClick={onClose}
          >
            <Icon name="close" size={16} />
          </button>
        </header>

        <div
          className="dialog-body"
          style={{ display: "flex", flexDirection: "column", gap: "14px" }}
        >
          {error && (
            <div
              className="dialog-alert"
              role="alert"
              style={{
                color: "var(--red)",
                background: "var(--red-subtle)",
                padding: "8px 12px",
                borderRadius: "6px",
              }}
            >
              {error}
            </div>
          )}

          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              background: "var(--paper-subtle)",
              padding: "10px 14px",
              borderRadius: "8px",
              border: "1px solid var(--line)",
            }}
          >
            <div>
              <span
                style={{
                  fontSize: "12px",
                  color: "var(--text-muted)",
                  display: "block",
                }}
              >
                目标工具
              </span>
              <strong style={{ fontSize: "16px", fontFamily: "monospace" }}>
                {approval.tool_name}
              </strong>
            </div>
            <span
              style={{
                fontSize: "11px",
                fontWeight: 700,
                textTransform: "uppercase",
                padding: "4px 8px",
                borderRadius: "4px",
                backgroundColor: "var(--paper)",
                border: `1px solid ${dangerColor}`,
                color: dangerColor,
              }}
            >
              {approval.danger_level ?? "standard"}
            </span>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
            <span
              style={{
                fontSize: "12px",
                fontWeight: 600,
                color: "var(--text-muted)",
              }}
            >
              调用参数 (Parameters)
            </span>
            <pre
              style={{
                margin: 0,
                padding: "12px",
                borderRadius: "6px",
                background: "var(--code-bg, #1a1a1a)",
                color: "var(--code-text, #f0f0f0)",
                fontSize: "13px",
                fontFamily: "monospace",
                overflowX: "auto",
                maxHeight: "220px",
              }}
            >
              {JSON.stringify(approval.tool_params, null, 2)}
            </pre>
          </div>

          {approval.diff_preview && (
            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <span
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--text-muted)",
                }}
              >
                文件变更预览 (Diff Preview)
              </span>
              <pre
                style={{
                  margin: 0,
                  padding: "12px",
                  borderRadius: "6px",
                  background: "var(--code-bg, #1a1a1a)",
                  color: "#a6e22e",
                  fontSize: "12px",
                  fontFamily: "monospace",
                  overflowX: "auto",
                  maxHeight: "180px",
                }}
              >
                {approval.diff_preview}
              </pre>
            </div>
          )}

          <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
            <label
              style={{
                fontSize: "12px",
                fontWeight: 600,
                color: "var(--text-muted)",
              }}
            >
              自定义反馈 (可选 - 拒绝时将传递给 Agent)
            </label>
            <input
              type="text"
              value={feedback}
              onChange={(e) => setFeedback(e.target.value)}
              placeholder="e.g. 请勿删除此目录，改用临时文件测试"
              style={{
                padding: "8px 12px",
                borderRadius: "6px",
                border: "1px solid var(--line)",
                background: "var(--paper)",
              }}
            />
          </div>

          <footer
            className="dialog-footer"
            style={{
              display: "flex",
              justifyContent: "flex-end",
              gap: "10px",
              marginTop: "10px",
            }}
          >
            <button
              type="button"
              className="btn btn-secondary"
              style={{ color: "var(--red)", borderColor: "var(--red)" }}
              onClick={() => handleDecision("rejected")}
              disabled={busy}
            >
              <Icon name="close" size={14} />
              <span>拒绝执行 (Reject)</span>
            </button>
            <button
              type="button"
              className="btn btn-primary"
              style={{
                background: "var(--green, #10b981)",
                borderColor: "var(--green, #10b981)",
              }}
              onClick={() => handleDecision("approved")}
              disabled={busy}
            >
              <Icon name="check" size={14} />
              <span>批准放行 (Approve)</span>
            </button>
          </footer>
        </div>
      </div>
    </div>
  );
}
