import { randomUUID } from "node:crypto";

import { AccountError, normalizeEmail } from "./account-helpers.js";
import { DUMMY_PASSWORD_HASH, accountView, newUserId } from "./account-auth.js";
import type {
  AccountStore,
  AccountUser,
  AccountView,
  LoginAttemptResult,
  PasswordChangeResult,
  SessionIdentity,
  SubscriptionRecord,
  WebSessionView,
} from "./account-types.js";
import { MemoryAccountState } from "./memory-account-state.js";
import { MemoryAccountWorkspaceStore } from "./memory-account-workspace.js";

export class InMemoryAccountStore implements AccountStore {
  readonly #state = new MemoryAccountState();
  readonly #workspace = new MemoryAccountWorkspaceStore(this.#state);

  async hasAnyAdmin(): Promise<boolean> {
    for (const user of this.#state.users.values()) {
      if (user.status === "active") return true;
    }
    return false;
  }

  async createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
    singleAdmin?: boolean;
  }): Promise<AccountView> {
    if (input.singleAdmin && (await this.hasAnyAdmin())) {
      throw new AccountError(
        "admin_already_exists",
        "administrator is already configured",
        409,
      );
    }
    if (this.#state.userIdsByEmail.has(input.email)) {
      throw new AccountError("email_already_registered", "email exists", 409);
    }
    const user: AccountUser = {
      user_id: input.userId,
      email: input.email,
      display_name: input.displayName,
      password_hash: input.passwordHash,
      status: "active",
      created_at: new Date().toISOString(),
    };
    this.#state.users.set(user.user_id, user);
    this.#state.userIdsByEmail.set(user.email, user.user_id);
    this.#state.subscriptions.set(user.user_id, {
      plan: "free",
      status: "active",
    });
    this.#state.workspaceMembers.set(
      user.user_id,
      new Map([[user.user_id, { role: "owner", createdAt: user.created_at }]]),
    );
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: user.user_id,
      action: "account.created",
      target_type: "account",
      target_id: user.user_id,
      metadata: { plan: "free" },
      created_at: user.created_at,
    });
    return accountView(user, { plan: "free", status: "active" });
  }

  async findAccountByEmail(email: string): Promise<AccountUser | undefined> {
    const userId = this.#state.userIdsByEmail.get(email);
    return userId === undefined ? undefined : this.#state.users.get(userId);
  }

  async findAccountByOAuth(
    provider: string,
    sub: string,
  ): Promise<AccountUser | undefined> {
    return [...this.#state.users.values()].find(
      (user) => user.oauth_provider === provider && user.oauth_sub === sub,
    );
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
    const existing = await this.findAccountByOAuth(input.provider, input.sub);
    if (existing !== undefined) {
      if (existing.status !== "active") {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          403,
        );
      }
      if (input.emailVerified && !existing.email_verified_at) {
        existing.email_verified_at = timestamp;
      }
      if (input.displayName && existing.display_name !== input.displayName) {
        existing.display_name = input.displayName;
      }
      const view = await this.accountView(existing.user_id);
      if (view === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          403,
        );
      }
      return view;
    }

    const byEmail = await this.findAccountByEmail(email);
    if (byEmail !== undefined) {
      if (byEmail.status !== "active") {
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
        byEmail.oauth_provider !== undefined &&
        (byEmail.oauth_provider !== input.provider ||
          byEmail.oauth_sub !== input.sub)
      ) {
        throw new AccountError(
          "oauth_account_conflict",
          "account is already linked to another OAuth identity",
          409,
        );
      }
      byEmail.oauth_provider = input.provider;
      byEmail.oauth_sub = input.sub;
      if (!byEmail.email_verified_at) {
        byEmail.email_verified_at = timestamp;
      }
      const view = await this.accountView(byEmail.user_id);
      if (view === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          403,
        );
      }
      return view;
    }

    const userId = newUserId();
    const newUser: AccountUser = {
      user_id: userId,
      email,
      display_name: input.displayName || email.split("@")[0] || "User",
      password_hash: DUMMY_PASSWORD_HASH,
      status: "active",
      oauth_provider: input.provider,
      oauth_sub: input.sub,
      created_at: timestamp,
      ...(input.emailVerified ? { email_verified_at: timestamp } : {}),
    };
    this.#state.users.set(userId, newUser);
    this.#state.userIdsByEmail.set(email, userId);
    this.#state.subscriptions.set(userId, {
      plan: "free",
      status: "active",
    });
    this.#state.workspaceMembers.set(
      userId,
      new Map([[userId, { role: "owner", createdAt: timestamp }]]),
    );
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: userId,
      action: "account.created",
      target_type: "account",
      target_id: userId,
      metadata: { plan: "free", reason: "oauth", provider: input.provider },
      created_at: timestamp,
    });

    const view = await this.accountView(userId);
    if (view === undefined) {
      throw new Error("failed to generate account view for new OAuth user");
    }
    return view;
  }

  async accountView(userId: string): Promise<AccountView | undefined> {
    return this.#state.accountView(userId);
  }

  async setAvatar(
    userId: string,
    avatarDataUrl: string | undefined,
    updatedAt?: string,
  ): Promise<boolean> {
    const user = this.#state.users.get(userId);
    if (user === undefined || user.status !== "active") return false;
    if (avatarDataUrl === undefined) {
      delete user.avatar_data_url;
    } else {
      user.avatar_data_url = avatarDataUrl;
    }
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: userId,
      action: "account.avatar_updated",
      target_type: "account",
      target_id: userId,
      metadata: { removed: avatarDataUrl === undefined ? 1 : 0 },
      created_at: updatedAt ?? new Date().toISOString(),
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
    this.#state.sessions.set(input.digest, {
      sessionId: input.sessionId,
      userId: input.userId,
      workspaceOwnerId: input.userId,
      createdAt: input.createdAt,
      lastSeenAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: input.userId,
      action: "auth.session_created",
      target_type: "web_session",
      target_id: input.sessionId,
      metadata: { reason: input.reason ?? "login" },
      created_at: input.createdAt,
    });
  }

  async findWebSession(
    digest: string,
    now: string,
  ): Promise<SessionIdentity | undefined> {
    const session = this.#state.sessions.get(digest);
    if (session === undefined || session.expiresAt <= now) return undefined;
    const user = this.#state.users.get(session.userId);
    const owner = this.#state.users.get(session.workspaceOwnerId);
    if (user?.status !== "active" || owner?.status !== "active") {
      return undefined;
    }
    const role = this.#state.workspaceMembers
      .get(session.workspaceOwnerId)
      ?.get(user.user_id)?.role;
    if (role === undefined) return undefined;
    session.lastSeenAt = now;
    return {
      actor_user_id: user.user_id,
      workspace_owner_id: session.workspaceOwnerId,
      role,
    };
  }

  async deleteWebSession(digest: string): Promise<void> {
    const session = this.#state.sessions.get(digest);
    if (session === undefined) return;
    this.#state.sessions.delete(digest);
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: session.userId,
      action: "auth.session_revoked",
      target_type: "web_session",
      target_id: session.sessionId,
      metadata: { reason: "logout" },
      created_at: new Date().toISOString(),
    });
  }

  async listWebSessions(
    userId: string,
    currentDigest: string,
    now: string,
  ): Promise<WebSessionView[]> {
    return [...this.#state.sessions.entries()]
      .filter(
        ([, session]) => session.userId === userId && session.expiresAt > now,
      )
      .map(([digest, session]) => ({
        session_id: session.sessionId,
        created_at: session.createdAt,
        last_seen_at: session.lastSeenAt,
        expires_at: session.expiresAt,
        current: digest === currentDigest,
      }))
      .sort((left, right) =>
        right.last_seen_at.localeCompare(left.last_seen_at),
      );
  }

  async revokeWebSession(userId: string, sessionId: string): Promise<boolean> {
    for (const [digest, session] of this.#state.sessions) {
      if (session.userId === userId && session.sessionId === sessionId) {
        this.#state.sessions.delete(digest);
        this.#state.recordAudit({
          scope: "account",
          actor_user_id: userId,
          action: "auth.session_revoked",
          target_type: "web_session",
          target_id: sessionId,
          metadata: { reason: "manual" },
          created_at: new Date().toISOString(),
        });
        return true;
      }
    }
    return false;
  }

  async revokeOtherWebSessions(
    userId: string,
    currentDigest: string,
  ): Promise<number> {
    let revoked = 0;
    for (const [digest, session] of this.#state.sessions) {
      if (session.userId === userId && digest !== currentDigest) {
        this.#state.sessions.delete(digest);
        revoked += 1;
      }
    }
    if (revoked > 0) {
      this.#state.recordAudit({
        scope: "account",
        actor_user_id: userId,
        action: "auth.other_sessions_revoked",
        metadata: { revoked_sessions: revoked },
        created_at: new Date().toISOString(),
      });
    }
    return revoked;
  }

  async issueEmailVerification(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    const user = this.#state.users.get(input.userId);
    if (user === undefined || user.status !== "active") {
      throw new AccountError("account_unavailable", "account unavailable", 403);
    }
    if (user.email_verified_at !== undefined) {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = [...this.#state.emailVerifications.values()]
      .filter((verification) => verification.userId === input.userId)
      .sort((left, right) => right.createdAt.localeCompare(left.createdAt))[0];
    const nextAllowedAt =
      latest === undefined
        ? 0
        : Date.parse(latest.createdAt) + input.minimumIntervalSeconds * 1_000;
    if (nextAllowedAt > Date.parse(input.createdAt)) {
      return {
        allowed: false,
        retry_after_seconds: Math.ceil(
          (nextAllowedAt - Date.parse(input.createdAt)) / 1_000,
        ),
      };
    }
    for (const verification of this.#state.emailVerifications.values()) {
      if (
        verification.userId === input.userId &&
        verification.consumedAt === undefined
      ) {
        verification.consumedAt = input.createdAt;
      }
    }
    this.#state.emailVerifications.set(input.digest, {
      userId: input.userId,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: input.userId,
      action: "auth.email_verification_requested",
      metadata: {},
      created_at: input.createdAt,
    });
    return { allowed: true, retry_after_seconds: 0 };
  }

  async consumeEmailVerification(
    digest: string,
    now: string,
  ): Promise<AccountView | undefined> {
    const verification = this.#state.emailVerifications.get(digest);
    if (
      verification === undefined ||
      verification.consumedAt !== undefined ||
      verification.expiresAt <= now
    ) {
      return undefined;
    }
    const user = this.#state.users.get(verification.userId);
    const subscription = this.#state.subscriptions.get(verification.userId);
    if (
      user === undefined ||
      subscription === undefined ||
      user.status !== "active"
    ) {
      return undefined;
    }
    verification.consumedAt = now;
    user.email_verified_at ??= now;
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: user.user_id,
      action: "auth.email_verified",
      target_type: "account",
      target_id: user.user_id,
      metadata: {},
      created_at: now,
    });
    return accountView(user, subscription);
  }

  async deletePendingEmailVerification(digest: string): Promise<void> {
    const verification = this.#state.emailVerifications.get(digest);
    if (verification?.consumedAt === undefined) {
      this.#state.emailVerifications.delete(digest);
    }
  }

  async issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    const user = this.#state.users.get(input.userId);
    if (user === undefined || user.status !== "active") {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = [...this.#state.passwordResets.values()]
      .filter((reset) => reset.userId === input.userId)
      .sort((left, right) => right.createdAt.localeCompare(left.createdAt))[0];
    const nextAllowedAt =
      latest === undefined
        ? 0
        : Date.parse(latest.createdAt) + input.minimumIntervalSeconds * 1_000;
    if (nextAllowedAt > Date.parse(input.createdAt)) {
      return {
        allowed: false,
        retry_after_seconds: Math.ceil(
          (nextAllowedAt - Date.parse(input.createdAt)) / 1_000,
        ),
      };
    }
    for (const reset of this.#state.passwordResets.values()) {
      if (reset.userId === input.userId && reset.consumedAt === undefined) {
        reset.consumedAt = input.createdAt;
      }
    }
    this.#state.passwordResets.set(input.digest, {
      userId: input.userId,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: input.userId,
      action: "auth.password_reset_requested",
      metadata: {},
      created_at: input.createdAt,
    });
    return { allowed: true, retry_after_seconds: 0 };
  }

  async consumePasswordReset(input: {
    digest: string;
    passwordHash: string;
    now: string;
  }): Promise<PasswordChangeResult> {
    const reset = this.#state.passwordResets.get(input.digest);
    if (
      reset === undefined ||
      reset.consumedAt !== undefined ||
      reset.expiresAt <= input.now
    ) {
      return { updated: false, revoked_sessions: 0 };
    }
    const user = this.#state.users.get(reset.userId);
    if (user === undefined || user.status !== "active") {
      return { updated: false, revoked_sessions: 0 };
    }
    reset.consumedAt = input.now;
    for (const candidate of this.#state.passwordResets.values()) {
      if (
        candidate.userId === user.user_id &&
        candidate.consumedAt === undefined
      ) {
        candidate.consumedAt = input.now;
      }
    }
    user.password_hash = input.passwordHash;
    let revoked = 0;
    for (const [digest, session] of this.#state.sessions) {
      if (session.userId === user.user_id) {
        this.#state.sessions.delete(digest);
        revoked += 1;
      }
    }
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: user.user_id,
      action: "auth.password_reset_completed",
      target_type: "account",
      target_id: user.user_id,
      metadata: { revoked_sessions: revoked },
      created_at: input.now,
    });
    return { updated: true, revoked_sessions: revoked };
  }

  async deletePendingPasswordReset(digest: string): Promise<void> {
    const reset = this.#state.passwordResets.get(digest);
    if (reset?.consumedAt === undefined)
      this.#state.passwordResets.delete(digest);
  }

  async changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<PasswordChangeResult> {
    const user = this.#state.users.get(input.userId);
    if (
      user === undefined ||
      user.status !== "active" ||
      user.password_hash !== input.expectedPasswordHash
    ) {
      return { updated: false, revoked_sessions: 0 };
    }
    user.password_hash = input.passwordHash;
    const changedAt = input.changedAt ?? new Date().toISOString();
    for (const reset of this.#state.passwordResets.values()) {
      if (reset.userId === input.userId && reset.consumedAt === undefined) {
        reset.consumedAt = changedAt;
      }
    }
    let revoked = 0;
    for (const [digest, session] of this.#state.sessions) {
      if (
        session.userId === input.userId &&
        digest !== input.currentSessionDigest
      ) {
        this.#state.sessions.delete(digest);
        revoked += 1;
      }
    }
    this.#state.recordAudit({
      scope: "account",
      actor_user_id: input.userId,
      action: "auth.password_changed",
      target_type: "account",
      target_id: input.userId,
      metadata: { revoked_sessions: revoked },
      created_at: changedAt,
    });
    return { updated: true, revoked_sessions: revoked };
  }

  async consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult> {
    const nowMs = Date.parse(input.now);
    const current = this.#state.loginLimits.get(input.scopeDigest);
    if (current === undefined) {
      this.#state.loginLimits.set(input.scopeDigest, {
        attemptCount: 1,
        windowStartedAt: input.now,
      });
      return { allowed: true, retry_after_seconds: 0 };
    }
    if (current.blockedUntil !== undefined) {
      const blockedUntil = Date.parse(current.blockedUntil);
      if (blockedUntil > nowMs) {
        return {
          allowed: false,
          retry_after_seconds: Math.ceil((blockedUntil - nowMs) / 1_000),
        };
      }
      this.#state.loginLimits.set(input.scopeDigest, {
        attemptCount: 1,
        windowStartedAt: input.now,
      });
      return { allowed: true, retry_after_seconds: 0 };
    }
    if (
      Date.parse(current.windowStartedAt) + input.windowSeconds * 1_000 <=
      nowMs
    ) {
      this.#state.loginLimits.set(input.scopeDigest, {
        attemptCount: 1,
        windowStartedAt: input.now,
      });
      return { allowed: true, retry_after_seconds: 0 };
    }
    current.attemptCount += 1;
    if (current.attemptCount > input.maximumAttempts) {
      current.blockedUntil = new Date(
        nowMs + input.blockSeconds * 1_000,
      ).toISOString();
      return { allowed: false, retry_after_seconds: input.blockSeconds };
    }
    return { allowed: true, retry_after_seconds: 0 };
  }

  async clearLoginAttempts(scopeDigest: string): Promise<void> {
    this.#state.loginLimits.delete(scopeDigest);
  }

  listWorkspaces(
    ...args: Parameters<AccountStore["listWorkspaces"]>
  ): ReturnType<AccountStore["listWorkspaces"]> {
    return this.#workspace.listWorkspaces(...args);
  }

  workspaceView(
    ...args: Parameters<AccountStore["workspaceView"]>
  ): ReturnType<AccountStore["workspaceView"]> {
    return this.#workspace.workspaceView(...args);
  }

  issueTeamInvitation(
    ...args: Parameters<AccountStore["issueTeamInvitation"]>
  ): ReturnType<AccountStore["issueTeamInvitation"]> {
    return this.#workspace.issueTeamInvitation(...args);
  }

  consumeTeamInvitation(
    ...args: Parameters<AccountStore["consumeTeamInvitation"]>
  ): ReturnType<AccountStore["consumeTeamInvitation"]> {
    return this.#workspace.consumeTeamInvitation(...args);
  }

  revokeTeamInvitation(
    ...args: Parameters<AccountStore["revokeTeamInvitation"]>
  ): ReturnType<AccountStore["revokeTeamInvitation"]> {
    return this.#workspace.revokeTeamInvitation(...args);
  }

  updateWorkspaceMember(
    ...args: Parameters<AccountStore["updateWorkspaceMember"]>
  ): ReturnType<AccountStore["updateWorkspaceMember"]> {
    return this.#workspace.updateWorkspaceMember(...args);
  }

  switchWebSessionWorkspace(
    ...args: Parameters<AccountStore["switchWebSessionWorkspace"]>
  ): ReturnType<AccountStore["switchWebSessionWorkspace"]> {
    return this.#workspace.switchWebSessionWorkspace(...args);
  }

  applySubscriptionEvent(
    ...args: Parameters<AccountStore["applySubscriptionEvent"]>
  ): ReturnType<AccountStore["applySubscriptionEvent"]> {
    return this.#workspace.applySubscriptionEvent(...args);
  }

  billingReference(
    ...args: Parameters<AccountStore["billingReference"]>
  ): ReturnType<AccountStore["billingReference"]> {
    return this.#workspace.billingReference(...args);
  }

  listAuditEvents(
    ...args: Parameters<AccountStore["listAuditEvents"]>
  ): ReturnType<AccountStore["listAuditEvents"]> {
    return this.#workspace.listAuditEvents(...args);
  }

  recordAuditEvent(
    ...args: Parameters<AccountStore["recordAuditEvent"]>
  ): ReturnType<AccountStore["recordAuditEvent"]> {
    return this.#workspace.recordAuditEvent(...args);
  }

  setMemberRemovedHook(
    hook: (
      workspaceOwnerId: string,
      memberUserId: string,
    ) => void | Promise<void>,
  ): void {
    this.#state.onMemberRemoved = hook;
  }

  setSubscription(userId: string, subscription: SubscriptionRecord): void {
    if (!this.#state.users.has(userId)) {
      throw new Error("account does not exist");
    }
    this.#state.subscriptions.set(userId, subscription);
  }
}
