import type { SessionRecord } from "./types";

/**
 * Tabs are filters over the same session list, mirroring the read views in
 * ARCHITECTURE §4.4. They intentionally overlap: a PERSISTED+FAILED session
 * appears under both 最近完成 and 历史.
 */
export const SESSION_TABS = [
  "all",
  "live",
  "waiting",
  "finished",
  "historical",
] as const;

export type SessionTab = (typeof SESSION_TABS)[number];

export function matchesTab(session: SessionRecord, tab: SessionTab): boolean {
  const { presence, activity_status: activity } = session.payload.state;
  switch (tab) {
    case "all":
      return true;
    case "live":
      return presence === "LIVE";
    case "waiting":
      return activity === "WAITING_INPUT" || activity === "WAITING_PERMISSION";
    case "finished":
      return (
        activity === "COMPLETED" ||
        activity === "FAILED" ||
        activity === "STOPPED"
      );
    case "historical":
      return presence === "PERSISTED";
  }
}

export function filterSessions(
  sessions: SessionRecord[],
  tab: SessionTab,
  deviceId: string | undefined,
): SessionRecord[] {
  return sessions.filter(
    (session) =>
      matchesTab(session, tab) &&
      (deviceId === undefined || session.device_id === deviceId),
  );
}

export function tabCounts(
  sessions: SessionRecord[],
  deviceId: string | undefined,
): Record<SessionTab, number> {
  const counts: Record<SessionTab, number> = {
    all: 0,
    live: 0,
    waiting: 0,
    finished: 0,
    historical: 0,
  };
  for (const session of sessions) {
    if (deviceId !== undefined && session.device_id !== deviceId) {
      continue;
    }
    for (const tab of SESSION_TABS) {
      if (matchesTab(session, tab)) {
        counts[tab] += 1;
      }
    }
  }
  return counts;
}

/** Waiting sessions first, then most recent observation first. */
export function sortSessions(sessions: SessionRecord[]): SessionRecord[] {
  return [...sessions].sort((left, right) => {
    const leftWaiting = matchesTab(left, "waiting") ? 0 : 1;
    const rightWaiting = matchesTab(right, "waiting") ? 0 : 1;
    if (leftWaiting !== rightWaiting) {
      return leftWaiting - rightWaiting;
    }
    const time = Date.parse(right.observed_at) - Date.parse(left.observed_at);
    return time === 0 ? right.sequence - left.sequence : time;
  });
}
