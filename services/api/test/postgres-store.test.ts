import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { parseSyncBatch } from "@agent-platform/contracts";
import postgres from "postgres";

import {
  PostgresGatewayStore,
  migratePostgres,
} from "../src/postgres-store.js";

const databaseUrl = process.env.AGENT_PLATFORM_TEST_DATABASE_URL;
const fixture = parseSyncBatch(
  JSON.parse(
    await readFile(
      new URL(
        "../../../packages/contracts/fixtures/sync-batch-v1.json",
        import.meta.url,
      ),
      "utf8",
    ),
  ),
);

test(
  "PostgreSQL store persists registration, replay state, and session queries",
  { skip: databaseUrl === undefined },
  async () => {
    assert.ok(databaseUrl);
    const admin = postgres(databaseUrl, { max: 1, onnotice: () => {} });
    const schema = `agent_platform_test_${randomUUID().replaceAll("-", "")}`;
    let firstSql: postgres.Sql | undefined;
    let secondSql: postgres.Sql | undefined;
    let schemaCreated = false;
    try {
      await admin`CREATE SCHEMA ${admin(schema)}`;
      schemaCreated = true;
      firstSql = scopedConnection(databaseUrl, schema);
      await migratePostgres(firstSql);
      const firstStore = new PostgresGatewayStore(firstSql);
      const registration = await firstStore.registerDevice(
        "postgres-user",
        fixture.device_id,
        fixture.public_key,
      );
      assert.equal(registration.created, true);
      const ingestion = await firstStore.ingestBatch(fixture);
      assert.equal(ingestion.accepted, 1);
      assert.equal(ingestion.owner_id, "postgres-user");
      assert.deepEqual(ingestion.changes, [
        {
          device_id: fixture.device_id,
          agent_type: "codex",
          agent_session_id: "contract-session",
          project_display_name: "contract-project",
          presence: "LIVE",
          activity_status: "WAITING_PERMISSION",
          waiting_reason: "permission",
          observed_at: fixture.events[0]?.observed_at,
        },
      ]);
      const replayed = await firstStore.ingestBatch(fixture);
      assert.equal(replayed.accepted, 0);
      assert.deepEqual(
        replayed.changes,
        [],
        "a replayed batch changes nothing and must not notify",
      );
      assert.equal(
        (await firstStore.listSessions("postgres-user", {})).length,
        1,
      );
      await firstStore.close();
      firstSql = undefined;

      secondSql = scopedConnection(databaseUrl, schema);
      await migratePostgres(secondSql);
      const secondStore = new PostgresGatewayStore(secondSql);
      const retry = await secondStore.registerDevice(
        "postgres-user",
        fixture.device_id,
        fixture.public_key,
      );
      assert.equal(retry.created, false);
      assert.equal((await secondStore.listDevices("postgres-user")).length, 1);
      assert.equal(
        (
          await secondStore.listSessions("postgres-user", {
            activity_status: "WAITING_PERMISSION",
            presence: "LIVE",
          })
        )[0]?.agent_session_id,
        "contract-session",
      );
      assert.equal(
        (await secondStore.listSessions("another-user", {})).length,
        0,
      );
      const timeline = await secondStore.listSessionEvents("postgres-user", {
        device_id: fixture.device_id,
        agent_type: "codex",
        agent_session_id: "contract-session",
        limit: 10,
      });
      assert.equal(timeline.length, 1);
      assert.equal(timeline[0]?.sequence, 1);
      assert.equal(typeof timeline[0]?.received_at, "string");
      assert.equal(
        (
          await secondStore.listSessionEvents("another-user", {
            device_id: fixture.device_id,
            agent_type: "codex",
            agent_session_id: "contract-session",
            limit: 10,
          })
        ).length,
        0,
      );

      const subscription = {
        endpoint: "https://push.example/subscription/abc",
        p256dh: Buffer.concat([
          Buffer.from([0x04]),
          Buffer.alloc(64, 7),
        ]).toString("base64url"),
        auth: Buffer.alloc(16, 3).toString("base64url"),
      };
      await secondStore.savePushSubscription("postgres-user", subscription);
      // Re-subscribing is an upsert, not a duplicate row.
      await secondStore.savePushSubscription("postgres-user", subscription);
      assert.deepEqual(
        await secondStore.listPushSubscriptions("postgres-user"),
        [subscription],
      );
      assert.deepEqual(
        await secondStore.listPushSubscriptions("another-user"),
        [],
      );
      await secondStore.deletePushSubscription(
        "another-user",
        subscription.endpoint,
      );
      assert.equal(
        (await secondStore.listPushSubscriptions("postgres-user")).length,
        1,
        "another user must not be able to unsubscribe this browser",
      );
      await secondStore.deletePushSubscription(
        "postgres-user",
        subscription.endpoint,
      );
      assert.equal(
        (await secondStore.listPushSubscriptions("postgres-user")).length,
        0,
      );

      await secondStore.close();
      secondSql = undefined;
    } finally {
      await Promise.allSettled(
        [firstSql, secondSql]
          .filter((sql): sql is postgres.Sql => sql !== undefined)
          .map(async (sql) => sql.end({ timeout: 1 })),
      );
      try {
        if (schemaCreated) {
          await admin`DROP SCHEMA ${admin(schema)} CASCADE`;
        }
      } finally {
        await admin.end({ timeout: 5 });
      }
    }
  },
);

function scopedConnection(
  connectionString: string,
  schema: string,
): postgres.Sql {
  return postgres(connectionString, {
    max: 1,
    onnotice: () => {},
    connection: {
      application_name: "agent-platform-postgres-test",
      search_path: schema,
    },
  });
}
