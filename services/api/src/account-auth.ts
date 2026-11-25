import {
  createHash,
  randomBytes,
  randomUUID,
  scrypt as scryptCallback,
  timingSafeEqual,
} from "node:crypto";

import { ContractError } from "@agent-pulse/contracts";
import type { FastifyReply, FastifyRequest } from "fastify";

import { AuthenticationError, type OwnerAuthenticator } from "./auth.js";
import { AccountError } from "./account-helpers.js";
import type {
  AccountStore,
  AccountUser,
  AccountView,
  Entitlements,
  SessionIdentity,
  SubscriptionEvent,
  SubscriptionPlan,
  SubscriptionRecord,
  WorkspaceSummary,
} from "./account-types.js";
import {
  extractSessionToken,
  scryptSemaphore,
  serializeCookieHeader,
} from "./security-guards.js";

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

export class SessionAuthenticator implements OwnerAuthenticator {
  readonly #accounts: AccountStore;
  readonly #secureCookies: boolean;

  constructor(accounts: AccountStore, secureCookies = false) {
    this.#accounts = accounts;
    this.#secureCookies = secureCookies;
  }

  async authenticate(request: FastifyRequest): Promise<string> {
    const token = sessionToken(request, this.#secureCookies);
    if (token === undefined) throw new AuthenticationError();
    const identity = await this.#accounts.findWebSession(
      tokenDigest(token),
      new Date().toISOString(),
    );
    if (identity === undefined) throw new AuthenticationError();
    return identity.workspace_owner_id;
  }
}

export async function currentSessionIdentity(
  accounts: AccountStore,
  request: FastifyRequest,
  now: Date = new Date(),
  secureCookies = false,
): Promise<SessionIdentity | undefined> {
  const digest = currentSessionDigest(request, secureCookies);
  return digest === undefined
    ? undefined
    : accounts.findWebSession(digest, now.toISOString());
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
  secureCookies = false,
): string | undefined {
  const token = extractSessionToken(
    request,
    LOCAL_COOKIE_NAME,
    SECURE_COOKIE_NAME,
    secureCookies,
  );
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

export function subscriptionFromEvent(
  event: SubscriptionEvent,
): SubscriptionRecord {
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
  const token = extractSessionToken(
    request,
    LOCAL_COOKIE_NAME,
    SECURE_COOKIE_NAME,
    secure,
  );
  if (token !== undefined) {
    await accounts.deleteWebSession(tokenDigest(token));
  }
  reply.header(
    "set-cookie",
    serializeCookieHeader(
      secure ? SECURE_COOKIE_NAME : LOCAL_COOKIE_NAME,
      "",
      new Date(0),
      0,
      secure,
    ),
  );
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

function sessionToken(
  request: FastifyRequest,
  secure = false,
): string | undefined {
  return extractSessionToken(
    request,
    LOCAL_COOKIE_NAME,
    SECURE_COOKIE_NAME,
    secure,
  );
}

export function tokenDigest(token: string): string {
  return createHash("sha256").update(token, "utf8").digest("hex");
}

function serializeSessionCookie(
  token: string,
  expiresAt: Date,
  secure: boolean,
): string {
  return serializeCookieHeader(
    secure ? SECURE_COOKIE_NAME : LOCAL_COOKIE_NAME,
    token,
    expiresAt,
    Math.floor(SESSION_TTL_MS / 1_000),
    secure,
  );
}

function derivePassword(password: string, salt: Buffer): Promise<Buffer> {
  return scryptSemaphore.run(
    () =>
      new Promise((resolve, reject) => {
        scryptCallback(
          password,
          salt,
          PASSWORD_HASH_BYTES,
          { N: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P, maxmem: 32 * 1024 * 1024 },
          (error, derived) => {
            if (error !== null) {
              reject(error);
              return;
            }
            resolve(derived);
          },
        );
      }),
  );
}
