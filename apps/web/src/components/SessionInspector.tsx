import { useCallback, useEffect, useRef, useState } from "react";

import {
  AGENT_LABELS,
  formatClockCompact,
  formatDateTimeCompact,
  formatRelativeCompact,
  sessionStatus,
  sessionTask,
  sessionTool,
} from "../lib/platform-view";
import { useMediaQuery } from "../lib/media-query";
import type { SessionRecord } from "../lib/types";
import { sessionKey, sessionRef } from "../lib/types";
import { useModalDialog } from "../lib/dialog-focus";
import { Icon } from "./Icon";
import { InspectorPreviewFields } from "./PlatformViews";

interface SessionInspectorProps {
  session: SessionRecord;
  devices: import("../lib/types").DeviceRecord[];
  timeline: SessionRecord[] | undefined;
  timelineError: string | undefined;
  nowMs: number;
  position: number;
  total: number;
  expanded: boolean;
  onPrevious?: (() => void) | undefined;
  onNext?: (() => void) | undefined;
  onClose: () => void;
  onOpenFull: () => void;
  onExitFull: () => void;
}

type InspectorTab = "overview" | "timeline" | "metadata" | "raw";

function StatusLine({ session }: { session: SessionRecord }) {
  const status = sessionStatus(session);
  return (
    <span className="status-mark" data-tone={status.tone}>
      <span className="status-dot" />
      {status.label}
    </span>
  );
}

function TimelineContent({
  timeline,
  timelineError,
  nowMs,
}: {
  timeline: SessionRecord[] | undefined;
  timelineError: string | undefined;
  nowMs: number;
}) {
  if (timelineError !== undefined) {
    return <p className="inspector-empty error-text">{timelineError}</p>;
  }
  if (timeline === undefined) {
    return <p className="inspector-empty">正在加载时间线…</p>;
  }
  if (timeline.length === 0) {
    return <p className="inspector-empty">暂无状态事件。</p>;
  }
  return (
    <ol className="inspector-timeline">
      {timeline.map((event, index) => {
        const status = sessionStatus(event);
        return (
          <li
            data-tone={status.tone}
            key={`${event.event_id}-${event.sequence}-${index}`}
          >
            <span className="timeline-point">
              <span className="status-dot" />
            </span>
            <span className="timeline-copy">
              <strong>{status.label}</strong>
              <span>{sessionTask(event)}</span>
              <small>
                <span className="mono">
                  {formatClockCompact(event.observed_at)}
                </span>{" "}
                · {formatRelativeCompact(event.observed_at, nowMs)}
              </small>
            </span>
          </li>
        );
      })}
    </ol>
  );
}

export function SessionInspector({
  session,
  devices,
  timeline,
  timelineError,
  nowMs,
  position,
  total,
  expanded,
  onPrevious,
  onNext,
  onClose,
  onOpenFull,
  onExitFull,
}: SessionInspectorProps) {
  const [tab, setTab] = useState<InspectorTab>("overview");
  const mobileModal = useMediaQuery("(max-width: 919px)");
  const key = sessionKey(sessionRef(session));
  const currentKey = useRef(key);
  currentKey.current = key;
  const restoreFocus = useCallback(
    () =>
      Array.from(
        document.querySelectorAll<HTMLElement>("[data-session-key]"),
      ).find((element) => element.dataset.sessionKey === currentKey.current),
    [],
  );
  const inspectorRef = useModalDialog<HTMLElement>(
    onClose,
    mobileModal,
    restoreFocus,
  );

  useEffect(() => setTab("overview"), [key]);

  const stateDetails = session.payload.state.raw_status_detail;
  const status = sessionStatus(session);
  const tool = sessionTool(session);
  const errorSummary =
    stateDetails?.error ??
    (status.tone === "error" ? "Agent 报告了错误" : "暂无错误");

  return (
    <aside
      aria-labelledby="inspector-title"
      aria-modal={mobileModal ? true : undefined}
      className={`inspector ${expanded ? "inspector-expanded" : ""}`.trim()}
      data-mobile-modal={mobileModal}
      ref={inspectorRef}
      role={mobileModal ? "dialog" : undefined}
      tabIndex={mobileModal ? -1 : undefined}
    >
      <header className="inspector-header">
        <div>
          <p className="inspector-kicker">会话检查器</p>
          <h2 id="inspector-title">{session.payload.project.display_name}</h2>
          <p className="inspector-id">
            <StatusLine session={session} />
            <span className="mono">{session.agent_session_id}</span>
          </p>
        </div>
        <button
          aria-label={expanded ? "关闭完整会话" : "关闭检查器"}
          className="icon-button"
          onClick={expanded ? onExitFull : onClose}
          type="button"
        >
          <Icon name="close" size={19} />
        </button>
      </header>

      <nav className="inspector-tabs" aria-label="会话详情视图">
        {(
          [
            ["overview", "总览"],
            ["timeline", "时间线"],
            ["metadata", "元数据"],
            ["raw", "原始事件"],
          ] as const
        ).map(([entry, label]) => (
          <button
            data-selected={tab === entry}
            key={entry}
            onClick={() => setTab(entry)}
            type="button"
          >
            {label}
          </button>
        ))}
      </nav>

      <div className="inspector-scroll">
        {tab === "overview" && (
          <>
            <InspectorPreviewFields
              devices={devices}
              nowMs={nowMs}
              session={session}
            />
            <section className="inspector-subsection">
              <div className="inspector-subsection-header">
                <h3>当前工具调用</h3>
                <span className="mono">
                  {tool === undefined ? "—" : "活动中"}
                </span>
              </div>
              <p className="inspector-code">{tool ?? "未报告工具调用"}</p>
              <div className="inspector-divider" />
              <div className="inspector-subsection-header">
                <h3>错误摘要</h3>
                <span
                  className={
                    status.tone === "error" ? "tone-error" : "tone-muted"
                  }
                >
                  {status.tone === "error" ? "已报告" : "正常"}
                </span>
              </div>
              <p className="inspector-muted">{errorSummary}</p>
            </section>
            <section className="inspector-subsection inspector-telemetry">
              <div className="inspector-subsection-header">
                <h3>Token 用量</h3>
                <span className="mono">
                  {stateDetails?.token_usage ?? "未上报"}
                </span>
              </div>
              <div className="telemetry-bar">
                <span
                  style={{
                    width:
                      stateDetails?.token_usage === undefined ? "0%" : "18%",
                  }}
                />
              </div>
            </section>
          </>
        )}
        {tab === "timeline" && (
          <TimelineContent
            nowMs={nowMs}
            timeline={timeline}
            timelineError={timelineError}
          />
        )}
        {tab === "metadata" && (
          <dl className="metadata-list">
            <div>
              <dt>来源</dt>
              <dd>{AGENT_LABELS[session.agent_type]}</dd>
            </div>
            <div>
              <dt>观测时间</dt>
              <dd>{formatDateTimeCompact(session.observed_at)}</dd>
            </div>
            <div>
              <dt>数据来源</dt>
              <dd>{session.source}</dd>
            </div>
            <div>
              <dt>可信度</dt>
              <dd>{session.confidence}</dd>
            </div>
            <div>
              <dt>适配器</dt>
              <dd className="mono">{session.adapter_version}</dd>
            </div>
            <div>
              <dt>项目指纹</dt>
              <dd className="mono">{session.payload.project.fingerprint}</dd>
            </div>
            <div>
              <dt>设备 ID</dt>
              <dd className="mono">{session.device_id}</dd>
            </div>
          </dl>
        )}
        {tab === "raw" && (
          <pre className="raw-event">{JSON.stringify(session, null, 2)}</pre>
        )}
      </div>

      <footer className="inspector-footer">
        <div className="inspector-navigation">
          <button
            aria-label="上一个会话"
            className="icon-button"
            disabled={onPrevious === undefined}
            onClick={onPrevious}
            type="button"
          >
            <Icon name="arrow-left" size={17} />
          </button>
          <span>
            {position} / {total}
          </span>
          <button
            aria-label="下一个会话"
            className="icon-button"
            disabled={onNext === undefined}
            onClick={onNext}
            type="button"
          >
            <Icon name="arrow-right" size={17} />
          </button>
        </div>
        {!expanded && (
          <button
            className="primary inspector-open-full"
            onClick={onOpenFull}
            type="button"
          >
            打开完整会话 <Icon name="external" size={15} />
          </button>
        )}
        {expanded && (
          <button
            className="secondary inspector-open-full"
            onClick={onExitFull}
            type="button"
          >
            <Icon name="arrow-left" size={15} /> 返回会话列表
          </button>
        )}
      </footer>
    </aside>
  );
}
