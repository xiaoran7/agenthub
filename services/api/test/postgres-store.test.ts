import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { parseSyncBatch, type DeviceHeartbeat } from "@agent-pulse/contracts";
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
import { hashPassword, tokenDigest, verifyPassword } from "../src/accounts.js";

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
  "PostgreSQL account lifecycle persists verification, sessions, limits, and ordered billing",
  { skip: databaseUrl === undefined },
  async () => {
    assert.ok(databaseUrl);
    const admin = postgres(databaseUrl, { max: 1, onnotice: () => {} });
    const schema = `agent_pulse_accounts_${randomUUID().replaceAll("-", "")}`;
    let sql: postgres.Sql | undefined;
    let schemaCreated = false;
    try {
      await admin`CREATE SCHEMA ${admin(schema)}`;
      schemaCreated = true;
      sql = scopedConnection(databaseUrl, schema);
      await migratePostgres(sql);
      const store = new PostgresGatewayStore(sql);
      const userId = randomUUID();
      const account = await store.createAccount({
        userId,
        email: "postgres-account@example.com",
        displayName: "Postgres Account",
        passwordHash: await hashPassword("correct horse battery staple"),
      });
      assert.equal(account.user.email_verified_at, undefined);

      const issuedAt = "2026-08-13T12:00:00.000Z";
      const verificationToken = "v".repeat(43);
      const verificationDigest = tokenDigest(verificationToken);
      assert.deepEqual(
        await store.issueEmailVerification({
          digest: verificationDigest,
          userId,
          createdAt: issuedAt,
          expiresAt: "2026-08-14T12:00:00.000Z",
          minimumIntervalSeconds: 60,
        }),
        { allowed: true, retry_after_seconds: 0 },
      );
      const verified = await store.consumeEmailVerification(
        verificationDigest,
        "2026-08-13T12:01:00.000Z",
      );
      assert.equal(
        verified?.user.email_verified_at,
        "2026-08-13T12:01:00.000Z",
      );
      assert.equal(
        await store.consumeEmailVerification(
          verificationDigest,
          "2026-08-13T12:02:00.000Z",
        ),
        undefined,
      );

      const currentDigest = tokenDigest("a".repeat(43));
      const otherDigest = tokenDigest("b".repeat(43));
      const currentId = randomUUID();
      const otherId = randomUUID();
      for (const [sessionId, digest] of [
        [currentId, currentDigest],
        [otherId, otherDigest],
      ] as const) {
        await store.createWebSession({
          sessionId,
          digest,
          userId,
          createdAt: issuedAt,
          expiresAt: "2026-09-13T12:00:00.000Z",
        });
      }
      const sessions = await store.listWebSessions(
        userId,
        currentDigest,
        "2026-08-13T12:00:01.000Z",
      );
      assert.equal(sessions.length, 2);
      assert.equal(
        sessions.find((session) => session.current)?.session_id,
        currentId,
      );
      assert.equal(await store.revokeWebSession(randomUUID(), otherId), false);
      assert.equal(
        await store.revokeOtherWebSessions(userId, currentDigest),
        1,
      );

      const resetToken = "r".repeat(43);
      const resetDigest = tokenDigest(resetToken);
      assert.deepEqual(
        await store.issuePasswordReset({
          digest: resetDigest,
          userId,
          createdAt: issuedAt,
          expiresAt: "2026-08-13T13:00:00.000Z",
          minimumIntervalSeconds: 60,
        }),
        { allowed: true, retry_after_seconds: 0 },
      );
      const changedHash = await hashPassword("postgres changed password");
      assert.deepEqual(
        await store.consumePasswordReset({
          digest: resetDigest,
          passwordHash: changedHash,
          now: "2026-08-13T12:05:00.000Z",
        }),
        { updated: true, revoked_sessions: 1 },
      );
      assert.deepEqual(
        await store.consumePasswordReset({
          digest: resetDigest,
          passwordHash: changedHash,
          now: "2026-08-13T12:06:00.000Z",
        }),
        { updated: false, revoked_sessions: 0 },
      );
      assert.equal(
        await verifyPassword(
          "postgres changed password",
          (await store.findAccountByEmail("postgres-account@example.com"))
            ?.password_hash ?? "",
        ),
        true,
      );

      const scopeDigest = tokenDigest("c".repeat(43));
      for (let attempt = 0; attempt < 5; attempt += 1) {
        assert.equal(
          (
            await store.consumeLoginAttempt({
              scopeDigest,
              now: issuedAt,
              windowSeconds: 900,
              maximumAttempts: 5,
              blockSeconds: 900,
            })
          ).allowed,
          true,
        );
      }
      const blocked = await store.consumeLoginAttempt({
        scopeDigest,
        now: issuedAt,
        windowSeconds: 900,
        maximumAttempts: 5,
        blockSeconds: 900,
      });
      assert.deepEqual(blocked, { allowed: false, retry_after_seconds: 900 });
      await store.clearLoginAttempts(scopeDigest);

      const newer = {
        event_id: randomUUID(),
        user_id: userId,
        provider: "postgres-adapter",
        provider_customer_id: "customer-postgres",
        provider_subscription_id: "subscription-postgres",
        plan: "team" as const,
        status: "active" as const,
        current_period_end: "2026-09-13T12:00:00.000Z",
        occurred_at: "2026-08-13T12:10:00.000Z",
      };
      assert.equal((await store.applySubscriptionEvent(newer)).applied, true);
      assert.equal((await store.applySubscriptionEvent(newer)).duplicate, true);
      const older = {
        ...newer,
        event_id: randomUUID(),
        plan: "free" as const,
        status: "expired" as const,
        occurred_at: "2026-08-13T12:09:00.000Z",
      };
      assert.equal((await store.applySubscriptionEvent(older)).applied, false);
      assert.equal(
        (await store.accountView(userId))?.subscription.plan,
        "team",
      );
      const deviceId = `dev_${"1".repeat(32)}`;
      await store.registerDevice(userId, deviceId, "2".repeat(64), userId);
      const taskCreatedAt = new Date();
      const taskExpiresAt = new Date(taskCreatedAt.getTime() + 3_600_000);
      const remoteTask = await store.createRemoteTask(
        userId,
        {
          task_id: randomUUID(),
          target_device_id: deviceId,
          agent_type: "codex",
          prompt: "postgres audit fixture",
          idempotency_key: "postgres-audit-task",
          execution_policy: "local_confirmation",
          expires_in_seconds: 3_600,
          created_at: taskCreatedAt.toISOString(),
          expires_at: taskExpiresAt.toISOString(),
          max_concurrent_tasks: 10,
        },
        userId,
      );
      await store.cancelRemoteTask(userId, remoteTask.task.task_id, userId);
      const audit = await store.listAuditEvents({
        scope: "account",
        actorUserId: userId,
        workspaceOwnerId: userId,
        limit: 100,
      });
      assert.ok(audit.some((event) => event.action === "account.created"));
      assert.ok(
        audit.some((event) => event.action === "auth.password_reset_completed"),
      );
      assert.equal(
        audit.filter((event) => event.action === "subscription.updated").length,
        1,
      );
      assert.ok(audit.some((event) => event.action === "device.registered"));
      assert.ok(audit.some((event) => event.action === "task.created"));
      assert.ok(audit.some((event) => event.action === "task.canceled"));
      assert.equal(
        JSON.stringify(audit).includes("postgres-account@example.com"),
        false,
      );
      const migration = await sql<
        Array<{ version: string | number }>
      >`SELECT version FROM agent_platform_schema_migrations WHERE version = 13`;
      assert.equal(Number(migration[0]?.version), 13);

      await store.close();
      sql = undefined;
    } finally {
      if (sql !== undefined)
        await sql.end({ timeout: 1 }).catch(() => undefined);
      try {
        if (schemaCreated) await admin`DROP SCHEMA ${admin(schema)} CASCADE`;
      } finally {
        await admin.end({ timeout: 5 });
      }
    }
  },
);

test(
  "PostgreSQL team workspaces persist invitations, roles, and session selection",
  { skip: databaseUrl === undefined },
  async () => {
    assert.ok(databaseUrl);
    const admin = postgres(databaseUrl, { max: 1, onnotice: () => {} });
    const schema = `agent_pulse_team_${randomUUID().replaceAll("-", "")}`;
    let sql: postgres.Sql | undefined;
    let schemaCreated = false;
    try {
      await admin`CREATE SCHEMA ${admin(schema)}`;
      schemaCreated = true;
      sql = scopedConnection(databaseUrl, schema);
      await migratePostgres(sql);
      const store = new PostgresGatewayStore(sql);
      const ownerId = randomUUID();
      const memberId = randomUUID();
      const passwordHash = await hashPassword("correct horse battery staple");
      await store.createAccount({
        userId: ownerId,
        email: "postgres-team-owner@example.com",
        displayName: "Postgres Team Owner",
        passwordHash,
      });
      await store.createAccount({
        userId: memberId,
        email: "postgres-team-member@example.com",
        displayName: "Postgres Team Member",
        passwordHash,
      });
      await store.applySubscriptionEvent({
        event_id: randomUUID(),
        user_id: ownerId,
        provider: "postgres-test",
        provider_customer_id: "customer-team",
        provider_subscription_id: "subscription-team",
        plan: "team",
        status: "active",
        current_period_end: "2026-09-13T12:00:00.000Z",
        occurred_at: "2026-08-13T12:00:00.000Z",
      });
      const invitationToken = "t".repeat(43);
      await store.issueTeamInvitation({
        invitationId: randomUUID(),
        digest: tokenDigest(invitationToken),
        workspaceOwnerId: ownerId,
        invitedUserId: memberId,
        invitedEmail: "postgres-team-member@example.com",
        role: "member",
        invitedBy: ownerId,
        createdAt: "2026-08-13T12:01:00.000Z",
        expiresAt: "2026-08-16T12:01:00.000Z",
        memberLimit: 10,
      });
      assert.equal(
        await store.consumeTeamInvitation({
          digest: tokenDigest(invitationToken),
          userId: memberId,
          now: "2026-08-13T12:02:00.000Z",
        }),
        ownerId,
      );
      const sessionDigest = tokenDigest("s".repeat(43));
      await store.createWebSession({
        sessionId: randomUUID(),
        digest: sessionDigest,
        userId: memberId,
        createdAt: "2026-08-13T12:03:00.000Z",
        expiresAt: "2026-09-13T12:03:00.000Z",
      });
      const switched = await store.switchWebSessionWorkspace({
        digest: sessionDigest,
        actorUserId: memberId,
        workspaceOwnerId: ownerId,
      });
      assert.equal(switched?.workspace_owner_id, ownerId);
      assert.equal(
        (await store.findWebSession(sessionDigest, "2026-08-13T12:04:00.000Z"))
          ?.actor_user_id,
        memberId,
      );
      const team = await store.workspaceView(
        memberId,
        ownerId,
        10,
        "2026-08-13T12:04:00.000Z",
      );
      assert.equal(team?.workspace.role, "member");
      assert.equal(team?.members.length, 2);
      const memberAudit = await store.listAuditEvents({
        scope: "workspace",
        actorUserId: memberId,
        workspaceOwnerId: ownerId,
        limit: 100,
      });
      assert.deepEqual(
        memberAudit.map((event) => event.action),
        ["team.invitation_accepted", "team.invitation_created"],
      );
      assert.equal(
        await store.updateWorkspaceMember({
          actorUserId: ownerId,
          workspaceOwnerId: ownerId,
          memberUserId: memberId,
          remove: true,
        }),
        true,
      );
      const ownerAudit = await store.listAuditEvents({
        scope: "workspace",
        actorUserId: ownerId,
        workspaceOwnerId: ownerId,
        limit: 100,
      });
      assert.equal(ownerAudit[0]?.action, "team.member_removed");
      assert.deepEqual(
        await store.listAuditEvents({
          scope: "workspace",
          actorUserId: memberId,
          workspaceOwnerId: ownerId,
          limit: 100,
        }),
        [],
      );
      assert.equal(
        (await store.findWebSession(sessionDigest, "2026-08-13T12:05:00.000Z"))
          ?.workspace_owner_id,
        memberId,
      );
      await store.close();
      sql = undefined;
    } finally {
      if (sql !== undefined) {
        await sql.end({ timeout: 1 }).catch(() => undefined);
      }
      try {
        if (schemaCreated) await admin`DROP SCHEMA ${admin(schema)} CASCADE`;
      } finally {
        await admin.end({ timeout: 5 });
      }
    }
  },
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
      const heartbeat: DeviceHeartbeat = {
        protocol_version: 1,
        device_id: fixture.device_id,
        public_key: fixture.public_key,
        sent_at: "2026-08-14T00:00:01.000Z",
        signature: "0".repeat(128),
      };
      assert.deepEqual(
        await firstStore.recordHeartbeat(heartbeat, "2026-08-14T00:00:02.000Z"),
        {
          accepted: true,
          owner_id: "postgres-user",
          last_seen_at: "2026-08-14T00:00:02.000Z",
        },
      );
      assert.equal(
        (
          await firstStore.recordHeartbeat(
            heartbeat,
            "2026-08-14T00:00:03.000Z",
          )
        ).accepted,
        false,
      );
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
      application_name: "agent-pulse-postgres-test",
      search_path: schema,
    },
  });
}
