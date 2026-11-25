import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";

import {
  AccountError,
  type AccountStore,
  type AccountUser,
  type AccountView,
  type AuditEvent,
  type AuditQuery,
  type AuditRecordInput,
  type BillingReference,
  type InviteRole,
  type LoginAttemptResult,
  type PasswordChangeResult,
  type SessionIdentity,
  type SubscriptionEvent,
  type SubscriptionEventResult,
  type SubscriptionRecord,
  type WebSessionView,
  type WorkspaceSummary,
} from "./accounts.js";
import {
  changeSqlitePassword,
  consumeSqliteEmailVerification,
  consumeSqliteLoginAttempt,
  consumeSqlitePasswordReset,
  issueSqliteEmailVerification,
  issueSqlitePasswordReset,
  mapSqliteAccountUser,
  mapSqliteAccountView,
  runSqliteTransaction,
  type SqliteUserRow,
} from "./sqlite-account-helpers.js";
import { SQLITE_ACCOUNT_SCHEMA } from "./store-schemas.js";

const SESSION_ACTIVITY_REFRESH_MS = 60_000;

export class SqliteAccountStore implements AccountStore {
  readonly #db: DatabaseSync;
  #closed = false;
  readonly #now: () => Date;

  constructor(
    pathOrDb: string | DatabaseSync,
    options: { now?: () => Date } = {},
  ) {
    if (typeof pathOrDb === "string") {
      const dir = dirname(pathOrDb);
      if (dir && dir !== "." && dir !== "") {
        mkdirSync(dir, { recursive: true });
      }
      this.#db = new DatabaseSync(pathOrDb);
    } else {
      this.#db = pathOrDb;
    }
    this.#now = options.now ?? (() => new Date());
    this.#initTables();
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#db.close();
  }

  #initTables(): void {
    this.#db.exec(SQLITE_ACCOUNT_SCHEMA);
    this.#migrateAvatarColumn();
    try {
      this.#db.exec(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_nocase ON agent_pulse_users(email COLLATE NOCASE)",
      );
    } catch {
      // Existing databases with legacy case-conflicting rows keep the table constraint.
    }
  }

  /**
   * Adds the avatar column to a database written by an older build.
   *
   * `CREATE TABLE IF NOT EXISTS` leaves an existing table alone, so a store
   * created before avatars existed has no `avatar_data_url` column and every
   * query that mentions it would fail. `ALTER TABLE ADD COLUMN` is supported
   * from SQLite 3.35 and errors if the column is already present, so the
   * guard checks the current shape first.
   */
  #migrateAvatarColumn(): void {
    const columns = this.#db
      .prepare("PRAGMA table_info(agent_pulse_users)")
      .all() as Array<{ name: string }>;
    if (!columns.some((column) => column.name === "avatar_data_url")) {
      this.#db.exec(
        "ALTER TABLE agent_pulse_users ADD COLUMN avatar_data_url TEXT",
      );
    }
  }

  async hasAnyAdmin(): Promise<boolean> {
    const row = this.#db
      .prepare(
        "SELECT COUNT(*) as count FROM agent_pulse_users WHERE status = 'active'",
      )
      .get() as { count: number | bigint } | undefined;
    const count = Number(row?.count ?? 0);
    return count > 0;
  }

  async createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
    singleAdmin?: boolean;
  }): Promise<AccountView> {
    const createdAt = this.#now().toISOString();
    try {
      runSqliteTransaction(this.#db, () => {
        const countRow = this.#db
          .prepare(
            "SELECT COUNT(*) as count FROM agent_pulse_users WHERE status = 'active'",
          )
          .get() as { count: number | bigint } | undefined;
        if (input.singleAdmin && Number(countRow?.count ?? 0) > 0) {
          throw new AccountError(
            "admin_already_exists",
            "administrator is already configured",
            409,
          );
        }
        this.#db
          .prepare(
            `INSERT INTO agent_pulse_users (
              user_id, email, display_name, password_hash, status, email_verified_at, created_at
            ) VALUES (?, ?, ?, ?, 'active', ?, ?)`,
          )
          .run(
            input.userId,
            input.email,
            input.displayName,
            input.passwordHash,
            createdAt,
            createdAt,
          );
      });
    } catch (error) {
      if (error instanceof AccountError) throw error;
      if (
        error instanceof Error &&
        (error.message.includes("UNIQUE constraint") ||
          (error as { code?: string }).code === "ERR_SQLITE_ERROR") &&
        (error.message.includes("agent_pulse_users.email") ||
          error.message.includes("UNIQUE"))
      ) {
        throw new AccountError("email_already_registered", "email exists", 409);
      }
      throw error;
    }

    const view = await this.accountView(input.userId);
    if (!view) {
      throw new Error("failed to retrieve created admin account view");
    }
    return view;
  }

  async findAccountByEmail(email: string): Promise<AccountUser | undefined> {
    const row = this.#db
      .prepare("SELECT * FROM agent_pulse_users WHERE email = ? COLLATE NOCASE")
      .get(email) as SqliteUserRow | undefined;
    return row ? mapSqliteAccountUser(row) : undefined;
  }

  async accountView(userId: string): Promise<AccountView | undefined> {
    const row = this.#db
      .prepare("SELECT * FROM agent_pulse_users WHERE user_id = ?")
      .get(userId) as SqliteUserRow | undefined;
    return mapSqliteAccountView(row);
  }

  async setAvatar(
    userId: string,
    avatarDataUrl: string | undefined,
    updatedAt?: string,
  ): Promise<boolean> {
    const status = this.#db
      .prepare("SELECT status FROM agent_pulse_users WHERE user_id = ?")
      .get(userId) as { status: string } | undefined;
    if (status === undefined || status.status !== "active") return false;
    this.#db
      .prepare(
        "UPDATE agent_pulse_users SET avatar_data_url = ? WHERE user_id = ?",
      )
      .run(avatarDataUrl ?? null, userId);
    await this.recordAuditEvent({
      scope: "account",
      accountUserId: userId,
      actorUserId: userId,
      action: "account.avatar_updated",
      targetType: "account",
      targetId: userId,
      metadata: { removed: avatarDataUrl === undefined ? 1 : 0 },
      createdAt: updatedAt ?? this.#now().toISOString(),
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
    this.#db
      .prepare(
        `INSERT INTO agent_pulse_web_sessions (
          digest, session_id, user_id, workspace_owner_id, created_at, last_seen_at, expires_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        input.digest,
        input.sessionId,
        input.userId,
        input.userId,
        input.createdAt,
        input.createdAt,
        input.expiresAt,
      );
  }

  async findWebSession(
    digest: string,
    now: string,
  ): Promise<SessionIdentity | undefined> {
    const row = this.#db
      .prepare(
        `SELECT s.session_id, s.user_id, s.workspace_owner_id, s.last_seen_at, s.expires_at, u.status
         FROM agent_pulse_web_sessions s
         JOIN agent_pulse_users u ON u.user_id = s.user_id
         WHERE s.digest = ? AND s.expires_at > ?`,
      )
      .get(digest, now) as
      | {
          session_id: string;
          user_id: string;
          workspace_owner_id: string;
          last_seen_at: string;
          expires_at: string;
          status: string;
        }
      | undefined;

    if (!row || row.status !== "active") return undefined;

    const nowMs = Date.parse(now);
    const lastSeenMs = Date.parse(row.last_seen_at);
    if (
      !Number.isFinite(lastSeenMs) ||
      nowMs - lastSeenMs >= SESSION_ACTIVITY_REFRESH_MS
    ) {
      this.#db
        .prepare(
          "UPDATE agent_pulse_web_sessions SET last_seen_at = ? WHERE digest = ?",
        )
        .run(now, digest);
    }

    return {
      actor_user_id: row.user_id,
      workspace_owner_id: row.workspace_owner_id,
      role: "owner",
    };
  }

  async deleteWebSession(digest: string): Promise<void> {
    this.#db
      .prepare("DELETE FROM agent_pulse_web_sessions WHERE digest = ?")
      .run(digest);
  }

  async listWebSessions(
    userId: string,
    currentDigest: string,
    now: string,
  ): Promise<WebSessionView[]> {
    const rows = this.#db
      .prepare(
        `SELECT digest, session_id, created_at, last_seen_at, expires_at
         FROM agent_pulse_web_sessions
         WHERE user_id = ? AND expires_at > ?
         ORDER BY last_seen_at DESC`,
      )
      .all(userId, now) as Array<{
      digest: string;
      session_id: string;
      created_at: string;
      last_seen_at: string;
      expires_at: string;
    }>;

    return rows.map((row) => ({
      session_id: row.session_id,
      created_at: row.created_at,
      last_seen_at: row.last_seen_at,
      expires_at: row.expires_at,
      current: row.digest === currentDigest,
    }));
  }

  async revokeWebSession(userId: string, sessionId: string): Promise<boolean> {
    const info = this.#db
      .prepare(
        "DELETE FROM agent_pulse_web_sessions WHERE user_id = ? AND session_id = ?",
      )
      .run(userId, sessionId);
    return Number(info.changes ?? 0) > 0;
  }

  async revokeOtherWebSessions(
    userId: string,
    currentDigest: string,
  ): Promise<number> {
    const info = this.#db
      .prepare(
        "DELETE FROM agent_pulse_web_sessions WHERE user_id = ? AND digest != ?",
      )
      .run(userId, currentDigest);
    return Number(info.changes ?? 0);
  }

  async changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<PasswordChangeResult> {
    return changeSqlitePassword(this.#db, {
      ...input,
      changedAt: input.changedAt ?? this.#now().toISOString(),
    });
  }

  async consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult> {
    return consumeSqliteLoginAttempt(this.#db, input);
  }

  async clearLoginAttempts(scopeDigest: string): Promise<void> {
    this.#db
      .prepare("DELETE FROM agent_pulse_login_attempts WHERE scope_digest = ?")
      .run(scopeDigest);
  }

  async listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]> {
    const view = await this.accountView(userId);
    if (!view) return [];
    return [
      {
        workspace_owner_id: userId,
        name: "默认工作区",
        role: "owner",
        current: currentOwnerId === userId,
      },
    ];
  }

  async workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    _now?: string,
  ) {
    if (actorUserId !== workspaceOwnerId) return undefined;
    const view = await this.accountView(workspaceOwnerId);
    if (!view) return undefined;
    return {
      workspace: {
        workspace_owner_id: workspaceOwnerId,
        name: "默认工作区",
        role: "owner" as const,
        current: true,
      },
      members: [
        {
          user_id: view.user.user_id,
          email: view.user.email,
          display_name: view.user.display_name,
          role: "owner" as const,
          joined_at: view.user.created_at,
        },
      ],
      invitations: [],
      member_limit: memberLimit,
    };
  }

  async issueEmailVerification(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    return issueSqliteEmailVerification(this.#db, input);
  }

  async consumeEmailVerification(
    digest: string,
    now: string,
  ): Promise<AccountView | undefined> {
    return consumeSqliteEmailVerification(this.#db, digest, now);
  }

  async deletePendingEmailVerification(digest: string): Promise<void> {
    this.#db
      .prepare(
        "DELETE FROM agent_pulse_email_verifications WHERE digest = ? AND consumed_at IS NULL",
      )
      .run(digest);
  }

  async issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    return issueSqlitePasswordReset(this.#db, input);
  }

  async consumePasswordReset(input: {
    digest: string;
    passwordHash: string;
    now: string;
  }): Promise<PasswordChangeResult> {
    return consumeSqlitePasswordReset(this.#db, input);
  }

  async deletePendingPasswordReset(digest: string): Promise<void> {
    this.#db
      .prepare(
        "DELETE FROM agent_pulse_password_resets WHERE digest = ? AND consumed_at IS NULL",
      )
      .run(digest);
  }

  async issueTeamInvitation(_input: Record<string, unknown>): Promise<void> {
    throw new AccountError(
      "not_supported",
      "team invitations not supported in self-hosted single-admin mode",
      400,
    );
  }

  async consumeTeamInvitation(_input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined> {
    return undefined;
  }

  async revokeTeamInvitation(
    _actorUserId: string,
    _workspaceOwnerId: string,
    _invitationId: string,
    _now: string,
  ): Promise<boolean> {
    return false;
  }

  async updateMemberRole(_input: {
    actorUserId: string;
    workspaceOwnerId: string;
    targetUserId: string;
    role: InviteRole;
  }): Promise<boolean> {
    return false;
  }

  async removeMember(_input: {
    actorUserId: string;
    workspaceOwnerId: string;
    targetUserId: string;
  }): Promise<boolean> {
    return false;
  }

  async leaveWorkspace(_input: {
    userId: string;
    workspaceOwnerId: string;
  }): Promise<boolean> {
    return false;
  }

  async updateWorkspaceMember(_input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean> {
    return false;
  }

  async switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined> {
    if (input.workspaceOwnerId !== input.actorUserId) return undefined;
    const row = this.#db
      .prepare(
        "SELECT user_id FROM agent_pulse_web_sessions WHERE digest = ? AND user_id = ?",
      )
      .get(input.digest, input.actorUserId);
    if (!row) return undefined;
    return {
      actor_user_id: input.actorUserId,
      workspace_owner_id: input.actorUserId,
      role: "owner",
    };
  }

  async pruneExpiredData(nowIso: string): Promise<void> {
    const windowCutoff = new Date(
      Date.parse(nowIso) - 86_400_000,
    ).toISOString();
    this.#db
      .prepare("DELETE FROM agent_pulse_web_sessions WHERE expires_at <= ?")
      .run(nowIso);
    this.#db
      .prepare(
        "DELETE FROM agent_pulse_email_verifications WHERE expires_at <= ?",
      )
      .run(nowIso);
    this.#db
      .prepare("DELETE FROM agent_pulse_password_resets WHERE expires_at <= ?")
      .run(nowIso);
    this.#db
      .prepare(
        `DELETE FROM agent_pulse_login_attempts
         WHERE (blocked_until IS NOT NULL AND blocked_until <= ?)
            OR (blocked_until IS NULL AND window_started_at <= ?)`,
      )
      .run(nowIso, windowCutoff);
  }

  async applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult> {
    const view = await this.accountView(event.user_id);
    if (!view) {
      throw new AccountError("account_not_found", "account not found", 404);
    }
    return { applied: true, duplicate: false, account: view };
  }

  async billingReference(
    _userId: string,
  ): Promise<BillingReference | undefined> {
    return undefined;
  }

  async recordAuditEvent(_input: AuditRecordInput): Promise<boolean> {
    return true;
  }

  async listAuditEvents(_input: AuditQuery): Promise<AuditEvent[]> {
    return [];
  }

  async setSubscription(
    userId: string,
    _subscription: Partial<SubscriptionRecord>,
  ): Promise<AccountView | undefined> {
    return this.accountView(userId);
  }

  async deleteAccount(userId: string): Promise<boolean> {
    const info = this.#db
      .prepare(
        "UPDATE agent_pulse_users SET status = 'deleted' WHERE user_id = ?",
      )
      .run(userId);
    return Number(info.changes ?? 0) > 0;
  }
}
