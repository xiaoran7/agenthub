/**
 * Owner-scoped change notifications for the read panel.
 *
 * The stream carries a "something changed" signal, never session payloads:
 * clients re-read the existing query endpoints, so the realtime channel cannot
 * drift from the field policy those endpoints already enforce.
 *
 * The broker is in-process. A multi-instance gateway needs a shared fan-out
 * (PostgreSQL LISTEN/NOTIFY or a bus) before it can drop the polling fallback.
 */
export interface ChangeMessage {
  device_id: string;
  accepted: number;
  auth_snapshots?: boolean;
}

export type ChangeListener = (message: ChangeMessage) => void;

export class ChangeBroker {
  readonly #listeners = new Map<string, Set<ChangeListener>>();

  subscribe(ownerId: string, listener: ChangeListener): () => void {
    let listeners = this.#listeners.get(ownerId);
    if (listeners === undefined) {
      listeners = new Set();
      this.#listeners.set(ownerId, listeners);
    }
    listeners.add(listener);
    return () => {
      const current = this.#listeners.get(ownerId);
      if (current === undefined) {
        return;
      }
      current.delete(listener);
      if (current.size === 0) {
        this.#listeners.delete(ownerId);
      }
    };
  }

  subscriberCount(ownerId: string): number {
    return this.#listeners.get(ownerId)?.size ?? 0;
  }

  publish(ownerId: string, message: ChangeMessage): void {
    const listeners = this.#listeners.get(ownerId);
    if (listeners === undefined) {
      return;
    }
    // Copy first: a listener may unsubscribe itself while being notified.
    for (const listener of [...listeners]) {
      listener(message);
    }
  }
}
