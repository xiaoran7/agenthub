import { describe, expect, it } from "vitest";

import { activityTime, relativeTime, statusView } from "./presentation";
import type { Session } from "./types";

describe("desktop presentation", () => {
  it("keeps persisted sessions visibly distinct from unknown live sessions", () => {
    expect(statusView("UNKNOWN", "PERSISTED")).toEqual({
      label: "历史会话",
      className: "history",
    });
    expect(statusView("UNKNOWN", "LIVE")).toEqual({
      label: "未知",
      className: "history",
    });
  });

  it("uses the last activity timestamp when available", () => {
    const session = {
      observed_at: "2026-08-14T10:00:00.000Z",
      payload: { last_activity_at: "2026-08-14T09:59:00.000Z" },
    } as Session;

    expect(activityTime(session)).toBe("2026-08-14T09:59:00.000Z");
  });

  it("formats bounded relative times", () => {
    const now = Date.parse("2026-08-14T10:00:00.000Z");
    expect(relativeTime("2026-08-14T09:59:30.000Z", now)).toBe("刚刚");
    expect(relativeTime("2026-08-14T09:52:00.000Z", now)).toBe("8 分钟前");
    expect(relativeTime("2026-08-12T10:00:00.000Z", now)).toBe("2 天前");
  });
});
