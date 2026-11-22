import type { AttentionEntry } from "../lib/session-tree";
import { CheckIcon, CopyIcon } from "./RemoteMessageCard";

interface MobileHeaderActionsProps {
  isMobile: boolean;
  selectedSessionKey: string;
  copiedSessionId: string | null;
  onCopySessionId: (id: string, e: React.MouseEvent) => void;
  onNewSession: () => void;
}

export function MobileHeaderActions({
  isMobile,
  selectedSessionKey,
  copiedSessionId,
  onCopySessionId,
  onNewSession,
}: MobileHeaderActionsProps) {
  const isCopied =
    Boolean(selectedSessionKey) && copiedSessionId === selectedSessionKey;

  return (
    <div className="remote-main-header-actions">
      {selectedSessionKey && (
        <button
          type="button"
          className={`remote-header-copy-id ${isCopied ? "copied" : ""}`}
          title={isCopied ? "已复制会话ID" : "复制当前会话ID"}
          aria-label={isCopied ? "已复制会话ID" : "复制当前会话ID"}
          onClick={(e) => onCopySessionId(selectedSessionKey, e)}
        >
          {isCopied ? <CheckIcon size={12} /> : <CopyIcon size={12} />}
          <span>{isCopied ? "已复制" : "会话ID"}</span>
        </button>
      )}
      {isMobile && (
        <button
          type="button"
          className="remote-icon-btn mobile-new-btn"
          title="新建会话"
          aria-label="新建会话"
          onClick={onNewSession}
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
      )}
    </div>
  );
}

interface MobileAttentionBannerProps {
  entries: AttentionEntry[];
  selectedSessionKey: string;
  onOpenSingle: (entry: AttentionEntry) => void;
  onOpenDrawer: () => void;
}

export function MobileAttentionBanner({
  entries,
  selectedSessionKey,
  onOpenSingle,
  onOpenDrawer,
}: MobileAttentionBannerProps) {
  const others = entries.filter(
    (entry) => entry.sessionKey !== selectedSessionKey,
  );
  if (others.length === 0) return null;

  const first = others[0]!;
  const single = others.length === 1;

  return (
    <div
      className="remote-mobile-attention-strip"
      role="region"
      aria-label="跨设备待处理提醒"
    >
      <span className="remote-mobile-attention-dot" aria-hidden="true" />
      <span className="remote-mobile-attention-text">
        {single
          ? `${first.deviceLabel} · ${first.title} 等待你处理`
          : `有 ${others.length} 个会话正在等待授权或输入`}
      </span>
      <button
        type="button"
        className="remote-mobile-attention-btn"
        onClick={() => (single ? onOpenSingle(first) : onOpenDrawer())}
      >
        {single ? "前往处理" : "查看列表"}
      </button>
    </div>
  );
}
