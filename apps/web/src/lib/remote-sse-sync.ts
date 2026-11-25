import type { RemoteBinding } from "@agent-pulse/contracts/remote";
import { mergeMonotonicEvents } from "./remote-session-state";
import type { RemoteEvent, RemoteSessionDetail } from "./remote-types";
import type { StreamFrame } from "./stream";

export interface DirectSseBatchResult {
  /** Session-scoped events decoded directly from SSE `data:` payloads. */
  events: RemoteEvent[];
  /** Latest binding snapshot carried by any matching event in this batch. */
  latestBinding: RemoteBinding | null;
  /** True when a command receipt, runner `ready`, or unparseable frame needs an HTTP reconcile. */
  needsDetailReconcile: boolean;
  /** True when at least one frame in the batch was relevant to this session. */
  relevant: boolean;
}

export interface RemoteEventsBatchResponse {
  events: RemoteEvent[];
  truncated?: boolean | undefined;
}

function isRemoteBinding(value: unknown): value is RemoteBinding {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.hub_session_id === "string" &&
    typeof candidate.provider_session_id === "string" &&
    typeof candidate.execution_state === "string" &&
    typeof candidate.control_connected === "boolean"
  );
}

function extractFrameCursor(
  rawCursor: unknown,
  frameId: string | undefined,
): number | null {
  const cursor =
    typeof rawCursor === "number" && Number.isSafeInteger(rawCursor)
      ? rawCursor
      : Number(frameId);
  return Number.isSafeInteger(cursor) && cursor > 0 ? cursor : null;
}

/**
 * Computes the starting `after` cursor from a session detail `resume` hint so
 * cold starts and truncated resets begin just before the earliest needed mark.
 */
export function computeResumeStartCursor(
  resume: RemoteSessionDetail["resume"] | undefined,
): number {
  if (!resume) return 0;
  const marks = [resume.transcript, resume.approvals]
    .filter((mark): mark is number => typeof mark === "number" && mark > 0)
    .sort((a, b) => a - b);
  return marks.length > 0 ? Math.max(0, marks[0]! - 1) : 0;
}

/**
 * Decodes `/v3/remote/stream` `remote` SSE frames for an open session directly
 * into `RemoteEvent` records so live streaming deltas render with zero extra
 * HTTP round-trips.
 */
export function parseDirectSseBatch(
  frames: readonly StreamFrame[] | undefined,
  runnerId: string,
  hubSessionId: string,
): DirectSseBatchResult {
  if (!frames || frames.length === 0) {
    return {
      events: [],
      latestBinding: null,
      needsDetailReconcile: true,
      relevant: true,
    };
  }
  const events: RemoteEvent[] = [];
  let latestBinding: RemoteBinding | null = null;
  let needsDetailReconcile = false;
  let relevant = false;

  for (const frame of frames) {
    try {
      const parsed = JSON.parse(frame.data) as {
        cursor?: unknown;
        runner_id?: unknown;
        payload?: {
          type?: unknown;
          event?: {
            hub_session_id?: unknown;
            kind?: unknown;
            binding?: unknown;
          };
          receipt?: {
            hub_session_id?: unknown;
          };
        };
      };
      if (typeof parsed?.runner_id !== "string") {
        relevant = true;
        needsDetailReconcile = true;
        continue;
      }
      if (parsed.runner_id !== runnerId) continue;

      const payload = parsed.payload;
      if (!payload || typeof payload.type !== "string") {
        relevant = true;
        needsDetailReconcile = true;
        continue;
      }

      if (payload.type === "event") {
        const hub = payload.event?.hub_session_id;
        if (typeof hub === "string" && hub !== hubSessionId) continue;
        relevant = true;
        const cursor = extractFrameCursor(parsed.cursor, frame.id);
        if (cursor === null) {
          needsDetailReconcile = true;
          continue;
        }
        events.push({
          cursor,
          runner_id: parsed.runner_id,
          payload: payload as RemoteEvent["payload"],
        });
        if (isRemoteBinding(payload.event?.binding)) {
          latestBinding = payload.event.binding;
        }
        if (payload.event?.kind === "turn/completed") {
          needsDetailReconcile = true;
        }
        continue;
      }

      if (payload.type === "receipt") {
        const hub = payload.receipt?.hub_session_id;
        if (typeof hub === "string" && hub !== hubSessionId) continue;
        relevant = true;
        needsDetailReconcile = true;
        const cursor = extractFrameCursor(parsed.cursor, frame.id);
        if (cursor !== null) {
          events.push({
            cursor,
            runner_id: parsed.runner_id,
            payload: payload as RemoteEvent["payload"],
          });
        }
        continue;
      }

      if (payload.type === "ready" || payload.type === "host_lost") {
        relevant = true;
        needsDetailReconcile = true;
        const cursor = extractFrameCursor(parsed.cursor, frame.id);
        if (cursor !== null) {
          events.push({
            cursor,
            runner_id: parsed.runner_id,
            payload: payload as RemoteEvent["payload"],
          });
        }
        continue;
      }

      // Unknown runner-wide frames may update session bindings/approvals.
      relevant = true;
      needsDetailReconcile = true;
    } catch {
      relevant = true;
      needsDetailReconcile = true;
    }
  }

  return { events, latestBinding, needsDetailReconcile, relevant };
}

export interface FetchSessionEventsOptions {
  requestPage: (after: number) => Promise<RemoteEventsBatchResponse>;
  getAccumulated: () => RemoteEvent[];
  setAccumulated: (next: RemoteEvent[]) => void;
  startCursor: number;
  resume?: RemoteSessionDetail["resume"] | undefined;
  isStopped?: () => boolean;
  onTruncatedReset?: () => void;
  pageLimit?: number;
}

export interface FetchSessionEventsResult {
  nextHttpCursor: number;
  maxSeenCursor: number;
  changed: boolean;
  truncatedReset: boolean;
}

/**
 * Paginates `/v3/remote/events` from `startCursor`, filling any gaps left by
 * optimistic SSE frames without advancing `nextHttpCursor` past unread pages,
 * and recovering once from `truncated: true` by resetting state and re-seeding
 * from `resume`.
 */
export async function fetchSessionEventsSince({
  requestPage,
  getAccumulated,
  setAccumulated,
  startCursor,
  resume,
  isStopped,
  onTruncatedReset,
  pageLimit = 256,
}: FetchSessionEventsOptions): Promise<FetchSessionEventsResult> {
  let pageCursor = startCursor;
  let maxSeenCursor = startCursor;
  let changed = false;
  let truncatedReset = false;
  let retriedAfterTruncated = false;

  while (true) {
    const batch = await requestPage(pageCursor);
    if (isStopped?.()) {
      return {
        nextHttpCursor: pageCursor,
        maxSeenCursor,
        changed,
        truncatedReset,
      };
    }
    if (batch.truncated === true && !retriedAfterTruncated) {
      retriedAfterTruncated = true;
      truncatedReset = true;
      setAccumulated([]);
      onTruncatedReset?.();
      const reseedCursor = computeResumeStartCursor(resume);
      pageCursor =
        reseedCursor > 0 && reseedCursor !== pageCursor ? reseedCursor : 0;
      maxSeenCursor = pageCursor;
      continue;
    }
    if (batch.events.length > 0) {
      const merged = mergeMonotonicEvents(
        getAccumulated(),
        batch.events,
        pageCursor,
      );
      let batchMaxCursor = pageCursor;
      for (const ev of batch.events) {
        if (ev.cursor > batchMaxCursor) batchMaxCursor = ev.cursor;
      }
      pageCursor = batchMaxCursor;
      if (merged.nextCursor > maxSeenCursor) maxSeenCursor = merged.nextCursor;
      if (pageCursor > maxSeenCursor) maxSeenCursor = pageCursor;
      if (merged.changed) {
        setAccumulated(merged.nextEvents);
        changed = true;
      }
    }
    if (batch.events.length < pageLimit) break;
  }

  return {
    nextHttpCursor: pageCursor,
    maxSeenCursor,
    changed,
    truncatedReset,
  };
}

export interface RafBatcher<T> {
  push: (items: readonly T[]) => void;
  flushNow: () => void;
  cancel: () => void;
}

/**
 * Coalesces bursts of incoming streaming frames into at most one UI commit per
 * animation frame (~16ms), locking streaming updates to 60 FPS on mobile/web.
 */
export function createRafBatcher<T>(
  flush: (items: T[]) => void,
): RafBatcher<T> {
  let queue: T[] = [];
  let rafId: number | null = null;
  let timerId: ReturnType<typeof setTimeout> | null = null;

  const drain = (): void => {
    rafId = null;
    timerId = null;
    if (queue.length === 0) return;
    const batch = queue;
    queue = [];
    flush(batch);
  };

  const schedule = (): void => {
    if (rafId !== null || timerId !== null) return;
    if (
      typeof requestAnimationFrame === "function" &&
      (typeof document === "undefined" || !document.hidden)
    ) {
      rafId = requestAnimationFrame(drain);
    } else {
      timerId = setTimeout(drain, 16);
    }
  };

  return {
    push(items: readonly T[]) {
      if (items.length === 0) return;
      queue.push(...items);
      schedule();
    },
    flushNow() {
      if (rafId !== null && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(rafId);
      }
      if (timerId !== null) clearTimeout(timerId);
      drain();
    },
    cancel() {
      if (rafId !== null && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(rafId);
      }
      if (timerId !== null) clearTimeout(timerId);
      rafId = null;
      timerId = null;
      queue = [];
    },
  };
}
