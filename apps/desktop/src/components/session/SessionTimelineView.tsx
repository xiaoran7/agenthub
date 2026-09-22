import {
  Activity,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import type { SessionTimelineEvent } from "../../types";
import { relativeTime, statusView } from "../../presentation";

export function formatTimelineTimestamp(value: string): string {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "时间未知";
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(timestamp);
}

export function SessionTimelineView({
  events,
  loading,
  error,
  onReload,
}: {
  events: SessionTimelineEvent[];
  loading: boolean;
  error?: string;
  onReload: () => void;
}) {
  if (loading) {
    return (
      <section className="timeline-view timeline-state" aria-live="polite">
        <RefreshCw className="spin" />
        <strong>正在读取状态时间线</strong>
        <p>只读取这一个会话的状态观察，不读取对话正文。</p>
      </section>
    );
  }
  if (error !== undefined) {
    return (
      <section className="timeline-view timeline-state" aria-live="polite">
        <AlertTriangle />
        <strong>时间线暂时不可用</strong>
        <p>{error}</p>
      </section>
    );
  }
  if (events.length === 0) {
    return (
      <section className="timeline-view timeline-state">
        <Activity />
        <strong>还没有云端状态记录</strong>
        <p>本地会话可能尚未完成一次同步，当前状态仍可在运行上下文中查看。</p>
      </section>
    );
  }
  return (
    <section className="timeline-view" aria-label="会话状态时间线">
      <header className="timeline-heading">
        <div>
          <span className="card-kicker">SESSION HISTORY</span>
          <strong>{events.length} 条状态观察</strong>
        </div>
        <div className="timeline-heading-actions">
          <span>最新在上</span>
          <button
            className="icon-button"
            aria-label="刷新时间线"
            title="刷新时间线"
            disabled={loading}
            onClick={onReload}
          >
            <RefreshCw />
          </button>
        </div>
      </header>
      <ol className="timeline-list">
        {events.map((event) => {
          const status = statusView(
            event.payload.state.activity_status,
            event.payload.state.presence,
          );
          return (
            <li className="timeline-item" key={event.event_id}>
              <span className={"timeline-marker " + status.className}>
                <i />
              </span>
              <div className="timeline-event">
                <header>
                  <strong>{status.label}</strong>
                  <span>序列 #{event.sequence}</span>
                  <time title={event.observed_at}>
                    {formatTimelineTimestamp(event.observed_at)}
                  </time>
                </header>
                <p>
                  {event.payload.project.display_name || "未分类项目"}
                  <span> · </span>
                  {event.payload.state.presence === "LIVE"
                    ? "实时观察"
                    : "历史观察"}
                </p>
                <small>接收于 {relativeTime(event.received_at)}</small>
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
