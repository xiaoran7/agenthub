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

  markOnline(runner: string): void {
    this.#online.add(runner);
  }

  markOffline(runner: string): void {
    this.#online.delete(runner);
  }

  isOnline(runner: string): boolean {
    return this.#online.has(runner);
  }
}
