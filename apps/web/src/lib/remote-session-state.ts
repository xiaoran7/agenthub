import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import { trimSupersededEvents, type RemoteEvent } from "./remote";

export const MAX_ATTACHMENT_BYTES = Math.floor(7.5 * 1024 * 1024);
export const MAX_CACHED_SESSIONS = 24;

export type AttachmentSizeCheck = "ok" | "empty" | "too_large";

/** Validate per-file attachment size: reject 0-byte empties and >7.5 MiB uploads. */
export function checkAttachmentSize(byteLength: number): AttachmentSizeCheck {
  if (!Number.isFinite(byteLength) || byteLength <= 0) return "empty";
  if (byteLength > MAX_ATTACHMENT_BYTES) return "too_large";
  return "ok";
}

/** Update a session-keyed Record while bounding retained entries by LRU order. */
export function setBoundedSessionRecord<T>(
  prev: Record<string, T>,
  lruOrder: string[],
  sessionKey: string,
  value: T,
  limit = MAX_CACHED_SESSIONS,
): Record<string, T> {
  const idx = lruOrder.indexOf(sessionKey);
  if (idx >= 0) lruOrder.splice(idx, 1);
  lruOrder.push(sessionKey);
  const next: Record<string, T> = { ...prev, [sessionKey]: value };
  while (lruOrder.length > limit) {
    const evicted = lruOrder.shift();
    if (evicted !== undefined && evicted !== sessionKey) {
      delete next[evicted];
    }
  }
  return next;
}

/**
 * Append only strictly newer event cursors (`ev.cursor > cursor`), advance the
 * cursor monotonically, and compact superseded history frames inside memory.
 */
export function mergeMonotonicEvents(
  accumulated: RemoteEvent[],
  batch: RemoteEvent[],
  cursor: number,
): { nextEvents: RemoteEvent[]; nextCursor: number; changed: boolean } {
  const fresh = batch.filter((ev) => ev.cursor > cursor);
  if (fresh.length === 0) {
    return { nextEvents: accumulated, nextCursor: cursor, changed: false };
  }
  let maxCursor = cursor;
  for (const ev of fresh) {
    if (ev.cursor > maxCursor) maxCursor = ev.cursor;
  }
  const merged = trimSupersededEvents([...accumulated, ...fresh]);
  return { nextEvents: merged, nextCursor: maxCursor, changed: true };
}

/**
 * Derive an idempotency Map key for a command without serializing raw base64
 * attachment bytes onto the Map key heap.
 */
export function buildCommandAttemptKey(
  runnerId: string,
  hubSessionId: string,
  action: RemoteCommand["command"],
  generation: string,
): string {
  if ("attachments" in action && Array.isArray(action.attachments)) {
    const compact = {
      ...action,
      attachments: action.attachments.map((att) => ({
        name: att.name,
        media_type: att.media_type,
        encoding: att.encoding,
        length: att.data.length,
        head: att.data.slice(0, 24),
        tail: att.data.slice(-16),
      })),
    };
    return JSON.stringify([runnerId, hubSessionId, compact, generation]);
  }
  return JSON.stringify([runnerId, hubSessionId, action, generation]);
}

/**
 * Exact option selection matcher preventing substring false-positives (e.g.
 * `"Yes"` matching `"Yesterday"`). Supports structured JSON answer maps as well
 * as newline/comma-separated plain output strings.
 */
export function matchesSelectedOption(
  output: string | undefined,
  label: string,
): boolean {
  if (!output) return false;
  const target = label.trim();
  if (!target) return false;
  const trimmed = output.trim();
  if (trimmed === target) return true;
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const parsed = JSON.parse(trimmed) as {
        answers?: Record<string, unknown>;
      };
      const values = Object.values(parsed.answers ?? parsed);
      for (const value of values) {
        if (typeof value === "string" && value.trim() === target) return true;
        if (
          Array.isArray(value) &&
          value.some(
            (entry) => typeof entry === "string" && entry.trim() === target,
          )
        ) {
          return true;
        }
      }
    } catch {
      // Fall back to line/token parsing below.
    }
  }
  const tokens = trimmed
    .split(/\r?\n|,|、|;/)
    .map((part) => part.trim())
    .filter(Boolean);
  if (tokens.includes(target)) return true;
  return tokens.some((line) => {
    const colon = line.indexOf(":");
    if (colon < 0) return false;
    return line.slice(colon + 1).trim() === target;
  });
}
