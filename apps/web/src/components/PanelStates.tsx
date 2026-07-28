interface PanelSkeletonProps {
  rows?: number;
}

export function PanelSkeleton({ rows = 4 }: PanelSkeletonProps) {
  return (
    <div className="panel-skeleton" aria-label="正在加载会话" role="status">
      {Array.from({ length: rows }, (_, index) => (
        <div className="skeleton-card" aria-hidden="true" key={index}>
          <span className="skeleton-mark" />
          <span className="skeleton-copy">
            <span />
            <span />
          </span>
          <span className="skeleton-badge" />
        </div>
      ))}
      <span className="sr-only">正在加载设备与会话数据…</span>
    </div>
  );
}

interface EmptySessionsProps {
  filtered: boolean;
  onReset: () => void;
}

export function EmptySessions({ filtered, onReset }: EmptySessionsProps) {
  return (
    <div className="empty-state">
      <span className="empty-state-icon" aria-hidden="true">
        {filtered ? "⌕" : "○"}
      </span>
      <h3>{filtered ? "当前筛选下没有会话" : "还没有可显示的会话"}</h3>
      <p>
        {filtered
          ? "可以清除设备与分类条件，回到完整会话列表。"
          : "Runner 上报首个 Session 后，会自动出现在这里。"}
      </p>
      {filtered && (
        <button type="button" className="secondary" onClick={onReset}>
          查看全部会话
        </button>
      )}
    </div>
  );
}
