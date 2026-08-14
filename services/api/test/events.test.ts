import assert from "node:assert/strict";
import test from "node:test";

import {
  ChangeBroker,
  PostgresChangeBroker,
  type ChangeMessage,
} from "../src/events.js";

const message: ChangeMessage = {
  device_id: `dev_${"a".repeat(32)}`,
  accepted: 1,
  remote_tasks: true,
};

test("change broker keeps notifications owner scoped", () => {
  const broker = new ChangeBroker();
  const owned: ChangeMessage[] = [];
  const foreign: ChangeMessage[] = [];
  const unsubscribe = broker.subscribe("owner-1", (value) => owned.push(value));
  broker.subscribe("owner-2", (value) => foreign.push(value));

  broker.publish("owner-1", message);
  assert.deepEqual(owned, [message]);
  assert.deepEqual(foreign, []);
  assert.equal(broker.subscriberCount("owner-1"), 1);

  unsubscribe();
  assert.equal(broker.subscriberCount("owner-1"), 0);
});

test(
  "PostgreSQL brokers fan out once across API instances",
  { skip: process.env.AGENT_PLATFORM_TEST_DATABASE_URL === undefined },
  async () => {
    const databaseUrl = process.env.AGENT_PLATFORM_TEST_DATABASE_URL;
    assert.ok(databaseUrl);
    const first = await PostgresChangeBroker.connect(databaseUrl);
    const second = await PostgresChangeBroker.connect(databaseUrl);
    let localCount = 0;
    let remoteCount = 0;
    const remote = new Promise<ChangeMessage>((resolve) => {
      second.subscribe("owner-1", (value) => {
        remoteCount += 1;
        resolve(value);
      });
    });
    first.subscribe("owner-1", () => {
      localCount += 1;
    });

    try {
      first.publish("owner-1", message);
      assert.deepEqual(await withTimeout(remote, 2_000), message);
      await new Promise((resolve) => setTimeout(resolve, 50));
      assert.equal(localCount, 1, "the publishing instance ignores its echo");
      assert.equal(remoteCount, 1);
    } finally {
      await Promise.all([first.close(), second.close()]);
    }
  },
);

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () => reject(new Error("notification timed out")),
          timeoutMs,
        );
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
