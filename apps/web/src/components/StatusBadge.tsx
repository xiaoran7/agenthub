import { ACTIVITY_LABELS, PRESENCE_LABELS } from "../lib/format";
import type { SessionState } from "../lib/types";

export function StatusBadge({ state }: { state: SessionState }) {
  return (
    <span className="badge" data-status={state.activity_status}>
      <span
        className="badge-dot"
        data-presence={state.presence}
        title={`Presence：${PRESENCE_LABELS[state.presence]}`}
      />
      {ACTIVITY_LABELS[state.activity_status]}
    </span>
  );
}
