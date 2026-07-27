import {
  AGENT_LABELS,
  WAITING_REASON_LABELS,
  deviceShortId,
  formatRelative,
} from "../lib/format";
import type { SessionRecord } from "../lib/types";
import { sessionKey, sessionRef } from "../lib/types";
import { StatusBadge } from "./StatusBadge";

interface SessionListProps {
  sessions: SessionRecord[];
  selectedKey: string | undefined;
  nowMs: number;
  onSelect: (session: SessionRecord) => void;
}

export function SessionList({
  sessions,
  selectedKey,
  nowMs,
  onSelect,
}: SessionListProps) {
  if (sessions.length === 0) {
    return <p className="empty">当前分类下没有会话</p>;
  }
  return (
    <ul className="session-list">
      {sessions.map((session) => {
        const key = sessionKey(sessionRef(session));
        const { state } = session.payload;
        return (
          <li key={key}>
            <button
              type="button"
              className="session-card"
              data-selected={key === selectedKey}
              data-status={state.activity_status}
              onClick={() => onSelect(session)}
            >
              <div className="session-card-top">
                <span className="session-project">
                  {session.payload.project.display_name}
                </span>
                <StatusBadge state={state} />
              </div>
              <div className="session-card-bottom">
                <span>{AGENT_LABELS[session.agent_type]}</span>
                {state.waiting_reason !== undefined && (
                  <span className="session-waiting">
                    {WAITING_REASON_LABELS[state.waiting_reason]}
                  </span>
                )}
                <span className="mono">{deviceShortId(session.device_id)}</span>
                <span className="session-time">
                  {formatRelative(session.observed_at, nowMs)}
                </span>
              </div>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
