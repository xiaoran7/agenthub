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
    }
    // Other fields (id, retry) are ignored: reconnection is driven by the
    // panel's own backoff, and the stream carries no resumable cursor.
  }
  return hasField ? { event, data: data.join("\n") } : undefined;
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
