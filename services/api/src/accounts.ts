import {
  createHash,
  randomBytes,
  randomUUID,
  scrypt as scryptCallback,
  timingSafeEqual,
} from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import { ContractError } from "@agent-pulse/contracts";
import type { FastifyReply, FastifyRequest } from "fastify";

import { AuthenticationError, type OwnerAuthenticator } from "./auth.js";

const SCRYPT_N = 16_384;
const SCRYPT_R = 8;
const SCRYPT_P = 1;
const PASSWORD_HASH_BYTES = 32;
const SESSION_BYTES = 32;
const SESSION_TTL_MS = 30 * 24 * 60 * 60 * 1_000;
const LOCAL_COOKIE_NAME = "agent_pulse_session";
const SECURE_COOKIE_NAME = "__Host-agent_pulse_session";
const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/u;
export const DUMMY_PASSWORD_HASH = [
  "scrypt",
  SCRYPT_N,
  SCRYPT_R,
  SCRYPT_P,
  Buffer.alloc(16).toString("base64url"),
  Buffer.alloc(PASSWORD_HASH_BYTES).toString("base64url"),
].join("$");

export const subscriptionPlans = ["free", "pro", "team"] as const;
export const subscriptionStatuses = [
  "trialing",
  "active",
  "past_due",
  "canceled",
  "expired",
] as const;

export type SubscriptionPlan = (typeof subscriptionPlans)[number];
export type SubscriptionStatus = (typeof subscriptionStatuses)[number];

export interface AccountUser {
  user_id: string;
  email: string;
  display_name: string;
  password_hash: string;
  status: "active" | "suspended" | "deleted";
  oauth_provider?: string;
  oauth_sub?: string;
  email_verified_at?: string;
  created_at: string;
}

export interface SubscriptionRecord {
  plan: SubscriptionPlan;
  status: SubscriptionStatus;
  provider?: string;
  provider_customer_id?: string;
  provider_subscription_id?: string;
  current_period_end?: string;
  last_event_at?: string;
  last_event_id?: string;
}

export interface WebSessionView {
  session_id: string;
  created_at: string;
  last_seen_at: string;
  expires_at: string;
  current: boolean;
}

export type WorkspaceRole = "owner" | "admin" | "member";
export type InviteRole = Exclude<WorkspaceRole, "owner">;

export interface SessionIdentity {
  actor_user_id: string;
  workspace_owner_id: string;
  role: WorkspaceRole;
}

export interface WorkspaceSummary {
  workspace_owner_id: string;
  name: string;
  role: WorkspaceRole;
  current: boolean;
}

export interface WorkspaceMemberView {
  user_id: string;
  email: string;
  display_name: string;
  role: WorkspaceRole;
  joined_at: string;
}

export interface TeamInvitationView {
  invitation_id: string;
  email: string;
  role: InviteRole;
  created_at: string;
  expires_at: string;
}

export interface TeamWorkspaceView {
  workspace: WorkspaceSummary;
  members: WorkspaceMemberView[];
  invitations: TeamInvitationView[];
  member_limit: number;
}

export interface SubscriptionEvent {
  event_id: string;
  user_id: string;
  provider: string;
  provider_customer_id: string;
  provider_subscription_id: string;
  plan: SubscriptionPlan;
  status: SubscriptionStatus;
  current_period_end?: string;
  occurred_at: string;
}

export interface SubscriptionEventResult {
  applied: boolean;
  duplicate: boolean;
  account: AccountView;
}

export interface BillingReference {
  provider: string;
  customer_id: string;
  subscription_id?: string;
}

export interface LoginAttemptResult {
  allowed: boolean;
  retry_after_seconds: number;
}

export interface PasswordChangeResult {
  updated: boolean;
  revoked_sessions: number;
}

export const auditActions = [
  "account.created",
  "auth.session_created",
  "auth.session_revoked",
  "auth.other_sessions_revoked",
  "auth.email_verification_requested",
  "auth.email_verified",
  "auth.password_reset_requested",
  "auth.password_reset_completed",
  "auth.password_changed",
  "workspace.switched",
  "team.invitation_created",
  "team.invitation_revoked",
  "team.invitation_accepted",
  "team.member_role_updated",
  "team.member_removed",
  "subscription.updated",
  "device.binding_created",
  "device.registered",
  "device.revoked",
  "task.created",
  "task.canceled",
  "task.claimed",
  "task.status_updated",
] as const;

export type AuditAction = (typeof auditActions)[number];
export type AuditScope = "account" | "workspace";
export type AuditTargetType =
  | "account"
  | "web_session"
  | "workspace"
  | "team_invitation"
  | "team_member"
  | "subscription"
  | "device"
  | "remote_task";
export type AuditMetadataValue = string | number | boolean;

export interface AuditEvent {
  event_id: string;
  scope: AuditScope;
  actor_user_id?: string;
  workspace_owner_id?: string;
  action: AuditAction;
  target_type?: AuditTargetType;
  target_id?: string;
  metadata: Record<string, AuditMetadataValue>;
  created_at: string;
}

export interface AuditQuery {
  scope: AuditScope;
  actorUserId: string;
  workspaceOwnerId: string;
  beforeEventId?: string;
  limit: number;
}

export interface AuditRecordInput {
  scope: AuditScope;
  accountUserId?: string | undefined;
  workspaceOwnerId?: string | undefined;
  actorUserId?: string | undefined;
  action: AuditAction;
  targetType?: AuditTargetType | undefined;
  targetId?: string | undefined;
  metadata: Record<string, AuditMetadataValue>;
  createdAt?: string | undefined;
}

export interface Entitlements {
  max_devices: number;
  max_concurrent_tasks: number;
  history_days: number;
  remote_tasks: boolean;
  team_members: number;
}

export interface AccountView {
  user: Omit<AccountUser, "password_hash">;
  subscription: Pick<
    SubscriptionRecord,
    "plan" | "status" | "current_period_end"
  >;
  entitlements: Entitlements;
}

export interface AccountStore {
  createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
  }): Promise<AccountView>;
  findAccountByEmail(email: string): Promise<AccountUser | undefined>;
  findAccountByOAuth?(
    provider: string,
    sub: string,
  ): Promise<AccountUser | undefined>;
  upsertOAuthAccount?(input: {
    provider: string;
    sub: string;
    email: string;
    displayName: string;
    emailVerified?: boolean;
    now?: string;
  }): Promise<AccountView>;
  accountView(userId: string): Promise<AccountView | undefined>;
  createWebSession(input: {
    sessionId: string;
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    reason?: "registration" | "login";
  }): Promise<void>;
  findWebSession(
    digest: string,
    now: string,
  ): Promise<SessionIdentity | undefined>;
  deleteWebSession(digest: string): Promise<void>;
  listWebSessions(
    userId: string,
    currentDigest: string,
    now: string,
  ): Promise<WebSessionView[]>;
  revokeWebSession(userId: string, sessionId: string): Promise<boolean>;
  revokeOtherWebSessions(
    userId: string,
    currentDigest: string,
  ): Promise<number>;
  issueEmailVerification(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult>;
  consumeEmailVerification(
    digest: string,
    now: string,
  ): Promise<AccountView | undefined>;
  deletePendingEmailVerification(digest: string): Promise<void>;
  issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult>;
  consumePasswordReset(input: {
    digest: string;
    passwordHash: string;
    now: string;
  }): Promise<PasswordChangeResult>;
  deletePendingPasswordReset(digest: string): Promise<void>;
  changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<PasswordChangeResult>;
  consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult>;
  clearLoginAttempts(scopeDigest: string): Promise<void>;
  listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]>;
  workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined>;
  issueTeamInvitation(input: {
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
  }): Promise<void>;
  consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined>;
  revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean>;
  updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean>;
  switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined>;
  applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult>;
  billingReference(userId: string): Promise<BillingReference | undefined>;
  recordAuditEvent(input: AuditRecordInput): Promise<boolean>;
  listAuditEvents(input: AuditQuery): Promise<AuditEvent[]>;
}

export class InMemoryAccountStore implements AccountStore {
  readonly #users = new Map<string, AccountUser>();
  readonly #userIdsByEmail = new Map<string, string>();
  readonly #subscriptions = new Map<string, SubscriptionRecord>();
  readonly #sessions = new Map<
    string,
    {
      sessionId: string;
      userId: string;
      workspaceOwnerId: string;
      createdAt: string;
      lastSeenAt: string;
      expiresAt: string;
    }
  >();
  readonly #emailVerifications = new Map<
    string,
    {
      userId: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
    }
  >();
  readonly #loginLimits = new Map<
    string,
    { attemptCount: number; windowStartedAt: string; blockedUntil?: string }
  >();
  readonly #passwordResets = new Map<
    string,
    {
      userId: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
    }
  >();
  readonly #subscriptionEvents = new Map<
    string,
    { event: SubscriptionEvent; applied: boolean }
  >();
  readonly #workspaceMembers = new Map<
    string,
    Map<string, { role: WorkspaceRole; createdAt: string }>
  >();
  readonly #teamInvitations = new Map<
    string,
    {
      invitationId: string;
      workspaceOwnerId: string;
      invitedUserId: string;
      invitedEmail: string;
      role: InviteRole;
      invitedBy: string;
      createdAt: string;
      expiresAt: string;
      consumedAt?: string;
      revokedAt?: string;
    }
  >();
  readonly #auditEvents: AuditEvent[] = [];

  async createAccount(input: {
    userId: string;
    email: string;
    displayName: string;
    passwordHash: string;
  }): Promise<AccountView> {
    if (this.#userIdsByEmail.has(input.email)) {
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
    this.#users.set(user.user_id, user);
    this.#userIdsByEmail.set(user.email, user.user_id);
    this.#subscriptions.set(user.user_id, { plan: "free", status: "active" });
    this.#workspaceMembers.set(
      user.user_id,
      new Map([[user.user_id, { role: "owner", createdAt: user.created_at }]]),
    );
    this.#recordAudit({
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
    const userId = this.#userIdsByEmail.get(email);
    return userId === undefined ? undefined : this.#users.get(userId);
  }

  async findAccountByOAuth(
    provider: string,
    sub: string,
  ): Promise<AccountUser | undefined> {
    return [...this.#users.values()].find(
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
    const timestamp = input.now ?? new Date().toISOString();
    const existing = await this.findAccountByOAuth(input.provider, input.sub);
    if (existing !== undefined) {
      if (input.emailVerified && !existing.email_verified_at) {
        existing.email_verified_at = timestamp;
      }
      if (input.displayName && existing.display_name !== input.displayName) {
        existing.display_name = input.displayName;
      }
      const view = await this.accountView(existing.user_id);
      if (view !== undefined) return view;
    }

    const byEmail = await this.findAccountByEmail(input.email);
    if (byEmail !== undefined) {
      byEmail.oauth_provider = input.provider;
      byEmail.oauth_sub = input.sub;
      if (input.emailVerified && !byEmail.email_verified_at) {
        byEmail.email_verified_at = timestamp;
      }
      const view = await this.accountView(byEmail.user_id);
      if (view !== undefined) return view;
    }

    const userId = newUserId();
    const newUser: AccountUser = {
      user_id: userId,
      email: input.email,
      display_name: input.displayName || input.email.split("@")[0] || "User",
      password_hash: DUMMY_PASSWORD_HASH,
      status: "active",
      oauth_provider: input.provider,
      oauth_sub: input.sub,
      created_at: timestamp,
      ...(input.emailVerified ? { email_verified_at: timestamp } : {}),
    };
    this.#users.set(userId, newUser);
    this.#userIdsByEmail.set(input.email, userId);
    this.#subscriptions.set(userId, {
      plan: "free",
      status: "active",
    });
    this.#workspaceMembers.set(
      userId,
      new Map([[userId, { role: "owner", createdAt: timestamp }]]),
    );
    this.#recordAudit({
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
    const user = this.#users.get(userId);
    const subscription = this.#subscriptions.get(userId);
    return user === undefined || subscription === undefined
      ? undefined
      : accountView(user, subscription);
  }

  async createWebSession(input: {
    sessionId: string;
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    reason?: "registration" | "login";
  }): Promise<void> {
    this.#sessions.set(input.digest, {
      sessionId: input.sessionId,
      userId: input.userId,
      workspaceOwnerId: input.userId,
      createdAt: input.createdAt,
      lastSeenAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#recordAudit({
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
    const session = this.#sessions.get(digest);
    if (session === undefined || session.expiresAt <= now) return undefined;
    const user = this.#users.get(session.userId);
    session.lastSeenAt = now;
    if (user?.status !== "active") return undefined;
    const role = this.#workspaceMembers
      .get(session.workspaceOwnerId)
      ?.get(user.user_id)?.role;
    if (role === undefined) return undefined;
    return {
      actor_user_id: user.user_id,
      workspace_owner_id: session.workspaceOwnerId,
      role,
    };
  }

  async deleteWebSession(digest: string): Promise<void> {
    const session = this.#sessions.get(digest);
    if (session === undefined) return;
    this.#sessions.delete(digest);
    this.#recordAudit({
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
    return [...this.#sessions.entries()]
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
    for (const [digest, session] of this.#sessions) {
      if (session.userId === userId && session.sessionId === sessionId) {
        this.#sessions.delete(digest);
        this.#recordAudit({
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
    for (const [digest, session] of this.#sessions) {
      if (session.userId === userId && digest !== currentDigest) {
        this.#sessions.delete(digest);
        revoked += 1;
      }
    }
    if (revoked > 0) {
      this.#recordAudit({
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
    const user = this.#users.get(input.userId);
    if (user === undefined || user.status !== "active") {
      throw new AccountError("account_unavailable", "account unavailable", 403);
    }
    if (user.email_verified_at !== undefined) {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = [...this.#emailVerifications.values()]
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
    for (const verification of this.#emailVerifications.values()) {
      if (
        verification.userId === input.userId &&
        verification.consumedAt === undefined
      ) {
        verification.consumedAt = input.createdAt;
      }
    }
    this.#emailVerifications.set(input.digest, {
      userId: input.userId,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#recordAudit({
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
    const verification = this.#emailVerifications.get(digest);
    if (
      verification === undefined ||
      verification.consumedAt !== undefined ||
      verification.expiresAt <= now
    ) {
      return undefined;
    }
    const user = this.#users.get(verification.userId);
    const subscription = this.#subscriptions.get(verification.userId);
    if (
      user === undefined ||
      subscription === undefined ||
      user.status !== "active"
    ) {
      return undefined;
    }
    verification.consumedAt = now;
    user.email_verified_at ??= now;
    this.#recordAudit({
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
    const verification = this.#emailVerifications.get(digest);
    if (verification?.consumedAt === undefined) {
      this.#emailVerifications.delete(digest);
    }
  }

  async issuePasswordReset(input: {
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
    minimumIntervalSeconds: number;
  }): Promise<LoginAttemptResult> {
    const user = this.#users.get(input.userId);
    if (user === undefined || user.status !== "active") {
      return { allowed: false, retry_after_seconds: 0 };
    }
    const latest = [...this.#passwordResets.values()]
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
    for (const reset of this.#passwordResets.values()) {
      if (reset.userId === input.userId && reset.consumedAt === undefined) {
        reset.consumedAt = input.createdAt;
      }
    }
    this.#passwordResets.set(input.digest, {
      userId: input.userId,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#recordAudit({
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
    const reset = this.#passwordResets.get(input.digest);
    if (
      reset === undefined ||
      reset.consumedAt !== undefined ||
      reset.expiresAt <= input.now
    ) {
      return { updated: false, revoked_sessions: 0 };
    }
    const user = this.#users.get(reset.userId);
    if (user === undefined || user.status !== "active") {
      return { updated: false, revoked_sessions: 0 };
    }
    reset.consumedAt = input.now;
    for (const candidate of this.#passwordResets.values()) {
      if (
        candidate.userId === user.user_id &&
        candidate.consumedAt === undefined
      ) {
        candidate.consumedAt = input.now;
      }
    }
    user.password_hash = input.passwordHash;
    let revoked = 0;
    for (const [digest, session] of this.#sessions) {
      if (session.userId === user.user_id) {
        this.#sessions.delete(digest);
        revoked += 1;
      }
    }
    this.#recordAudit({
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
    const reset = this.#passwordResets.get(digest);
    if (reset?.consumedAt === undefined) this.#passwordResets.delete(digest);
  }

  async changePassword(input: {
    userId: string;
    expectedPasswordHash: string;
    passwordHash: string;
    currentSessionDigest: string;
    changedAt?: string;
  }): Promise<PasswordChangeResult> {
    const user = this.#users.get(input.userId);
    if (
      user === undefined ||
      user.status !== "active" ||
      user.password_hash !== input.expectedPasswordHash
    ) {
      return { updated: false, revoked_sessions: 0 };
    }
    user.password_hash = input.passwordHash;
    const changedAt = input.changedAt ?? new Date().toISOString();
    for (const reset of this.#passwordResets.values()) {
      if (reset.userId === input.userId && reset.consumedAt === undefined) {
        reset.consumedAt = changedAt;
      }
    }
    let revoked = 0;
    for (const [digest, session] of this.#sessions) {
      if (
        session.userId === input.userId &&
        digest !== input.currentSessionDigest
      ) {
        this.#sessions.delete(digest);
        revoked += 1;
      }
    }
    this.#recordAudit({
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
    const current = this.#loginLimits.get(input.scopeDigest);
    if (
      current === undefined ||
      Date.parse(current.windowStartedAt) + input.windowSeconds * 1_000 <= nowMs
    ) {
      this.#loginLimits.set(input.scopeDigest, {
        attemptCount: 1,
        windowStartedAt: input.now,
      });
      return { allowed: true, retry_after_seconds: 0 };
    }
    if (
      current.blockedUntil !== undefined &&
      Date.parse(current.blockedUntil) > nowMs
    ) {
      return {
        allowed: false,
        retry_after_seconds: Math.ceil(
          (Date.parse(current.blockedUntil) - nowMs) / 1_000,
        ),
      };
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
    this.#loginLimits.delete(scopeDigest);
  }

  async listWorkspaces(
    userId: string,
    currentOwnerId: string,
  ): Promise<WorkspaceSummary[]> {
    const result: WorkspaceSummary[] = [];
    for (const [ownerId, members] of this.#workspaceMembers) {
      const membership = members.get(userId);
      const owner = this.#users.get(ownerId);
      if (membership === undefined || owner?.status !== "active") continue;
      result.push({
        workspace_owner_id: ownerId,
        name:
          ownerId === userId ? "个人工作区" : `${owner.display_name} 的工作区`,
        role: membership.role,
        current: ownerId === currentOwnerId,
      });
    }
    return result.sort((left, right) => {
      if (left.workspace_owner_id === userId) return -1;
      if (right.workspace_owner_id === userId) return 1;
      return left.name.localeCompare(right.name);
    });
  }

  async workspaceView(
    actorUserId: string,
    workspaceOwnerId: string,
    memberLimit: number,
    now: string,
  ): Promise<TeamWorkspaceView | undefined> {
    const members = this.#workspaceMembers.get(workspaceOwnerId);
    const actor = members?.get(actorUserId);
    const owner = this.#users.get(workspaceOwnerId);
    if (members === undefined || actor === undefined || owner === undefined) {
      return undefined;
    }
    const canManage = actor.role === "owner" || actor.role === "admin";
    return {
      workspace: {
        workspace_owner_id: workspaceOwnerId,
        name:
          workspaceOwnerId === actorUserId
            ? "个人工作区"
            : `${owner.display_name} 的工作区`,
        role: actor.role,
        current: true,
      },
      members: [...members.entries()]
        .map(([userId, membership]) => {
          const user = this.#users.get(userId);
          return user === undefined
            ? undefined
            : {
                user_id: userId,
                email: user.email,
                display_name: user.display_name,
                role: membership.role,
                joined_at: membership.createdAt,
              };
        })
        .filter((member): member is WorkspaceMemberView => member !== undefined)
        .sort((left, right) => roleOrder(left.role) - roleOrder(right.role)),
      invitations: canManage
        ? [...this.#teamInvitations.values()]
            .filter(
              (invitation) =>
                invitation.workspaceOwnerId === workspaceOwnerId &&
                invitation.consumedAt === undefined &&
                invitation.revokedAt === undefined &&
                invitation.expiresAt > now,
            )
            .map((invitation) => ({
              invitation_id: invitation.invitationId,
              email: invitation.invitedEmail,
              role: invitation.role,
              created_at: invitation.createdAt,
              expires_at: invitation.expiresAt,
            }))
        : [],
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
    const members = this.#workspaceMembers.get(input.workspaceOwnerId);
    const actorRole = members?.get(input.invitedBy)?.role;
    if (
      members === undefined ||
      (actorRole !== "owner" && actorRole !== "admin") ||
      (actorRole === "admin" && input.role === "admin")
    ) {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
    if (members.has(input.invitedUserId)) {
      throw new AccountError(
        "team_member_exists",
        "user is already a member",
        409,
      );
    }
    const pending = [...this.#teamInvitations.values()].filter(
      (invitation) =>
        invitation.workspaceOwnerId === input.workspaceOwnerId &&
        invitation.consumedAt === undefined &&
        invitation.revokedAt === undefined &&
        invitation.expiresAt > input.createdAt,
    );
    if (
      pending.some(
        (invitation) => invitation.invitedUserId === input.invitedUserId,
      )
    ) {
      throw new AccountError(
        "team_invitation_exists",
        "invitation already exists",
        409,
      );
    }
    if (members.size + pending.length >= input.memberLimit) {
      throw new AccountError(
        "team_member_limit_reached",
        "team member limit reached",
        403,
      );
    }
    this.#teamInvitations.set(input.digest, {
      invitationId: input.invitationId,
      workspaceOwnerId: input.workspaceOwnerId,
      invitedUserId: input.invitedUserId,
      invitedEmail: input.invitedEmail,
      role: input.role,
      invitedBy: input.invitedBy,
      createdAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
    this.#recordAudit({
      scope: "workspace",
      workspace_owner_id: input.workspaceOwnerId,
      actor_user_id: input.invitedBy,
      action: "team.invitation_created",
      target_type: "team_invitation",
      target_id: input.invitationId,
      metadata: {
        role: input.role,
        invited_user_id: input.invitedUserId,
      },
      created_at: input.createdAt,
    });
  }

  async consumeTeamInvitation(input: {
    digest: string;
    userId: string;
    now: string;
  }): Promise<string | undefined> {
    const invitation = this.#teamInvitations.get(input.digest);
    if (
      invitation === undefined ||
      invitation.invitedUserId !== input.userId ||
      invitation.consumedAt !== undefined ||
      invitation.revokedAt !== undefined ||
      invitation.expiresAt <= input.now
    ) {
      return undefined;
    }
    const members = this.#workspaceMembers.get(invitation.workspaceOwnerId);
    const subscription = this.#subscriptions.get(invitation.workspaceOwnerId);
    const memberLimit =
      subscription === undefined
        ? 0
        : entitlementsFor(subscription).team_members;
    if (members === undefined || members.size >= memberLimit) {
      throw new AccountError(
        "team_member_limit_reached",
        "team member limit reached",
        403,
      );
    }
    invitation.consumedAt = input.now;
    members.set(input.userId, {
      role: invitation.role,
      createdAt: input.now,
    });
    this.#recordAudit({
      scope: "workspace",
      workspace_owner_id: invitation.workspaceOwnerId,
      actor_user_id: input.userId,
      action: "team.invitation_accepted",
      target_type: "team_member",
      target_id: input.userId,
      metadata: {
        role: invitation.role,
        invitation_id: invitation.invitationId,
      },
      created_at: input.now,
    });
    return invitation.workspaceOwnerId;
  }

  async revokeTeamInvitation(
    actorUserId: string,
    workspaceOwnerId: string,
    invitationId: string,
    now: string,
  ): Promise<boolean> {
    const actorRole = this.#workspaceMembers
      .get(workspaceOwnerId)
      ?.get(actorUserId)?.role;
    for (const invitation of this.#teamInvitations.values()) {
      if (
        invitation.invitationId === invitationId &&
        invitation.workspaceOwnerId === workspaceOwnerId &&
        invitation.consumedAt === undefined &&
        invitation.revokedAt === undefined &&
        (actorRole === "owner" ||
          (actorRole === "admin" && invitation.role === "member"))
      ) {
        invitation.revokedAt = now;
        this.#recordAudit({
          scope: "workspace",
          workspace_owner_id: workspaceOwnerId,
          actor_user_id: actorUserId,
          action: "team.invitation_revoked",
          target_type: "team_invitation",
          target_id: invitationId,
          metadata: { role: invitation.role },
          created_at: now,
        });
        return true;
      }
    }
    return false;
  }

  async updateWorkspaceMember(input: {
    actorUserId: string;
    workspaceOwnerId: string;
    memberUserId: string;
    role?: InviteRole;
    remove: boolean;
    updatedAt?: string;
  }): Promise<boolean> {
    const members = this.#workspaceMembers.get(input.workspaceOwnerId);
    const actorRole = members?.get(input.actorUserId)?.role;
    const target = members?.get(input.memberUserId);
    if (
      members === undefined ||
      target === undefined ||
      target.role === "owner" ||
      (actorRole !== "owner" && actorRole !== "admin") ||
      (actorRole === "admin" &&
        (target.role !== "member" || input.role === "admin"))
    ) {
      return false;
    }
    if (input.remove) {
      members.delete(input.memberUserId);
      for (const session of this.#sessions.values()) {
        if (
          session.userId === input.memberUserId &&
          session.workspaceOwnerId === input.workspaceOwnerId
        ) {
          session.workspaceOwnerId = input.memberUserId;
        }
      }
      this.#recordAudit({
        scope: "workspace",
        workspace_owner_id: input.workspaceOwnerId,
        actor_user_id: input.actorUserId,
        action: "team.member_removed",
        target_type: "team_member",
        target_id: input.memberUserId,
        metadata: { previous_role: target.role },
        created_at: input.updatedAt ?? new Date().toISOString(),
      });
    } else if (input.role !== undefined) {
      const previousRole = target.role;
      target.role = input.role;
      this.#recordAudit({
        scope: "workspace",
        workspace_owner_id: input.workspaceOwnerId,
        actor_user_id: input.actorUserId,
        action: "team.member_role_updated",
        target_type: "team_member",
        target_id: input.memberUserId,
        metadata: { previous_role: previousRole, role: input.role },
        created_at: input.updatedAt ?? new Date().toISOString(),
      });
    }
    return true;
  }

  async switchWebSessionWorkspace(input: {
    digest: string;
    actorUserId: string;
    workspaceOwnerId: string;
    switchedAt?: string;
  }): Promise<SessionIdentity | undefined> {
    const session = this.#sessions.get(input.digest);
    const role = this.#workspaceMembers
      .get(input.workspaceOwnerId)
      ?.get(input.actorUserId)?.role;
    if (session?.userId !== input.actorUserId || role === undefined) {
      return undefined;
    }
    const previousWorkspaceOwnerId = session.workspaceOwnerId;
    session.workspaceOwnerId = input.workspaceOwnerId;
    if (previousWorkspaceOwnerId !== input.workspaceOwnerId) {
      this.#recordAudit({
        scope: "account",
        actor_user_id: input.actorUserId,
        action: "workspace.switched",
        target_type: "workspace",
        target_id: input.workspaceOwnerId,
        metadata: {},
        created_at: input.switchedAt ?? new Date().toISOString(),
      });
    }
    return {
      actor_user_id: input.actorUserId,
      workspace_owner_id: input.workspaceOwnerId,
      role,
    };
  }

  async applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult> {
    const existing = this.#subscriptionEvents.get(event.event_id);
    if (existing !== undefined) {
      if (!isDeepStrictEqual(existing.event, event)) {
        throw new AccountError(
          "subscription_event_conflict",
          "event ID was reused with different data",
          409,
        );
      }
      const account = await this.accountView(event.user_id);
      if (account === undefined) {
        throw new AccountError(
          "account_unavailable",
          "account unavailable",
          404,
        );
      }
      return { applied: existing.applied, duplicate: true, account };
    }
    const user = this.#users.get(event.user_id);
    const subscription = this.#subscriptions.get(event.user_id);
    if (
      user === undefined ||
      subscription === undefined ||
      user.status !== "active"
    ) {
      throw new AccountError("account_unavailable", "account unavailable", 404);
    }
    const applied = isNewerSubscriptionEvent(subscription, event);
    this.#subscriptionEvents.set(event.event_id, {
      event: { ...event },
      applied,
    });
    if (applied) {
      this.#subscriptions.set(event.user_id, subscriptionFromEvent(event));
      this.#recordAudit({
        scope: "account",
        actor_user_id: event.user_id,
        action: "subscription.updated",
        target_type: "subscription",
        target_id: event.event_id,
        metadata: {
          plan: event.plan,
          status: event.status,
          provider: event.provider,
        },
        created_at: event.occurred_at,
      });
    }
    const account = await this.accountView(event.user_id);
    if (account === undefined) throw new Error("updated account disappeared");
    return { applied, duplicate: false, account };
  }

  async billingReference(
    userId: string,
  ): Promise<BillingReference | undefined> {
    const subscription = this.#subscriptions.get(userId);
    if (
      subscription?.provider === undefined ||
      subscription.provider_customer_id === undefined
    ) {
      return undefined;
    }
    return {
      provider: subscription.provider,
      customer_id: subscription.provider_customer_id,
      ...(subscription.provider_subscription_id === undefined
        ? {}
        : { subscription_id: subscription.provider_subscription_id }),
    };
  }

  async listAuditEvents(input: AuditQuery): Promise<AuditEvent[]> {
    if (
      input.scope === "workspace" &&
      !this.#workspaceMembers
        .get(input.workspaceOwnerId)
        ?.has(input.actorUserId)
    ) {
      return [];
    }
    const visible = this.#auditEvents
      .filter((event) =>
        input.scope === "account"
          ? event.scope === "account" &&
            event.actor_user_id === input.actorUserId
          : event.scope === "workspace" &&
            event.workspace_owner_id === input.workspaceOwnerId,
      )
      .sort(compareAuditEvents);
    const cursorIndex =
      input.beforeEventId === undefined
        ? -1
        : visible.findIndex((event) => event.event_id === input.beforeEventId);
    if (input.beforeEventId !== undefined && cursorIndex < 0) return [];
    return visible
      .slice(
        cursorIndex < 0 ? 0 : cursorIndex + 1,
        cursorIndex < 0 ? input.limit : cursorIndex + 1 + input.limit,
      )
      .map(cloneAuditEvent);
  }

  async recordAuditEvent(input: AuditRecordInput): Promise<boolean> {
    if (
      (input.scope === "account" &&
        (input.accountUserId === undefined ||
          input.workspaceOwnerId !== undefined ||
          !this.#users.has(input.accountUserId))) ||
      (input.scope === "workspace" &&
        (input.workspaceOwnerId === undefined ||
          input.accountUserId !== undefined ||
          !this.#workspaceMembers.has(input.workspaceOwnerId))) ||
      (input.actorUserId !== undefined &&
        !this.#users.has(input.actorUserId)) ||
      (input.targetType === undefined) !== (input.targetId === undefined)
    ) {
      return false;
    }
    this.#recordAudit({
      scope: input.scope,
      ...(input.actorUserId === undefined
        ? {}
        : { actor_user_id: input.actorUserId }),
      ...(input.workspaceOwnerId === undefined
        ? {}
        : { workspace_owner_id: input.workspaceOwnerId }),
      action: input.action,
      ...(input.targetType === undefined
        ? {}
        : { target_type: input.targetType }),
      ...(input.targetId === undefined ? {} : { target_id: input.targetId }),
      metadata: input.metadata,
      created_at: input.createdAt ?? new Date().toISOString(),
    });
    return true;
  }

  #recordAudit(event: Omit<AuditEvent, "event_id">): void {
    this.#auditEvents.push({
      ...event,
      event_id: randomUUID(),
      metadata: auditMetadata(event.metadata),
    });
  }

  setSubscription(userId: string, subscription: SubscriptionRecord): void {
    if (!this.#users.has(userId)) {
      throw new Error("account does not exist");
    }
    this.#subscriptions.set(userId, subscription);
  }
}

export class AccountError extends ContractError {
  readonly statusCode: number;

  constructor(code: string, message: string, statusCode: number) {
    super(code, message);
    this.name = "AccountError";
    this.statusCode = statusCode;
  }
}

export class SessionAuthenticator implements OwnerAuthenticator {
  readonly #accounts: AccountStore;

  constructor(accounts: AccountStore) {
    this.#accounts = accounts;
  }

  async authenticate(request: FastifyRequest): Promise<string> {
    const token = sessionToken(request);
    if (token === undefined) {
      throw new AuthenticationError();
    }
    const identity = await this.#accounts.findWebSession(
      tokenDigest(token),
      new Date().toISOString(),
    );
    if (identity === undefined) {
      throw new AuthenticationError();
    }
    return identity.workspace_owner_id;
  }
}

export async function currentSessionIdentity(
  accounts: AccountStore,
  request: FastifyRequest,
  now: Date = new Date(),
): Promise<SessionIdentity | undefined> {
  const digest = currentSessionDigest(request);
  return digest === undefined
    ? undefined
    : accounts.findWebSession(digest, now.toISOString());
}

export function normalizeEmail(value: unknown): string {
  if (typeof value !== "string") {
    throw new AccountError("invalid_email", "email is invalid", 400);
  }
  const normalized = value.trim().toLowerCase();
  if (
    normalized.length < 3 ||
    normalized.length > 254 ||
    !EMAIL_PATTERN.test(normalized)
  ) {
    throw new AccountError("invalid_email", "email is invalid", 400);
  }
  return normalized;
}

export function normalizeDisplayName(value: unknown, email: string): string {
  if (value === undefined) {
    return email.slice(0, email.indexOf("@"));
  }
  if (typeof value !== "string") {
    throw new AccountError(
      "invalid_display_name",
      "display name is invalid",
      400,
    );
  }
  const normalized = value.trim();
  if (
    normalized.length < 1 ||
    normalized.length > 80 ||
    /[\u0000-\u001f\u007f]/u.test(normalized)
  ) {
    throw new AccountError(
      "invalid_display_name",
      "display name is invalid",
      400,
    );
  }
  return normalized;
}

export function validatePassword(value: unknown): string {
  if (typeof value !== "string") {
    throw new AccountError("invalid_password", "password is invalid", 400);
  }
  const bytes = Buffer.byteLength(value, "utf8");
  if (value.length < 10 || value.length > 128 || bytes > 512) {
    throw new AccountError("invalid_password", "password is invalid", 400);
  }
  return value;
}

export async function hashPassword(password: string): Promise<string> {
  const salt = randomBytes(16);
  const derived = await derivePassword(password, salt);
  return [
    "scrypt",
    SCRYPT_N,
    SCRYPT_R,
    SCRYPT_P,
    salt.toString("base64url"),
    derived.toString("base64url"),
  ].join("$");
}

export async function verifyPassword(
  password: string,
  encoded: string,
): Promise<boolean> {
  const [algorithm, n, r, p, saltValue, hashValue, extra] = encoded.split("$");
  if (
    algorithm !== "scrypt" ||
    extra !== undefined ||
    n !== String(SCRYPT_N) ||
    r !== String(SCRYPT_R) ||
    p !== String(SCRYPT_P) ||
    saltValue === undefined ||
    hashValue === undefined
  ) {
    return false;
  }
  let salt: Buffer;
  let expected: Buffer;
  try {
    salt = Buffer.from(saltValue, "base64url");
    expected = Buffer.from(hashValue, "base64url");
  } catch {
    return false;
  }
  if (salt.length !== 16 || expected.length !== PASSWORD_HASH_BYTES) {
    return false;
  }
  const actual = await derivePassword(password, salt);
  return timingSafeEqual(actual, expected);
}

export async function verifyAccountPassword(
  password: string,
  user: AccountUser | undefined,
): Promise<boolean> {
  const matches = await verifyPassword(
    password,
    user?.password_hash ?? DUMMY_PASSWORD_HASH,
  );
  return user !== undefined && user.status === "active" && matches;
}

export function newUserId(): string {
  return randomUUID();
}

export function entitlementsFor(
  subscription: SubscriptionRecord,
  now: Date = new Date(),
): Entitlements {
  if (process.env.AGENT_PULSE_ENFORCE_ENTITLEMENTS === "true") {
    const paidThrough =
      subscription.current_period_end !== undefined &&
      Date.parse(subscription.current_period_end) > now.getTime();
    const active =
      subscription.status === "active" ||
      subscription.status === "trialing" ||
      ((subscription.status === "past_due" ||
        subscription.status === "canceled") &&
        paidThrough);
    const plan = active ? subscription.plan : "free";
    if (plan === "team") {
      return {
        max_devices: 25,
        max_concurrent_tasks: 10,
        history_days: 365,
        remote_tasks: true,
        team_members: 10,
      };
    }
    if (plan === "pro") {
      return {
        max_devices: 5,
        max_concurrent_tasks: 3,
        history_days: 90,
        remote_tasks: true,
        team_members: 1,
      };
    }
    return {
      max_devices: 1,
      max_concurrent_tasks: 0,
      history_days: 7,
      remote_tasks: false,
      team_members: 1,
    };
  }

  // 测试阶段全量解锁：所有方案默认开放完整设备数、并发任务数与历史回溯
  return {
    max_devices: 999,
    max_concurrent_tasks: 100,
    history_days: 3650,
    remote_tasks: true,
    team_members: 100,
  };
}

export function accountView(
  user: AccountUser,
  subscription: SubscriptionRecord,
): AccountView {
  const { password_hash: _passwordHash, ...safeUser } = user;
  return {
    user: safeUser,
    subscription: {
      plan: subscription.plan,
      status: subscription.status,
      ...(subscription.current_period_end === undefined
        ? {}
        : { current_period_end: subscription.current_period_end }),
    },
    entitlements: entitlementsFor(subscription),
  };
}

export async function createLoginSession(
  accounts: AccountStore,
  userId: string,
  reply: FastifyReply,
  secure: boolean,
  now: Date = new Date(),
  reason: "registration" | "login" = "login",
): Promise<void> {
  const token = randomBytes(SESSION_BYTES).toString("base64url");
  const sessionId = randomUUID();
  const expiresAt = new Date(now.getTime() + SESSION_TTL_MS);
  await accounts.createWebSession({
    sessionId,
    digest: tokenDigest(token),
    userId,
    createdAt: now.toISOString(),
    expiresAt: expiresAt.toISOString(),
    reason,
  });
  reply.header("set-cookie", serializeSessionCookie(token, expiresAt, secure));
}

export function currentSessionDigest(
  request: FastifyRequest,
): string | undefined {
  const token = sessionToken(request);
  return token === undefined ? undefined : tokenDigest(token);
}

export function isNewerSubscriptionEvent(
  current: SubscriptionRecord,
  event: SubscriptionEvent,
): boolean {
  if (
    current.last_event_at === undefined ||
    current.last_event_id === undefined
  ) {
    return true;
  }
  const timeOrder = event.occurred_at.localeCompare(current.last_event_at);
  return (
    timeOrder > 0 || (timeOrder === 0 && event.event_id > current.last_event_id)
  );
}

function subscriptionFromEvent(event: SubscriptionEvent): SubscriptionRecord {
  return {
    plan: event.plan,
    status: event.status,
    provider: event.provider,
    provider_customer_id: event.provider_customer_id,
    provider_subscription_id: event.provider_subscription_id,
    ...(event.current_period_end === undefined
      ? {}
      : { current_period_end: event.current_period_end }),
    last_event_at: event.occurred_at,
    last_event_id: event.event_id,
  };
}

export async function clearLoginSession(
  accounts: AccountStore,
  request: FastifyRequest,
  reply: FastifyReply,
  secure: boolean,
): Promise<void> {
  const token = sessionToken(request);
  if (token !== undefined) {
    await accounts.deleteWebSession(tokenDigest(token));
  }
  reply.header("set-cookie", serializeExpiredSessionCookie(secure));
}

export function accountBody(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new AccountError("invalid_body", "body must be an object", 400);
  }
  return value as Record<string, unknown>;
}

export function assertOnlyFields(
  body: Record<string, unknown>,
  fields: readonly string[],
): void {
  if (Object.keys(body).some((key) => !fields.includes(key))) {
    throw new AccountError("unknown_field", "body field is invalid", 400);
  }
}

function sessionToken(request: FastifyRequest): string | undefined {
  const cookie = request.headers.cookie;
  if (cookie === undefined) return undefined;
  for (const segment of cookie.split(";")) {
    const separator = segment.indexOf("=");
    if (separator < 0) continue;
    const name = segment.slice(0, separator).trim();
    if (name !== LOCAL_COOKIE_NAME && name !== SECURE_COOKIE_NAME) continue;
    const token = segment.slice(separator + 1);
    if (/^[A-Za-z0-9_-]{43}$/.test(token)) return token;
  }
  return undefined;
}

export function tokenDigest(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function serializeSessionCookie(
  token: string,
  expiresAt: Date,
  secure: boolean,
): string {
  return [
    `${secure ? SECURE_COOKIE_NAME : LOCAL_COOKIE_NAME}=${token}`,
    "Path=/",
    "HttpOnly",
    "SameSite=Lax",
    `Expires=${expiresAt.toUTCString()}`,
    `Max-Age=${Math.floor(SESSION_TTL_MS / 1_000)}`,
    ...(secure ? ["Secure"] : []),
  ].join("; ");
}

function serializeExpiredSessionCookie(secure: boolean): string {
  return [
    `${secure ? SECURE_COOKIE_NAME : LOCAL_COOKIE_NAME}=`,
    "Path=/",
    "HttpOnly",
    "SameSite=Lax",
    "Expires=Thu, 01 Jan 1970 00:00:00 GMT",
    "Max-Age=0",
    ...(secure ? ["Secure"] : []),
  ].join("; ");
}

function derivePassword(password: string, salt: Buffer): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    scryptCallback(
      password,
      salt,
      PASSWORD_HASH_BYTES,
      {
        N: SCRYPT_N,
        r: SCRYPT_R,
        p: SCRYPT_P,
        maxmem: 32 * 1024 * 1024,
      },
      (error, derived) => {
        if (error !== null) {
          reject(error);
          return;
        }
        resolve(derived);
      },
    );
  });
}

function roleOrder(role: WorkspaceRole): number {
  return role === "owner" ? 0 : role === "admin" ? 1 : 2;
}

const AUDIT_METADATA_KEYS = new Set([
  "reason",
  "plan",
  "status",
  "provider",
  "role",
  "previous_role",
  "invited_user_id",
  "invitation_id",
  "revoked_sessions",
  "agent_type",
  "task_status",
  "previous_status",
]);

export function auditMetadata(
  input: Record<string, AuditMetadataValue>,
): Record<string, AuditMetadataValue> {
  const output: Record<string, AuditMetadataValue> = {};
  for (const [key, value] of Object.entries(input)) {
    if (!AUDIT_METADATA_KEYS.has(key)) {
      throw new Error(`audit metadata key is not allowed: ${key}`);
    }
    if (
      (typeof value === "string" &&
        (value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value))) ||
      (typeof value === "number" && (!Number.isSafeInteger(value) || value < 0))
    ) {
      throw new Error(`audit metadata value is invalid: ${key}`);
    }
    output[key] = value;
  }
  return output;
}

function compareAuditEvents(left: AuditEvent, right: AuditEvent): number {
  const timeOrder = right.created_at.localeCompare(left.created_at);
  return timeOrder === 0
    ? right.event_id.localeCompare(left.event_id)
    : timeOrder;
}

function cloneAuditEvent(event: AuditEvent): AuditEvent {
  return { ...event, metadata: { ...event.metadata } };
}
