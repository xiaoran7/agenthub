import { invoke } from "@tauri-apps/api/core";

import { type ConversationMessage, deduplicateMessages } from "./turn-stream";

interface CacheEntry {
  messages: ConversationMessage[];
  fetchedAt: number;
}

const CACHE_LIMIT = 60;
const cache = new Map<string, CacheEntry>();
const inflight = new Map<string, Promise<ConversationMessage[]>>();
const listeners = new Set<(key: string, entry: CacheEntry) => void>();

const hasTauri = () => "__TAURI_INTERNALS__" in window;

function store(key: string, messages: ConversationMessage[]) {
  const deduped = deduplicateMessages(messages);
  cache.delete(key);
  cache.set(key, { messages: deduped, fetchedAt: Date.now() });
  while (cache.size > CACHE_LIMIT) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
  const entry = cache.get(key);
  if (entry) listeners.forEach((listener) => listener(key, entry));
}

export function cachedConversation(
  key: string,
): ConversationMessage[] | undefined {
  return cache.get(key)?.messages;
}

/** Test/demo hook: pre-seed a transcript without touching the backend. */
export function seedConversation(key: string, messages: ConversationMessage[]) {
  store(key, messages);
}

export function subscribeConversation(
  listener: (key: string, entry: CacheEntry) => void,
) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Deduplicated fetch of a session transcript; results feed the shared cache. */
export function loadConversation(
  agentType: string,
  agentSessionId: string,
  forceRefresh = false,
): Promise<ConversationMessage[]> {
  const key = `${agentType}:${agentSessionId}`;
  if (!forceRefresh) {
    const running = inflight.get(key);
    if (running) return running;
  }
  // Outside Tauri (demo mode) keep whatever was seeded instead of wiping it.
  if (!hasTauri()) return Promise.resolve(cachedConversation(key) ?? []);
  const request = invoke<ConversationMessage[]>("load_session_conversation", {
    input: { agent_type: agentType, agent_session_id: agentSessionId },
  })
    .then((messages) => {
      const deduped = deduplicateMessages(messages);
      store(key, deduped);
      return deduped;
    })
    .finally(() => {
      inflight.delete(key);
    });
  inflight.set(key, request);
  return request;
}

/**
 * Warm the cache for the sessions the user is most likely to open next
 * (pinned first, then most recent). Runs sequentially and idly so it never
 * competes with an open conversation or an active turn.
 */
export function prefetchConversations(
  requests: Array<{ key: string; agentType: string; agentSessionId: string }>,
) {
  if (!hasTauri() || requests.length === 0) return;
  let chain: Promise<void> = Promise.resolve();
  const schedule = () =>
    new Promise<void>((resolve) => {
      if (typeof window.requestIdleCallback === "function") {
        window.requestIdleCallback(() => resolve(), { timeout: 3_000 });
      } else {
        window.setTimeout(resolve, 400);
      }
    });
  for (const request of requests) {
    if (cache.has(request.key)) continue;
    chain = chain.then(async () => {
      await schedule();
      try {
        await loadConversation(request.agentType, request.agentSessionId);
      } catch {
        // Prefetch is best-effort; the panel fetches on demand anyway.
      }
    });
  }
}
