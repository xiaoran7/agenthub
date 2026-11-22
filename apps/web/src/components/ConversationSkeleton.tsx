export function ConversationSkeleton({ label }: { label?: string }) {
  return (
    <div
      className="remote-conversation-skeleton"
      role="status"
      aria-live="polite"
      aria-label={label ?? "正在加载会话内容"}
    >
      <div className="remote-conv-skel-user">
        <div className="remote-conv-skel-bubble" />
      </div>
      <div className="remote-conv-skel-assistant">
        <div className="remote-conv-skel-tag">
          <span className="remote-conv-skel-dot" />
          <span className="remote-conv-skel-bar w-20" />
        </div>
        <div className="remote-conv-skel-lines">
          <div className="remote-conv-skel-bar w-88" />
          <div className="remote-conv-skel-bar w-72" />
          <div className="remote-conv-skel-bar w-54" />
        </div>
      </div>
      <div className="remote-conv-skel-caption">
        <span className="remote-notice-spinner" aria-hidden="true" />
        <span>{label ?? "正在同步会话上下文…"}</span>
      </div>
    </div>
  );
}
