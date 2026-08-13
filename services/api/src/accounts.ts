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
const DUMMY_PASSWORD_HASH = [
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

export interface LoginAttemptResult {
  allowed: boolean;
  retry_after_seconds: number;
}

export interface PasswordChangeResult {
  updated: boolean;
  revoked_sessions: number;
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
  accountView(userId: string): Promise<AccountView | undefined>;
  createWebSession(input: {
    sessionId: string;
    digest: string;
    userId: string;
    createdAt: string;
    expiresAt: string;
  }): Promise<void>;
  findWebSession(digest: string, now: string): Promise<string | undefined>;
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
  }): Promise<PasswordChangeResult>;
  consumeLoginAttempt(input: {
    scopeDigest: string;
    now: string;
    windowSeconds: number;
    maximumAttempts: number;
    blockSeconds: number;
  }): Promise<LoginAttemptResult>;
  clearLoginAttempts(scopeDigest: string): Promise<void>;
  applySubscriptionEvent(
    event: SubscriptionEvent,
  ): Promise<SubscriptionEventResult>;
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
    return accountView(user, { plan: "free", status: "active" });
  }

  async findAccountByEmail(email: string): Promise<AccountUser | undefined> {
    const userId = this.#userIdsByEmail.get(email);
    return userId === undefined ? undefined : this.#users.get(userId);
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
  }): Promise<void> {
    this.#sessions.set(input.digest, {
      sessionId: input.sessionId,
      userId: input.userId,
      createdAt: input.createdAt,
      lastSeenAt: input.createdAt,
      expiresAt: input.expiresAt,
    });
  }

  async findWebSession(
    digest: string,
    now: string,
  ): Promise<string | undefined> {
    const session = this.#sessions.get(digest);
    if (session === undefined || session.expiresAt <= now) return undefined;
    const user = this.#users.get(session.userId);
    session.lastSeenAt = now;
    return user?.status === "active" ? user.user_id : undefined;
  }

  async deleteWebSession(digest: string): Promise<void> {
    this.#sessions.delete(digest);
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
    const changedAt = new Date().toISOString();
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
    }
    const account = await this.accountView(event.user_id);
    if (account === undefined) throw new Error("updated account disappeared");
    return { applied, duplicate: false, account };
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
    const userId = await this.#accounts.findWebSession(
      tokenDigest(token),
      new Date().toISOString(),
    );
    if (userId === undefined) {
      throw new AuthenticationError();
    }
    return userId;
  }
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
