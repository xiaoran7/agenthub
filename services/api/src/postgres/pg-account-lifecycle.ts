import type postgres from "postgres";
import {
  AccountError,
  type AccountView,
  type LoginAttemptResult,
} from "../accounts.js";
import { isoTimestamp } from "../postgres-helpers.js";
import { insertAuditEvent, postgresAccountView } from "./pg-shared.js";

export class PostgresAccountLifecycleRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
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
      const activeUsers = await sql<Array<{ user_id: string }>>`
        SELECT users.user_id::text
        FROM agent_pulse_users AS users
        WHERE users.status = 'active'
          AND users.user_id = (
            SELECT verifications.user_id
            FROM agent_pulse_email_verifications AS verifications
            WHERE verifications.token_digest = ${digest}
              AND verifications.consumed_at IS NULL
              AND verifications.expires_at > ${now}
          )
        FOR UPDATE OF users
      `;
      const lockedUserId = activeUsers[0]?.user_id;
      if (lockedUserId === undefined) return undefined;
      const rows = await sql<Array<{ user_id: string }>>`
        UPDATE agent_pulse_email_verifications
        SET consumed_at = ${now}
        WHERE token_digest = ${digest}
          AND user_id = ${lockedUserId}::uuid
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
        WHERE user_id = ${id}::uuid AND status = 'active'
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
    return userId === undefined
      ? undefined
      : postgresAccountView(this.#sql, userId);
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
      let row:
        | {
            attempt_count: number;
            window_started_at: unknown;
            blocked_until: unknown | null;
          }
        | undefined;
      for (let attempt = 0; attempt < 5; attempt += 1) {
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
        row = rows[0];
        if (row !== undefined) break;
      }
      if (row === undefined) throw new Error("login limit row disappeared");
      const nowMs = Date.parse(input.now);
      if (row.blocked_until !== null) {
        const blockedUntil = Date.parse(isoTimestamp(row.blocked_until));
        if (blockedUntil > nowMs) {
          return {
            allowed: false,
            retry_after_seconds: Math.ceil((blockedUntil - nowMs) / 1_000),
          };
        }
        await sql`
          UPDATE agent_pulse_login_limits
          SET attempt_count = 1, window_started_at = ${input.now}, blocked_until = NULL
          WHERE scope_digest = ${input.scopeDigest}
        `;
        return { allowed: true, retry_after_seconds: 0 };
      }
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
}
