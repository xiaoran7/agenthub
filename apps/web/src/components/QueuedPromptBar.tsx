import type { RemoteAttachment } from "../lib/remote";

export interface QueuedPrompt {
  text: string;
  attachments: RemoteAttachment[];
  queuedAt: number;
}

interface QueuedPromptBarProps {
  queued: QueuedPrompt;
  canSendNow: boolean;
  onEdit: () => void;
  onCancel: () => void;
  onSendNow: () => void;
}

export function QueuedPromptBar({
  queued,
  canSendNow,
  onEdit,
  onCancel,
  onSendNow,
}: QueuedPromptBarProps) {
  const preview =
    queued.text.trim().replace(/\s+/gu, " ") ||
    (queued.attachments.length > 0
      ? `${queued.attachments.length} 个附件`
      : "");

  return (
    <div
      className="remote-queued-prompt-bar"
      role="status"
      aria-live="polite"
      aria-label="待发送消息"
    >
      <div className="remote-queued-prompt-info">
        <span className="remote-queued-prompt-badge">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="12" cy="12" r="9" />
            <polyline points="12 7 12 12 15.5 14" />
          </svg>
          <span>{canSendNow ? "已暂存" : "待发"}</span>
        </span>
        <span className="remote-queued-prompt-text" title={queued.text}>
          {preview}
        </span>
        {queued.attachments.length > 0 && queued.text.trim().length > 0 && (
          <span className="remote-queued-prompt-att">
            +{queued.attachments.length} 附件
          </span>
        )}
      </div>
      <div className="remote-queued-prompt-actions">
        {canSendNow && (
          <button
            type="button"
            className="remote-queued-prompt-btn primary"
            onClick={onSendNow}
          >
            立即发送
          </button>
        )}
        <button
          type="button"
          className="remote-queued-prompt-btn"
          onClick={onEdit}
        >
          编辑
        </button>
        <button
          type="button"
          className="remote-queued-prompt-btn cancel"
          aria-label="取消排队消息"
          title="取消排队消息"
          onClick={onCancel}
        >
          取消
        </button>
      </div>
    </div>
  );
}
