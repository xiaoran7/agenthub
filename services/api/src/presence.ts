/**
 * Which Runners currently hold an authenticated v3 stream.
 *
 * Presence is a property of the live connection, not of the database. The
 * `web`/`daemon` Runner keeps a WebSocket open and never calls the HTTP
 * heartbeat endpoint, so `last_seen_at` stops advancing after pairing and the
 * device list reported a connected Runner as offline forever. The console and
 * the device list read this set, so the two surfaces cannot disagree.
 */
export class RunnerPresence {
  readonly #online = new Set<string>();
  readonly #offlineTimers = new Map<string, ReturnType<typeof setTimeout>>();

  markOnline(runner: string): boolean {
    const pending = this.#offlineTimers.get(runner);
    if (pending !== undefined) {
      clearTimeout(pending);
      this.#offlineTimers.delete(runner);
    }
    const wasOnline = this.#online.has(runner);
    this.#online.add(runner);
    return !wasOnline;
  }

  markOffline(runner: string): boolean {
    const pending = this.#offlineTimers.get(runner);
    if (pending !== undefined) {
      clearTimeout(pending);
      this.#offlineTimers.delete(runner);
    }
    return this.#online.delete(runner);
  }

  scheduleOffline(
    runner: string,
    graceMs: number,
    onOffline?: () => void,
  ): void {
    if (graceMs <= 0) {
      if (this.markOffline(runner)) {
        onOffline?.();
      }
      return;
    }
    const existing = this.#offlineTimers.get(runner);
    if (existing !== undefined) {
      clearTimeout(existing);
    }
    const timer = setTimeout(() => {
      this.#offlineTimers.delete(runner);
      if (this.#online.delete(runner)) {
        onOffline?.();
      }
    }, graceMs);
    timer.unref();
    this.#offlineTimers.set(runner, timer);
  }

  close(): void {
    for (const timer of this.#offlineTimers.values()) {
      clearTimeout(timer);
    }
    this.#offlineTimers.clear();
  }

  isOnline(runner: string): boolean {
    return this.#online.has(runner);
  }
}
