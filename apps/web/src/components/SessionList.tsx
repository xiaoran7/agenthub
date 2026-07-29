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
              data-session-key={key}
              data-selected={key === selectedKey}
              data-status={state.activity_status}
              onClick={() => onSelect(session)}
            >
              <div className="session-card-top">
                <span
                  className="session-agent-mark"
                  data-agent={session.agent_type}
                  aria-hidden="true"
                >
                  {session.agent_type === "claude_code" ? "C" : "X"}
                </span>
                <span className="session-copy">
                  <span className="session-project">
                    {session.payload.project.display_name}
                  </span>
                  <span className="session-agent">
                    {AGENT_LABELS[session.agent_type]}
                  </span>
                </span>
                <StatusBadge state={state} />
              </div>
              <div className="session-card-bottom">
                {state.waiting_reason !== undefined && (
                  <span className="session-waiting">
                    <span aria-hidden="true">●</span>
                    {WAITING_REASON_LABELS[state.waiting_reason]}
                  </span>
                )}
                <span className="session-device">
                  设备{" "}
                  <span className="mono">
                    {deviceShortId(session.device_id)}
                  </span>
                </span>
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
