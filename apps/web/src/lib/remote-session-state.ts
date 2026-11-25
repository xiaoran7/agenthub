import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import { trimSupersededEvents, type RemoteEvent } from "./remote";

export const MAX_ATTACHMENT_BYTES = Math.floor(7.5 * 1024 * 1024);
export const MAX_CACHED_SESSIONS = 24;

export type AttachmentSizeCheck = "ok" | "empty" | "too_large";

export interface BoundedSessionState<T> {
  readonly byKey: Record<string, T>;
  readonly order: readonly string[];
}

export interface BoundedSessionUpdateResult<T> {
  next: Record<string, T>;
  nextOrder: string[];
  evictedKeys: string[];
}

/** Validate per-file attachment size: reject 0-byte empties and >7.5 MiB uploads. */
export function checkAttachmentSize(byteLength: number): AttachmentSizeCheck {
  if (!Number.isFinite(byteLength) || byteLength <= 0) return "empty";
  if (byteLength > MAX_ATTACHMENT_BYTES) return "too_large";
  return "ok";
}

/**
 * Pure LRU update for a session-keyed Record. Never mutates `prev` or
 * `lruOrder`, making it safe under React StrictMode double-invoked state
 * updaters.
 */
export function setBoundedSessionRecord<T>(
  prev: Readonly<Record<string, T>>,
  lruOrder: readonly string[],
  sessionKey: string,
  value: T,
  limit = MAX_CACHED_SESSIONS,
): BoundedSessionUpdateResult<T> {
  const effectiveLimit = Math.max(1, limit);
  const nextOrder = lruOrder.filter((key) => key !== sessionKey);
  nextOrder.push(sessionKey);
  const next: Record<string, T> = { ...prev, [sessionKey]: value };
  const evictedKeys: string[] = [];
  while (nextOrder.length > effectiveLimit) {
    const evicted = nextOrder.shift();
    if (evicted !== undefined && evicted !== sessionKey) {
      delete next[evicted];
      evictedKeys.push(evicted);
    }
  }
  return { next, nextOrder, evictedKeys };
}

/** Pure helper updating a `{ byKey, order }` bounded session cache state. */
export function updateBoundedSessionState<T>(
  prev: BoundedSessionState<T>,
  sessionKey: string,
  value: T,
  limit = MAX_CACHED_SESSIONS,
): BoundedSessionState<T> {
  const { next, nextOrder } = setBoundedSessionRecord(
    prev.byKey,
    prev.order,
    sessionKey,
    value,
    limit,
  );
  return { byKey: next, order: nextOrder };
}

/**
 * Merge events with cursor strictly greater than the verified floor `cursor`,
 * deduplicating by `ev.cursor` and inserting missing lower-cursor gap events in
 * ascending cursor order while compacting superseded history frames in memory.
 */
export function mergeMonotonicEvents(
  accumulated: RemoteEvent[],
  batch: RemoteEvent[],
  cursor: number,
): { nextEvents: RemoteEvent[]; nextCursor: number; changed: boolean } {
  const seenCursors = new Set<number>();
  let maxAccCursor = cursor;
  for (const ev of accumulated) {
    seenCursors.add(ev.cursor);
    if (ev.cursor > maxAccCursor) maxAccCursor = ev.cursor;
  }

  const fresh: RemoteEvent[] = [];
  for (const ev of batch) {
    if (ev.cursor <= cursor) continue;
    if (seenCursors.has(ev.cursor)) continue;
    seenCursors.add(ev.cursor);
    fresh.push(ev);
  }
  if (fresh.length === 0) {
    return {
      nextEvents: accumulated,
      nextCursor: maxAccCursor,
      changed: false,
    };
  }

  let maxCursor = maxAccCursor;
  let needsSort = false;
  let prevCursor =
    accumulated.length > 0
      ? accumulated[accumulated.length - 1]!.cursor
      : -Infinity;
  for (const ev of fresh) {
    if (ev.cursor > maxCursor) maxCursor = ev.cursor;
    if (ev.cursor < prevCursor) needsSort = true;
    prevCursor = ev.cursor;
  }

  const combined = [...accumulated, ...fresh];
  if (needsSort) {
    combined.sort((a, b) => a.cursor - b.cursor);
  }
  const merged = trimSupersededEvents(combined);
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
  if (typeof output !== "string" || !output || typeof label !== "string") {
    return false;
  }
  const target = label.trim();
  if (!target) return false;
  const trimmed = output.trim();
  if (trimmed === target) return true;
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const parsed: unknown = JSON.parse(trimmed);
      if (parsed !== null && typeof parsed === "object") {
        const record = parsed as { answers?: unknown };
        const source =
          record.answers !== null && typeof record.answers === "object"
            ? (record.answers as Record<string, unknown>)
            : (record as Record<string, unknown>);
        const values = Object.values(source);
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
