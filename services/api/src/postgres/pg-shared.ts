import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";

import type { ActivityStatus } from "@agent-pulse/contracts";
import type postgres from "postgres";

import {
  accountView,
  auditMetadata,
  type AccountUser,
  type AccountView,
  type AuditRecordInput,
  type SubscriptionRecord,
} from "../accounts.js";
import { MIGRATIONS } from "../migrations-list.js";
import {
  normalizeAccountUser,
  normalizeSubscription,
} from "../postgres-helpers.js";

export interface ExistingEventRow {
  event_id: string;
  device_id: string;
  sequence: string | number;
  event_data: unknown;
}

export interface BindingCodeRow {
  owner_id: string;
  expired: boolean;
  claimed_device_id: string | null;
  issued_by: string | null;
}

export interface SessionRow {
  event_data: unknown;
  received_at: unknown;
}

export interface AuthSnapshotRow {
  device_id: string;
  agent_type: string;
  checked_at: unknown;
  received_at: unknown;
  snapshot_data: unknown;
}

export interface SessionStatusRow {
  agent_type: string;
  agent_session_id: string;
  activity_status: ActivityStatus;
  event_data: unknown;
}

export type QuerySql = postgres.Sql | postgres.TransactionSql;

/**
 * How stale `web_sessions.last_seen_at` may get before authentication refreshes
 * it. The web console polls roughly every 1.5s, so writing per request would
 * multiply a cheap read into thousands of row versions a day. The displayed
 * "recent activity" clock tolerates this much lag; session expiry and
 * membership are still evaluated on every request.
 */
export const SESSION_ACTIVITY_REFRESH_SECONDS = 60;

export async function migratePostgres(sql: postgres.Sql): Promise<void> {
  await sql.begin(async (transaction) => {
    await transaction`SELECT pg_advisory_xact_lock(1701733484, 1886152045)`;
    await transaction.unsafe(`
      CREATE TABLE IF NOT EXISTS agent_platform_schema_migrations (
        version integer PRIMARY KEY,
        checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
        applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
      )
    `);
    for (const migration of MIGRATIONS) {
      const source = await readFile(migration.url, "utf8");
      // Normalize line endings before hashing: a CRLF checkout on Windows and
      // an LF checkout on the deploy host must agree on the checksum.
      const checksum = createHash("sha256")
        .update(source.replaceAll("\r\n", "\n"))
        .digest("hex");
      const existing = await transaction<
        Array<{ version: number; checksum: string }>
      >`
        SELECT version, checksum
        FROM agent_platform_schema_migrations
        WHERE version = ${migration.version}
      `;
      if (existing.length > 0) {
        if (existing[0]?.checksum !== checksum) {
          throw new Error(
            `database migration ${migration.version} checksum mismatch`,
          );
        }
        continue;
      }
      await transaction.unsafe(source);
      await transaction`
        INSERT INTO agent_platform_schema_migrations (version, checksum)
        VALUES (${migration.version}, ${checksum})
      `;
    }
  });
}

export async function insertAuditEvent(
  sql: QuerySql,
  input: AuditRecordInput,
): Promise<void> {
  if (
    (input.scope === "account" &&
      (input.accountUserId === undefined ||
        input.workspaceOwnerId !== undefined)) ||
    (input.scope === "workspace" &&
      (input.workspaceOwnerId === undefined ||
        input.accountUserId !== undefined)) ||
    (input.targetType === undefined) !== (input.targetId === undefined)
  ) {
    throw new Error("audit event scope or target is invalid");
  }
  const metadata = auditMetadata(input.metadata);
  await sql`
    INSERT INTO agent_pulse_audit_events (
      event_id, scope, account_user_id, workspace_owner_id, actor_user_id,
      action, target_type, target_id, metadata, created_at
    ) VALUES (
      ${randomUUID()}, ${input.scope}, ${input.accountUserId ?? null},
      ${input.workspaceOwnerId ?? null}, ${input.actorUserId ?? null},
      ${input.action}, ${input.targetType ?? null}, ${input.targetId ?? null},
      ${sql.json(metadata as postgres.JSONValue)},
      COALESCE(${input.createdAt ?? null}::timestamptz, clock_timestamp())
    )
  `;
}

export async function insertWorkspaceAuditIfVisible(
  sql: QuerySql,
  input: Omit<AuditRecordInput, "scope"> & {
    workspaceOwnerId: string;
  },
): Promise<boolean> {
  if (!UUID_RE.test(input.workspaceOwnerId)) return false;
  if (input.actorUserId !== undefined && !UUID_RE.test(input.actorUserId)) {
    return false;
  }
  const owners = await sql`
    SELECT 1 FROM agent_pulse_users
    WHERE user_id = ${input.workspaceOwnerId}::uuid
    LIMIT 1
  `;
  if (owners.length === 0) return false;
  if (input.actorUserId !== undefined) {
    const actors = await sql`
      SELECT 1 FROM agent_pulse_users
      WHERE user_id = ${input.actorUserId}::uuid
      LIMIT 1
    `;
    if (actors.length === 0) return false;
  }
  await insertAuditEvent(sql, { ...input, scope: "workspace" });
  return true;
}

export const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export async function postgresAccountView(
  sql: QuerySql,
  userId: string,
): Promise<AccountView | undefined> {
  if (!UUID_RE.test(userId)) return undefined;
  const rows = await sql<Array<AccountUser & SubscriptionRecord>>`
    SELECT
      users.user_id::text,
      users.email,
      users.display_name,
      users.password_hash,
      users.status,
      users.email_verified_at,
      users.oauth_provider,
      users.oauth_sub,
      users.avatar_data_url,
      users.created_at,
      subscriptions.plan,
      subscriptions.status AS subscription_status,
      subscriptions.provider,
      subscriptions.provider_customer_id,
      subscriptions.provider_subscription_id,
      subscriptions.current_period_end,
      subscriptions.last_event_at,
      subscriptions.last_event_id::text
    FROM agent_pulse_users AS users
    INNER JOIN agent_pulse_subscriptions AS subscriptions
      ON subscriptions.user_id = users.user_id
    WHERE users.user_id = ${userId}
    LIMIT 1
  `;
  const row = rows[0] as
    | (AccountUser &
        SubscriptionRecord & {
          subscription_status?: SubscriptionRecord["status"];
        })
    | undefined;
  if (row === undefined || row.status !== "active") return undefined;
  return accountView(
    normalizeAccountUser(row),
    normalizeSubscription({
      plan: row.plan,
      status: row.subscription_status ?? row.status,
      ...(row.current_period_end === undefined ||
      row.current_period_end === null
        ? {}
        : { current_period_end: row.current_period_end }),
      ...(row.provider === undefined || row.provider === null
        ? {}
        : { provider: row.provider }),
      ...(row.provider_customer_id === undefined ||
      row.provider_customer_id === null
        ? {}
        : { provider_customer_id: row.provider_customer_id }),
      ...(row.provider_subscription_id === undefined ||
      row.provider_subscription_id === null
        ? {}
        : { provider_subscription_id: row.provider_subscription_id }),
      ...(row.last_event_at === undefined || row.last_event_at === null
        ? {}
        : { last_event_at: row.last_event_at }),
      ...(row.last_event_id === undefined || row.last_event_id === null
        ? {}
        : { last_event_id: row.last_event_id }),
    }),
  );
}
