import type {
  RemoteRunner,
  RemoteRunnerHello,
  RemoteRunnerHelloResponse,
} from "./remote";

export const HELLO_TTL_MS = 30_000;
export const HELLO_RETRY_MS = 3_000;

export type RunnerHelloStatus = "idle" | "loading" | "ready" | "error";

export interface RunnerHelloEntry {
  hello: RemoteRunnerHello | undefined;
  status: RunnerHelloStatus;
  fetchedAt?: number | undefined;
  failedAt?: number | undefined;
  initialAttemptFinished: boolean;
  hostGeneration?: string | null | undefined;
  hadThinHello?: boolean | undefined;
}

export interface RunnerHelloSyncTarget {
  runnerId: string;
  hasThinHello: boolean;
  hostGeneration?: string | null | undefined;
}

export type RunnerHelloFetcher = (
  runnerId: string,
  signal: AbortSignal,
) => Promise<RemoteRunnerHelloResponse>;

export interface RunnerHelloSyncOptions {
  fetcher: RunnerHelloFetcher;
  onChange: (snapshot: Record<string, RunnerHelloEntry>) => void;
  ttlMs?: number | undefined;
  retryMs?: number | undefined;
  now?: (() => number) | undefined;
  scheduleTimer?:
    ((fn: () => void, ms: number) => ReturnType<typeof setTimeout>) | undefined;
  clearTimer?: ((id: ReturnType<typeof setTimeout>) => void) | undefined;
}

function isAuthoritativeEmptyHelloError(error: unknown): boolean {
  return (
    error instanceof Error &&
    error.name === "RemoteRequestRejected" &&
    (error.message === "runner_not_ready" ||
      error.message === "device_not_found")
  );
}

/**
 * Returns whether the active runner's initial full `/hello` is still pending,
 * so the console can wait before locking in a partial or prefix-guessed Agent list.
 */
export function isRunnerHelloBlocking(
  entry: RunnerHelloEntry | undefined,
  runner: Pick<RemoteRunner, "revoked" | "hello"> | undefined,
): boolean {
  if (runner === undefined || runner.revoked || runner.hello === null) {
    return false;
  }
  return entry?.initialAttemptFinished !== true;
}

/**
 * Merges cached full `hello` payloads (containing `models` and `discovery`)
 * back onto the thin runner rows returned by `GET /v3/remote/sessions`.
 */
export function mergeRunnerHelloEntries(
  snapshotRunners: readonly RemoteRunner[],
  entries: Readonly<Record<string, RunnerHelloEntry>>,
): RemoteRunner[] {
  if (Object.keys(entries).length === 0) {
    return [...snapshotRunners];
  }
  return snapshotRunners.map((runner) => {
    const hello = entries[runner.runner_id]?.hello;
    if (hello === undefined || hello === null) {
      return runner;
    }
    const merged = {
      ...(runner.hello ?? undefined),
      ...hello,
    } as RemoteRunnerHello;
    return { ...runner, hello: merged };
  });
}

/**
 * Coordinates per-runner `/hello` fetches across `/sessions` polls and SSE pushes
 * without tearing down in-flight downloads or poisoning cache timestamps on error/abort.
 */
export class RunnerHelloSyncController {
  private readonly fetcher: RunnerHelloFetcher;
  private readonly onChange: (
    snapshot: Record<string, RunnerHelloEntry>,
  ) => void;
  private readonly ttlMs: number;
  private readonly retryMs: number;
  private readonly now: () => number;
  private readonly scheduleTimer: (
    fn: () => void,
    ms: number,
  ) => ReturnType<typeof setTimeout>;
  private readonly clearTimer: (id: ReturnType<typeof setTimeout>) => void;

  private readonly entries: Record<string, RunnerHelloEntry> = {};
  private readonly inFlight = new Map<string, AbortController>();
  private readonly retryTimers = new Map<
    string,
    ReturnType<typeof setTimeout>
  >();
  private readonly latestTargets = new Map<string, RunnerHelloSyncTarget>();
  private disposed = false;

  constructor(options: RunnerHelloSyncOptions) {
    this.fetcher = options.fetcher;
    this.onChange = options.onChange;
    this.ttlMs = options.ttlMs ?? HELLO_TTL_MS;
    this.retryMs = options.retryMs ?? HELLO_RETRY_MS;
    this.now = options.now ?? (() => Date.now());
    this.scheduleTimer =
      options.scheduleTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = options.clearTimer ?? ((id) => clearTimeout(id));
  }

  sync(targets: readonly RunnerHelloSyncTarget[]): void {
    if (this.disposed) return;

    const activeIds = new Set(targets.map((target) => target.runnerId));
    for (const existingId of this.latestTargets.keys()) {
      if (!activeIds.has(existingId)) {
        this.latestTargets.delete(existingId);
        this.clearRetryTimer(existingId);
      }
    }
    for (const target of targets) {
      this.latestTargets.set(target.runnerId, target);
    }

    const currentNow = this.now();
    let changed = false;

    for (const target of targets) {
      const id = target.runnerId;
      const entry = this.entries[id];

      if (!target.hasThinHello) {
        if (!entry) {
          this.entries[id] = {
            hello: null,
            status: "ready",
            fetchedAt: currentNow,
            initialAttemptFinished: true,
            hostGeneration: target.hostGeneration ?? null,
            hadThinHello: false,
          };
          changed = true;
        }
        continue;
      }

      if (this.inFlight.has(id)) {
        continue;
      }

      const thinHelloAppeared =
        entry !== undefined && entry.hadThinHello === false;
      const generationChanged =
        entry !== undefined &&
        target.hostGeneration !== undefined &&
        target.hostGeneration !== null &&
        entry.hostGeneration !== target.hostGeneration;

      let needsFetch = false;
      if (
        !entry ||
        entry.status === "idle" ||
        thinHelloAppeared ||
        generationChanged
      ) {
        needsFetch = true;
      } else if (entry.failedAt !== undefined) {
        needsFetch = currentNow - entry.failedAt >= this.retryMs;
      } else if (entry.status === "ready") {
        needsFetch = currentNow - (entry.fetchedAt ?? 0) >= this.ttlMs;
      }

      if (needsFetch) {
        this.clearRetryTimer(id);
        if (this.startFetch(target)) {
          changed = true;
        }
      }
    }

    if (changed) {
      this.emit();
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const ac of this.inFlight.values()) {
      ac.abort();
    }
    this.inFlight.clear();
    for (const timer of this.retryTimers.values()) {
      this.clearTimer(timer);
    }
    this.retryTimers.clear();
  }

  private startFetch(target: RunnerHelloSyncTarget): boolean {
    const id = target.runnerId;
    const prev = this.entries[id];
    const ac = new AbortController();
    this.inFlight.set(id, ac);

    let entryChanged = false;
    if (!prev || !prev.initialAttemptFinished || prev.hadThinHello === false) {
      this.entries[id] = {
        hello: prev?.hello,
        status: "loading",
        fetchedAt: prev?.fetchedAt,
        failedAt: prev?.failedAt,
        initialAttemptFinished: false,
        hostGeneration: prev?.hostGeneration,
        hadThinHello: true,
      };
      entryChanged = true;
    }

    void (async () => {
      try {
        const value = await this.fetcher(id, ac.signal);
        if (this.inFlight.get(id) === ac) {
          this.inFlight.delete(id);
        }
        if (this.disposed || ac.signal.aborted) {
          return;
        }
        const latestTarget = this.latestTargets.get(id) ?? target;
        this.entries[id] = {
          hello: value.hello,
          status: "ready",
          fetchedAt: this.now(),
          initialAttemptFinished: true,
          hostGeneration:
            value.hello?.host_generation ?? latestTarget.hostGeneration ?? null,
          hadThinHello: true,
        };
        this.emit();
      } catch (err) {
        if (this.inFlight.get(id) === ac) {
          this.inFlight.delete(id);
        }
        if (this.disposed || ac.signal.aborted) {
          return;
        }
        const currentPrev = this.entries[id];
        const latestTarget = this.latestTargets.get(id) ?? target;
        if (isAuthoritativeEmptyHelloError(err)) {
          this.entries[id] = {
            hello:
              currentPrev?.hello !== undefined && currentPrev.hello !== null
                ? currentPrev.hello
                : null,
            status: "ready",
            fetchedAt: this.now(),
            initialAttemptFinished: true,
            hostGeneration: latestTarget.hostGeneration ?? null,
            hadThinHello: latestTarget.hasThinHello,
          };
          this.emit();
          return;
        }
        const hasCachedHello =
          currentPrev?.hello !== undefined && currentPrev.hello !== null;
        this.entries[id] = {
          hello: currentPrev?.hello,
          status: hasCachedHello ? "ready" : "error",
          fetchedAt: currentPrev?.fetchedAt,
          failedAt: this.now(),
          initialAttemptFinished: true,
          hostGeneration: currentPrev?.hostGeneration,
          hadThinHello: true,
        };
        this.emit();
        this.scheduleRetry(id);
      }
    })();

    return entryChanged;
  }

  private scheduleRetry(runnerId: string): void {
    this.clearRetryTimer(runnerId);
    if (this.disposed || !this.latestTargets.has(runnerId)) return;
    const timer = this.scheduleTimer(() => {
      this.retryTimers.delete(runnerId);
      if (this.disposed) return;
      this.sync(Array.from(this.latestTargets.values()));
    }, this.retryMs);
    this.retryTimers.set(runnerId, timer);
  }

  private clearRetryTimer(runnerId: string): void {
    const existing = this.retryTimers.get(runnerId);
    if (existing !== undefined) {
      this.clearTimer(existing);
      this.retryTimers.delete(runnerId);
    }
  }

  private emit(): void {
    if (!this.disposed) {
      this.onChange({ ...this.entries });
    }
  }
}
