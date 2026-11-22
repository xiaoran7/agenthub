import assert from "node:assert/strict";
import test from "node:test";
import type {
  RemoteRunner,
  RemoteRunnerHelloResponse,
} from "../src/lib/remote";
import {
  RunnerHelloSyncController,
  isRunnerHelloBlocking,
  mergeRunnerHelloEntries,
  type RunnerHelloEntry,
} from "../src/lib/runner-hello-sync";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeHelloResponse(
  runnerId: string,
  hostGeneration = "gen-1",
  candidatesCount = 2,
): RemoteRunnerHelloResponse {
  return {
    runner_id: runnerId,
    online: true,
    revoked: false,
    hello: {
      host_generation: hostGeneration,
      device_name: "xiaotang",
      workspaces: ["ws-demo-0123456789abcdef"],
      discovery: {
        candidates: Array.from({ length: candidatesCount }, (_, idx) => ({
          provider_session_id: `claude_cand_${idx}`,
          workspace_path: "ws-demo-0123456789abcdef",
          title: `Session ${idx}`,
          provider: "claude_desktop",
          resumable: true,
        })),
      },
    },
  };
}

test("repeated sync calls during an in-flight /hello fetch deduplicate and keep its result", async () => {
  const pending = deferred<RemoteRunnerHelloResponse>();
  let fetchCalls = 0;
  let latestEntries: Record<string, RunnerHelloEntry> = {};

  const controller = new RunnerHelloSyncController({
    fetcher: () => {
      fetchCalls += 1;
      return pending.promise;
    },
    onChange: (snapshot) => {
      latestEntries = snapshot;
    },
    now: () => 1_000,
  });

  const target = {
    runnerId: "dev_a",
    hasThinHello: true,
    hostGeneration: "gen-1",
  };
  controller.sync([target]);
  assert.equal(fetchCalls, 1);
  assert.equal(latestEntries.dev_a?.status, "loading");
  assert.equal(latestEntries.dev_a?.initialAttemptFinished, false);

  // Simulate SSE `ready` push triggering another `/sessions` snapshot while `/hello` is downloading.
  controller.sync([target]);
  controller.sync([target]);
  assert.equal(fetchCalls, 1);

  pending.resolve(makeHelloResponse("dev_a", "gen-1", 3));
  await pending.promise;
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(latestEntries.dev_a?.status, "ready");
  assert.equal(latestEntries.dev_a?.initialAttemptFinished, true);
  assert.equal(latestEntries.dev_a?.fetchedAt, 1_000);
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 3);
  controller.dispose();
});

test("dispose aborts in-flight fetch and never records fetchedAt or emits after disposal", async () => {
  let aborted = false;
  let rejectFetch!: (reason?: unknown) => void;
  const emissions: Array<Record<string, RunnerHelloEntry>> = [];

  const controller = new RunnerHelloSyncController({
    fetcher: (_id, signal) =>
      new Promise<RemoteRunnerHelloResponse>((_resolve, reject) => {
        rejectFetch = reject;
        signal.addEventListener("abort", () => {
          aborted = true;
          reject(new DOMException("Aborted", "AbortError"));
        });
      }),
    onChange: (snapshot) => {
      emissions.push(snapshot);
    },
    now: () => 2_000,
  });

  controller.sync([
    { runnerId: "dev_a", hasThinHello: true, hostGeneration: "gen-1" },
  ]);
  assert.equal(emissions.length, 1);

  controller.dispose();
  assert.equal(aborted, true);
  rejectFetch(new DOMException("Aborted", "AbortError"));
  await new Promise((resolve) => setTimeout(resolve, 0));

  // Only the initial "loading" emission happened; no error or fetchedAt was recorded after dispose.
  assert.equal(emissions.length, 1);
  assert.equal(emissions[0]?.dev_a?.fetchedAt, undefined);
});

test("background refresh transient failure preserves cached hello and recovers on retry", async () => {
  let clock = 0;
  let attempt = 0;
  const scheduled: Array<{ fn: () => void; ms: number }> = [];
  let latestEntries: Record<string, RunnerHelloEntry> = {};

  const controller = new RunnerHelloSyncController({
    fetcher: async (id) => {
      attempt += 1;
      if (attempt === 1) return makeHelloResponse(id, "gen-1", 4);
      if (attempt === 2) throw new Error("request_timeout");
      return makeHelloResponse(id, "gen-1", 5);
    },
    onChange: (snapshot) => {
      latestEntries = snapshot;
    },
    ttlMs: 30_000,
    retryMs: 3_000,
    now: () => clock,
    scheduleTimer: (fn, ms) => {
      scheduled.push({ fn, ms });
      return scheduled.length as unknown as ReturnType<typeof setTimeout>;
    },
    clearTimer: () => {},
  });

  const target = {
    runnerId: "dev_a",
    hasThinHello: true,
    hostGeneration: "gen-1",
  };

  controller.sync([target]);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 4);
  assert.equal(latestEntries.dev_a?.fetchedAt, 0);

  // Trigger TTL refresh that fails transiently.
  clock = 30_000;
  controller.sync([target]);
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(attempt, 2);
  assert.equal(latestEntries.dev_a?.status, "ready");
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 4);
  assert.equal(latestEntries.dev_a?.failedAt, 30_000);
  assert.equal(scheduled.length, 1);
  assert.equal(scheduled[0]?.ms, 3_000);

  // Intermediate poll before retryMs does not hammer the endpoint.
  clock = 31_000;
  controller.sync([target]);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(attempt, 2);

  // Retry timer fires after retryMs and recovers with updated candidates.
  clock = 33_000;
  scheduled[0]?.fn();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(attempt, 3);
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 5);
  assert.equal(latestEntries.dev_a?.fetchedAt, 33_000);
  assert.equal(latestEntries.dev_a?.failedAt, undefined);
  controller.dispose();
});

test("initial transient failure unblocks UI and recovers via scheduled retry", async () => {
  let clock = 100;
  let attempt = 0;
  const scheduled: Array<{ fn: () => void; ms: number }> = [];
  let latestEntries: Record<string, RunnerHelloEntry> = {};

  const runnerRow: RemoteRunner = {
    runner_id: "dev_a",
    online: true,
    revoked: false,
    hello: { host_generation: "gen-1", workspaces: [] },
  };

  const controller = new RunnerHelloSyncController({
    fetcher: async (id) => {
      attempt += 1;
      if (attempt === 1) throw new Error("gateway_unreachable");
      return makeHelloResponse(id, "gen-1", 2);
    },
    onChange: (snapshot) => {
      latestEntries = snapshot;
    },
    retryMs: 3_000,
    now: () => clock,
    scheduleTimer: (fn, ms) => {
      scheduled.push({ fn, ms });
      return scheduled.length as unknown as ReturnType<typeof setTimeout>;
    },
    clearTimer: () => {},
  });

  assert.equal(isRunnerHelloBlocking(latestEntries.dev_a, runnerRow), true);

  controller.sync([
    { runnerId: "dev_a", hasThinHello: true, hostGeneration: "gen-1" },
  ]);
  assert.equal(isRunnerHelloBlocking(latestEntries.dev_a, runnerRow), true);

  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(latestEntries.dev_a?.status, "error");
  assert.equal(latestEntries.dev_a?.initialAttemptFinished, true);
  assert.equal(isRunnerHelloBlocking(latestEntries.dev_a, runnerRow), false);
  assert.equal(scheduled.length, 1);

  clock = 3_100;
  scheduled[0]?.fn();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(latestEntries.dev_a?.status, "ready");
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 2);
  controller.dispose();
});

test("hostGeneration change triggers immediate re-fetch even within ttlMs", async () => {
  let attempt = 0;
  let latestEntries: Record<string, RunnerHelloEntry> = {};

  const controller = new RunnerHelloSyncController({
    fetcher: async (id) => {
      attempt += 1;
      return makeHelloResponse(id, attempt === 1 ? "gen-1" : "gen-2", attempt);
    },
    onChange: (snapshot) => {
      latestEntries = snapshot;
    },
    ttlMs: 30_000,
    now: () => 5_000,
  });

  controller.sync([
    { runnerId: "dev_a", hasThinHello: true, hostGeneration: "gen-1" },
  ]);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(attempt, 1);
  assert.equal(latestEntries.dev_a?.hostGeneration, "gen-1");

  // Same timestamp (within ttlMs), but Runner restarted with gen-2.
  controller.sync([
    { runnerId: "dev_a", hasThinHello: true, hostGeneration: "gen-2" },
  ]);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(attempt, 2);
  assert.equal(latestEntries.dev_a?.hostGeneration, "gen-2");
  assert.equal(latestEntries.dev_a?.hello?.discovery?.candidates.length, 2);
  controller.dispose();
});

test("hasThinHello false does not block UI and flipping to true triggers immediate fetch", async () => {
  let attempt = 0;
  let latestEntries: Record<string, RunnerHelloEntry> = {};

  const controller = new RunnerHelloSyncController({
    fetcher: async (id) => {
      attempt += 1;
      return makeHelloResponse(id, "gen-1", 6);
    },
    onChange: (snapshot) => {
      latestEntries = snapshot;
    },
    ttlMs: 30_000,
    now: () => 1_000,
  });

  const emptyRunner: RemoteRunner = {
    runner_id: "dev_new",
    online: false,
    revoked: false,
    hello: null,
  };
  assert.equal(
    isRunnerHelloBlocking(latestEntries.dev_new, emptyRunner),
    false,
  );

  controller.sync([
    { runnerId: "dev_new", hasThinHello: false, hostGeneration: null },
  ]);
  assert.equal(attempt, 0);
  assert.equal(latestEntries.dev_new?.initialAttemptFinished, true);
  assert.equal(
    isRunnerHelloBlocking(latestEntries.dev_new, emptyRunner),
    false,
  );

  controller.sync([
    { runnerId: "dev_new", hasThinHello: true, hostGeneration: "gen-1" },
  ]);
  assert.equal(attempt, 1);
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(latestEntries.dev_new?.hello?.discovery?.candidates.length, 6);
  controller.dispose();
});

test("mergeRunnerHelloEntries overlays full hello without mutating input rows", () => {
  const thinRunner: RemoteRunner = {
    runner_id: "dev_a",
    online: true,
    revoked: false,
    hello: {
      host_generation: "gen-1",
      device_name: "xiaotang",
      workspaces: ["ws-demo-0123456789abcdef"],
    },
  };
  const input = [thinRunner];
  const entries: Record<string, RunnerHelloEntry> = {
    dev_a: {
      status: "ready",
      initialAttemptFinished: true,
      hello: makeHelloResponse("dev_a", "gen-1", 2).hello,
    },
  };

  const merged = mergeRunnerHelloEntries(input, entries);
  assert.equal(thinRunner.hello?.discovery, undefined);
  assert.equal(merged[0]?.hello?.discovery?.candidates.length, 2);
  assert.equal(merged[0]?.hello?.device_name, "xiaotang");
});
