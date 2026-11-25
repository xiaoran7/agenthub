import type { DatabaseSync } from "node:sqlite";

import {
  AccountError,
  entitlementsFor,
  type AccountUser,
  type AccountView,
  type LoginAttemptResult,
  type PasswordChangeResult,
  type SubscriptionRecord,
} from "./accounts.js";

export interface SqliteUserRow {
  user_id: string;
  email: string;
  display_name: string;
  password_hash: string;
  status: "active" | "suspended" | "deleted";
  email_verified_at: string | null;
  avatar_data_url: string | null;
  created_at: string;
}

export function runSqliteTransaction<T>(db: DatabaseSync, work: () => T): T {
  db.exec("BEGIN IMMEDIATE");
  try {
    const result = work();
    db.exec("COMMIT");
    return result;
  } catch (error) {
    try {
      db.exec("ROLLBACK");
    } catch {
      // already rolled back
    }
    throw error;
  }
}

export function mapSqliteAccountUser(row: SqliteUserRow): AccountUser {
  const user: AccountUser = {
    user_id: row.user_id,
    email: row.email,
    display_name: row.display_name,
    password_hash: row.password_hash,
    status: row.status,
    created_at: row.created_at,
  };
  if (row.email_verified_at !== null) {
    user.email_verified_at = row.email_verified_at;
  }
  if (row.avatar_data_url !== null) {
    user.avatar_data_url = row.avatar_data_url;
  }
  return user;
}

export function mapSqliteAccountView(
  row: SqliteUserRow | undefined,
): AccountView | undefined {
  if (!row || row.status !== "active") return undefined;
  const user: Omit<AccountUser, "password_hash"> = {
    user_id: row.user_id,
    email: row.email,
    display_name: row.display_name,
    status: row.status,
    created_at: row.created_at,
  };
  if (row.email_verified_at !== null) {
    user.email_verified_at = row.email_verified_at;
  }
  if (row.avatar_data_url !== null) {
    user.avatar_data_url = row.avatar_data_url;
  }
  const subscription: SubscriptionRecord = {
    plan: "pro",
    status: "active",
  };
  return {
    user,
    subscription,
    entitlements: entitlementsFor(subscription),
  };
}

export function consumeSqliteLoginAttempt(
  db: DatabaseSync,
  input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  },
): LoginAttemptResult {
  const nowMs = Date.parse(input.now);
  return runSqliteTransaction(db, () => {
    const row = db
      .prepare(
        "SELECT * FROM agent_pulse_login_attempts WHERE scope_digest = ?",
      )
      .get(input.scopeDigest) as
      | {
          scope_digest: string;
          attempt_count: number;
          window_started_at: string;
          blocked_until: string | null;
        }
      | undefined;

    const resetWindow = (): LoginAttemptResult => {
      db.prepare(
        `INSERT INTO agent_pulse_login_attempts (scope_digest, attempt_count, window_started_at, blocked_until)
         VALUES (?, 1, ?, NULL)
         ON CONFLICT(scope_digest) DO UPDATE SET
           attempt_count = 1,
           window_started_at = excluded.window_started_at,
           blocked_until = NULL`,
      ).run(input.scopeDigest, input.now);
      return { allowed: true, retry_after_seconds: 0 };
    };

    if (!row) return resetWindow();

    if (row.blocked_until !== null) {
      const blockedUntil = Date.parse(row.blocked_until);
      if (blockedUntil > nowMs) {
        return {
          allowed: false,
          retry_after_seconds: Math.ceil((blockedUntil - nowMs) / 1_000),
        };
      }
      return resetWindow();
    }

    if (
      Date.parse(row.window_started_at) + input.windowSeconds * 1_000 <=
      nowMs
    ) {
      return resetWindow();
    }

    const nextAttempts = row.attempt_count + 1;
    if (nextAttempts > input.maximumAttempts) {
      const blockedUntil = new Date(
        nowMs + input.blockSeconds * 1_000,
      ).toISOString();
      db.prepare(
        "UPDATE agent_pulse_login_attempts SET attempt_count = ?, blocked_until = ? WHERE scope_digest = ?",
      ).run(nextAttempts, blockedUntil, input.scopeDigest);
      return { allowed: false, retry_after_seconds: input.blockSeconds };
    }

    db.prepare(
      "UPDATE agent_pulse_login_attempts SET attempt_count = ? WHERE scope_digest = ?",
    ).run(nextAttempts, input.scopeDigest);
    return { allowed: true, retry_after_seconds: 0 };
  });
}

export function changeSqlitePassword(
  db: DatabaseSync,
  input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt: string;
  },
): PasswordChangeResult {
  return runSqliteTransaction(db, () => {
    const updated = db
      .prepare(
        "UPDATE agent_pulse_users SET password_hash = ? WHERE user_id = ? AND status = 'active' AND password_hash = ?",
      )
      .run(input.passwordHash, input.userId, input.expectedPasswordHash);
    if (Number(updated.changes ?? 0) === 0) {
      return { updated: false, revoked_sessions: 0 };
    }
    db.prepare(
      "UPDATE agent_pulse_password_resets SET consumed_at = ? WHERE user_id = ? AND consumed_at IS NULL",
    ).run(input.changedAt, input.userId);
    const revoked = db
      .prepare(
        "DELETE FROM agent_pulse_web_sessions WHERE user_id = ? AND digest != ?",
      )
      .run(input.userId, input.currentSessionDigest);
    return { updated: true, revoked_sessions: Number(revoked.changes ?? 0) };
  });
}

export function issueSqliteEmailVerification(
  db: DatabaseSync,
  input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  },
): LoginAttemptResult {
  return runSqliteTransaction(db, () => {
    const user = db
      .prepare(
        "SELECT email_verified_at, status FROM agent_pulse_users WHERE user_id = ?",
      )
      .get(input.userId) as
      { email_verified_at: string | null; status: string } | undefined;
    if (!user || user.status !== "active") {
      throw new AccountError("account_unavailable", "account unavailable", 403);
    }
    if (user.email_verified_at !== null) {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = db
      .prepare(
        "SELECT created_at FROM agent_pulse_email_verifications WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
      )
      .get(input.userId) as { created_at: string } | undefined;
    if (latest) {
      const waitSec = Math.ceil(
        (Date.parse(latest.created_at) +
          input.minimumIntervalSeconds * 1_000 -
          Date.parse(input.createdAt)) /
          1_000,
      );
      if (waitSec > 0) return { allowed: false, retry_after_seconds: waitSec };
    }
    db.prepare(
      "UPDATE agent_pulse_email_verifications SET consumed_at = ? WHERE user_id = ? AND consumed_at IS NULL",
    ).run(input.createdAt, input.userId);
    db.prepare(
      "INSERT INTO agent_pulse_email_verifications (digest, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    ).run(input.digest, input.userId, input.createdAt, input.expiresAt);
    return { allowed: true, retry_after_seconds: 0 };
  });
}

export function consumeSqliteEmailVerification(
  db: DatabaseSync,
  digest: string,
  now: string,
): AccountView | undefined {
  return runSqliteTransaction(db, () => {
    const row = db
      .prepare(
        "SELECT user_id, expires_at, consumed_at FROM agent_pulse_email_verifications WHERE digest = ?",
      )
      .get(digest) as
      | { user_id: string; expires_at: string; consumed_at: string | null }
      | undefined;
    if (!row || row.consumed_at !== null || row.expires_at <= now) {
      return undefined;
    }
    const updatedUser = db
      .prepare(
        "UPDATE agent_pulse_users SET email_verified_at = COALESCE(email_verified_at, ?) WHERE user_id = ? AND status = 'active'",
      )
      .run(now, row.user_id);
    if (Number(updatedUser.changes ?? 0) === 0) return undefined;
    db.prepare(
      "UPDATE agent_pulse_email_verifications SET consumed_at = ? WHERE digest = ?",
    ).run(now, digest);
    const userRow = db
      .prepare("SELECT * FROM agent_pulse_users WHERE user_id = ?")
      .get(row.user_id) as SqliteUserRow | undefined;
    return mapSqliteAccountView(userRow);
  });
}

export function issueSqlitePasswordReset(
  db: DatabaseSync,
  input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  },
): LoginAttemptResult {
  return runSqliteTransaction(db, () => {
    const user = db
      .prepare("SELECT status FROM agent_pulse_users WHERE user_id = ?")
      .get(input.userId) as { status: string } | undefined;
    if (!user || user.status !== "active") {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = db
      .prepare(
        "SELECT created_at FROM agent_pulse_password_resets WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
      )
      .get(input.userId) as { created_at: string } | undefined;
    if (latest) {
      const waitSec = Math.ceil(
        (Date.parse(latest.created_at) +
          input.minimumIntervalSeconds * 1_000 -
          Date.parse(input.createdAt)) /
          1_000,
      );
      if (waitSec > 0) return { allowed: false, retry_after_seconds: waitSec };
    }
    db.prepare(
      "UPDATE agent_pulse_password_resets SET consumed_at = ? WHERE user_id = ? AND consumed_at IS NULL",
    ).run(input.createdAt, input.userId);
    db.prepare(
      "INSERT INTO agent_pulse_password_resets (digest, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)",
    ).run(input.digest, input.userId, input.createdAt, input.expiresAt);
    return { allowed: true, retry_after_seconds: 0 };
  });
}

export function consumeSqlitePasswordReset(
  db: DatabaseSync,
  input: {
    digest: string;
    passwordHash: string;
    now: string;
  },
): PasswordChangeResult {
  return runSqliteTransaction(db, () => {
    const row = db
      .prepare(
        "SELECT user_id, expires_at, consumed_at FROM agent_pulse_password_resets WHERE digest = ?",
      )
      .get(input.digest) as
      | { user_id: string; expires_at: string; consumed_at: string | null }
      | undefined;
    if (!row || row.consumed_at !== null || row.expires_at <= input.now) {
      return { updated: false, revoked_sessions: 0 };
    }
    const updatedUser = db
      .prepare(
        "UPDATE agent_pulse_users SET password_hash = ? WHERE user_id = ? AND status = 'active'",
      )
      .run(input.passwordHash, row.user_id);
    if (Number(updatedUser.changes ?? 0) === 0) {
      return { updated: false, revoked_sessions: 0 };
    }
    db.prepare(
      "UPDATE agent_pulse_password_resets SET consumed_at = ? WHERE user_id = ? AND consumed_at IS NULL",
    ).run(input.now, row.user_id);
    const info = db
      .prepare("DELETE FROM agent_pulse_web_sessions WHERE user_id = ?")
      .run(row.user_id);
    return { updated: true, revoked_sessions: Number(info.changes ?? 0) };
  });
}
