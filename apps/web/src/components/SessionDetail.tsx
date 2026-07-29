import { useCallback, useEffect, useRef } from "react";

import { useModalDialog } from "../lib/dialog-focus";
import {
  AGENT_LABELS,
  CONFIDENCE_LABELS,
  PRESENCE_LABELS,
  SOURCE_LABELS,
  WAITING_REASON_LABELS,
  formatClock,
  formatRelative,
} from "../lib/format";
import { useMediaQuery } from "../lib/media-query";
import type { SessionRecord } from "../lib/types";
import { sessionKey, sessionRef } from "../lib/types";
import { StatusBadge } from "./StatusBadge";

interface SessionDetailProps {
  session: SessionRecord;
  timeline: SessionRecord[] | undefined;
  timelineError: string | undefined;
  nowMs: number;
  position: number;
  total: number;
  onPrevious: (() => void) | undefined;
  onNext: (() => void) | undefined;
  onClose: () => void;
}

export function SessionDetail({
  session,
  timeline,
  timelineError,
  nowMs,
  position,
  total,
  onPrevious,
  onNext,
  onClose,
}: SessionDetailProps) {
  const { state } = session.payload;
  const mobileModal = useMediaQuery("(max-width: 919px)");
  const detailSessionKey = sessionKey(sessionRef(session));
  const currentSessionKey = useRef(detailSessionKey);
  currentSessionKey.current = detailSessionKey;
  const restoreCurrentSessionFocus = useCallback(
    () =>
      Array.from(
        document.querySelectorAll<HTMLElement>("[data-session-key]"),
      ).find(
        (element) => element.dataset.sessionKey === currentSessionKey.current,
      ),
    [],
  );
  const detailRef = useModalDialog<HTMLElement>(
    onClose,
    mobileModal,
    restoreCurrentSessionFocus,
  );
  const pendingNavigationFocus = useRef<"previous" | "next" | undefined>(
    undefined,
  );

  useEffect(() => {
    if (mobileModal) {
      detailRef.current?.scrollTo({ top: 0 });
    }
    const preferred = pendingNavigationFocus.current;
    const detail = detailRef.current;
    if (preferred === undefined || detail === null) {
      return;
    }
    const fallback = preferred === "next" ? "previous" : "next";
    const target =
      detail.querySelector<HTMLButtonElement>(
        `[data-detail-${preferred}]:not(:disabled)`,
      ) ??
      detail.querySelector<HTMLButtonElement>(
        `[data-detail-${fallback}]:not(:disabled)`,
      ) ??
      detail.querySelector<HTMLButtonElement>(".detail-close");
    target?.focus();
    pendingNavigationFocus.current = undefined;
  }, [detailRef, detailSessionKey, mobileModal]);

  const navigateAndKeepFocus = (
    navigate: () => void,
    preferred: "previous" | "next",
  ) => {
    pendingNavigationFocus.current = preferred;
    navigate();
  };

  return (
    <aside
      ref={detailRef}
      className="detail"
      role={mobileModal ? "dialog" : undefined}
      aria-modal={mobileModal ? true : undefined}
      aria-labelledby="session-detail-title"
      tabIndex={mobileModal ? -1 : undefined}
      data-mobile-modal={mobileModal}
    >
      <header className="detail-header">
        <div className="detail-title">
          <p className="eyebrow">SESSION DETAIL</p>
          <h2 id="session-detail-title">
            {session.payload.project.display_name}
          </h2>
          <p className="detail-subtitle">
            {AGENT_LABELS[session.agent_type]} ·{" "}
            <span className="mono">{session.agent_session_id}</span>
          </p>
        </div>
        <button
          type="button"
          className="icon-button detail-close"
          onClick={onClose}
          aria-label="关闭详情"
        >
          <span className="detail-close-desktop-icon" aria-hidden="true">
            ✕
          </span>
          <span className="detail-close-mobile-icon" aria-hidden="true">
            ←
          </span>
          <span className="detail-close-label">返回</span>
        </button>
        {position > 0 && total > 0 && (
          <nav className="detail-session-nav" aria-label="筛选结果中的会话">
            <button
              type="button"
              className="secondary"
              data-detail-previous
              disabled={onPrevious === undefined}
              onClick={() => {
                if (onPrevious !== undefined) {
                  navigateAndKeepFocus(onPrevious, "previous");
                }
              }}
            >
              ← 上一条
            </button>
            <span role="status" aria-live="polite">
              {position} / {total}
            </span>
            <button
              type="button"
              className="secondary"
              data-detail-next
              disabled={onNext === undefined}
              onClick={() => {
                if (onNext !== undefined) {
                  navigateAndKeepFocus(onNext, "next");
                }
              }}
            >
              下一条 →
            </button>
          </nav>
        )}
      </header>

      <section className="detail-state">
        <StatusBadge state={state} />
        <dl className="detail-grid">
          <div>
            <dt>Presence</dt>
            <dd>{PRESENCE_LABELS[state.presence]}</dd>
          </div>
          {state.waiting_reason !== undefined && (
            <div>
              <dt>等待原因</dt>
              <dd>{WAITING_REASON_LABELS[state.waiting_reason]}</dd>
            </div>
          )}
          <div>
            <dt>原始状态</dt>
            <dd className="mono">{state.raw_status}</dd>
          </div>
          <div>
            <dt>来源</dt>
            <dd>
              {SOURCE_LABELS[session.source]}（
              {CONFIDENCE_LABELS[session.confidence]}）
            </dd>
          </div>
          {session.payload.started_at !== undefined && (
            <div>
              <dt>开始时间</dt>
              <dd>{formatClock(session.payload.started_at)}</dd>
            </div>
          )}
          {session.payload.last_activity_at !== undefined && (
            <div>
              <dt>最近活动</dt>
              <dd>{formatClock(session.payload.last_activity_at)}</dd>
            </div>
          )}
          <div>
            <dt>观察时间</dt>
            <dd>{formatClock(session.observed_at)}</dd>
          </div>
          <div>
            <dt>设备</dt>
            <dd className="mono">{session.device_id}</dd>
          </div>
        </dl>
        {state.raw_status_detail !== undefined && (
          <dl className="detail-grid detail-raw">
            {Object.entries(state.raw_status_detail).map(([key, value]) => (
              <div key={key}>
                <dt className="mono">{key}</dt>
                <dd className="mono">{value}</dd>
              </div>
            ))}
          </dl>
        )}
      </section>

      <section className="timeline">
        <h3>状态时间线</h3>
        {timelineError !== undefined && (
          <p className="error-text">{timelineError}</p>
        )}
        {timeline === undefined && timelineError === undefined && (
          <p className="empty">加载中…</p>
        )}
        {timeline !== undefined && timeline.length === 0 && (
          <p className="empty">还没有同步到状态事件</p>
        )}
        {timeline !== undefined && timeline.length > 0 && (
          <ol className="timeline-list">
            {timeline.map((event) => (
              <li
                key={event.event_id}
                className="timeline-item"
                data-status={event.payload.state.activity_status}
              >
                <div className="timeline-item-head">
                  <StatusBadge state={event.payload.state} />
                  <span
                    className="timeline-time"
                    title={formatClock(event.observed_at)}
                  >
                    {formatRelative(event.observed_at, nowMs)}
                  </span>
                </div>
                <p className="timeline-meta">
                  #{event.sequence} · {SOURCE_LABELS[event.source]} ·{" "}
                  {CONFIDENCE_LABELS[event.confidence]} ·{" "}
                  <span className="mono">{event.payload.state.raw_status}</span>
                </p>
              </li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
