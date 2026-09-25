import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";

import type {
  ActivityStatus,
  AuthSnapshot,
  AuthSyncBatch,
  DeviceHeartbeat,
  SyncBatch,
  SyncEvent,
} from "@agent-pulse/contracts";
import postgres from "postgres";

import {
  AccountError,
  DUMMY_PASSWORD_HASH,
  accountView,
  auditMetadata,
  entitlementsFor,
  isNewerSubscriptionEvent,
  type AccountStore,
  type AccountUser,
  type AccountView,
  type AuditEvent,
  type AuditMetadataValue,
  type AuditQuery,
  type AuditRecordInput,
  type BillingReference,
  type InviteRole,
  type LoginAttemptResult,
  type SessionIdentity,
  type SubscriptionEvent,
  type SubscriptionEventResult,
  type SubscriptionRecord,
  type WebSessionView,
  type WorkspaceMemberView,
  type WorkspaceRole,
  type WorkspaceSummary,
  type TeamWorkspaceView,
} from "./accounts.js";
import type { PushSubscription } from "./push.js";
import {
  GatewayStoreError,
  sessionStatusChange,
  type AuthFilters,
  type AuthIngestResult,
  type DeviceRegistrationResult,
  type DeviceView,
  type GatewayStore,
  type IngestResult,
  type HeartbeatResult,
  type RegisteredDevice,
  type SessionEventFilters,
  type SessionFilters,
  type SessionStatusChange,
  type StoredAuthSnapshot,
  type StoredSession,
} from "./store.js";

const MIGRATIONS = [
  {
    version: 1,
    url: new URL("../migrations/0001_gateway.sql", import.meta.url),
  },
  {
    version: 2,
    url: new URL(
      "../migrations/0002_session_timeline_index.sql",
      import.meta.url,
    ),
  },
  {
    version: 3,
    url: new URL("../migrations/0003_push_subscriptions.sql", import.meta.url),
  },
  {
    version: 4,
    url: new URL("../migrations/0004_device_lifecycle.sql", import.meta.url),
  },
  {
    version: 5,
    url: new URL("../migrations/0005_device_auth.sql", import.meta.url),
  },
  {
    version: 6,
    url: new URL(
      "../migrations/0006_accounts_subscriptions.sql",
      import.meta.url,
    ),
  },
  {
    version: 7,
    url: new URL("../migrations/0007_remote_tasks.sql", import.meta.url),
  },
  {
    version: 8,
    url: new URL("../migrations/0008_account_lifecycle.sql", import.meta.url),
  },
  {
    version: 9,
    url: new URL("../migrations/0009_password_lifecycle.sql", import.meta.url),
  },
  {
    version: 10,
    url: new URL("../migrations/0010_team_workspaces.sql", import.meta.url),
  },
  {
    version: 11,
    url: new URL("../migrations/0011_audit_events.sql", import.meta.url),
  },
  {
    version: 12,
    url: new URL("../migrations/0012_device_heartbeat.sql", import.meta.url),
  },
  {
    version: 13,
    url: new URL("../migrations/0013_device_task_audit.sql", import.meta.url),
  },
  {
    version: 14,
    url: new URL(
      "../migrations/0014_remote_task_receipts.sql",
      import.meta.url,
    ),
  },
  {
    version: 15,
    url: new URL("../migrations/0015_oauth_accounts.sql", import.meta.url),
  },
];

interface DeviceRow {
  device_id: string;
  public_key: string;
  owner_id: string;
  created_at: unknown;
  last_seen_at: unknown;
  last_sequence: string | number;
  revoked_at: unknown | null;
  last_heartbeat_at?: unknown | null;
}

interface BindingCodeRow {
  owner_id: string;
  expired: boolean;
  claimed_device_id: string | null;
  issued_by: string | null;
}

interface ExistingEventRow {
  event_id: string;
  device_id: string;
  sequence: string | number;
  event_data: unknown;
}

interface SessionRow {
  event_data: unknown;
  received_at: unknown;
}

interface AuthSnapshotRow {
  device_id: string;
  agent_type: string;
  checked_at: unknown;
  received_at: unknown;
  snapshot_data: unknown;
}

interface SessionStatusRow {
  agent_type: string;
  agent_session_id: string;
  activity_status: ActivityStatus;
  event_data: unknown;
}

interface AuditEventRow {
  event_id: string;
  scope: AuditEvent["scope"];
  actor_user_id: string | null;
  workspace_owner_id: string | null;
  action: AuditEvent["action"];
  target_type: Exclude<AuditEvent["target_type"], undefined> | null;
  target_id: string | null;
  metadata: unknown;
  created_at: unknown;
}

type QuerySql = postgres.Sql | postgres.TransactionSql;

/**
 * How stale `web_sessions.last_seen_at` may get before authentication refreshes
 * it. The web console polls roughly every 1.5s, so writing per request would
 * multiply a cheap read into thousands of row versions a day. The displayed
 * "recent activity" clock tolerates this much lag; session expiry and
 * membership are still evaluated on every request.
 */
const SESSION_ACTIVITY_REFRESH_SECONDS = 60;

export class PostgresGatewayStore implements GatewayStore, AccountStore {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  static async connect(databaseUrl: string): Promise<PostgresGatewayStore> {
    const sql = postgres(databaseUrl, {
      max: 10,
      connect_timeout: 10,
      idle_timeout: 20,
      onnotice: () => {},
      connection: { application_name: "agent-pulse-api" },
    });
    try {
      await migratePostgres(sql);
      return new PostgresGatewayStore(sql);
    } catch (error) {
      await sql.end({ timeout: 1 });
      throw error;
    }
  }

  async createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
  }): Promise<AccountView> {
    try {
      return await this.#sql.begin(async (sql) => {
        const rows = await sql<AccountUser[]>`
          INSERT INTO agent_pulse_users (
            user_id,
            email,
            password_hash,
            display_name
          )
          VALUES (
            ${input.userId},
            ${input.email},
            ${input.passwordHash},
            ${input.displayName}
          )
          RETURNING
            user_id::text,
            email,
            display_name,
            password_hash,
            status,
            email_verified_at,
            created_at
        `;
        const user = rows[0];
        if (user === undefined)
          throw new Error("account insert returned no row");
        const subscriptions = await sql<SubscriptionRecord[]>`
          INSERT INTO agent_pulse_subscriptions (user_id)
          VALUES (${input.userId})
          RETURNING plan, status, current_period_end
        `;
        const subscription = subscriptions[0];
        if (subscription === undefined) {
          throw new Error("subscription insert returned no row");
        }
        await sql`
          INSERT INTO agent_pulse_workspace_members (
            workspace_owner_id, user_id, role
          ) VALUES (${input.userId}, ${input.userId}, 'owner')
        `;
        const normalizedUser = normalizeAccountUser(user);
        await insertAuditEvent(sql, {
          scope: "account",
          accountUserId: input.userId,
          actorUserId: input.userId,
          action: "account.created",
          targetType: "account",
          targetId: input.userId,
          metadata: { plan: "free" },
          createdAt: normalizedUser.created_at,
        });
        return accountView(normalizedUser, normalizeSubscription(subscription));
      });
    } catch (error) {
      if (postgresConstraint(error) === "agent_pulse_users_email_unique_idx") {
        throw new AccountError(
          "email_already_registered",
          "email is already registered",
          409,
        );
      }
      throw error;
    }
  }

  async findAccountByEmail(email: string): Promise<AccountUser | undefined> {
    const rows = await this.#sql<AccountUser[]>`
      SELECT
        user_id::text,
        email,
        display_name,
        password_hash,
        status,
        email_verified_at,
        oauth_provider,
        oauth_sub,
        created_at
      FROM agent_pulse_users
      WHERE lower(email) = ${email}
      LIMIT 1
    `;
    return rows[0] === undefined ? undefined : normalizeAccountUser(rows[0]);
  }

  async findAccountByOAuth(
    provider: string,
    sub: string,
  ): Promise<AccountUser | undefined> {
    const rows = await this.#sql<AccountUser[]>`
      SELECT
        user_id::text,
        email,
        display_name,
        password_hash,
        status,
        email_verified_at,
        oauth_provider,
        oauth_sub,
        created_at
      FROM agent_pulse_users
      WHERE oauth_provider = ${provider} AND oauth_sub = ${sub}
      LIMIT 1
    `;
    return rows[0] === undefined ? undefined : normalizeAccountUser(rows[0]);
  }

  async upsertOAuthAccount(input: {
    provider: string;
    sub: string;
    email: string;
    displayName: string;
    emailVerified?: boolean;
    now?: string;
  }): Promise<AccountView> {
    const timestamp = input.now ?? new Date().toISOString();
    return this.#sql.begin(async (sql) => {
      const byOAuth = await sql<AccountUser[]>`
        SELECT
          user_id::text,
          email,
          display_name,
          password_hash,
          status,
          email_verified_at,
          oauth_provider,
          oauth_sub,
          created_at
        FROM agent_pulse_users
        WHERE oauth_provider = ${input.provider} AND oauth_sub = ${input.sub}
        LIMIT 1
        FOR UPDATE
      `;
      if (byOAuth[0] !== undefined) {
        const user = byOAuth[0];
        if (input.emailVerified && !user.email_verified_at) {
          await sql`
            UPDATE agent_pulse_users
            SET email_verified_at = ${timestamp}::timestamptz, updated_at = clock_timestamp()
            WHERE user_id = ${user.user_id}
          `;
        }
        if (input.displayName && user.display_name !== input.displayName) {
          await sql`
            UPDATE agent_pulse_users
            SET display_name = ${input.displayName}, updated_at = clock_timestamp()
            WHERE user_id = ${user.user_id}
          `;
        }
        const view = await postgresAccountView(sql, user.user_id);
        if (view !== undefined) return view;
      }

      const byEmail = await sql<AccountUser[]>`
        SELECT
          user_id::text,
          email,
          display_name,
          password_hash,
          status,
          email_verified_at,
          oauth_provider,
          oauth_sub,
          created_at
        FROM agent_pulse_users
        WHERE lower(email) = ${input.email}
        LIMIT 1
        FOR UPDATE
      `;
      if (byEmail[0] !== undefined) {
        const user = byEmail[0];
        await sql`
          UPDATE agent_pulse_users
          SET oauth_provider = ${input.provider},
              oauth_sub = ${input.sub},
              email_verified_at = COALESCE(email_verified_at, ${input.emailVerified ? timestamp : null}::timestamptz),
              updated_at = clock_timestamp()
          WHERE user_id = ${user.user_id}
        `;
        const view = await postgresAccountView(sql, user.user_id);
        if (view !== undefined) return view;
      }

      const userId = randomUUID();
      const newUser = await sql<AccountUser[]>`
        INSERT INTO agent_pulse_users (
          user_id, email, password_hash, display_name, status, oauth_provider, oauth_sub, email_verified_at, created_at, updated_at
        ) VALUES (
          ${userId}, ${input.email}, ${DUMMY_PASSWORD_HASH}, ${input.displayName || input.email.split("@")[0] || "User"},
          'active', ${input.provider}, ${input.sub}, ${input.emailVerified ? timestamp : null}::timestamptz, ${timestamp}::timestamptz, ${timestamp}::timestamptz
        )
        RETURNING user_id::text, email, display_name, password_hash, status, email_verified_at, oauth_provider, oauth_sub, created_at
      `;
      const normalizedUser = normalizeAccountUser(newUser[0]!);
      const subscription = await sql<SubscriptionRecord[]>`
        INSERT INTO agent_pulse_subscriptions (
          user_id, plan, status, created_at, updated_at
        ) VALUES (
          ${userId}, 'free', 'active', ${timestamp}::timestamptz, ${timestamp}::timestamptz
        )
        RETURNING user_id::text, plan, status, provider, provider_customer_id, provider_subscription_id, current_period_end, created_at, updated_at
      `;
      await sql`
        INSERT INTO agent_pulse_workspace_members (
          workspace_owner_id, user_id, role, created_at
        ) VALUES (
          ${userId}, ${userId}, 'owner', ${timestamp}::timestamptz
        )
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: userId,
        actorUserId: userId,
        action: "account.created",
        targetType: "account",
        targetId: userId,
        metadata: { plan: "free", reason: "oauth", provider: input.provider },
        createdAt: timestamp,
      });
      return accountView(
        normalizedUser,
        normalizeSubscription(subscription[0]!),
      );
    });
  }

  async accountView(userId: string): Promise<AccountView | undefined> {
    return postgresAccountView(this.#sql, userId);
  }

  async createWebSession(input: {
    sessionId: string;
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    reason?: "registration" | "login";
  }): Promise<void> {
    await this.#sql.begin(async (sql) => {
      await sql`
        INSERT INTO agent_pulse_web_sessions (
          session_id,
          session_digest,
          user_id,
          workspace_owner_id,
          created_at,
          last_seen_at,
          expires_at
        )
        VALUES (
          ${input.sessionId}, ${input.digest}, ${input.userId}, ${input.userId},
          ${input.createdAt}, ${input.createdAt}, ${input.expiresAt}
        )
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: input.userId,
        actorUserId: input.userId,
        action: "auth.session_created",
        targetType: "web_session",
        targetId: input.sessionId,
        metadata: { reason: input.reason ?? "login" },
        createdAt: input.createdAt,
      });
    });
  }

  async findWebSession(
    digest: string,
    now: string,
  ): Promise<SessionIdentity | undefined> {
    // Session validity is re-evaluated on every request, but `last_seen_at` only
    // feeds the "recent activity" display. Writing it per request turns a 1.5s
    // console poll into a row version and a WAL record per poll, so the
    // timestamp only advances once per SESSION_ACTIVITY_REFRESH_SECONDS.
    //
    // The refresh is a data-modifying CTE that matches nothing once it is
    // fresh, so the common path costs no write while the read below still
    // performs the full expiry and membership check on every single request.
    // One statement, one round trip.
    const rows = await this.#sql<
      Array<{
        actor_user_id: string;
        workspace_owner_id: string;
        role: WorkspaceRole;
      }>
    >`
      WITH refreshed AS (
        UPDATE agent_pulse_web_sessions AS sessions
        SET last_seen_at = clock_timestamp()
        WHERE sessions.session_digest = ${digest}
          AND sessions.expires_at > ${now}
          AND sessions.last_seen_at <=
            clock_timestamp() - interval '${SESSION_ACTIVITY_REFRESH_SECONDS} seconds'
        RETURNING sessions.session_digest
      )
      SELECT
        sessions.user_id::text AS actor_user_id,
        sessions.workspace_owner_id::text,
        members.role
      FROM agent_pulse_web_sessions AS sessions
      INNER JOIN agent_pulse_users AS users
        ON users.user_id = sessions.user_id
      INNER JOIN agent_pulse_workspace_members AS members
        ON members.user_id = users.user_id
        AND members.workspace_owner_id = sessions.workspace_owner_id
      WHERE sessions.session_digest = ${digest}
        AND sessions.expires_at > ${now}
        AND users.status = 'active'
      LIMIT 1
    `;
    return rows[0];
  }

  async deleteWebSession(digest: string): Promise<void> {
    await this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ session_id: string; user_id: string }>>`
        DELETE FROM agent_pulse_web_sessions
        WHERE session_digest = ${digest}
        RETURNING session_id::text, user_id::text
      `;
      const session = rows[0];
      if (session === undefined) return;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: session.user_id,
        actorUserId: session.user_id,
        action: "auth.session_revoked",
        targetType: "web_session",
        targetId: session.session_id,
        metadata: { reason: "logout" },
      });
    });
  }

  async listWebSessions(
    userId: string,
    currentDigest: string,
    now: string,
  ): Promise<WebSessionView[]> {
    const rows = await this.#sql<
      Array<{
        session_id: string;
        created_at: unknown;
        last_seen_at: unknown;
        expires_at: unknown;
        current: boolean;
      }>
    >`
      SELECT
        session_id::text,
        created_at,
        last_seen_at,
        expires_at,
        session_digest = ${currentDigest} AS current
      FROM agent_pulse_web_sessions
      WHERE user_id = ${userId} AND expires_at > ${now}
      ORDER BY last_seen_at DESC
    `;
    return rows.map((row) => ({
      session_id: row.session_id,
      created_at: isoTimestamp(row.created_at),
      last_seen_at: isoTimestamp(row.last_seen_at),
      expires_at: isoTimestamp(row.expires_at),
      current: row.current,
    }));
  }

  async revokeWebSession(userId: string, sessionId: string): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ session_id: string }>>`
        DELETE FROM agent_pulse_web_sessions
        WHERE user_id = ${userId} AND session_id = ${sessionId}
        RETURNING session_id::text
      `;
      if (rows.length === 0) return false;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: userId,
        actorUserId: userId,
        action: "auth.session_revoked",
        targetType: "web_session",
        targetId: sessionId,
        metadata: { reason: "manual" },
      });
      return true;
    });
  }

  async revokeOtherWebSessions(
    userId: string,
    currentDigest: string,
  ): Promise<number> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ session_id: string }>>`
        DELETE FROM agent_pulse_web_sessions
        WHERE user_id = ${userId} AND session_digest <> ${currentDigest}
        RETURNING session_id::text
      `;
      if (rows.length > 0) {
        await insertAuditEvent(sql, {
          scope: "account",
          accountUserId: userId,
          actorUserId: userId,
          action: "auth.other_sessions_revoked",
          metadata: { revoked_sessions: rows.length },
        });
      }
      return rows.length;
    });
  }

  async issueEmailVerification(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    return this.#sql.begin(async (sql) => {
      const users = await sql<Array<{ email_verified_at: unknown | null }>>`
        SELECT email_verified_at
        FROM agent_pulse_users
        WHERE user_id = ${input.userId} AND status = 'active'
        FOR UPDATE
      `;
      const user = users[0];
      if (user === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          403,
        );
      }
      if (user.email_verified_at !== null) {
        return { allowed: false, retry_after_seconds: 0 };
      }
      const latest = await sql<Array<{ created_at: unknown }>>`
        SELECT created_at
        FROM agent_pulse_email_verifications
        WHERE user_id = ${input.userId}
        ORDER BY created_at DESC
        LIMIT 1
      `;
      const nextAllowedAt =
        latest[0] === undefined
          ? 0
          : Date.parse(isoTimestamp(latest[0].created_at)) +
            input.minimumIntervalSeconds * 1_000;
      if (nextAllowedAt > Date.parse(input.createdAt)) {
        return {
          allowed: false,
          retry_after_seconds: Math.ceil(
            (nextAllowedAt - Date.parse(input.createdAt)) / 1_000,
          ),
        };
      }
      await sql`
        UPDATE agent_pulse_email_verifications
        SET consumed_at = ${input.createdAt}
        WHERE user_id = ${input.userId} AND consumed_at IS NULL
      `;
      await sql`
        INSERT INTO agent_pulse_email_verifications (
          token_digest, user_id, created_at, expires_at
        ) VALUES (
          ${input.digest}, ${input.userId}, ${input.createdAt}, ${input.expiresAt}
        )
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: input.userId,
        actorUserId: input.userId,
        action: "auth.email_verification_requested",
        metadata: {},
        createdAt: input.createdAt,
      });
      return { allowed: true, retry_after_seconds: 0 };
    });
  }

  async consumeEmailVerification(
    digest: string,
    now: string,
  ): Promise<AccountView | undefined> {
    const userId = await this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ user_id: string }>>`
        UPDATE agent_pulse_email_verifications
        SET consumed_at = ${now}
        WHERE token_digest = ${digest}
          AND consumed_at IS NULL
          AND expires_at > ${now}
        RETURNING user_id::text
      `;
      const id = rows[0]?.user_id;
      if (id === undefined) return undefined;
      await sql`
        UPDATE agent_pulse_users
        SET email_verified_at = COALESCE(email_verified_at, ${now}),
            updated_at = clock_timestamp()
        WHERE user_id = ${id} AND status = 'active'
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: id,
        actorUserId: id,
        action: "auth.email_verified",
        targetType: "account",
        targetId: id,
        metadata: {},
        createdAt: now,
      });
      return id;
    });
    return userId === undefined ? undefined : this.accountView(userId);
  }

  async deletePendingEmailVerification(digest: string): Promise<void> {
    await this.#sql`
      DELETE FROM agent_pulse_email_verifications
      WHERE token_digest = ${digest} AND consumed_at IS NULL
    `;
  }

  async issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    return this.#sql.begin(async (sql) => {
      const users = await sql<Array<{ user_id: string }>>`
        SELECT user_id::text
        FROM agent_pulse_users
        WHERE user_id = ${input.userId} AND status = 'active'
        FOR UPDATE
      `;
      if (users[0] === undefined) {
        return { allowed: false, retry_after_seconds: 0 };
      }
      const latest = await sql<Array<{ created_at: unknown }>>`
        SELECT created_at
        FROM agent_pulse_password_resets
        WHERE user_id = ${input.userId}
        ORDER BY created_at DESC
        LIMIT 1
      `;
      const nextAllowedAt =
        latest[0] === undefined
          ? 0
          : Date.parse(isoTimestamp(latest[0].created_at)) +
            input.minimumIntervalSeconds * 1_000;
      if (nextAllowedAt > Date.parse(input.createdAt)) {
        return {
          allowed: false,
          retry_after_seconds: Math.ceil(
            (nextAllowedAt - Date.parse(input.createdAt)) / 1_000,
          ),
        };
      }
      await sql`
        UPDATE agent_pulse_password_resets
        SET consumed_at = ${input.createdAt}
        WHERE user_id = ${input.userId} AND consumed_at IS NULL
      `;
      await sql`
        INSERT INTO agent_pulse_password_resets (
          token_digest, user_id, created_at, expires_at
        ) VALUES (
          ${input.digest}, ${input.userId}, ${input.createdAt}, ${input.expiresAt}
        )
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: input.userId,
        actorUserId: input.userId,
        action: "auth.password_reset_requested",
        metadata: {},
        createdAt: input.createdAt,
      });
      return { allowed: true, retry_after_seconds: 0 };
    });
  }

  async consumePasswordReset(input: {
    digest: string;
    passwordHash: string;
    now: string;
  }): Promise<{ updated: boolean; revoked_sessions: number }> {
    return this.#sql.begin(async (sql) => {
      const resets = await sql<Array<{ user_id: string }>>`
        SELECT user_id::text
        FROM agent_pulse_password_resets
        WHERE token_digest = ${input.digest}
          AND consumed_at IS NULL
          AND expires_at > ${input.now}
        FOR UPDATE
      `;
      const userId = resets[0]?.user_id;
      if (userId === undefined) {
        return { updated: false, revoked_sessions: 0 };
      }
      const users = await sql<Array<{ user_id: string }>>`
        UPDATE agent_pulse_users
        SET password_hash = ${input.passwordHash}, updated_at = clock_timestamp()
        WHERE user_id = ${userId} AND status = 'active'
        RETURNING user_id::text
      `;
      if (users[0] === undefined) {
        return { updated: false, revoked_sessions: 0 };
      }
      await sql`
        UPDATE agent_pulse_password_resets
        SET consumed_at = ${input.now}
        WHERE user_id = ${userId} AND consumed_at IS NULL
      `;
      const revoked = await sql<Array<{ session_id: string }>>`
        DELETE FROM agent_pulse_web_sessions
        WHERE user_id = ${userId}
        RETURNING session_id::text
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: userId,
        actorUserId: userId,
        action: "auth.password_reset_completed",
        targetType: "account",
        targetId: userId,
        metadata: { revoked_sessions: revoked.length },
        createdAt: input.now,
      });
      return { updated: true, revoked_sessions: revoked.length };
    });
  }

  async deletePendingPasswordReset(digest: string): Promise<void> {
    await this.#sql`
      DELETE FROM agent_pulse_password_resets
      WHERE token_digest = ${digest} AND consumed_at IS NULL
    `;
  }

  async changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<{ updated: boolean; revoked_sessions: number }> {
    return this.#sql.begin(async (sql) => {
      const users = await sql<Array<{ user_id: string }>>`
        UPDATE agent_pulse_users
        SET password_hash = ${input.passwordHash}, updated_at = clock_timestamp()
        WHERE user_id = ${input.userId}
          AND status = 'active'
          AND password_hash = ${input.expectedPasswordHash}
        RETURNING user_id::text
      `;
      if (users[0] === undefined) {
        return { updated: false, revoked_sessions: 0 };
      }
      await sql`
        UPDATE agent_pulse_password_resets
        SET consumed_at = clock_timestamp()
        WHERE user_id = ${input.userId} AND consumed_at IS NULL
      `;
      const revoked = await sql<Array<{ session_id: string }>>`
        DELETE FROM agent_pulse_web_sessions
        WHERE user_id = ${input.userId}
          AND session_digest <> ${input.currentSessionDigest}
        RETURNING session_id::text
      `;
      await insertAuditEvent(sql, {
        scope: "account",
        accountUserId: input.userId,
        actorUserId: input.userId,
        action: "auth.password_changed",
        targetType: "account",
        targetId: input.userId,
        metadata: { revoked_sessions: revoked.length },
        ...(input.changedAt === undefined
          ? {}
          : { createdAt: input.changedAt }),
      });
      return { updated: true, revoked_sessions: revoked.length };
    });
  }

  async consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult> {
    return this.#sql.begin(async (sql) => {
      const inserted = await sql<Array<{ scope_digest: string }>>`
        INSERT INTO agent_pulse_login_limits (
          scope_digest, attempt_count, window_started_at
        ) VALUES (${input.scopeDigest}, 1, ${input.now})
        ON CONFLICT (scope_digest) DO NOTHING
        RETURNING scope_digest
      `;
      if (inserted.length > 0) {
        return { allowed: true, retry_after_seconds: 0 };
      }
      const rows = await sql<
        Array<{
          attempt_count: number;
          window_started_at: unknown;
          blocked_until: unknown | null;
        }>
      >`
        SELECT attempt_count, window_started_at, blocked_until
        FROM agent_pulse_login_limits
        WHERE scope_digest = ${input.scopeDigest}
        FOR UPDATE
      `;
      const row = rows[0];
      if (row === undefined) throw new Error("login limit row disappeared");
      const nowMs = Date.parse(input.now);
      if (
        Date.parse(isoTimestamp(row.window_started_at)) +
          input.windowSeconds * 1_000 <=
        nowMs
      ) {
        await sql`
          UPDATE agent_pulse_login_limits
          SET attempt_count = 1, window_started_at = ${input.now}, blocked_until = NULL
          WHERE scope_digest = ${input.scopeDigest}
        `;
        return { allowed: true, retry_after_seconds: 0 };
      }
      if (row.blocked_until !== null) {
        const blockedUntil = Date.parse(isoTimestamp(row.blocked_until));
        if (blockedUntil > nowMs) {
          return {
            allowed: false,
            retry_after_seconds: Math.ceil((blockedUntil - nowMs) / 1_000),
          };
        }
      }
      const attemptCount = Number(row.attempt_count) + 1;
      const blocked = attemptCount > input.maximumAttempts;
      const blockedUntil = blocked
        ? new Date(nowMs + input.blockSeconds * 1_000).toISOString()
        : undefined;
      await sql`
        UPDATE agent_pulse_login_limits
        SET attempt_count = ${attemptCount},
            blocked_until = ${blockedUntil ?? null}
        WHERE scope_digest = ${input.scopeDigest}
      `;
      return {
        allowed: !blocked,
        retry_after_seconds: blocked ? input.blockSeconds : 0,
      };
    });
  }

  async clearLoginAttempts(scopeDigest: string): Promise<void> {
    await this.#sql`
      DELETE FROM agent_pulse_login_limits
      WHERE scope_digest = ${scopeDigest}
    `;
  }

  async listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]> {
    const rows = await this.#sql<
      Array<{
        workspace_owner_id: string;
        owner_name: string;
        role: WorkspaceRole;
      }>
    >`
      SELECT
        members.workspace_owner_id::text,
        owners.display_name AS owner_name,
        members.role
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS owners
        ON owners.user_id = members.workspace_owner_id
      WHERE members.user_id = ${userId} AND owners.status = 'active'
      ORDER BY
        members.workspace_owner_id = ${userId} DESC,
        owners.display_name,
        members.workspace_owner_id
    `;
    return rows.map((row) => ({
      workspace_owner_id: row.workspace_owner_id,
      name:
        row.workspace_owner_id === userId
          ? "个人工作区"
          : `${row.owner_name} 的工作区`,
      role: row.role,
      current: row.workspace_owner_id === currentOwnerId,
    }));
  }

  async workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined> {
    const memberships = await this.#sql<
      Array<{ role: WorkspaceRole; owner_name: string }>
    >`
      SELECT members.role, owners.display_name AS owner_name
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS owners
        ON owners.user_id = members.workspace_owner_id
      WHERE members.workspace_owner_id = ${workspaceOwnerId}
        AND members.user_id = ${actorUserId}
        AND owners.status = 'active'
      LIMIT 1
    `;
    const membership = memberships[0];
    if (membership === undefined) return undefined;
    const members = await this.#sql<
      Array<{
        user_id: string;
        email: string;
        display_name: string;
        role: WorkspaceRole;
        created_at: unknown;
      }>
    >`
      SELECT
        users.user_id::text,
        users.email,
        users.display_name,
        members.role,
        members.created_at
      FROM agent_pulse_workspace_members AS members
      INNER JOIN agent_pulse_users AS users ON users.user_id = members.user_id
      WHERE members.workspace_owner_id = ${workspaceOwnerId}
      ORDER BY
        CASE members.role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END,
        users.display_name,
        users.user_id
    `;
    const canManage =
      membership.role === "owner" || membership.role === "admin";
    const invitations = canManage
      ? await this.#sql<
          Array<{
            invitation_id: string;
            invited_email: string;
            role: InviteRole;
            created_at: unknown;
            expires_at: unknown;
          }>
        >`
          SELECT
            invitation_id::text,
            invited_email,
            role,
            created_at,
            expires_at
          FROM agent_pulse_team_invitations
          WHERE workspace_owner_id = ${workspaceOwnerId}
            AND consumed_at IS NULL
            AND revoked_at IS NULL
            AND expires_at > ${now}
          ORDER BY created_at DESC, invitation_id
        `
      : [];
    return {
      workspace: {
        workspace_owner_id: workspaceOwnerId,
        name:
          workspaceOwnerId === actorUserId
            ? "个人工作区"
            : `${membership.owner_name} 的工作区`,
        role: membership.role,
        current: true,
      },
      members: members.map((member): WorkspaceMemberView => ({
        user_id: member.user_id,
        email: member.email,
        display_name: member.display_name,
        role: member.role,
        joined_at: isoTimestamp(member.created_at),
      })),
      invitations: invitations.map((invitation) => ({
        invitation_id: invitation.invitation_id,
        email: invitation.invited_email,
        role: invitation.role,
        created_at: isoTimestamp(invitation.created_at),
        expires_at: isoTimestamp(invitation.expires_at),
      })),
      member_limit: memberLimit,
    };
  }

  async issueTeamInvitation(input: {
    invitationId: string;
    digest: string;
    workspaceOwnerId: string;
    invitedUserId: string;
    invitedEmail: string;
    role: InviteRole;
    invitedBy: string;
    createdAt: string;
    expiresAt: string;
    memberLimit: number;
  }): Promise<void> {
    return this.#sql.begin(async (sql) => {
      await sql`
        SELECT user_id
        FROM agent_pulse_subscriptions
        WHERE user_id = ${input.workspaceOwnerId}
        FOR UPDATE
      `;
      const actors = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT role
        FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND user_id = ${input.invitedBy}
      `;
      const actorRole = actors[0]?.role;
      if (
        (actorRole !== "owner" && actorRole !== "admin") ||
        (actorRole === "admin" && input.role === "admin")
      ) {
        throw new AccountError(
          "team_permission_denied",
          "team permission denied",
          403,
        );
      }
      const counts = await sql<Array<{ count: string | number }>>`
        SELECT (
          SELECT count(*) FROM agent_pulse_workspace_members
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
        ) + (
          SELECT count(*) FROM agent_pulse_team_invitations
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND consumed_at IS NULL
            AND revoked_at IS NULL
            AND expires_at > ${input.createdAt}
        ) AS count
      `;
      if (Number(counts[0]?.count ?? 0) >= input.memberLimit) {
        throw new AccountError(
          "team_member_limit_reached",
          "team member limit reached",
          403,
        );
      }
      const existing = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT role FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND user_id = ${input.invitedUserId}
      `;
      if (existing[0] !== undefined) {
        throw new AccountError(
          "team_member_exists",
          "user is already a member",
          409,
        );
      }
      await sql`
        UPDATE agent_pulse_team_invitations
        SET revoked_at = ${input.createdAt}
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND invited_user_id = ${input.invitedUserId}
          AND consumed_at IS NULL
          AND revoked_at IS NULL
          AND expires_at <= ${input.createdAt}
      `;
      try {
        await sql`
          INSERT INTO agent_pulse_team_invitations (
            invitation_id, token_digest, workspace_owner_id, invited_user_id,
            invited_email, role, invited_by, created_at, expires_at
          ) VALUES (
            ${input.invitationId}, ${input.digest}, ${input.workspaceOwnerId},
            ${input.invitedUserId}, ${input.invitedEmail}, ${input.role},
            ${input.invitedBy}, ${input.createdAt}, ${input.expiresAt}
          )
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.invitedBy,
          action: "team.invitation_created",
          targetType: "team_invitation",
          targetId: input.invitationId,
          metadata: {
            role: input.role,
            invited_user_id: input.invitedUserId,
          },
          createdAt: input.createdAt,
        });
      } catch (error) {
        if (
          postgresConstraint(error) ===
          "agent_pulse_team_invitations_pending_idx"
        ) {
          throw new AccountError(
            "team_invitation_exists",
            "invitation already exists",
            409,
          );
        }
        throw error;
      }
    });
  }

  async consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined> {
    return this.#sql.begin(async (sql) => {
      const candidates = await sql<
        Array<{
          invitation_id: string;
          workspace_owner_id: string;
          role: InviteRole;
        }>
      >`
        SELECT invitation_id::text, workspace_owner_id::text, role
        FROM agent_pulse_team_invitations
        WHERE token_digest = ${input.digest}
          AND invited_user_id = ${input.userId}
          AND consumed_at IS NULL
          AND revoked_at IS NULL
          AND expires_at > ${input.now}
        FOR UPDATE
      `;
      const invitation = candidates[0];
      if (invitation === undefined) return undefined;
      const subscriptions = await sql<SubscriptionRecord[]>`
        SELECT plan, status, current_period_end
        FROM agent_pulse_subscriptions
        WHERE user_id = ${invitation.workspace_owner_id}
        FOR UPDATE
      `;
      const subscription = subscriptions[0];
      const memberLimit =
        subscription === undefined
          ? 0
          : entitlementsFor(normalizeSubscription(subscription)).team_members;
      const memberCounts = await sql<Array<{ count: string | number }>>`
        SELECT count(*) AS count
        FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${invitation.workspace_owner_id}
      `;
      if (Number(memberCounts[0]?.count ?? 0) >= memberLimit) {
        throw new AccountError(
          "team_member_limit_reached",
          "team member limit reached",
          403,
        );
      }
      await sql`
        UPDATE agent_pulse_team_invitations
        SET consumed_at = ${input.now}
        WHERE token_digest = ${input.digest}
      `;
      await sql`
        INSERT INTO agent_pulse_workspace_members (
          workspace_owner_id, user_id, role, created_at
        ) VALUES (
          ${invitation.workspace_owner_id}, ${input.userId},
          ${invitation.role}, ${input.now}
        )
        ON CONFLICT (workspace_owner_id, user_id) DO NOTHING
      `;
      await insertAuditEvent(sql, {
        scope: "workspace",
        workspaceOwnerId: invitation.workspace_owner_id,
        actorUserId: input.userId,
        action: "team.invitation_accepted",
        targetType: "team_member",
        targetId: input.userId,
        metadata: {
          role: invitation.role,
          invitation_id: invitation.invitation_id,
        },
        createdAt: input.now,
      });
      return invitation.workspace_owner_id;
    });
  }

  async revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{ invitation_id: string; role: InviteRole }>
      >`
        UPDATE agent_pulse_team_invitations AS invitations
        SET revoked_at = ${now}
        FROM agent_pulse_workspace_members AS actors
        WHERE invitations.invitation_id = ${invitationId}
          AND invitations.workspace_owner_id = ${workspaceOwnerId}
          AND invitations.consumed_at IS NULL
          AND invitations.revoked_at IS NULL
          AND actors.workspace_owner_id = invitations.workspace_owner_id
          AND actors.user_id = ${actorUserId}
          AND (
            actors.role = 'owner' OR
            (actors.role = 'admin' AND invitations.role = 'member')
          )
        RETURNING invitations.invitation_id::text, invitations.role
      `;
      const invitation = rows[0];
      if (invitation === undefined) return false;
      await insertAuditEvent(sql, {
        scope: "workspace",
        workspaceOwnerId,
        actorUserId,
        action: "team.invitation_revoked",
        targetType: "team_invitation",
        targetId: invitationId,
        metadata: { role: invitation.role },
        createdAt: now,
      });
      return true;
    });
  }

  async updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{ actor_role: WorkspaceRole; target_role: WorkspaceRole }>
      >`
        SELECT actors.role AS actor_role, targets.role AS target_role
        FROM agent_pulse_workspace_members AS actors
        INNER JOIN agent_pulse_workspace_members AS targets
          ON targets.workspace_owner_id = actors.workspace_owner_id
        WHERE actors.workspace_owner_id = ${input.workspaceOwnerId}
          AND actors.user_id = ${input.actorUserId}
          AND targets.user_id = ${input.memberUserId}
        FOR UPDATE OF targets
      `;
      const roles = rows[0];
      if (
        roles === undefined ||
        roles.target_role === "owner" ||
        (roles.actor_role !== "owner" &&
          (roles.actor_role !== "admin" ||
            roles.target_role !== "member" ||
            input.role === "admin"))
      ) {
        return false;
      }
      if (input.remove) {
        await sql`
          DELETE FROM agent_pulse_workspace_members
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND user_id = ${input.memberUserId}
        `;
        await sql`
          UPDATE agent_pulse_web_sessions
          SET workspace_owner_id = user_id
          WHERE user_id = ${input.memberUserId}
            AND workspace_owner_id = ${input.workspaceOwnerId}
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.actorUserId,
          action: "team.member_removed",
          targetType: "team_member",
          targetId: input.memberUserId,
          metadata: { previous_role: roles.target_role },
          ...(input.updatedAt === undefined
            ? {}
            : { createdAt: input.updatedAt }),
        });
      } else if (input.role !== undefined) {
        await sql`
          UPDATE agent_pulse_workspace_members
          SET role = ${input.role}
          WHERE workspace_owner_id = ${input.workspaceOwnerId}
            AND user_id = ${input.memberUserId}
        `;
        await insertAuditEvent(sql, {
          scope: "workspace",
          workspaceOwnerId: input.workspaceOwnerId,
          actorUserId: input.actorUserId,
          action: "team.member_role_updated",
          targetType: "team_member",
          targetId: input.memberUserId,
          metadata: {
            previous_role: roles.target_role,
            role: input.role,
          },
          ...(input.updatedAt === undefined
            ? {}
            : { createdAt: input.updatedAt }),
        });
      }
      return true;
    });
  }

  async switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined> {
    return this.#sql.begin(async (sql) => {
      const sessions = await sql<Array<{ workspace_owner_id: string }>>`
        SELECT workspace_owner_id::text
        FROM agent_pulse_web_sessions
        WHERE session_digest = ${input.digest}
          AND user_id = ${input.actorUserId}
        FOR UPDATE
      `;
      const previousWorkspaceOwnerId = sessions[0]?.workspace_owner_id;
      if (previousWorkspaceOwnerId === undefined) return undefined;
      const members = await sql<Array<{ role: WorkspaceRole }>>`
        SELECT role
        FROM agent_pulse_workspace_members
        WHERE workspace_owner_id = ${input.workspaceOwnerId}
          AND user_id = ${input.actorUserId}
      `;
      const role = members[0]?.role;
      if (role === undefined) return undefined;
      await sql`
        UPDATE agent_pulse_web_sessions
        SET workspace_owner_id = ${input.workspaceOwnerId}
        WHERE session_digest = ${input.digest}
      `;
      if (previousWorkspaceOwnerId !== input.workspaceOwnerId) {
        await insertAuditEvent(sql, {
          scope: "account",
          accountUserId: input.actorUserId,
          actorUserId: input.actorUserId,
          action: "workspace.switched",
          targetType: "workspace",
          targetId: input.workspaceOwnerId,
          metadata: {},
          ...(input.switchedAt === undefined
            ? {}
            : { createdAt: input.switchedAt }),
        });
      }
      return {
        actor_user_id: input.actorUserId,
        workspace_owner_id: input.workspaceOwnerId,
        role,
      };
    });
  }

  async applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult> {
    return this.#sql.begin(async (sql) => {
      const subscriptions = await sql<SubscriptionRecord[]>`
        SELECT
          plan, status, provider, provider_customer_id,
          provider_subscription_id, current_period_end,
          last_event_at, last_event_id::text
        FROM agent_pulse_subscriptions
        WHERE user_id = ${event.user_id}
        FOR UPDATE
      `;
      const currentRow = subscriptions[0];
      if (currentRow === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          404,
        );
      }
      const existing = await sql<
        Array<
          SubscriptionEvent & {
            applied: boolean;
            current_period_end: unknown | null;
          }
        >
      >`
        SELECT
          event_id::text, user_id::text, provider, provider_customer_id,
          provider_subscription_id, plan, status, current_period_end,
          occurred_at, applied
        FROM agent_pulse_subscription_events
        WHERE event_id = ${event.event_id}
      `;
      if (existing[0] !== undefined) {
        const stored = normalizeSubscriptionEvent(existing[0]);
        if (!isDeepStrictEqual(stored, event)) {
          throw new AccountError(
            "subscription_event_conflict",
            "event ID was reused with different data",
            409,
          );
        }
        const account = await postgresAccountView(sql, event.user_id);
        if (account === undefined) {
          throw new AccountError(
            "account_unavailable",
            "account unavailable",
            404,
          );
        }
        return { applied: existing[0].applied, duplicate: true, account };
      }
      const current = normalizeSubscription(currentRow);
      const applied = isNewerSubscriptionEvent(current, event);
      await sql`
        INSERT INTO agent_pulse_subscription_events (
          event_id, user_id, provider, provider_customer_id,
          provider_subscription_id, plan, status, current_period_end,
          occurred_at, applied
        ) VALUES (
          ${event.event_id}, ${event.user_id}, ${event.provider},
          ${event.provider_customer_id}, ${event.provider_subscription_id},
          ${event.plan}, ${event.status}, ${event.current_period_end ?? null},
          ${event.occurred_at}, ${applied}
        )
      `;
      if (applied) {
        await sql`
          UPDATE agent_pulse_subscriptions
          SET plan = ${event.plan}, status = ${event.status},
              provider = ${event.provider},
              provider_customer_id = ${event.provider_customer_id},
              provider_subscription_id = ${event.provider_subscription_id},
              current_period_end = ${event.current_period_end ?? null},
              last_event_at = ${event.occurred_at},
              last_event_id = ${event.event_id},
              updated_at = clock_timestamp()
          WHERE user_id = ${event.user_id}
        `;
        await insertAuditEvent(sql, {
          scope: "account",
          accountUserId: event.user_id,
          actorUserId: event.user_id,
          action: "subscription.updated",
          targetType: "subscription",
          targetId: event.event_id,
          metadata: {
            plan: event.plan,
            status: event.status,
            provider: event.provider,
          },
          createdAt: event.occurred_at,
        });
      }
      const account = await postgresAccountView(sql, event.user_id);
      if (account === undefined) throw new Error("updated account disappeared");
      return { applied, duplicate: false, account };
    });
  }

  async billingReference(
    userId: string,
  ): Promise<BillingReference | undefined> {
    const rows = await this.#sql<
      Array<{
        provider: string | null;
        provider_customer_id: string | null;
        provider_subscription_id: string | null;
      }>
    >`
      SELECT provider, provider_customer_id, provider_subscription_id
      FROM agent_pulse_subscriptions
      WHERE user_id = ${userId}
      LIMIT 1
    `;
    const row = rows[0];
    if (
      row === undefined ||
      row.provider === null ||
      row.provider_customer_id === null
    ) {
      return undefined;
    }
    return {
      provider: row.provider,
      customer_id: row.provider_customer_id,
      ...(row.provider_subscription_id === null
        ? {}
        : { subscription_id: row.provider_subscription_id }),
    };
  }

  async listAuditEvents(input: AuditQuery): Promise<AuditEvent[]> {
    const cursor =
      input.beforeEventId === undefined
        ? undefined
        : (
            await this.#sql<Array<{ event_id: string; created_at: unknown }>>`
              SELECT event_id::text, created_at
              FROM agent_pulse_audit_events
              WHERE event_id = ${input.beforeEventId}
                AND (
                  (${input.scope} = 'account' AND scope = 'account'
                    AND account_user_id = ${input.actorUserId}) OR
                  (${input.scope} = 'workspace' AND scope = 'workspace'
                    AND workspace_owner_id = ${input.workspaceOwnerId}
                    AND EXISTS (
                      SELECT 1 FROM agent_pulse_workspace_members
                      WHERE workspace_owner_id = ${input.workspaceOwnerId}
                        AND user_id = ${input.actorUserId}
                    ))
                )
              LIMIT 1
            `
          )[0];
    if (input.beforeEventId !== undefined && cursor === undefined) return [];
    const cursorTime =
      cursor === undefined ? undefined : isoTimestamp(cursor.created_at);
    const rows =
      input.scope === "account"
        ? await this.#sql<AuditEventRow[]>`
            SELECT
              event_id::text, scope, actor_user_id::text,
              workspace_owner_id::text, action, target_type,
              target_id::text, metadata, created_at
            FROM agent_pulse_audit_events
            WHERE scope = 'account'
              AND account_user_id = ${input.actorUserId}
              AND (
                ${cursorTime ?? null}::timestamptz IS NULL OR
                (created_at, event_id) < (
                  ${cursorTime ?? null}::timestamptz,
                  ${cursor?.event_id ?? null}::uuid
                )
              )
            ORDER BY created_at DESC, event_id DESC
            LIMIT ${input.limit}
          `
        : await this.#sql<AuditEventRow[]>`
            SELECT
              events.event_id::text, events.scope,
              events.actor_user_id::text, events.workspace_owner_id::text,
              events.action, events.target_type, events.target_id::text,
              events.metadata, events.created_at
            FROM agent_pulse_audit_events AS events
            WHERE events.scope = 'workspace'
              AND events.workspace_owner_id = ${input.workspaceOwnerId}
              AND EXISTS (
                SELECT 1 FROM agent_pulse_workspace_members AS members
                WHERE members.workspace_owner_id = events.workspace_owner_id
                  AND members.user_id = ${input.actorUserId}
              )
              AND (
                ${cursorTime ?? null}::timestamptz IS NULL OR
                (events.created_at, events.event_id) < (
                  ${cursorTime ?? null}::timestamptz,
                  ${cursor?.event_id ?? null}::uuid
                )
              )
            ORDER BY events.created_at DESC, events.event_id DESC
            LIMIT ${input.limit}
          `;
    return rows.map(normalizeAuditEvent);
  }

  async recordAuditEvent(input: AuditRecordInput): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      if (input.scope === "account") {
        if (input.accountUserId === undefined) return false;
        const users = await sql`
          SELECT 1 FROM agent_pulse_users
          WHERE user_id::text = ${input.accountUserId}
          LIMIT 1
        `;
        if (users.length === 0) return false;
      } else {
        if (input.workspaceOwnerId === undefined) return false;
        const owners = await sql`
          SELECT 1 FROM agent_pulse_users
          WHERE user_id::text = ${input.workspaceOwnerId}
          LIMIT 1
        `;
        if (owners.length === 0) return false;
      }
      await insertAuditEvent(sql, input);
      return true;
    });
  }

  async countActiveDevices(ownerId: string): Promise<number> {
    const rows = await this.#sql<Array<{ count: string | number }>>`
      SELECT count(*) AS count
      FROM agent_platform_devices
      WHERE owner_id = ${ownerId} AND revoked_at IS NULL
    `;
    return Number(rows[0]?.count ?? 0);
  }

  async savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
  ): Promise<void> {
    // The endpoint identifies one browser installation. Re-subscribing under a
    // different user moves it: the browser now belongs to that user.
    await this.#sql`
      INSERT INTO agent_platform_push_subscriptions (
        endpoint,
        owner_id,
        p256dh,
        auth
      )
      VALUES (
        ${subscription.endpoint},
        ${ownerId},
        ${subscription.p256dh},
        ${subscription.auth}
      )
      ON CONFLICT (endpoint) DO UPDATE SET
        owner_id = EXCLUDED.owner_id,
        p256dh = EXCLUDED.p256dh,
        auth = EXCLUDED.auth,
        updated_at = clock_timestamp()
    `;
  }

  async deletePushSubscription(
    ownerId: string,
    endpoint: string,
  ): Promise<void> {
    await this.#sql`
      DELETE FROM agent_platform_push_subscriptions
      WHERE endpoint = ${endpoint} AND owner_id = ${ownerId}
    `;
  }

  async listPushSubscriptions(ownerId: string): Promise<PushSubscription[]> {
    const rows = await this.#sql<
      Array<{ endpoint: string; p256dh: string; auth: string }>
    >`
      SELECT endpoint, p256dh, auth
      FROM agent_platform_push_subscriptions
      WHERE owner_id = ${ownerId}
      ORDER BY endpoint
    `;
    return rows.map((row) => ({
      endpoint: row.endpoint,
      p256dh: row.p256dh,
      auth: row.auth,
    }));
  }

  async createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
    actorUserId?: string,
  ): Promise<void> {
    try {
      await this.#sql.begin(async (sql) => {
        await sql`
          DELETE FROM agent_platform_binding_codes
          WHERE claimed_at IS NULL
            AND expires_at <= clock_timestamp()
        `;
        await sql`
          INSERT INTO agent_platform_binding_codes (
            code_digest,
            owner_id,
            issued_by,
            expires_at
          )
          VALUES (${codeDigest}, ${ownerId}, ${actorUserId ?? null}, ${expiresAt})
        `;
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: ownerId,
          actorUserId,
          action: "device.binding_created",
          metadata: {},
        });
      });
    } catch (error) {
      throw classifyPostgresConflict(error);
    }
  }

  async registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    return this.#sql.begin(async (sql) => {
      const bindings = await sql<BindingCodeRow[]>`
        SELECT
          owner_id,
          issued_by,
          expires_at <= clock_timestamp() AS expired,
          claimed_device_id
        FROM agent_platform_binding_codes
        WHERE code_digest = ${codeDigest}
        FOR UPDATE
      `;
      const binding = bindings[0];
      if (binding === undefined) {
        throw new GatewayStoreError(
          "invalid_binding_code",
          "binding code is invalid or expired",
        );
      }
      if (
        binding.claimed_device_id !== null &&
        binding.claimed_device_id !== deviceId
      ) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      if (binding.claimed_device_id === null && binding.expired) {
        throw new GatewayStoreError(
          "invalid_binding_code",
          "binding code is invalid or expired",
        );
      }

      const inserted = await sql<DeviceRow[]>`
        INSERT INTO agent_platform_devices (
          device_id,
          public_key,
          owner_id
        )
        VALUES (${deviceId}, ${publicKey}, ${binding.owner_id})
        ON CONFLICT (device_id) DO NOTHING
        RETURNING
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence,
          revoked_at
      `;
      const created = inserted.length === 1;
      const rows =
        inserted.length > 0
          ? inserted
          : await sql<DeviceRow[]>`
              SELECT
                device_id,
                public_key,
                owner_id,
                created_at,
                last_seen_at,
                last_sequence,
                revoked_at
              FROM agent_platform_devices
              WHERE device_id = ${deviceId}
              FOR UPDATE
            `;
      const device = rows[0];
      if (device === undefined) {
        throw new Error("registered device could not be loaded");
      }
      if (device.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (
        device.public_key !== publicKey ||
        device.owner_id !== binding.owner_id
      ) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      if (binding.claimed_device_id === null) {
        await sql`
          UPDATE agent_platform_binding_codes
          SET
            claimed_at = clock_timestamp(),
            claimed_device_id = ${deviceId}
          WHERE code_digest = ${codeDigest}
        `;
      }
      if (created) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: binding.owner_id,
          actorUserId: binding.issued_by ?? undefined,
          action: "device.registered",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(device.created_at),
        });
      }
      return { device: registeredDevice(device), created };
    });
  }

  async revokeDevice(
    ownerId: string,
    deviceId: string,
    actorUserId?: string,
  ): Promise<boolean> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<Array<{ device_id: string; revoked_at: unknown }>>`
        UPDATE agent_platform_devices
        SET revoked_at = clock_timestamp()
        WHERE
          device_id = ${deviceId}
          AND owner_id = ${ownerId}
          AND revoked_at IS NULL
        RETURNING device_id, revoked_at
      `;
      if (rows.length !== 1) return false;
      await insertWorkspaceAuditIfVisible(sql, {
        workspaceOwnerId: ownerId,
        actorUserId,
        action: "device.revoked",
        targetType: "device",
        targetId: deviceId,
        metadata: {},
        createdAt: isoTimestamp(rows[0]!.revoked_at),
      });
      return true;
    });
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
    actorUserId?: string,
  ): Promise<DeviceRegistrationResult> {
    return this.#sql.begin(async (sql) => {
      const inserted = await sql<DeviceRow[]>`
        INSERT INTO agent_platform_devices (
          device_id,
          public_key,
          owner_id
        )
        VALUES (${deviceId}, ${publicKey}, ${ownerId})
        ON CONFLICT (device_id) DO NOTHING
        RETURNING
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence,
          revoked_at
      `;
      const created = inserted.length === 1;
      const rows =
        inserted.length > 0
          ? inserted
          : await sql<DeviceRow[]>`
              SELECT
                device_id,
                public_key,
                owner_id,
                created_at,
                last_seen_at,
                last_sequence,
                revoked_at
              FROM agent_platform_devices
              WHERE device_id = ${deviceId}
              FOR UPDATE
            `;
      const row = rows[0];
      if (row === undefined) {
        throw new Error("registered device could not be loaded");
      }
      if (row.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (row.public_key !== publicKey || row.owner_id !== ownerId) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      if (created) {
        await insertWorkspaceAuditIfVisible(sql, {
          workspaceOwnerId: ownerId,
          actorUserId,
          action: "device.registered",
          targetType: "device",
          targetId: deviceId,
          metadata: {},
          createdAt: isoTimestamp(row.created_at),
        });
      }
      return { device: registeredDevice(row), created };
    });
  }

  async ingestBatch(batch: SyncBatch): Promise<IngestResult> {
    try {
      return await this.#sql.begin(async (sql) => {
        const devices = await sql<DeviceRow[]>`
          SELECT
            device_id,
            public_key,
            owner_id,
            created_at,
            last_seen_at,
            last_sequence,
            revoked_at
          FROM agent_platform_devices
          WHERE device_id = ${batch.device_id}
          FOR UPDATE
        `;
        const device = devices[0];
        if (device === undefined) {
          throw new GatewayStoreError(
            "device_not_registered",
            "device is not registered",
          );
        }
        if (device.revoked_at !== null) {
          throw new GatewayStoreError("device_revoked", "device is revoked");
        }
        if (device.public_key !== batch.public_key) {
          throw new GatewayStoreError(
            "device_key_mismatch",
            "batch public key does not match registered device",
          );
        }

        const eventIds = batch.events.map((event) => event.event_id);
        const existingRows = await sql<ExistingEventRow[]>`
          SELECT event_id::text, device_id, sequence, event_data
          FROM agent_platform_events
          WHERE
            event_id IN ${sql(eventIds)}
            OR (
              device_id = ${batch.device_id}
              AND sequence BETWEEN ${batch.first_sequence} AND ${batch.last_sequence}
            )
        `;
        const byId = new Map(
          existingRows.map((row) => [row.event_id, row] as const),
        );
        const bySequence = new Map(
          existingRows
            .filter((row) => row.device_id === batch.device_id)
            .map((row) => [Number(row.sequence), row] as const),
        );
        const newEvents: SyncEvent[] = [];
        for (const event of batch.events) {
          const existingById = byId.get(event.event_id);
          if (existingById !== undefined) {
            if (!isDeepStrictEqual(existingById.event_data, event)) {
              throw new GatewayStoreError(
                "event_conflict",
                "event ID was reused with different data",
              );
            }
            continue;
          }
          const existingAtSequence = bySequence.get(event.sequence);
          if (
            existingAtSequence !== undefined &&
            existingAtSequence.event_id !== event.event_id
          ) {
            throw new GatewayStoreError(
              "sequence_conflict",
              "device sequence was reused by another event",
            );
          }
          newEvents.push(event);
        }

        const lastSequence = Number(device.last_sequence);
        if (
          newEvents.length > 0 &&
          newEvents[0]?.sequence !== lastSequence + 1
        ) {
          throw new GatewayStoreError(
            "sequence_gap",
            "new events do not continue the stored device sequence",
          );
        }

        const receivedAt = new Date();
        const changes: SessionStatusChange[] = [];
        if (newEvents.length > 0) {
          const snapshotEvents = bestSessionEvents(newEvents);
          // The device row is already locked FOR UPDATE, so no concurrent
          // batch can move these snapshots between the before/after reads.
          const before = new Map<string, ActivityStatus>(
            (
              await sql<SessionStatusRow[]>`
                SELECT agent_type, agent_session_id, activity_status, event_data
                FROM agent_platform_sessions
                WHERE
                  device_id = ${batch.device_id}
                  AND agent_session_id IN ${sql(
                    snapshotEvents.map((event) => event.agent_session_id),
                  )}
              `
            ).map(
              (row) =>
                [
                  `${row.agent_type}:${row.agent_session_id}`,
                  row.activity_status,
                ] as const,
            ),
          );
          const eventRows = newEvents.map((event) => ({
            event_id: event.event_id,
            device_id: event.device_id,
            sequence: event.sequence,
            observed_at: event.observed_at,
            received_at: receivedAt,
            event_data: sql.json(event as unknown as postgres.JSONValue),
          }));
          await sql`
            INSERT INTO agent_platform_events ${sql(
              eventRows,
              "event_id",
              "device_id",
              "sequence",
              "observed_at",
              "received_at",
              "event_data",
            )}
          `;

          const sessionRows = snapshotEvents.map((event) => ({
            device_id: event.device_id,
            agent_type: event.agent_type,
            agent_session_id: event.agent_session_id,
            observed_at: event.observed_at,
            received_at: receivedAt,
            sequence: event.sequence,
            confidence: event.confidence,
            presence: event.payload.state.presence,
            activity_status: event.payload.state.activity_status,
            event_data: sql.json(event as unknown as postgres.JSONValue),
          }));
          await sql`
            INSERT INTO agent_platform_sessions ${sql(
              sessionRows,
              "device_id",
              "agent_type",
              "agent_session_id",
              "observed_at",
              "received_at",
              "sequence",
              "confidence",
              "presence",
              "activity_status",
              "event_data",
            )}
            ON CONFLICT (device_id, agent_type, agent_session_id)
            DO UPDATE SET
              observed_at = EXCLUDED.observed_at,
              received_at = EXCLUDED.received_at,
              sequence = EXCLUDED.sequence,
              confidence = EXCLUDED.confidence,
              presence = EXCLUDED.presence,
              activity_status = EXCLUDED.activity_status,
              event_data = EXCLUDED.event_data
            WHERE
              EXCLUDED.observed_at > agent_platform_sessions.observed_at
              OR (
                EXCLUDED.observed_at = agent_platform_sessions.observed_at
                AND (
                  CASE EXCLUDED.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                ) > (
                  CASE agent_platform_sessions.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                )
              )
              OR (
                EXCLUDED.observed_at = agent_platform_sessions.observed_at
                AND (
                  CASE EXCLUDED.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                ) = (
                  CASE agent_platform_sessions.confidence
                    WHEN 'unknown' THEN 0
                    WHEN 'inferred' THEN 1
                    WHEN 'observed' THEN 2
                    WHEN 'authoritative' THEN 3
                  END
                )
                AND EXCLUDED.sequence > agent_platform_sessions.sequence
              )
          `;

          const after = await sql<SessionStatusRow[]>`
            SELECT agent_type, agent_session_id, activity_status, event_data
            FROM agent_platform_sessions
            WHERE
              device_id = ${batch.device_id}
              AND agent_session_id IN ${sql(
                snapshotEvents.map((event) => event.agent_session_id),
              )}
          `;
          const touched = new Set<string>(
            snapshotEvents.map(
              (event) => `${event.agent_type}:${event.agent_session_id}`,
            ),
          );
          for (const row of after) {
            const key = `${row.agent_type}:${row.agent_session_id}`;
            if (!touched.has(key)) {
              continue;
            }
            const previous = before.get(key);
            if (previous === row.activity_status) {
              continue;
            }
            changes.push(
              sessionStatusChange(syncEvent(row.event_data), previous),
            );
          }
        }

        const acceptedSequence =
          newEvents.at(-1)?.sequence ?? Number(device.last_sequence);
        await sql`
          UPDATE agent_platform_devices
          SET
            last_seen_at = ${receivedAt},
            last_sequence = ${acceptedSequence}
          WHERE device_id = ${batch.device_id}
        `;
        return {
          accepted: newEvents.length,
          owner_id: device.owner_id,
          changes,
        };
      });
    } catch (error) {
      throw classifyPostgresConflict(error);
    }
  }

  async ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult> {
    return this.#sql.begin(async (sql) => {
      const devices = await sql<DeviceRow[]>`
        SELECT
          device_id,
          public_key,
          owner_id,
          created_at,
          last_seen_at,
          last_sequence,
          revoked_at
        FROM agent_platform_devices
        WHERE device_id = ${batch.device_id}
        FOR UPDATE
      `;
      const device = devices[0];
      if (device === undefined) {
        throw new GatewayStoreError(
          "device_not_registered",
          "device is not registered",
        );
      }
      if (device.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (device.public_key !== batch.public_key) {
        throw new GatewayStoreError(
          "device_key_mismatch",
          "auth batch public key does not match registered device",
        );
      }

      const receivedAt = new Date();
      // One read for the whole batch instead of a lookup per snapshot.
      const existingRows = await sql<
        Array<{
          agent_type: string;
          checked_at: unknown;
          snapshot_data: unknown;
        }>
      >`
        SELECT agent_type, checked_at, snapshot_data
        FROM agent_platform_auth_snapshots
        WHERE device_id = ${batch.device_id}
      `;
      const existingByProvider = new Map(
        existingRows.map((row) => [row.agent_type, row] as const),
      );
      let accepted = 0;
      for (const snapshot of batch.snapshots) {
        const existing = existingByProvider.get(snapshot.provider);
        const existingCheckedAt = existing?.checked_at;
        const existingSnapshot = existing?.snapshot_data as
          AuthSnapshot | undefined;
        if (
          existingCheckedAt !== undefined &&
          (Date.parse(isoTimestamp(existingCheckedAt)) >
            Date.parse(snapshot.checked_at) ||
            (Date.parse(isoTimestamp(existingCheckedAt)) ===
              Date.parse(snapshot.checked_at) &&
              existingSnapshot !== undefined &&
              isDeepStrictEqual(existingSnapshot, snapshot)))
        ) {
          continue;
        }
        await sql`
          INSERT INTO agent_platform_auth_snapshots (
            device_id,
            agent_type,
            checked_at,
            received_at,
            snapshot_data
          )
          VALUES (
            ${batch.device_id},
            ${snapshot.provider},
            ${snapshot.checked_at},
            ${receivedAt},
            ${sql.json(snapshot as unknown as postgres.JSONValue)}
          )
          ON CONFLICT (device_id, agent_type)
          DO UPDATE SET
            checked_at = EXCLUDED.checked_at,
            received_at = EXCLUDED.received_at,
            snapshot_data = EXCLUDED.snapshot_data
          WHERE EXCLUDED.checked_at >= agent_platform_auth_snapshots.checked_at
        `;
        accepted += 1;
      }
      await sql`
        UPDATE agent_platform_devices
        SET last_seen_at = ${receivedAt}
        WHERE device_id = ${batch.device_id}
      `;
      const rows = await sql<AuthSnapshotRow[]>`
        SELECT device_id, agent_type, checked_at, received_at, snapshot_data
        FROM agent_platform_auth_snapshots
        WHERE device_id = ${batch.device_id}
        ORDER BY checked_at DESC, agent_type
      `;
      return {
        accepted,
        owner_id: device.owner_id,
        snapshots: rows.map((row) => ({
          ...(row.snapshot_data as AuthSnapshot),
          device_id: batch.device_id,
          received_at: isoTimestamp(row.received_at),
        })),
      };
    });
  }

  async recordHeartbeat(
    heartbeat: DeviceHeartbeat,
    receivedAt: string,
  ): Promise<HeartbeatResult> {
    return this.#sql.begin(async (sql) => {
      const rows = await sql<
        Array<{
          owner_id: string;
          public_key: string;
          revoked_at: unknown | null;
          last_seen_at: unknown;
          last_heartbeat_at: unknown | null;
        }>
      >`
        SELECT owner_id, public_key, revoked_at, last_seen_at, last_heartbeat_at
        FROM agent_platform_devices
        WHERE device_id = ${heartbeat.device_id}
        FOR UPDATE
      `;
      const device = rows[0];
      if (device === undefined) {
        throw new GatewayStoreError(
          "device_not_registered",
          "device is not registered",
        );
      }
      if (device.revoked_at !== null) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (device.public_key !== heartbeat.public_key) {
        throw new GatewayStoreError(
          "device_key_mismatch",
          "heartbeat public key does not match registered device",
        );
      }
      const previous =
        device.last_heartbeat_at === null
          ? undefined
          : isoTimestamp(device.last_heartbeat_at);
      const accepted = previous === undefined || heartbeat.sent_at > previous;
      if (accepted) {
        await sql`
          UPDATE agent_platform_devices
          SET last_seen_at = ${receivedAt}, last_heartbeat_at = ${heartbeat.sent_at}
          WHERE device_id = ${heartbeat.device_id}
        `;
        return {
          accepted: true,
          owner_id: device.owner_id,
          last_seen_at: receivedAt,
        };
      }
      return {
        accepted: false,
        owner_id: device.owner_id,
        last_seen_at: isoTimestamp(device.last_seen_at),
      };
    });
  }

  async getRegisteredDevice(
    deviceId: string,
  ): Promise<RegisteredDevice | undefined> {
    const rows = await this.#sql<
      DeviceRow[]
    >`SELECT * FROM agent_platform_devices WHERE device_id = ${deviceId}`;
    return rows[0] === undefined ? undefined : registeredDevice(rows[0]);
  }

  async getRegisteredDevices(
    deviceIds: readonly string[],
  ): Promise<Map<string, RegisteredDevice>> {
    if (deviceIds.length === 0) {
      return new Map();
    }
    const rows = await this.#sql<DeviceRow[]>`
      SELECT * FROM agent_platform_devices
      WHERE device_id = ANY(${deviceIds as string[]})
    `;
    return new Map(
      rows.map((row) => [row.device_id, registeredDevice(row)] as const),
    );
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const rows = await this.#sql<
      Array<{
        device_id: string;
        created_at: unknown;
        last_seen_at: unknown;
        online: boolean;
        revoked_at: unknown | null;
      }>
    >`
      SELECT
        device_id,
        created_at,
        last_seen_at,
        revoked_at,
        revoked_at IS NULL
          AND last_seen_at >= clock_timestamp() - interval '45 seconds' AS online
      FROM agent_platform_devices
      WHERE owner_id = ${ownerId}
      ORDER BY device_id
    `;
    return rows.map((row) => ({
      device_id: row.device_id,
      created_at: isoTimestamp(row.created_at),
      last_seen_at: isoTimestamp(row.last_seen_at),
      online: row.online,
      revoked: row.revoked_at !== null,
    }));
  }

  async listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]> {
    const deviceFilter =
      filters.device_id === undefined
        ? this.#sql``
        : this.#sql`AND sessions.device_id = ${filters.device_id}`;
    const agentFilter =
      filters.agent_type === undefined
        ? this.#sql``
        : this.#sql`AND sessions.agent_type = ${filters.agent_type}`;
    const activityFilter =
      filters.activity_status === undefined
        ? this.#sql``
        : this.#sql`AND sessions.activity_status = ${filters.activity_status}`;
    const presenceFilter =
      filters.presence === undefined
        ? this.#sql``
        : this.#sql`AND sessions.presence = ${filters.presence}`;
    const rows = await this.#sql<SessionRow[]>`
      SELECT sessions.event_data, sessions.received_at
      FROM agent_platform_sessions AS sessions
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = sessions.device_id
      WHERE
        devices.owner_id = ${ownerId}
        ${deviceFilter}
        ${agentFilter}
        ${activityFilter}
        ${presenceFilter}
      ORDER BY sessions.observed_at DESC, sessions.sequence DESC
    `;
    return rows.map((row) => ({
      ...syncEvent(row.event_data),
      received_at: isoTimestamp(row.received_at),
    }));
  }

  async listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]> {
    const rows = await this.#sql<SessionRow[]>`
      SELECT events.event_data, events.received_at
      FROM agent_platform_events AS events
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = events.device_id
      WHERE
        devices.owner_id = ${ownerId}
        AND events.device_id = ${filters.device_id}
        AND events.event_data->>'agent_type' = ${filters.agent_type}
        AND events.event_data->>'agent_session_id' = ${filters.agent_session_id}
      ORDER BY events.sequence DESC
      LIMIT ${filters.limit}
    `;
    return rows.map((row) => ({
      ...syncEvent(row.event_data),
      received_at: isoTimestamp(row.received_at),
    }));
  }

  async listAuth(
    ownerId: string,
    filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    const deviceFilter =
      filters.device_id === undefined
        ? this.#sql``
        : this.#sql`AND snapshots.device_id = ${filters.device_id}`;
    const rows = await this.#sql<AuthSnapshotRow[]>`
      SELECT
        snapshots.device_id,
        snapshots.agent_type,
        snapshots.checked_at,
        snapshots.received_at,
        snapshots.snapshot_data
      FROM agent_platform_auth_snapshots AS snapshots
      INNER JOIN agent_platform_devices AS devices
        ON devices.device_id = snapshots.device_id
      WHERE devices.owner_id = ${ownerId}
        ${deviceFilter}
      ORDER BY snapshots.checked_at DESC, snapshots.agent_type
    `;
    return rows.map((row) => {
      const snapshot = row.snapshot_data as AuthSnapshot;
      return {
        ...snapshot,
        device_id: row.device_id,
        received_at: isoTimestamp(row.received_at),
      };
    });
  }

  async close(): Promise<void> {
    await this.#sql.end({ timeout: 5 });
  }
}

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

function registeredDevice(row: DeviceRow): RegisteredDevice {
  return {
    device_id: row.device_id,
    public_key: row.public_key,
    owner_id: row.owner_id,
    created_at: isoTimestamp(row.created_at),
    last_seen_at: isoTimestamp(row.last_seen_at),
    ...(row.revoked_at === null
      ? {}
      : { revoked_at: isoTimestamp(row.revoked_at) }),
  };
}

function isoTimestamp(value: unknown): string {
  const date =
    value instanceof Date
      ? value
      : typeof value === "string" || typeof value === "number"
        ? new Date(value)
        : undefined;
  if (date === undefined || Number.isNaN(date.getTime())) {
    throw new Error("database returned an invalid timestamp");
  }
  return date.toISOString();
}

function syncEvent(value: unknown): SyncEvent {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("database returned invalid event data");
  }
  return value as SyncEvent;
}

function bestSessionEvents(events: SyncEvent[]): SyncEvent[] {
  const sessions = new Map<string, SyncEvent>();
  for (const event of events) {
    const key = `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
    const current = sessions.get(key);
    if (current === undefined || shouldReplaceEvent(current, event)) {
      sessions.set(key, event);
    }
  }
  return [...sessions.values()];
}

function shouldReplaceEvent(current: SyncEvent, incoming: SyncEvent): boolean {
  const currentTime = Date.parse(current.observed_at);
  const incomingTime = Date.parse(incoming.observed_at);
  if (incomingTime !== currentTime) {
    return incomingTime > currentTime;
  }
  const confidenceRank: Record<SyncEvent["confidence"], number> = {
    unknown: 0,
    inferred: 1,
    observed: 2,
    authoritative: 3,
  };
  const rankDifference =
    confidenceRank[incoming.confidence] - confidenceRank[current.confidence];
  return rankDifference === 0
    ? incoming.sequence > current.sequence
    : rankDifference > 0;
}

function classifyPostgresConflict(error: unknown): unknown {
  if (
    typeof error !== "object" ||
    error === null ||
    !("code" in error) ||
    error.code !== "23505"
  ) {
    return error;
  }
  const constraint =
    "constraint_name" in error && typeof error.constraint_name === "string"
      ? error.constraint_name
      : "constraint" in error && typeof error.constraint === "string"
        ? error.constraint
        : undefined;
  if (constraint === "agent_platform_events_pkey") {
    return new GatewayStoreError(
      "event_conflict",
      "event ID was reused with different data",
    );
  }
  if (constraint === "agent_platform_events_device_id_sequence_key") {
    return new GatewayStoreError(
      "sequence_conflict",
      "device sequence was reused by another event",
    );
  }
  if (constraint === "agent_platform_binding_codes_pkey") {
    return new GatewayStoreError(
      "binding_code_conflict",
      "binding code digest already exists",
    );
  }
  return error;
}

function postgresConstraint(error: unknown): string | undefined {
  if (typeof error !== "object" || error === null) return undefined;
  if ("constraint_name" in error && typeof error.constraint_name === "string") {
    return error.constraint_name;
  }
  if ("constraint" in error && typeof error.constraint === "string") {
    return error.constraint;
  }
  return undefined;
}

async function insertAuditEvent(
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

async function insertWorkspaceAuditIfVisible(
  sql: QuerySql,
  input: Omit<AuditRecordInput, "scope"> & {
    workspaceOwnerId: string;
  },
): Promise<boolean> {
  const owners = await sql`
    SELECT 1 FROM agent_pulse_users
    WHERE user_id::text = ${input.workspaceOwnerId}
    LIMIT 1
  `;
  if (owners.length === 0) return false;
  if (input.actorUserId !== undefined) {
    const actors = await sql`
      SELECT 1 FROM agent_pulse_users
      WHERE user_id::text = ${input.actorUserId}
      LIMIT 1
    `;
    if (actors.length === 0) return false;
  }
  await insertAuditEvent(sql, { ...input, scope: "workspace" });
  return true;
}

function normalizeAuditEvent(row: AuditEventRow): AuditEvent {
  if (
    typeof row.metadata !== "object" ||
    row.metadata === null ||
    Array.isArray(row.metadata)
  ) {
    throw new Error("stored audit metadata is invalid");
  }
  return {
    event_id: row.event_id,
    scope: row.scope,
    ...(row.actor_user_id === null ? {} : { actor_user_id: row.actor_user_id }),
    ...(row.workspace_owner_id === null
      ? {}
      : { workspace_owner_id: row.workspace_owner_id }),
    action: row.action,
    ...(row.target_type === null ? {} : { target_type: row.target_type }),
    ...(row.target_id === null ? {} : { target_id: row.target_id }),
    metadata: auditMetadata(row.metadata as Record<string, AuditMetadataValue>),
    created_at: isoTimestamp(row.created_at),
  };
}

async function postgresAccountView(
  sql: QuerySql,
  userId: string,
): Promise<AccountView | undefined> {
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

function normalizeAccountUser(row: AccountUser): AccountUser {
  return {
    user_id: row.user_id,
    email: row.email,
    display_name: row.display_name,
    password_hash: row.password_hash,
    status: row.status,
    ...(row.email_verified_at === undefined || row.email_verified_at === null
      ? {}
      : { email_verified_at: isoTimestamp(row.email_verified_at) }),
    ...(row.oauth_provider === undefined || row.oauth_provider === null
      ? {}
      : { oauth_provider: row.oauth_provider }),
    ...(row.oauth_sub === undefined || row.oauth_sub === null
      ? {}
      : { oauth_sub: row.oauth_sub }),
    created_at: isoTimestamp(row.created_at),
  };
}

function normalizeSubscription(row: SubscriptionRecord): SubscriptionRecord {
  return {
    plan: row.plan,
    status: row.status,
    ...(row.current_period_end === undefined || row.current_period_end === null
      ? {}
      : { current_period_end: isoTimestamp(row.current_period_end) }),
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
      : { last_event_at: isoTimestamp(row.last_event_at) }),
    ...(row.last_event_id === undefined || row.last_event_id === null
      ? {}
      : { last_event_id: row.last_event_id }),
  };
}

function normalizeSubscriptionEvent(
  row: SubscriptionEvent & { current_period_end: unknown | null },
): SubscriptionEvent {
  return {
    event_id: row.event_id,
    user_id: row.user_id,
    provider: row.provider,
    provider_customer_id: row.provider_customer_id,
    provider_subscription_id: row.provider_subscription_id,
    plan: row.plan,
    status: row.status,
    ...(row.current_period_end === null
      ? {}
      : { current_period_end: isoTimestamp(row.current_period_end) }),
    occurred_at: isoTimestamp(row.occurred_at),
  };
}
