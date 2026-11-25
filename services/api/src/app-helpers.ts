import { randomBytes } from "node:crypto";
import { ContractError } from "@agent-pulse/contracts";
import type { FastifyInstance, FastifyRequest } from "fastify";
import {
  AccountError,
  currentSessionDigest,
  currentSessionIdentity,
  tokenDigest,
  type AccountStore,
  type AccountView,
  type InviteRole,
  type LoginAttemptResult,
} from "./accounts.js";
import { AuthenticationError, type OwnerAuthenticator } from "./auth.js";
import type { PaidSubscriptionPlan } from "./billing.js";
import { verificationUrl, type EmailVerificationSender } from "./email.js";
import type { ChangeBroker } from "./events.js";
import type { PushSender } from "./push.js";
import { mapWithConcurrency } from "./security-guards.js";
import type { GatewayStore, SessionStatusChange } from "./store.js";

const STREAM_RETRY_MS = 5_000;
const STREAM_HEARTBEAT_MS = 10_000;
const MAX_STREAMS_PER_OWNER = 8;
const MAX_DEVICE_CLOCK_SKEW_MS = 5 * 60_000;
const EMAIL_VERIFICATION_BYTES = 32;
const EMAIL_VERIFICATION_TTL_MS = 24 * 60 * 60_000;
const EMAIL_VERIFICATION_MIN_INTERVAL_SECONDS = 60;
const NOTIFIED_STATUSES = new Set(["WAITING_INPUT", "WAITING_PERMISSION"]);

export function registerV1StreamRoute(
  app: FastifyInstance,
  authenticator: OwnerAuthenticator,
  broker: ChangeBroker,
  openStreams: Set<(destroySocket?: boolean) => void>,
  streamAuthRevalidators: Set<() => void>,
  revalidateMs: number,
): void {
  app.get("/v1/stream", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    if (broker.subscriberCount(ownerId) >= MAX_STREAMS_PER_OWNER) {
      void reply.status(429).send({ error: "too_many_streams" });
      return;
    }
    reply.hijack();
    const raw = reply.raw;
    raw.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-store",
      connection: "keep-alive",
      "x-accel-buffering": "no",
    });
    request.raw.socket.setNoDelay(true);
    request.raw.socket.setTimeout(0);

    let closed = false;
    let writePaused = false;
    let drainTimer: ReturnType<typeof setTimeout> | undefined;
    const write = (chunk: string): void => {
      if (closed || raw.destroyed || raw.writableEnded || writePaused) return;
      if (!raw.write(chunk)) {
        writePaused = true;
        drainTimer = setTimeout(() => close(true), 15_000);
        drainTimer.unref?.();
        raw.once("drain", () => {
          if (drainTimer !== undefined) clearTimeout(drainTimer);
          drainTimer = undefined;
          writePaused = false;
        });
      }
    };
    write(`retry: ${STREAM_RETRY_MS}\n\n`);
    write("event: ready\ndata: {}\n\n");

    const unsubscribe = broker.subscribe(ownerId, (message) => {
      write(`event: sessions\ndata: ${JSON.stringify(message)}\n\n`);
    });
    const revalidate = (): void => {
      if (closed) return;
      void Promise.resolve(authenticator.authenticate(request)).then(
        (currentOwner) => {
          if (currentOwner !== ownerId) close();
        },
        () => close(),
      );
    };
    const authTimer = setInterval(revalidate, Math.max(revalidateMs, 1_000));
    authTimer.unref();
    const heartbeat = setInterval(
      () => write(": keepalive\n\n"),
      STREAM_HEARTBEAT_MS,
    );
    heartbeat.unref();

    function close(forceDestroy = false): void {
      if (closed) {
        if (forceDestroy && !raw.destroyed) raw.destroy();
        return;
      }
      closed = true;
      if (drainTimer !== undefined) clearTimeout(drainTimer);
      clearInterval(authTimer);
      clearInterval(heartbeat);
      unsubscribe();
      streamAuthRevalidators.delete(revalidate);
      openStreams.delete(closeStream);
      if (forceDestroy) {
        if (!raw.destroyed) raw.destroy();
      } else if (!raw.writableEnded) {
        raw.end();
      }
    }
    const closeStream = (destroySocket = false): void => close(destroySocket);
    streamAuthRevalidators.add(revalidate);
    openStreams.add(closeStream);
    raw.on("close", () => close(false));
    raw.on("error", () => close(false));
  });
}

export async function dispatchWaitingPushNotifications(
  push: { publicKey: string; sender: PushSender } | undefined,
  store: GatewayStore,
  app: FastifyInstance,
  ownerId: string,
  changes: readonly SessionStatusChange[],
): Promise<void> {
  if (push === undefined) return;
  const waiting = changes.filter((c) =>
    NOTIFIED_STATUSES.has(c.activity_status),
  );
  if (waiting.length === 0) return;
  let live = await store.listPushSubscriptions(ownerId);
  for (const change of waiting) {
    if (live.length === 0) return;
    const payload = JSON.stringify({
      device_id: change.device_id,
      agent_type: change.agent_type,
      agent_session_id: change.agent_session_id,
      project: change.project_display_name,
      activity_status: change.activity_status,
      waiting_reason: change.waiting_reason ?? "unknown",
      observed_at: change.observed_at,
    });
    const outcomes = await mapWithConcurrency(
      live,
      8,
      async (subscription) => ({
        subscription,
        outcome: await push.sender.send(subscription, payload),
      }),
    );
    const deliveryCounts = {
      delivered: outcomes.filter((e) => e.outcome === "delivered").length,
      gone: outcomes.filter((e) => e.outcome === "gone").length,
      failed: outcomes.filter((e) => e.outcome === "failed").length,
    };
    app.log.info({ push_delivery: deliveryCounts }, "push delivery completed");
    for (const { subscription, outcome } of outcomes) {
      if (outcome === "gone") {
        await store.deletePushSubscription(ownerId, subscription.endpoint);
      }
    }
    live = outcomes
      .filter((e) => e.outcome !== "gone")
      .map((e) => e.subscription);
  }
}

export function isOneOf<const T extends readonly string[]>(
  values: T,
  value: string,
): value is T[number] {
  return (values as readonly string[]).includes(value);
}

export function assertFreshDeviceTimestamp(value: string, now: Date): void {
  if (Math.abs(Date.parse(value) - now.getTime()) > MAX_DEVICE_CLOCK_SKEW_MS) {
    throw new ContractError(
      "stale_device_request",
      "device timestamp is stale",
    );
  }
}

export function requireAccounts(
  accounts: AccountStore | undefined,
): AccountStore {
  if (accounts === undefined) {
    throw new AccountError(
      "accounts_unavailable",
      "account service is not configured",
      503,
    );
  }
  return accounts;
}

export async function requireSessionIdentity(
  accounts: AccountStore,
  request: FastifyRequest,
  at: Date,
  secureCookies = false,
) {
  const identity = await currentSessionIdentity(
    accounts,
    request,
    at,
    secureCookies,
  );
  if (identity === undefined) throw new AuthenticationError();
  return identity;
}

export async function requireAccountIdentity(
  accounts: AccountStore | undefined,
  authenticator: OwnerAuthenticator,
  request: FastifyRequest,
  at: Date,
  secureCookies = false,
) {
  if (accounts !== undefined) {
    const sessionIdentity = await currentSessionIdentity(
      accounts,
      request,
      at,
      secureCookies,
    );
    if (sessionIdentity !== undefined) return sessionIdentity;
    if (currentSessionDigest(request, secureCookies) !== undefined) {
      throw new AuthenticationError();
    }
  }
  const ownerId = await authenticator.authenticate(request);
  return {
    actor_user_id: ownerId,
    workspace_owner_id: ownerId,
    role: "owner" as const,
  };
}

export async function mutationIdentity(
  accounts: AccountStore | undefined,
  authenticator: OwnerAuthenticator,
  request: FastifyRequest,
  at: Date,
  secureCookies = false,
): Promise<{
  workspace_owner_id: string;
  actor_user_id?: string;
  role: "owner" | "admin" | "member";
}> {
  if (accounts !== undefined) {
    const identity = await currentSessionIdentity(
      accounts,
      request,
      at,
      secureCookies,
    );
    if (identity !== undefined) {
      return {
        workspace_owner_id: identity.workspace_owner_id,
        actor_user_id: identity.actor_user_id,
        role: identity.role,
      };
    }
    if (currentSessionDigest(request, secureCookies) !== undefined) {
      throw new AuthenticationError();
    }
  }
  return {
    workspace_owner_id: await authenticator.authenticate(request),
    role: "owner",
  };
}

export function inviteRole(value: unknown): InviteRole {
  if (value !== "admin" && value !== "member") {
    throw new AccountError("invalid_team_role", "team role is invalid", 400);
  }
  return value;
}

export function paidPlan(value: unknown): PaidSubscriptionPlan {
  if (value !== "pro" && value !== "team") {
    throw new AccountError(
      "invalid_billing_plan",
      "billing plan is invalid",
      400,
    );
  }
  return value;
}

export async function issueEmailVerification(
  accounts: AccountStore,
  account: AccountView,
  delivery: { baseUrl: string; sender: EmailVerificationSender } | undefined,
  issuedAt: Date,
  app: FastifyInstance,
): Promise<LoginAttemptResult> {
  if (account.user.email_verified_at !== undefined) {
    return { allowed: false, retry_after_seconds: 0 };
  }
  if (delivery === undefined) {
    throw new AccountError(
      "email_delivery_unavailable",
      "email verification delivery is unavailable",
      503,
    );
  }
  const token = randomBytes(EMAIL_VERIFICATION_BYTES).toString("base64url");
  const expiresAt = new Date(issuedAt.getTime() + EMAIL_VERIFICATION_TTL_MS);
  const result = await accounts.issueEmailVerification({
    digest: tokenDigest(token),
    userId: account.user.user_id,
    createdAt: issuedAt.toISOString(),
    expiresAt: expiresAt.toISOString(),
    minimumIntervalSeconds: EMAIL_VERIFICATION_MIN_INTERVAL_SECONDS,
  });
  if (!result.allowed) return result;
  try {
    await delivery.sender.sendVerification({
      user_id: account.user.user_id,
      email: account.user.email,
      display_name: account.user.display_name,
      verification_url: verificationUrl(delivery.baseUrl, token),
      expires_at: expiresAt.toISOString(),
    });
  } catch {
    await accounts.deletePendingEmailVerification(tokenDigest(token));
    app.log.error({ error_code: "email_verification_delivery_failed" });
    throw new AccountError(
      "email_delivery_failed",
      "verification email could not be delivered",
      503,
    );
  }
  return result;
}

export function requireVerifiedEmail(
  account: AccountView,
  required: boolean,
): void {
  if (required && account.user.email_verified_at === undefined) {
    throw new AccountError(
      "email_verification_required",
      "email verification is required",
      403,
    );
  }
}
