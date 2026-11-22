/**
 * The event log must not become a snapshot archive.
 *
 * A Runner republishes its `ready` frame whenever its discovery result changes,
 * and that frame carries the whole model catalog and the whole discovery list.
 * On the production database 19281 of those frames had accumulated to 7.93 GiB
 * -- 99.9% of the file -- against 7 MB of real session events, because the log
 * was append-only and nothing ever trimmed it.
 *
 * These tests pin the two halves of the fix: the frame is stored without its
 * current-state snapshots, and the log is bounded per runner. Both are asserted
 * through the store rather than through HTTP, because the defect was in what
 * gets written, not in what a route returns.
 */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { RemoteContentStore } from "../src/remote-store.js";

const owner = "owner-a";
const runner = "dev_" + "c".repeat(32);

/** A `ready` frame with the two heavy fields populated, as a Runner sends it. */
function ready(overrides: Record<string, unknown> = {}) {
  return {
    type: "ready",
    host_generation: "host-one",
    device_name: "工作本",
    workspaces: ["ws-example-0123456789abcdef"],
    models: { data: [{ id: "gpt-5-codex" }] },
    discovery: {
      candidates: [
        {
          provider_session_id: "sess-old",
          workspace_path: "ws-example-0123456789abcdef",
          title: "旧会话",
          provider: "codex",
          resumable: true,
        },
      ],
    },
    sessions: [],
    pending_approvals: [],
    ...overrides,
  };
}

/**
 * A `history/snapshot` event, the frame `resumePoint` looks for.
 *
 * The key set is the one the contract allows and the `raw` shape is the one
 * `validateRemoteEventRaw` accepts: a transcript of role/text items. Anything
 * else is rejected before it reaches the store, which is why this is spelled
 * out rather than stubbed.
 */
function snapshotEvent(hub: string) {
  return {
    type: "event",
    event: {
      event_id: "event-history-snapshot",
      host_generation: "host-one",
      hub_session_id: hub,
      provider_session_id: "native-one",
      kind: "history/snapshot",
      binding: null,
      approval_id: null,
      raw: { params: { items: [{ role: "user", text: "hello" }] } },
    },
  };
}

function withStore(fn: (store: RemoteContentStore) => void): void {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-events-"));
  const path = join(dir, "remote.db");
  const store = new RemoteContentStore(path);
  try {
    fn(store);
  } finally {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * Counts every event this store holds for a runner.
 *
 * `events()` caps its limit at 1000, which is below the retention window, so
 * counting has to page. The cursor is exclusive, so each page starts one past
 * the last row it returned.
 */
function countEvents(store: RemoteContentStore, id: string): number {
  let after = 0;
  let total = 0;
  for (;;) {
    const page = store.events(owner, after, id, undefined, 1000);
    if (page.length === 0) return total;
    total += page.length;
    after = page[page.length - 1]?.cursor ?? after;
    if (page.length < 1000) return total;
  }
}

/** The newest event's cursor for a runner. */
function lastCursor(store: RemoteContentStore, id: string): number {
  let after = 0;
  let cursor = 0;
  for (;;) {
    const page = store.events(owner, after, id, undefined, 1000);
    if (page.length === 0) return cursor;
    cursor = page[page.length - 1]?.cursor ?? cursor;
    after = cursor;
    if (page.length < 1000) return cursor;
  }
}

test("a ready frame is stored without the catalog and the discovery list", () => {
  withStore((store) => {
    store.register(owner, runner);
    store.receive(owner, runner, 1, ready());

    const stored = store.events(owner, 0)[0]?.payload as Record<
      string,
      unknown
    >;
    // The two fields that made one frame weigh 440 KB are gone.
    assert.equal(stored.discovery, undefined);
    assert.equal(stored.models, undefined);
    // Everything the readers use survives, so the projection cannot be
    // satisfied by storing an empty frame.
    assert.equal(stored.host_generation, "host-one");
    assert.equal(stored.device_name, "工作本");
    assert.deepEqual(stored.pending_approvals, []);
    assert.equal(stored.type, "ready");
  });
});

test("the same frame is still available in full from hello", () => {
  withStore((store) => {
    store.register(owner, runner);
    const frame = ready();
    store.receive(owner, runner, 1, frame);

    // The projection only affects the log: the current-state table keeps the
    // frame verbatim, which is the only place any reader has ever taken the
    // catalog and the candidates from.
    const hello = store.runnerHello(owner, runner) as Record<string, unknown>;
    assert.deepEqual(hello.models, frame.models);
    assert.deepEqual(hello.discovery, frame.discovery);
  });
});

test("replaying the same ready frame is still recognised as a duplicate", () => {
  withStore((store) => {
    store.register(owner, runner);
    store.receive(owner, runner, 1, ready());
    // The duplicate check deep-compares the stored frame against the incoming
    // one. Without projecting both sides it would compare a full frame against
    // a trimmed one and reject the replay as a sequence conflict -- the exact
    // failure that check exists to prevent.
    assert.doesNotThrow(() => store.receive(owner, runner, 1, ready()));
  });
});

test("a changed ready frame at the same sequence is still a conflict", () => {
  withStore((store) => {
    store.register(owner, runner);
    store.receive(owner, runner, 1, ready());
    // The check must still bite. A field the log keeps is changed, so this is a
    // genuine conflict and not an artefact of the projection.
    assert.throws(
      () => store.receive(owner, runner, 1, ready({ device_name: "别的名字" })),
      /sequence_conflict/u,
    );
  });
});

test("an event frame keeps its whole payload", () => {
  withStore((store) => {
    store.register(owner, runner);
    const frame = snapshotEvent("hub-one");
    store.receive(owner, runner, 1, frame);

    const stored = store.events(owner, 0)[0]?.payload as Record<
      string,
      unknown
    >;
    // The transcript lives in the event frame's `raw`; trimming it would lose
    // history. Only `ready` is projected.
    assert.deepEqual(stored, frame);
  });
});

test("resumePoint still finds the approvals a ready frame published", () => {
  withStore((store) => {
    store.register(owner, runner);
    const hub = "hub-one";
    store.receive(owner, runner, 1, ready({ pending_approvals: [] }));
    // The marker resumePoint scans the payload text for must survive the
    // projection, or a resumed session would lose its pending-approval anchor.
    store.receive(owner, runner, 2, {
      ...ready(),
      pending_approvals: [
        {
          approval_id: `approval-${"a".repeat(64)}`,
          hub_session_id: hub,
          host_generation: "host-one",
          turn_id: "t1",
          can_approve: true,
          can_decline: true,
        },
      ],
    });

    const resume = store.resumePoint(owner, runner, hub);
    assert.equal(resume.approvals, 2);
  });
});

test("the log is bounded per runner", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-events-"));
  const path = join(dir, "remote.db");
  const store = new RemoteContentStore(path, 2000);
  try {
    // The store raises any window below SNAPSHOT_LOOKBACK (2000), because a
    // prune that ran ahead of it would delete the row `resumePoint()` walks
    // back to find. So the smallest window a test can actually reach is 2000,
    // and the bound is proven by writing past it rather than by asking for a
    // tiny one. 2050 rows keeps this to a couple of seconds.
    store.register(owner, runner);
    for (let seq = 1; seq <= 2050; seq += 1) {
      store.receive(owner, runner, seq, ready());
    }
    // 2050 written, 2000 kept: the window is enforced, not merely documented.
    assert.equal(countEvents(store, runner), 2000);
    // The survivors are the newest ones: the oldest kept row is the 51st
    // frame, and the newest is the last one written.
    assert.equal(lastCursor(store, runner), 2050);
  } finally {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a window below the snapshot lookback is raised, not honoured", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-events-"));
  const path = join(dir, "remote.db");
  const store = new RemoteContentStore(path, 5);
  try {
    // A caller asking for 5 rows would let the pruner delete the row
    // `resumePoint()` needs. The store raises the request to the lookback floor
    // instead of honouring it, which is observable as "the log keeps far more
    // than was asked for": 2050 frames written, 2000 kept, not 5.
    store.register(owner, runner);
    for (let seq = 1; seq <= 2050; seq += 1) {
      store.receive(owner, runner, seq, ready());
    }
    assert.equal(countEvents(store, runner), 2000);
  } finally {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  }
});

test("one runner cannot evict another runner's history", () => {
  const dir = mkdtempSync(join(tmpdir(), "agenthub-events-"));
  const path = join(dir, "remote.db");
  const store = new RemoteContentStore(path, 2000);
  try {
    // The lookback floor is 2000, so that is the smallest window the store will
    // honour and therefore the smallest one whose prune can be observed. The
    // busy device writes past it; the quiet device's frame is written first,
    // which is the order that would expose an owner-scoped delete.
    const busy = "dev_" + "b".repeat(32);
    const quiet = "dev_" + "q".repeat(32);
    store.register(owner, busy);
    store.register(owner, quiet);

    store.receive(owner, quiet, 1, ready());
    for (let seq = 1; seq <= 2010; seq += 1) {
      store.receive(owner, busy, seq, ready());
    }

    // The busy device is pruned to its window...
    assert.equal(countEvents(store, busy), 2000);
    // ...and the quiet device's single frame survives untouched, because the
    // prune is scoped by runner, not by owner.
    const quietEvents = store.events(owner, 0, quiet, undefined, 1000);
    assert.equal(quietEvents.length, 1);
    assert.equal(quietEvents[0]?.runner_id, quiet);
  } finally {
    store.close();
    rmSync(dir, { recursive: true, force: true });
  }
});
