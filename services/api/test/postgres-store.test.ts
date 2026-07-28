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
import {
  M4_DEVICE_COUNT,
  M4_SESSIONS_PER_DEVICE,
  makeM4Batch,
  makeM4Device,
} from "./m4-fixtures.js";
import { bindingCodeDigest } from "../src/store.js";

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
      const bindingDigest = bindingCodeDigest("postgres-persistent-code");
      await firstStore.createBindingCode(
        "postgres-user",
        bindingDigest,
        "2099-01-01T00:00:00.000Z",
      );
      const registration = await firstStore.registerDeviceWithBindingCode(
        bindingDigest,
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
      const retry = await secondStore.registerDeviceWithBindingCode(
        bindingDigest,
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

      assert.equal(
        await secondStore.revokeDevice("another-user", fixture.device_id),
        false,
      );
      assert.equal(
        await secondStore.revokeDevice("postgres-user", fixture.device_id),
        true,
      );
      const revokedDevices = await secondStore.listDevices("postgres-user");
      assert.equal(revokedDevices.length, 1);
      assert.equal(revokedDevices[0]?.device_id, fixture.device_id);
      assert.equal(
        revokedDevices[0]?.created_at,
        registration.device.created_at,
      );
      assert.equal(revokedDevices[0]?.online, false);
      assert.equal(revokedDevices[0]?.revoked, true);
      await assert.rejects(
        secondStore.ingestBatch(fixture),
        (error: unknown) =>
          typeof error === "object" &&
          error !== null &&
          "code" in error &&
          error.code === "device_revoked",
      );
      await assert.rejects(
        secondStore.registerDeviceWithBindingCode(
          bindingDigest,
          fixture.device_id,
          fixture.public_key,
        ),
        (error: unknown) =>
          typeof error === "object" &&
          error !== null &&
          "code" in error &&
          error.code === "device_revoked",
      );
      assert.equal(
        (await secondStore.listSessions("postgres-user", {})).length,
        1,
        "revocation must preserve existing session history",
      );

      const raceDigest = bindingCodeDigest("postgres-race-code");
      await secondStore.createBindingCode(
        "postgres-user",
        raceDigest,
        "2099-01-01T00:00:00.000Z",
      );
      const race = await Promise.allSettled([
        secondStore.registerDeviceWithBindingCode(
          raceDigest,
          "dev_00000000000000000000000000000001",
          "1".repeat(64),
        ),
        secondStore.registerDeviceWithBindingCode(
          raceDigest,
          "dev_00000000000000000000000000000002",
          "2".repeat(64),
        ),
      ]);
      assert.equal(
        race.filter((result) => result.status === "fulfilled").length,
        1,
      );
      const raceRejected = race.find((result) => result.status === "rejected");
      assert.ok(raceRejected);
      assert.equal(
        (raceRejected.reason as { code?: unknown }).code,
        "binding_code_used",
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

test(
  "PostgreSQL M4 workload isolates 12 devices and replays 3,000 sessions",
  { skip: databaseUrl === undefined, timeout: 120_000 },
  async () => {
    assert.ok(databaseUrl);
    const admin = postgres(databaseUrl, { max: 1, onnotice: () => {} });
    const schema = `agent_platform_m4_${randomUUID().replaceAll("-", "")}`;
    let sql: postgres.Sql | undefined;
    let schemaCreated = false;
    try {
      await admin`CREATE SCHEMA ${admin(schema)}`;
      schemaCreated = true;
      sql = scopedConnection(databaseUrl, schema, 4);
      await migratePostgres(sql);
      const store = new PostgresGatewayStore(sql);
      const devices = Array.from({ length: M4_DEVICE_COUNT }, (_, index) =>
        makeM4Device(
          index,
          index < 8 ? "postgres-load-owner-a" : "postgres-load-owner-b",
        ),
      );

      await Promise.all(
        devices.map((device) =>
          store.registerDevice(
            device.ownerId,
            device.deviceId,
            device.publicKey,
          ),
        ),
      );
      const batches = devices.map((device) =>
        makeM4Batch(device, M4_SESSIONS_PER_DEVICE),
      );
      const ingested = await Promise.all(
        batches.map((batch) => store.ingestBatch(batch)),
      );
      assert.ok(
        ingested.every((result) => result.accepted === M4_SESSIONS_PER_DEVICE),
      );
      assert.equal(
        (await store.listSessions("postgres-load-owner-a", {})).length,
        2_000,
      );
      assert.equal(
        (await store.listSessions("postgres-load-owner-b", {})).length,
        1_000,
      );

      const target = devices[0];
      assert.ok(target);
      assert.equal(
        (
          await store.listSessions("postgres-load-owner-a", {
            device_id: target.deviceId,
          })
        ).length,
        M4_SESSIONS_PER_DEVICE,
      );
      assert.equal(
        (
          await store.listSessions("postgres-load-owner-b", {
            device_id: target.deviceId,
          })
        ).length,
        0,
      );

      const replayed = await Promise.all(
        batches.map((batch) => store.ingestBatch(batch)),
      );
      assert.ok(replayed.every((result) => result.accepted === 0));
      assert.equal(
        (await store.listSessions("postgres-load-owner-a", {})).length,
        2_000,
      );

      await store.close();
      sql = undefined;
    } finally {
      if (sql !== undefined) {
        await sql.end({ timeout: 1 }).catch(() => undefined);
      }
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
  max = 1,
): postgres.Sql {
  return postgres(connectionString, {
    max,
    onnotice: () => {},
    connection: {
      application_name: "agent-platform-postgres-test",
      search_path: schema,
    },
  });
}
