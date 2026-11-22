import { streamUrl } from "./api";
import type { PanelConfig } from "./config";

/**
 * The panel reads the gateway change stream with fetch instead of EventSource:
 * fetch supports AbortSignal and the explicit local-development identity.
 * Production credentials are added only by the same-origin server proxy and
 * never enter this browser-side module.
 */
export interface StreamFrame {
  event: string;
  data: string;
  /** The `id:` line, when the stream carries one, used as a resume cursor. */
  id?: string;
}

export interface ChangeStreamHandlers {
  /** The owner has newly stored session events; re-read the query endpoints. */
  onChange: () => void;
  onConnected: (connected: boolean) => void;
}

const MAX_BACKOFF_MS = 15_000;

export function backoffMs(attempt: number): number {
  return Math.min(1_000 * 2 ** Math.max(0, attempt - 1), MAX_BACKOFF_MS);
}

export class SseDecoder {
  #buffer = "";
  /** A chunk may end mid-CRLF; hold the CR back instead of splitting a line. */
  #pendingCarriageReturn = false;

  push(chunk: string): StreamFrame[] {
    let text = this.#pendingCarriageReturn ? `\r${chunk}` : chunk;
    this.#pendingCarriageReturn = text.endsWith("\r");
    if (this.#pendingCarriageReturn) {
      text = text.slice(0, -1);
    }
    this.#buffer += text.replaceAll("\r\n", "\n").replaceAll("\r", "\n");
    const frames: StreamFrame[] = [];
    for (;;) {
      const boundary = this.#buffer.indexOf("\n\n");
      if (boundary < 0) {
        return frames;
      }
      const frame = parseFrame(this.#buffer.slice(0, boundary));
      this.#buffer = this.#buffer.slice(boundary + 2);
      if (frame !== undefined) {
        frames.push(frame);
      }
    }
  }
}

function parseFrame(frame: string): StreamFrame | undefined {
  let event = "message";
  const data: string[] = [];
  let id: string | undefined;
  let hasField = false;
  for (const line of frame.split("\n")) {
    if (line.length === 0 || line.startsWith(":")) {
      continue;
    }
    const separator = line.indexOf(":");
    const field = separator < 0 ? line : line.slice(0, separator);
    const rawValue = separator < 0 ? "" : line.slice(separator + 1);
    const value = rawValue.startsWith(" ") ? rawValue.slice(1) : rawValue;
    if (field === "event") {
      event = value;
      hasField = true;
    } else if (field === "data") {
      data.push(value);
      hasField = true;
    } else if (field === "id") {
      id = value;
    }
    // Other fields (retry) are ignored: reconnection is driven by the panel's
    // own backoff, which is bounded and works the same for every stream.
  }
  return hasField
    ? { event, data: data.join("\n"), ...(id === undefined ? {} : { id }) }
    : undefined;
}

export function openChangeStream(
  config: PanelConfig,
  handlers: ChangeStreamHandlers,
): () => void {
  const controller = new AbortController();
  let stopped = false;
  let attempt = 0;

  const run = async (): Promise<void> => {
    while (!stopped) {
      try {
        const response = await fetch(streamUrl(config.gatewayBase), {
          headers: {
            "x-user-id": config.userId,
            accept: "text/event-stream",
          },
          cache: "no-store",
          signal: controller.signal,
        });
        if (!response.ok || response.body === null) {
          throw new Error(`stream rejected with ${response.status}`);
        }
        attempt = 0;
        handlers.onConnected(true);
        const reader = response.body.getReader();
        const bytes = new TextDecoder();
        const decoder = new SseDecoder();
        for (;;) {
          const chunk = await reader.read();
          if (chunk.done) {
            break;
          }
          const frames = decoder.push(
            bytes.decode(chunk.value, { stream: true }),
          );
          for (const frame of frames) {
            if (frame.event === "sessions") {
              handlers.onChange();
            }
          }
        }
      } catch {
        // Any failure falls through to the same backoff: the panel keeps
        // polling meanwhile, so a permanently unavailable stream degrades to
        // the previous behaviour instead of breaking the page.
      }
      if (stopped) {
        return;
      }
      handlers.onConnected(false);
      attempt += 1;
      await sleep(backoffMs(attempt), controller.signal);
    }
  };
  void run();

  return () => {
    stopped = true;
    controller.abort();
  };
}

/**
 * Follows the Gateway's content stream (`/v3/remote/stream`) instead of a
 * timer.
 *
 * Each `remote` frame says the owner has newly stored content — the Runner's
 * frame itself — and the only correct answer is to re-read the authenticated
 * query endpoints, which is what the caller's `onChange` does. The frame's
 * cursor is kept and sent back as `after` on a reconnect, so a stream that
 * dropped between two frames resumes without replaying or skipping either.
 *
 * Unlike [`openChangeStream`], this carries no dev identity header: account
 * routes authenticate by the session cookie, and the header would resolve a
 * different owner than the cookie's on a deployment with accounts.
 */
export function openContentStream(options: {
  /** Gateway base; empty means the same-origin proxy. */
  readonly gatewayBase: string;
  /** Text of the `event:` line that means "re-read now". */
  readonly event: string;
  readonly onChange: (frames: readonly StreamFrame[]) => void;
  readonly onConnected?: (connected: boolean) => void;
}): { close: () => void } {
  const controller = new AbortController();
  let stopped = false;
  let attempt = 0;
  let after = 0;

  const run = async (): Promise<void> => {
    while (!stopped) {
      try {
        const query = after > 0 ? `?after=${after}` : "";
        const response = await fetch(
          `${options.gatewayBase}/v3/remote/stream${query}`,
          {
            headers: { accept: "text/event-stream" },
            credentials: "same-origin",
            cache: "no-store",
            signal: controller.signal,
          },
        );
        if (!response.ok || response.body === null) {
          throw new Error(`stream rejected with ${response.status}`);
        }
        const openedAt = Date.now();
        options.onConnected?.(true);
        const reader = response.body.getReader();
        const bytes = new TextDecoder();
        const decoder = new SseDecoder();
        for (;;) {
          const chunk = await reader.read();
          if (chunk.done) {
            break;
          }
          const matchedFrames: StreamFrame[] = [];
          for (const frame of decoder.push(
            bytes.decode(chunk.value, { stream: true }),
          )) {
            attempt = 0;
            const cursor = Number(frame.id);
            if (Number.isSafeInteger(cursor) && cursor > after) {
              after = cursor;
            }
            if (options.event === "*" || frame.event === options.event) {
              matchedFrames.push(frame);
            }
          }
          if (matchedFrames.length > 0) {
            options.onChange(matchedFrames);
          }
        }
        if (Date.now() - openedAt >= 10_000) {
          attempt = 0;
        }
      } catch {
        // The caller keeps a slow fallback read, so a stream that never
        // connects degrades to that cadence instead of breaking the page.
      }
      if (stopped) {
        return;
      }
      options.onConnected?.(false);
      attempt += 1;
      await sleep(backoffMs(attempt), controller.signal);
    }
  };
  void run();

  return {
    close: () => {
      stopped = true;
      controller.abort();
    },
  };
}

/**
 * One content stream per Gateway origin, shared by every listener in the tab.
 *
 * The Gateway caps how many streams it holds overall, and a console needs the
 * same signal in two places (the session list and the open conversation), so
 * a connection per listener would spend the cap twice on one page. Listeners
 * come and go with the conversation the reader opens; the connection stays up
 * while at least one is attached.
 */
type ContentFrameListener = {
  event: string;
  onChange: (frames: readonly StreamFrame[]) => void;
};

const sharedContentStreams = new Map<
  string,
  {
    close: () => void;
    listeners: Set<ContentFrameListener>;
    references: number;
  }
>();

export function followContentStream(options: {
  readonly gatewayBase: string;
  readonly event: string;
  readonly onChange: (frames?: readonly StreamFrame[]) => void;
}): () => void {
  const key = options.gatewayBase;
  let entry = sharedContentStreams.get(key);
  if (entry === undefined) {
    const listeners = new Set<ContentFrameListener>();
    const close = openContentStream({
      gatewayBase: key,
      event: "*",
      onChange: (frames) => {
        for (const listener of [...listeners]) {
          const filtered = frames.filter((f) => f.event === listener.event);
          if (filtered.length > 0) listener.onChange(filtered);
        }
      },
    }).close;
    entry = { close, listeners, references: 0 };
    sharedContentStreams.set(key, entry);
  }
  const current = entry;
  const wrapped: ContentFrameListener = {
    event: options.event,
    onChange: (frames) => options.onChange(frames),
  };
  current.listeners.add(wrapped);
  current.references += 1;
  return () => {
    current.listeners.delete(wrapped);
    current.references -= 1;
    if (current.references > 0) return;
    current.close();
    sharedContentStreams.delete(key);
  };
}

const SNAPSHOT_RELEVANT_EVENT_KINDS = new Set([
  "turn/started",
  "turn/completed",
  "approval_requested",
  "approval_resolved",
  "question_requested",
  "question_resolved",
  "unsupported_provider_request",
]);

/**
 * Returns whether a batch of `/v3/remote/stream` frames can change the global
 * `/v3/remote/sessions` list response. Pure intra-turn streaming content deltas
 * (`item/agentMessage/delta`, tool output deltas, etc.) without a `binding`
 * update do not alter any field returned by `/v3/remote/sessions`.
 */
export function streamBatchAffectsSnapshot(
  frames?: readonly StreamFrame[],
): boolean {
  if (!frames || frames.length === 0) return true;
  for (const frame of frames) {
    try {
      const parsed = JSON.parse(frame.data) as {
        payload?: {
          type?: unknown;
          event?: { kind?: unknown; binding?: unknown };
        };
      };
      const payload = parsed?.payload;
      if (!payload || typeof payload.type !== "string") return true;
      if (payload.type !== "event") return true;
      const ev = payload.event;
      if (!ev || typeof ev !== "object") return true;
      if (ev.binding !== null && ev.binding !== undefined) return true;
      if (
        typeof ev.kind !== "string" ||
        SNAPSHOT_RELEVANT_EVENT_KINDS.has(ev.kind)
      ) {
        return true;
      }
    } catch {
      return true;
    }
  }
  return false;
}

/**
 * Returns whether a batch of `/v3/remote/stream` frames can affect a specific
 * open `(runnerId, hubSessionId)` conversation. Frames belonging to another
 * Runner or explicitly scoped to another `hub_session_id` are ignored.
 */
export function streamBatchAffectsSession(
  frames: readonly StreamFrame[] | undefined,
  runnerId: string,
  hubSessionId: string,
): boolean {
  if (!frames || frames.length === 0) return true;
  for (const frame of frames) {
    try {
      const parsed = JSON.parse(frame.data) as {
        runner_id?: unknown;
        payload?: {
          type?: unknown;
          event?: { hub_session_id?: unknown };
          receipt?: { hub_session_id?: unknown };
        };
      };
      if (typeof parsed?.runner_id !== "string") return true;
      if (parsed.runner_id !== runnerId) continue;
      const payload = parsed.payload;
      if (!payload || typeof payload.type !== "string") return true;
      if (payload.type === "event") {
        const hub = payload.event?.hub_session_id;
        if (typeof hub === "string" && hub !== hubSessionId) continue;
        return true;
      }
      if (payload.type === "receipt") {
        const hub = payload.receipt?.hub_session_id;
        if (typeof hub === "string" && hub !== hubSessionId) continue;
        return true;
      }
      return true;
    } catch {
      return true;
    }
  }
  return false;
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(() => {
      signal.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      resolve();
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });
}
