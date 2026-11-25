import { randomUUID } from "node:crypto";
import type postgres from "postgres";
import {
  AccountError,
  DUMMY_PASSWORD_HASH,
  accountView,
  normalizeDisplayName,
  normalizeEmail,
  type AccountUser,
  type AccountView,
  type SessionIdentity,
  type SubscriptionRecord,
  type WebSessionView,
  type WorkspaceRole,
} from "../accounts.js";
import {
  isoTimestamp,
  normalizeAccountUser,
  normalizeSubscription,
  postgresConstraint,
} from "../postgres-helpers.js";
import {
  SESSION_ACTIVITY_REFRESH_SECONDS,
  insertAuditEvent,
  postgresAccountView,
} from "./pg-shared.js";

export class PostgresAccountAuthRepo {
  readonly #sql: postgres.Sql;

  constructor(sql: postgres.Sql) {
    this.#sql = sql;
  }

  async hasAnyAdmin(): Promise<boolean> {
    const rows = await this.#sql<{ count: string }[]>`
      SELECT COUNT(*)::text AS count FROM agent_pulse_users WHERE status = 'active'
    `;
    return Number(rows[0]?.count ?? "0") > 0;
  }

  async createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
    singleAdmin?: boolean;
  }): Promise<AccountView> {
    try {
      return await this.#sql.begin(async (sql) => {
        if (input.singleAdmin) {
          await sql`SELECT pg_advisory_xact_lock(1701733484, 99123)`;
          const admins = await sql<{ count: string }[]>`
            SELECT COUNT(*)::text AS count FROM agent_pulse_users WHERE status = 'active'
          `;
          if (Number(admins[0]?.count ?? "0") > 0) {
            throw new AccountError(
              "admin_already_exists",
              "administrator is already configured",
              409,
            );
          }
        }
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
        avatar_data_url,
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
        avatar_data_url,
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
    const email = normalizeEmail(input.email);
    const timestamp = input.now ?? new Date().toISOString();
    const cleanedDisplayName = input.displayName
      .replace(/[\u0000-\u001f\u007f]/gu, " ")
      .trim()
      .slice(0, 80)
      .trim();
    const fallbackDisplayName = normalizeDisplayName(undefined, email)
      .slice(0, 80)
      .trim();
    const effectiveDisplayName =
      cleanedDisplayName.length > 0
        ? cleanedDisplayName
        : fallbackDisplayName || "User";
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        return await this.#sql.begin(async (sql) => {
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
            if (user.status !== "active") {
              throw new AccountError(
                "account_unavailable",
                "account unavailable",
                403,
              );
            }
            if (input.emailVerified && !user.email_verified_at) {
              await sql`
            UPDATE agent_pulse_users
            SET email_verified_at = ${timestamp}::timestamptz, updated_at = clock_timestamp()
            WHERE user_id = ${user.user_id}
          `;
            }
            if (
              cleanedDisplayName.length > 0 &&
              user.display_name !== cleanedDisplayName
            ) {
              await sql`
            UPDATE agent_pulse_users
            SET display_name = ${cleanedDisplayName}, updated_at = clock_timestamp()
            WHERE user_id = ${user.user_id}
          `;
            }
            const view = await postgresAccountView(sql, user.user_id);
            if (view === undefined) {
              throw new AccountError(
                "account_unavailable",
                "account unavailable",
                403,
              );
            }
            return view;
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
        WHERE lower(email) = ${email}
        LIMIT 1
        FOR UPDATE
      `;
          if (byEmail[0] !== undefined) {
            const user = byEmail[0];
            if (user.status !== "active") {
              throw new AccountError(
                "account_unavailable",
                "account unavailable",
                403,
              );
            }
            if (!input.emailVerified) {
              throw new AccountError(
                "oauth_email_not_verified",
                "OAuth account email must be verified before linking",
                403,
              );
            }
            if (
              user.oauth_provider != null &&
              (user.oauth_provider !== input.provider ||
                user.oauth_sub !== input.sub)
            ) {
              throw new AccountError(
                "oauth_account_conflict",
                "account is already linked to another OAuth identity",
                409,
              );
            }
            await sql`
          UPDATE agent_pulse_users
          SET oauth_provider = ${input.provider},
              oauth_sub = ${input.sub},
              email_verified_at = COALESCE(email_verified_at, ${timestamp}::timestamptz),
              updated_at = clock_timestamp()
          WHERE user_id = ${user.user_id}
        `;
            const view = await postgresAccountView(sql, user.user_id);
            if (view === undefined) {
              throw new AccountError(
                "account_unavailable",
                "account unavailable",
                403,
              );
            }
            return view;
          }

          const userId = randomUUID();
          const newUser = await sql<AccountUser[]>`
        INSERT INTO agent_pulse_users (
          user_id, email, password_hash, display_name, status, oauth_provider, oauth_sub, email_verified_at, created_at, updated_at
        ) VALUES (
          ${userId}, ${email}, ${DUMMY_PASSWORD_HASH}, ${effectiveDisplayName},
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
            metadata: {
              plan: "free",
              reason: "oauth",
              provider: input.provider,
            },
            createdAt: timestamp,
          });
          return accountView(
            normalizedUser,
            normalizeSubscription(subscription[0]!),
          );
        });
      } catch (error) {
        if (
          typeof error === "object" &&
          error !== null &&
          "code" in error &&
          error.code === "23505"
        ) {
          const constraint = postgresConstraint(error);
          if (
            constraint === "agent_pulse_users_oauth_idx" ||
            constraint === "agent_pulse_users_email_unique_idx"
          ) {
            if (attempt === 0) continue;
            throw new AccountError(
              constraint === "agent_pulse_users_email_unique_idx"
                ? "email_already_registered"
                : "oauth_account_conflict",
              "account conflict during OAuth sign-in",
              409,
            );
          }
        }
        throw error;
      }
    }
    throw new Error("OAuth account upsert exhausted retries");
  }

  async accountView(userId: string): Promise<AccountView | undefined> {
    return postgresAccountView(this.#sql, userId);
  }

  async setAvatar(
    userId: string,
    avatarDataUrl: string | undefined,
    updatedAt?: string,
  ): Promise<boolean> {
    const rows = await this.#sql<{ user_id: string }[]>`
      UPDATE agent_pulse_users
      SET avatar_data_url = ${avatarDataUrl ?? null},
          updated_at = clock_timestamp()
      WHERE user_id = ${userId} AND status = 'active'
      RETURNING user_id::text
    `;
    if (rows.length === 0) return false;
    await insertAuditEvent(this.#sql, {
      scope: "account",
      accountUserId: userId,
      actorUserId: userId,
      action: "account.avatar_updated",
      targetType: "account",
      targetId: userId,
      metadata: { removed: avatarDataUrl === undefined ? 1 : 0 },
      ...(updatedAt === undefined ? {} : { createdAt: updatedAt }),
    });
    return true;
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
        WHERE sessions.session_digest = ${digest}::text
          AND sessions.expires_at > ${now}::timestamptz
          AND sessions.last_seen_at <=
            clock_timestamp() - make_interval(secs => ${SESSION_ACTIVITY_REFRESH_SECONDS}::int)
        RETURNING sessions.session_digest
      )
      SELECT
        sessions.user_id::text AS actor_user_id,
        sessions.workspace_owner_id::text,
        members.role
      FROM agent_pulse_web_sessions AS sessions
      INNER JOIN agent_pulse_users AS users
        ON users.user_id = sessions.user_id
      INNER JOIN agent_pulse_users AS owners
        ON owners.user_id = sessions.workspace_owner_id
      INNER JOIN agent_pulse_workspace_members AS members
        ON members.user_id = users.user_id
        AND members.workspace_owner_id = sessions.workspace_owner_id
      WHERE sessions.session_digest = ${digest}::text
        AND sessions.expires_at > ${now}::timestamptz
        AND users.status = 'active'
        AND owners.status = 'active'
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
}
