import { createHmac, randomBytes, randomUUID } from "node:crypto";

import {
  ContractError,
  activityStatuses,
  agentTypes,
  parseAuthSyncBatch,
  parseCreateRemoteTask,
  parseDeviceRegistration,
  parseDeviceHeartbeat,
  parseDeviceTaskClaim,
  parseDeviceTaskUpdate,
  parseSyncBatch,
  presences,
  remoteTaskStatuses,
  verifySyncBatch,
  validateCapabilityReport,
  validateLaunchProfile,
} from "@agent-pulse/contracts";
import Fastify, { type FastifyInstance } from "fastify";
import fastifyRawBody from "fastify-raw-body";
import fastifyWebsocket from "@fastify/websocket";
import { V2SessionManager } from "./v2-session-manager.js";
import { DualLayerEventStore } from "./v2-event-store.js";
import { registerRemoteRoutes } from "./remote-routes.js";
import type { RemoteContentStore } from "./remote-store.js";

import {
  AccountError,
  accountBody,
  assertOnlyFields,
  clearLoginSession,
  createLoginSession,
  currentSessionIdentity,
  currentSessionDigest,
  hashPassword,
  newUserId,
  normalizeDisplayName,
  normalizeEmail,
  tokenDigest,
  validatePassword,
  verifyAccountPassword,
  type AccountStore,
  type AccountView,
  type LoginAttemptResult,
  type InviteRole,
} from "./accounts.js";
import {
  BillingEventVerifier,
  type BillingProvider,
  type PaidSubscriptionPlan,
} from "./billing.js";
import {
  passwordResetUrl,
  teamInvitationUrl,
  verificationUrl,
  type EmailVerificationSender,
} from "./email.js";
import {
  AuthenticationError,
  RejectingAuthenticator,
  type OwnerAuthenticator,
} from "./auth.js";
import { generatePkcePair, TgOAuthClient, TgOAuthError } from "./tg-oauth.js";
import { ChangeBroker } from "./events.js";
import type { PushSender, PushSubscription } from "./push.js";
import {
  GatewayStoreError,
  InMemoryGatewayStore,
  OneTimeBindingCodes,
  bindingCodeDigest,
  type AuthFilters,
  type GatewayStore,
  type RemoteTaskFilters,
  type SessionFilters,
  type SessionStatusChange,
} from "./store.js";

const DEFAULT_SESSION_EVENT_LIMIT = 100;
const MAX_SESSION_EVENT_LIMIT = 500;
/** Reconnect delay advertised to EventSource clients. */
const STREAM_RETRY_MS = 5_000;
/** Keeps intermediaries from closing an idle stream, and detects dead peers. */
// Keep idle streams alive below common reverse-proxy idle/read timeouts.
const STREAM_HEARTBEAT_MS = 10_000;
/** One panel opens one stream; the cap bounds a misbehaving or hostile client. */
const MAX_STREAMS_PER_OWNER = 8;
const MAX_PUSH_ENDPOINT_LENGTH = 2_048;
const DEFAULT_BINDING_TTL_SECONDS = 600;
const MIN_BINDING_TTL_SECONDS = 60;
const MAX_BINDING_TTL_SECONDS = 3_600;
const DEVICE_ID_PATTERN = /^dev_[0-9a-f]{32}$/;
const TASK_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const MAX_DEVICE_CLOCK_SKEW_MS = 5 * 60_000;
const EMAIL_VERIFICATION_BYTES = 32;
const EMAIL_VERIFICATION_TTL_MS = 24 * 60 * 60_000;
const EMAIL_VERIFICATION_MIN_INTERVAL_SECONDS = 60;
const PASSWORD_RESET_BYTES = 32;
const PASSWORD_RESET_TTL_MS = 60 * 60_000;
const PASSWORD_RESET_MIN_INTERVAL_SECONDS = 60;
const TEAM_INVITATION_BYTES = 32;
const TEAM_INVITATION_TTL_MS = 72 * 60 * 60_000;
const LOGIN_WINDOW_SECONDS = 15 * 60;
const LOGIN_MAXIMUM_ATTEMPTS = 5;
const LOGIN_BLOCK_SECONDS = 15 * 60;
const DEFAULT_AUDIT_LIMIT = 30;
const MAX_AUDIT_LIMIT = 100;
const SESSION_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
/**
 * Statuses that interrupt the user's attention. The panel is a monitor, not a
 * feed: notifying on every completion would train people to ignore it, and the
 * pain point in the PRD is specifically "waiting and nobody noticed".
 */
const NOTIFIED_STATUSES = new Set(["WAITING_INPUT", "WAITING_PERMISSION"]);

export interface GatewayPushOptions {
  /** VAPID application server key handed to the browser at subscribe time. */
  publicKey: string;
  sender: PushSender;
}

export interface GatewayAppOptions {
  store?: GatewayStore;
  bindingCodes?: OneTimeBindingCodes;
  broker?: ChangeBroker;
  push?: GatewayPushOptions;
  authenticator?: OwnerAuthenticator;
  accounts?: AccountStore;
  secureCookies?: boolean;
  emailVerification?: {
    baseUrl: string;
    sender: EmailVerificationSender;
  };
  billingVerifier?: BillingEventVerifier;
  billingProvider?: BillingProvider;
  tgOAuthClient?: TgOAuthClient;
  v2Manager?: V2SessionManager;
  remoteContent?: RemoteContentStore;
  loginRateLimitSecret?: Buffer;
  trustProxy?: boolean;
  bindingCodeFactory?: () => string;
  now?: () => Date;
  logger?: boolean;
}

export function createGatewayApp(
  options: GatewayAppOptions = {},
): FastifyInstance {
  const store: GatewayStore = options.store ?? new InMemoryGatewayStore();
  const bindingCodes = options.bindingCodes;
  const now = options.now ?? (() => new Date());
  const tgOAuth = options.tgOAuthClient ?? new TgOAuthClient();
  const bindingCodeFactory =
    options.bindingCodeFactory ?? (() => randomBytes(32).toString("base64url"));
  const broker = options.broker ?? new ChangeBroker();
  const authenticator = options.authenticator ?? new RejectingAuthenticator();
  const loginRateLimitSecret = options.loginRateLimitSecret ?? randomBytes(32);
  if (options.accounts !== undefined) {
    store.setAuditRecorder?.(options.accounts);
  }
  const openStreams = new Set<() => void>();
  const app = Fastify({
    logger: options.logger ?? false,
    bodyLimit: 1024 * 1024,
    trustProxy: options.trustProxy ?? false,
  });

  app.setErrorHandler((error, _request, reply) => {
    app.log.error(error);
    if (error instanceof AccountError) {
      void reply.status(error.statusCode).send({ error: error.code });
      return;
    }
    if (error instanceof TgOAuthError) {
      void reply
        .status(error.statusCode)
        .send({ error: error.code, message: error.message });
      return;
    }
    if (error instanceof ContractError) {
      const status =
        error.code === "invalid_signature" ||
        error instanceof AuthenticationError
          ? 401
          : 400;
      if (error instanceof AuthenticationError) {
        void reply.header("www-authenticate", 'Bearer realm="agent-pulse"');
      }
      void reply.status(status).send({ error: error.code });
      return;
    }
    if (error instanceof GatewayStoreError) {
      const status = ["task_not_found"].includes(error.code)
        ? 404
        : [
              "device_not_registered",
              "device_key_mismatch",
              "device_revoked",
              "invalid_binding_code",
              "device_not_owned",
              "task_limit_reached",
            ].includes(error.code)
          ? 403
          : 409;
      void reply.status(status).send({ error: error.code });
      return;
    }
    const statusCode =
      typeof error === "object" &&
      error !== null &&
      "statusCode" in error &&
      typeof error.statusCode === "number"
        ? error.statusCode
        : undefined;
    if (statusCode !== undefined && statusCode >= 400 && statusCode < 500) {
      void reply.status(statusCode).send({
        error: "invalid_request",
        message: error instanceof Error ? error.message : undefined,
      });
      return;
    }
    void reply.status(500).send({ error: "internal_error" });
  });

  // Native JSONL permits 16 MiB; leave room for the Remote envelope.
  void app.register(fastifyWebsocket, {
    options: { maxPayload: 17 * 1024 * 1024 },
  });
  if (options.remoteContent) {
    const content = options.remoteContent;
    void app.register(async (remoteApp) =>
      registerRemoteRoutes(remoteApp, store, authenticator, content),
    );
  }

  if (options.billingProvider !== undefined) {
    const provider = options.billingProvider;
    void app.register(async (billingApp) => {
      await billingApp.register(fastifyRawBody, {
        global: false,
        encoding: false,
        runFirst: true,
      });
      billingApp.post(
        provider.webhookPath,
        { config: { rawBody: true } },
        async (request, reply) => {
          const accounts = requireAccounts(options.accounts);
          if (!Buffer.isBuffer(request.rawBody)) {
            throw new AccountError(
              "invalid_billing_event",
              "billing webhook body is unavailable",
              400,
            );
          }
          const signatureValue =
            request.headers[provider.webhookSignatureHeader];
          const event = provider.parseWebhook(
            request.rawBody,
            typeof signatureValue === "string" ? signatureValue : undefined,
            now(),
          );
          if (event === undefined) {
            return reply.send({ received: true, ignored: true });
          }
          const result = await accounts.applySubscriptionEvent(event);
          return reply.send({
            received: true,
            applied: result.applied,
            duplicate: result.duplicate,
          });
        },
      );
    });
  }

  app.get("/health", async () => ({ status: "ok" }));

  app.post("/v1/auth/register", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "password", "display_name"]);
    const email = normalizeEmail(body.email);
    const password = validatePassword(body.password);
    const account = await accounts.createAccount({
      userId: newUserId(),
      email,
      displayName: normalizeDisplayName(body.display_name, email),
      passwordHash: await hashPassword(password),
    });
    await createLoginSession(
      accounts,
      account.user.user_id,
      reply,
      options.secureCookies ?? false,
      now(),
      "registration",
    );
    if (options.emailVerification !== undefined) {
      try {
        await issueEmailVerification(
          accounts,
          account,
          options.emailVerification,
          now(),
          app,
        );
      } catch (error) {
        if (
          !(error instanceof AccountError) ||
          error.code !== "email_delivery_failed"
        ) {
          throw error;
        }
      }
    }
    return reply.status(201).send(account);
  });

  app.post("/v1/auth/login", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "password"]);
    const email = normalizeEmail(body.email);
    const password = validatePassword(body.password);
    const loginScope = createHmac("sha256", loginRateLimitSecret)
      .update(`${email}\n${request.ip}`, "utf8")
      .digest("hex");
    const rateLimit = await accounts.consumeLoginAttempt({
      scopeDigest: loginScope,
      now: now().toISOString(),
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: LOGIN_MAXIMUM_ATTEMPTS,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!rateLimit.allowed) {
      reply.header("retry-after", String(rateLimit.retry_after_seconds));
      throw new AccountError(
        "login_rate_limited",
        "too many login attempts",
        429,
      );
    }
    const user = await accounts.findAccountByEmail(email);
    if (!(await verifyAccountPassword(password, user)) || user === undefined) {
      throw new AccountError(
        "invalid_credentials",
        "email or password is incorrect",
        401,
      );
    }
    const account = await accounts.accountView(user.user_id);
    if (account === undefined) {
      throw new AccountError("account_unavailable", "account unavailable", 403);
    }
    await accounts.clearLoginAttempts(loginScope);
    await createLoginSession(
      accounts,
      user.user_id,
      reply,
      options.secureCookies ?? false,
      now(),
      "login",
    );
    return reply.send(account);
  });

  app.post("/v1/auth/logout", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    await clearLoginSession(
      accounts,
      request,
      reply,
      options.secureCookies ?? false,
    );
    return reply.status(204).send();
  });

  app.post("/v1/auth/oauth/tg/authorize-url", async (request, reply) => {
    const body =
      typeof request.body === "object" && request.body !== null
        ? (request.body as Record<string, unknown>)
        : {};
    const redirectUri =
      typeof body.redirect_uri === "string" && body.redirect_uri.trim() !== ""
        ? body.redirect_uri.trim()
        : undefined;
    const scope =
      typeof body.scope === "string" && body.scope.trim() !== ""
        ? body.scope.trim()
        : undefined;
    const state = randomBytes(24).toString("base64url");
    const pkce = generatePkcePair();
    const authorizeUrl = await tgOAuth.buildAuthorizeUrl({
      state,
      codeChallenge: pkce.codeChallenge,
      ...(redirectUri ? { redirectUri } : {}),
      ...(scope ? { scope } : {}),
    });
    return reply.send({
      authorize_url: authorizeUrl,
      code_verifier: pkce.codeVerifier,
      state,
    });
  });

  app.post("/v1/auth/oauth/tg/callback", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["code", "code_verifier", "redirect_uri"]);
    if (typeof body.code !== "string" || !body.code) {
      throw new AccountError(
        "invalid_oauth_code",
        "authorization code is required",
        400,
      );
    }
    if (typeof body.code_verifier !== "string" || !body.code_verifier) {
      throw new AccountError(
        "invalid_code_verifier",
        "code verifier is required",
        400,
      );
    }
    const redirectUri =
      typeof body.redirect_uri === "string" ? body.redirect_uri : undefined;

    const tokens = await tgOAuth.exchangeCodeForTokens({
      code: body.code,
      codeVerifier: body.code_verifier,
      ...(redirectUri ? { redirectUri } : {}),
    });
    const userinfo = await tgOAuth.fetchUserInfo(tokens.access_token);
    const email = normalizeEmail(
      userinfo.email || `${userinfo.sub}@tg.oauth.local`,
    );
    const displayName = normalizeDisplayName(userinfo.name, email);

    if (accounts.upsertOAuthAccount === undefined) {
      throw new AccountError(
        "oauth_unsupported",
        "OAuth account storage not supported",
        500,
      );
    }

    const account = await accounts.upsertOAuthAccount({
      provider: "tg_oauth",
      sub: userinfo.sub,
      email,
      displayName,
      emailVerified: userinfo.email_verified ?? true,
      now: now().toISOString(),
    });

    await createLoginSession(
      accounts,
      account.user.user_id,
      reply,
      options.secureCookies ?? false,
      now(),
      "login",
    );
    return reply.send(account);
  });

  app.post("/v1/auth/oauth/tg/device/code", async (request, reply) => {
    const body =
      request.body && typeof request.body === "object"
        ? (request.body as Record<string, unknown>)
        : {};
    const scope = typeof body.scope === "string" ? body.scope : undefined;
    const deviceFlow = await tgOAuth.startDeviceFlow(scope ? { scope } : {});
    return reply.send(deviceFlow);
  });

  app.post("/v1/auth/oauth/tg/device/poll", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["device_code"]);
    if (typeof body.device_code !== "string" || !body.device_code) {
      throw new AccountError(
        "invalid_device_code",
        "device_code is required",
        400,
      );
    }

    const pollResult = await tgOAuth.pollDeviceToken(body.device_code);
    if (pollResult.status === "pending") {
      return reply.send({
        status: "pending",
        slow_down: pollResult.slowDown ?? false,
      });
    }

    const userinfo = await tgOAuth.fetchUserInfo(
      pollResult.tokens.access_token,
    );
    const email = normalizeEmail(
      userinfo.email || `${userinfo.sub}@tg.oauth.local`,
    );
    const displayName = normalizeDisplayName(userinfo.name, email);

    if (accounts.upsertOAuthAccount === undefined) {
      throw new AccountError(
        "oauth_unsupported",
        "OAuth account storage not supported",
        500,
      );
    }

    const account = await accounts.upsertOAuthAccount({
      provider: "tg_oauth",
      sub: userinfo.sub,
      email,
      displayName,
      emailVerified: userinfo.email_verified ?? true,
      now: now().toISOString(),
    });

    await createLoginSession(
      accounts,
      account.user.user_id,
      reply,
      options.secureCookies ?? false,
      now(),
      "login",
    );
    return reply.send({
      status: "success",
      account,
    });
  });

  app.get("/v1/auth/me", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireAccountIdentity(
      accounts,
      authenticator,
      request,
      now(),
    );
    const account = await accounts.accountView(identity.actor_user_id);
    if (account === undefined) throw new AuthenticationError();
    const workspaceAccount = await accounts.accountView(
      identity.workspace_owner_id,
    );
    if (workspaceAccount === undefined) throw new AuthenticationError();
    const workspaces = await accounts.listWorkspaces(
      identity.actor_user_id,
      identity.workspace_owner_id,
    );
    return reply.send({
      ...account,
      workspaces,
      workspace: workspaces.find((workspace) => workspace.current),
      workspace_subscription: workspaceAccount.subscription,
      workspace_entitlements: workspaceAccount.entitlements,
    });
  });

  app.post("/v1/auth/email-verifications", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const account = await accounts.accountView(identity.actor_user_id);
    if (account === undefined) throw new AuthenticationError();
    const issued = await issueEmailVerification(
      accounts,
      account,
      options.emailVerification,
      now(),
      app,
    );
    if (!issued.allowed && issued.retry_after_seconds > 0) {
      reply.header("retry-after", String(issued.retry_after_seconds));
      throw new AccountError(
        "email_verification_rate_limited",
        "verification was sent recently",
        429,
      );
    }
    return reply.status(202).send({
      accepted: issued.allowed,
      verified: account.user.email_verified_at !== undefined,
    });
  });

  app.post("/v1/auth/email-verifications/confirm", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["token"]);
    if (
      typeof body.token !== "string" ||
      !/^[A-Za-z0-9_-]{43}$/.test(body.token)
    ) {
      throw new AccountError(
        "invalid_email_verification",
        "verification token is invalid",
        400,
      );
    }
    const account = await accounts.consumeEmailVerification(
      tokenDigest(body.token),
      now().toISOString(),
    );
    if (account === undefined) {
      throw new AccountError(
        "invalid_email_verification",
        "verification token is invalid or expired",
        400,
      );
    }
    return reply.send(account);
  });

  app.post("/v1/auth/password-resets", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const delivery = options.emailVerification;
    if (delivery?.sender.sendPasswordReset === undefined) {
      throw new AccountError(
        "email_delivery_unavailable",
        "password reset delivery is unavailable",
        404,
      );
    }
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email"]);
    const email = normalizeEmail(body.email);
    const user = await accounts.findAccountByEmail(email);
    if (user !== undefined && user.status === "active") {
      const token = randomBytes(PASSWORD_RESET_BYTES).toString("base64url");
      const digest = tokenDigest(token);
      const createdAt = now();
      const expiresAt = new Date(createdAt.getTime() + PASSWORD_RESET_TTL_MS);
      const issued = await accounts.issuePasswordReset({
        digest,
        userId: user.user_id,
        createdAt: createdAt.toISOString(),
        expiresAt: expiresAt.toISOString(),
        minimumIntervalSeconds: PASSWORD_RESET_MIN_INTERVAL_SECONDS,
      });
      if (issued.allowed) {
        try {
          await delivery.sender.sendPasswordReset({
            user_id: user.user_id,
            email: user.email,
            display_name: user.display_name,
            reset_url: passwordResetUrl(delivery.baseUrl, token),
            expires_at: expiresAt.toISOString(),
          });
        } catch (error) {
          await accounts.deletePendingPasswordReset(digest);
          app.log.error(error, "password reset delivery failed");
        }
      }
    }
    return reply.status(202).send({ accepted: true });
  });

  app.post("/v1/auth/password-resets/confirm", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["token", "password"]);
    if (
      typeof body.token !== "string" ||
      !/^[A-Za-z0-9_-]{43}$/.test(body.token)
    ) {
      throw new AccountError(
        "invalid_password_reset",
        "password reset token is invalid",
        400,
      );
    }
    const password = validatePassword(body.password);
    const result = await accounts.consumePasswordReset({
      digest: tokenDigest(body.token),
      passwordHash: await hashPassword(password),
      now: now().toISOString(),
    });
    if (!result.updated) {
      throw new AccountError(
        "invalid_password_reset",
        "password reset token is invalid or expired",
        400,
      );
    }
    await clearLoginSession(
      accounts,
      request,
      reply,
      options.secureCookies ?? false,
    );
    return reply.send({ changed: true });
  });

  app.post("/v1/auth/password", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const userId = identity.actor_user_id;
    const currentDigest = currentSessionDigest(request);
    if (currentDigest === undefined) throw new AuthenticationError();
    const body = accountBody(request.body);
    assertOnlyFields(body, ["current_password", "new_password"]);
    const currentPassword = validatePassword(body.current_password);
    const newPassword = validatePassword(body.new_password);
    const account = await accounts.accountView(userId);
    if (account === undefined) throw new AuthenticationError();
    const user = await accounts.findAccountByEmail(account.user.email);
    if (
      !(await verifyAccountPassword(currentPassword, user)) ||
      user === undefined
    ) {
      throw new AccountError(
        "invalid_current_password",
        "current password is incorrect",
        401,
      );
    }
    if (currentPassword === newPassword) {
      throw new AccountError(
        "password_unchanged",
        "new password must be different",
        400,
      );
    }
    const result = await accounts.changePassword({
      userId,
      expectedPasswordHash: user.password_hash,
      passwordHash: await hashPassword(newPassword),
      currentSessionDigest: currentDigest,
      changedAt: now().toISOString(),
    });
    if (!result.updated) throw new AuthenticationError();
    return reply.send({ changed: true, revoked: result.revoked_sessions });
  });

  app.get("/v1/auth/sessions", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const digest = currentSessionDigest(request);
    if (digest === undefined) throw new AuthenticationError();
    return reply.send({
      sessions: await accounts.listWebSessions(
        identity.actor_user_id,
        digest,
        now().toISOString(),
      ),
    });
  });

  app.delete("/v1/auth/sessions/:sessionId", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const userId = identity.actor_user_id;
    const { sessionId } = request.params as { sessionId?: unknown };
    if (typeof sessionId !== "string" || !SESSION_ID_PATTERN.test(sessionId)) {
      throw new AccountError(
        "invalid_session_id",
        "session ID is invalid",
        400,
      );
    }
    const digest = currentSessionDigest(request);
    if (digest === undefined) throw new AuthenticationError();
    const sessions = await accounts.listWebSessions(
      userId,
      digest,
      now().toISOString(),
    );
    const target = sessions.find((session) => session.session_id === sessionId);
    if (
      target === undefined ||
      !(await accounts.revokeWebSession(userId, sessionId))
    ) {
      throw new AccountError("session_not_found", "session was not found", 404);
    }
    if (target.current) {
      await clearLoginSession(
        accounts,
        request,
        reply,
        options.secureCookies ?? false,
      );
    }
    return reply.status(204).send();
  });

  app.delete("/v1/auth/sessions", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const digest = currentSessionDigest(request);
    if (digest === undefined) throw new AuthenticationError();
    const revoked = await accounts.revokeOtherWebSessions(
      identity.actor_user_id,
      digest,
    );
    return reply.send({ revoked });
  });

  app.get("/v1/audit", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const query = accountBody(request.query);
    assertOnlyFields(query, ["scope", "before", "limit"]);
    const scope = query.scope === undefined ? "account" : query.scope;
    if (scope !== "account" && scope !== "workspace") {
      throw new AccountError("invalid_audit_query", "scope is invalid", 400);
    }
    const before = query.before;
    if (
      before !== undefined &&
      (typeof before !== "string" || !SESSION_ID_PATTERN.test(before))
    ) {
      throw new AccountError("invalid_audit_query", "cursor is invalid", 400);
    }
    const requestedLimit =
      query.limit === undefined
        ? DEFAULT_AUDIT_LIMIT
        : typeof query.limit === "string" && /^\d{1,3}$/.test(query.limit)
          ? Number(query.limit)
          : Number.NaN;
    if (
      !Number.isSafeInteger(requestedLimit) ||
      requestedLimit < 1 ||
      requestedLimit > MAX_AUDIT_LIMIT
    ) {
      throw new AccountError("invalid_audit_query", "limit is invalid", 400);
    }
    const events = await accounts.listAuditEvents({
      scope,
      actorUserId: identity.actor_user_id,
      workspaceOwnerId: identity.workspace_owner_id,
      ...(before === undefined ? {} : { beforeEventId: before }),
      limit: requestedLimit,
    });
    return reply.send({
      scope,
      events,
      ...(events.length < requestedLimit
        ? {}
        : { next_before: events.at(-1)?.event_id }),
    });
  });

  app.get("/v1/team", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const workspaceAccount = await accounts.accountView(
      identity.workspace_owner_id,
    );
    if (workspaceAccount === undefined) throw new AuthenticationError();
    const workspace = await accounts.workspaceView(
      identity.actor_user_id,
      identity.workspace_owner_id,
      workspaceAccount.entitlements.team_members,
      now().toISOString(),
    );
    if (workspace === undefined) throw new AuthenticationError();
    return reply.send(workspace);
  });

  app.post("/v1/team/invitations", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const workspaceAccount = await accounts.accountView(
      identity.workspace_owner_id,
    );
    if (workspaceAccount === undefined) throw new AuthenticationError();
    if (workspaceAccount.entitlements.team_members <= 1) {
      throw new AccountError(
        "team_plan_required",
        "Team plan is required",
        403,
      );
    }
    requireVerifiedEmail(
      workspaceAccount,
      options.emailVerification !== undefined,
    );
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "role"]);
    const email = normalizeEmail(body.email);
    const role = inviteRole(body.role);
    const invited = await accounts.findAccountByEmail(email);
    if (invited === undefined || invited.status !== "active") {
      throw new AccountError(
        "team_invitee_unavailable",
        "invitee is unavailable",
        404,
      );
    }
    const token = randomBytes(TEAM_INVITATION_BYTES).toString("base64url");
    const createdAt = now();
    const expiresAt = new Date(createdAt.getTime() + TEAM_INVITATION_TTL_MS);
    const invitationId = randomUUID();
    await accounts.issueTeamInvitation({
      invitationId,
      digest: tokenDigest(token),
      workspaceOwnerId: identity.workspace_owner_id,
      invitedUserId: invited.user_id,
      invitedEmail: invited.email,
      role,
      invitedBy: identity.actor_user_id,
      createdAt: createdAt.toISOString(),
      expiresAt: expiresAt.toISOString(),
      memberLimit: workspaceAccount.entitlements.team_members,
    });
    const invitationUrl =
      options.emailVerification === undefined
        ? `#token=${encodeURIComponent(token)}`
        : teamInvitationUrl(options.emailVerification.baseUrl, token);
    if (options.emailVerification?.sender.sendTeamInvitation !== undefined) {
      const inviter = await accounts.accountView(identity.actor_user_id);
      try {
        await options.emailVerification.sender.sendTeamInvitation({
          email: invited.email,
          display_name: invited.display_name,
          inviter_name: inviter?.user.display_name ?? "AgentPulse 团队管理员",
          invitation_url: invitationUrl,
          role,
          expires_at: expiresAt.toISOString(),
        });
      } catch (error) {
        await accounts.revokeTeamInvitation(
          identity.actor_user_id,
          identity.workspace_owner_id,
          invitationId,
          now().toISOString(),
        );
        app.log.error({ error_code: "team_invitation_delivery_failed" });
        throw new AccountError(
          "email_delivery_failed",
          "team invitation could not be delivered",
          503,
        );
      }
    }
    return reply.status(201).send({
      invitation_id: invitationId,
      invitation_url: invitationUrl,
      email: invited.email,
      role,
      expires_at: expiresAt.toISOString(),
    });
  });

  app.post("/v1/team/invitations/accept", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const body = accountBody(request.body);
    assertOnlyFields(body, ["token"]);
    if (
      typeof body.token !== "string" ||
      !/^[A-Za-z0-9_-]{43}$/.test(body.token)
    ) {
      throw new AccountError(
        "invalid_team_invitation",
        "invitation is invalid",
        400,
      );
    }
    const workspaceOwnerId = await accounts.consumeTeamInvitation({
      digest: tokenDigest(body.token),
      userId: identity.actor_user_id,
      now: now().toISOString(),
    });
    if (workspaceOwnerId === undefined) {
      throw new AccountError(
        "invalid_team_invitation",
        "invitation is invalid or expired",
        400,
      );
    }
    return reply.send({ accepted: true, workspace_owner_id: workspaceOwnerId });
  });

  app.delete("/v1/team/invitations/:invitationId", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const { invitationId } = request.params as { invitationId?: unknown };
    if (
      typeof invitationId !== "string" ||
      !SESSION_ID_PATTERN.test(invitationId)
    ) {
      throw new AccountError(
        "invalid_team_invitation",
        "invitation is invalid",
        400,
      );
    }
    if (
      !(await accounts.revokeTeamInvitation(
        identity.actor_user_id,
        identity.workspace_owner_id,
        invitationId,
        now().toISOString(),
      ))
    ) {
      throw new AccountError(
        "team_invitation_not_found",
        "invitation not found",
        404,
      );
    }
    return reply.status(204).send();
  });

  app.patch("/v1/team/members/:userId", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const { userId } = request.params as { userId?: unknown };
    if (typeof userId !== "string" || !SESSION_ID_PATTERN.test(userId)) {
      throw new AccountError("invalid_team_member", "member is invalid", 400);
    }
    const body = accountBody(request.body);
    assertOnlyFields(body, ["role"]);
    const role = inviteRole(body.role);
    if (
      !(await accounts.updateWorkspaceMember({
        actorUserId: identity.actor_user_id,
        workspaceOwnerId: identity.workspace_owner_id,
        memberUserId: userId,
        role,
        remove: false,
        updatedAt: now().toISOString(),
      }))
    ) {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
    return reply.send({ updated: true });
  });

  app.delete("/v1/team/members/:userId", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const { userId } = request.params as { userId?: unknown };
    if (typeof userId !== "string" || !SESSION_ID_PATTERN.test(userId)) {
      throw new AccountError("invalid_team_member", "member is invalid", 400);
    }
    if (
      !(await accounts.updateWorkspaceMember({
        actorUserId: identity.actor_user_id,
        workspaceOwnerId: identity.workspace_owner_id,
        memberUserId: userId,
        remove: true,
        updatedAt: now().toISOString(),
      }))
    ) {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
    return reply.status(204).send();
  });

  app.post("/v1/team/workspace", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const digest = currentSessionDigest(request);
    if (digest === undefined) throw new AuthenticationError();
    const body = accountBody(request.body);
    assertOnlyFields(body, ["workspace_owner_id"]);
    if (
      typeof body.workspace_owner_id !== "string" ||
      !SESSION_ID_PATTERN.test(body.workspace_owner_id)
    ) {
      throw new AccountError("invalid_workspace", "workspace is invalid", 400);
    }
    const switched = await accounts.switchWebSessionWorkspace({
      digest,
      actorUserId: identity.actor_user_id,
      workspaceOwnerId: body.workspace_owner_id,
      switchedAt: now().toISOString(),
    });
    if (switched === undefined) {
      throw new AccountError("workspace_not_found", "workspace not found", 404);
    }
    return reply.send({
      switched: true,
      workspaces: await accounts.listWorkspaces(
        identity.actor_user_id,
        switched.workspace_owner_id,
      ),
    });
  });

  app.post("/v1/billing/events", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    if (options.billingVerifier === undefined) {
      throw new AccountError(
        "billing_unavailable",
        "billing integration is unavailable",
        404,
      );
    }
    const event = options.billingVerifier.verify(request, now());
    const result = await accounts.applySubscriptionEvent(event);
    return reply.send({
      applied: result.applied,
      duplicate: result.duplicate,
      subscription: result.account.subscription,
      entitlements: result.account.entitlements,
    });
  });

  app.get("/v1/billing", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const account = await accounts.accountView(identity.actor_user_id);
    if (account === undefined) throw new AuthenticationError();
    const provider = options.billingProvider;
    const reference = await accounts.billingReference(identity.actor_user_id);
    return reply.send({
      available: provider !== undefined,
      ...(provider === undefined ? {} : { provider: provider.name }),
      checkout: provider !== undefined,
      portal:
        provider !== undefined &&
        reference?.provider === provider.name &&
        reference.customer_id.length > 0,
      subscription: account.subscription,
    });
  });

  app.post("/v1/billing/checkout", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const provider = options.billingProvider;
    if (provider === undefined) {
      throw new AccountError(
        "billing_unavailable",
        "billing integration is unavailable",
        503,
      );
    }
    const account = await accounts.accountView(identity.actor_user_id);
    if (account === undefined) throw new AuthenticationError();
    requireVerifiedEmail(account, options.emailVerification !== undefined);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["plan"]);
    const plan = paidPlan(body.plan);
    const reference = await accounts.billingReference(identity.actor_user_id);
    if (reference?.subscription_id !== undefined) {
      throw new AccountError(
        "billing_portal_required",
        "manage the existing subscription in the billing portal",
        409,
      );
    }
    if (reference !== undefined && reference.provider !== provider.name) {
      throw new AccountError(
        "billing_provider_mismatch",
        "the existing subscription uses another billing provider",
        409,
      );
    }
    const url = await provider.createCheckout({
      userId: identity.actor_user_id,
      email: account.user.email,
      plan,
      ...(reference === undefined ? {} : { customerId: reference.customer_id }),
    });
    return reply.send({ provider: provider.name, url });
  });

  app.post("/v1/billing/portal", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(accounts, request, now());
    const provider = options.billingProvider;
    if (provider === undefined) {
      throw new AccountError(
        "billing_unavailable",
        "billing integration is unavailable",
        503,
      );
    }
    const reference = await accounts.billingReference(identity.actor_user_id);
    if (reference?.provider !== provider.name) {
      throw new AccountError(
        "billing_customer_unavailable",
        "billing customer is not available",
        409,
      );
    }
    const url = await provider.createPortal(reference.customer_id);
    return reply.send({ provider: provider.name, url });
  });

  app.post("/v1/device-bindings", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
    );
    const ownerId = identity.workspace_owner_id;
    if (options.accounts !== undefined) {
      const account = await options.accounts.accountView(ownerId);
      if (account === undefined) throw new AuthenticationError();
      requireVerifiedEmail(account, options.emailVerification !== undefined);
      if (
        (await store.countActiveDevices(ownerId)) >=
        account.entitlements.max_devices
      ) {
        throw new AccountError(
          "device_limit_reached",
          "subscription device limit reached",
          403,
        );
      }
    }
    const expiresInSeconds = bindingTtlSeconds(request.body);
    const bindingCode = bindingCodeFactory();
    const expiresAt = new Date(
      now().getTime() + expiresInSeconds * 1_000,
    ).toISOString();
    await store.createBindingCode(
      ownerId,
      bindingCodeDigest(bindingCode),
      expiresAt,
      identity.actor_user_id,
    );
    return reply.status(201).send({
      binding_code: bindingCode,
      expires_at: expiresAt,
    });
  });

  app.post("/v1/devices/register", async (request, reply) => {
    const registration = parseDeviceRegistration(request.body);
    let result;
    if (bindingCodes === undefined) {
      result = await store.registerDeviceWithBindingCode(
        bindingCodeDigest(registration.binding_code),
        registration.device_id,
        registration.public_key,
      );
    } else {
      const ownerId = bindingCodes.claim(
        registration.binding_code,
        registration.device_id,
      );
      try {
        result = await store.registerDevice(
          ownerId,
          registration.device_id,
          registration.public_key,
        );
      } catch (error) {
        bindingCodes.release(registration.binding_code, registration.device_id);
        throw error;
      }
    }
    return reply.status(result.created ? 201 : 200).send({
      device_id: result.device.device_id,
      owner_id: result.device.owner_id,
      registered: true,
    });
  });

  app.post("/v1/sync/batches", async (request, reply) => {
    const batch = parseSyncBatch(request.body);
    verifySyncBatch(batch);
    const result = await store.ingestBatch(batch);
    if (result.accepted > 0) {
      broker.publish(result.owner_id, {
        device_id: batch.device_id,
        accepted: result.accepted,
      });
      // Delivery happens off the request path: the Runner's upload must not
      // wait on a third-party push service, and a failed push must never turn
      // a stored batch into an error the Runner would retry.
      void notifyWaitingSessions(result.owner_id, result.changes).catch(
        (error: unknown) => {
          app.log.error(error);
        },
      );
    }
    return reply.status(200).send({
      protocol_version: batch.protocol_version,
      batch_id: batch.batch_id,
      device_id: batch.device_id,
      accepted_through: batch.last_sequence,
    });
  });

  app.post("/v1/device-auth", async (request, reply) => {
    const batch = parseAuthSyncBatch(request.body);
    const result = await store.ingestAuthBatch(batch);
    if (result.accepted > 0) {
      broker.publish(result.owner_id, {
        device_id: batch.device_id,
        accepted: result.accepted,
        auth_snapshots: true,
      });
    }
    return reply.status(200).send({
      protocol_version: batch.protocol_version,
      device_id: batch.device_id,
      accepted: result.accepted,
    });
  });

  app.post("/v1/devices/heartbeat", async (request, reply) => {
    const heartbeat = parseDeviceHeartbeat(request.body);
    const receivedAt = now();
    assertFreshDeviceTimestamp(heartbeat.sent_at, receivedAt);
    const result = await store.recordHeartbeat(
      heartbeat,
      receivedAt.toISOString(),
    );
    return reply.send({
      protocol_version: heartbeat.protocol_version,
      device_id: heartbeat.device_id,
      accepted: result.accepted,
    });
  });

  app.get("/v1/devices", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    return reply.send({ devices: await store.listDevices(ownerId) });
  });

  app.get("/v1/device-auth", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find((key) => key !== "device_id");
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    const filters: AuthFilters = {};
    if (query.device_id !== undefined) {
      if (
        typeof query.device_id !== "string" ||
        !DEVICE_ID_PATTERN.test(query.device_id)
      ) {
        throw new ContractError("invalid_query", "device_id is invalid");
      }
      filters.device_id = query.device_id;
    }
    return reply.send({ snapshots: await store.listAuth(ownerId, filters) });
  });

  app.post("/v1/tasks", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
    );
    const ownerId = identity.workspace_owner_id;
    const accounts = requireAccounts(options.accounts);
    const account = await accounts.accountView(ownerId);
    if (account === undefined) throw new AuthenticationError();
    requireVerifiedEmail(account, options.emailVerification !== undefined);
    if (!account.entitlements.remote_tasks) {
      throw new AccountError(
        "remote_tasks_unavailable",
        "remote tasks require a paid subscription",
        403,
      );
    }
    const input = parseCreateRemoteTask(request.body);
    if (input.execution_policy !== "local_confirmation") {
      throw new AccountError(
        "execution_policy_unavailable",
        "execution policy is not enabled",
        403,
      );
    }
    const createdAt = now();
    const result = await store.createRemoteTask(
      ownerId,
      {
        ...input,
        task_id: randomUUID(),
        created_at: createdAt.toISOString(),
        expires_at: new Date(
          createdAt.getTime() + input.expires_in_seconds * 1_000,
        ).toISOString(),
        max_concurrent_tasks: account.entitlements.max_concurrent_tasks,
      },
      identity.actor_user_id,
    );
    if (result.created) {
      broker.publish(ownerId, {
        device_id: result.task.target_device_id,
        accepted: 0,
        remote_tasks: true,
      });
    }
    return reply.status(result.created ? 201 : 200).send(result.task);
  });

  app.get("/v1/tasks", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) => !["target_device_id", "status"].includes(key),
    );
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    const filters: RemoteTaskFilters = {};
    if (query.target_device_id !== undefined) {
      if (
        typeof query.target_device_id !== "string" ||
        !DEVICE_ID_PATTERN.test(query.target_device_id)
      ) {
        throw new ContractError("invalid_query", "target_device_id is invalid");
      }
      filters.target_device_id = query.target_device_id;
    }
    if (query.status !== undefined) {
      if (
        typeof query.status !== "string" ||
        !isOneOf(remoteTaskStatuses, query.status)
      ) {
        throw new ContractError("invalid_query", "status is invalid");
      }
      filters.status = query.status;
    }
    return reply.send({ tasks: await store.listRemoteTasks(ownerId, filters) });
  });

  app.post("/v1/tasks/:taskId/cancel", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
    );
    const ownerId = identity.workspace_owner_id;
    const { taskId } = request.params as { taskId?: unknown };
    if (typeof taskId !== "string" || !TASK_ID_PATTERN.test(taskId)) {
      throw new ContractError("invalid_task_id", "task ID is invalid");
    }
    const task = await store.cancelRemoteTask(
      ownerId,
      taskId,
      identity.actor_user_id,
    );
    broker.publish(ownerId, {
      device_id: task.target_device_id,
      accepted: 0,
      remote_tasks: true,
    });
    return reply.send(task);
  });

  app.post("/v1/device-tasks/claim", async (request, reply) => {
    const claim = parseDeviceTaskClaim(request.body);
    assertFreshDeviceTimestamp(claim.requested_at, now());
    const task = await store.claimRemoteTask(claim);
    if (task === undefined) return reply.status(204).send();
    broker.publish(task.owner_id, {
      device_id: task.task.target_device_id,
      accepted: 0,
      remote_tasks: true,
    });
    return reply.send(task.task);
  });

  app.post("/v1/device-tasks/:taskId/status", async (request, reply) => {
    const { taskId } = request.params as { taskId?: unknown };
    if (typeof taskId !== "string" || !TASK_ID_PATTERN.test(taskId)) {
      throw new ContractError("invalid_task_id", "task ID is invalid");
    }
    const update = parseDeviceTaskUpdate(request.body);
    if (update.task_id !== taskId) {
      throw new ContractError(
        "task_id_mismatch",
        "task ID does not match route",
      );
    }
    assertFreshDeviceTimestamp(update.updated_at, now());
    const result = await store.updateRemoteTask(update);
    broker.publish(result.owner_id, {
      device_id: result.task.target_device_id,
      accepted: 0,
      remote_tasks: true,
    });
    return reply.send(result.task);
  });

  app.delete("/v1/devices/:deviceId", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
    );
    const ownerId = identity.workspace_owner_id;
    const { deviceId } = request.params as { deviceId?: unknown };
    if (typeof deviceId !== "string" || !DEVICE_ID_PATTERN.test(deviceId)) {
      throw new ContractError("invalid_device_id", "device ID is invalid");
    }
    await store.revokeDevice(ownerId, deviceId, identity.actor_user_id);
    return reply.status(204).send();
  });

  app.get("/v1/sessions", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) =>
        !["device_id", "agent_type", "activity_status", "presence"].includes(
          key,
        ),
    );
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    const filters: SessionFilters = {};
    if (query.device_id !== undefined) {
      if (typeof query.device_id !== "string") {
        throw new ContractError("invalid_query", "device_id is invalid");
      }
      filters.device_id = query.device_id;
    }
    if (query.agent_type !== undefined) {
      if (
        typeof query.agent_type !== "string" ||
        !isOneOf(agentTypes, query.agent_type)
      ) {
        throw new ContractError("invalid_query", "agent_type is invalid");
      }
      filters.agent_type = query.agent_type;
    }
    if (query.activity_status !== undefined) {
      if (
        typeof query.activity_status !== "string" ||
        !isOneOf(activityStatuses, query.activity_status)
      ) {
        throw new ContractError("invalid_query", "activity_status is invalid");
      }
      filters.activity_status = query.activity_status;
    }
    if (query.presence !== undefined) {
      if (
        typeof query.presence !== "string" ||
        !isOneOf(presences, query.presence)
      ) {
        throw new ContractError("invalid_query", "presence is invalid");
      }
      filters.presence = query.presence;
    }
    const sessions = await store.listSessions(ownerId, filters);
    return reply.send({ sessions });
  });

  app.get("/v1/sessions/events", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) =>
        !["device_id", "agent_type", "agent_session_id", "limit"].includes(key),
    );
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    if (typeof query.device_id !== "string" || query.device_id.length === 0) {
      throw new ContractError("invalid_query", "device_id is required");
    }
    if (
      typeof query.agent_type !== "string" ||
      !isOneOf(agentTypes, query.agent_type)
    ) {
      throw new ContractError("invalid_query", "agent_type is invalid");
    }
    if (
      typeof query.agent_session_id !== "string" ||
      query.agent_session_id.length === 0 ||
      query.agent_session_id.length > 512
    ) {
      throw new ContractError("invalid_query", "agent_session_id is required");
    }
    let limit = DEFAULT_SESSION_EVENT_LIMIT;
    if (query.limit !== undefined) {
      if (
        typeof query.limit !== "string" ||
        !/^[1-9][0-9]{0,3}$/.test(query.limit)
      ) {
        throw new ContractError("invalid_query", "limit is invalid");
      }
      limit = Number(query.limit);
      if (limit > MAX_SESSION_EVENT_LIMIT) {
        throw new ContractError("invalid_query", "limit is invalid");
      }
    }
    const events = await store.listSessionEvents(ownerId, {
      device_id: query.device_id,
      agent_type: query.agent_type,
      agent_session_id: query.agent_session_id,
      limit,
    });
    return reply.send({ events });
  });

  app.get("/v1/push/key", async (_request, reply) => {
    if (options.push === undefined) {
      return reply.status(404).send({ error: "push_disabled" });
    }
    return reply.send({ public_key: options.push.publicKey });
  });

  app.post("/v1/push/subscriptions", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    if (options.push === undefined) {
      return reply.status(404).send({ error: "push_disabled" });
    }
    await store.savePushSubscription(ownerId, parseSubscription(request.body));
    return reply.status(204).send();
  });

  app.delete("/v1/push/subscriptions", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const body = expectObject(request.body);
    const unknownField = Object.keys(body).find((key) => key !== "endpoint");
    if (unknownField !== undefined) {
      throw new ContractError("unknown_field", "body field is invalid");
    }
    await store.deletePushSubscription(ownerId, parseEndpoint(body.endpoint));
    return reply.status(204).send();
  });

  async function notifyWaitingSessions(
    ownerId: string,
    changes: readonly SessionStatusChange[],
  ): Promise<void> {
    const push = options.push;
    if (push === undefined) {
      return;
    }
    const waiting = changes.filter((change) =>
      NOTIFIED_STATUSES.has(change.activity_status),
    );
    if (waiting.length === 0) {
      return;
    }
    let live = await store.listPushSubscriptions(ownerId);
    for (const change of waiting) {
      if (live.length === 0) {
        return;
      }
      // Only fields the panel already shows: no task text, no working
      // directory, no raw agent status detail.
      const payload = JSON.stringify({
        device_id: change.device_id,
        agent_type: change.agent_type,
        agent_session_id: change.agent_session_id,
        project: change.project_display_name,
        activity_status: change.activity_status,
        waiting_reason: change.waiting_reason ?? "unknown",
        observed_at: change.observed_at,
      });
      const outcomes = await Promise.all(
        live.map(async (subscription) => ({
          subscription,
          outcome: await push.sender.send(subscription, payload),
        })),
      );
      const deliveryCounts = {
        delivered: outcomes.filter((entry) => entry.outcome === "delivered")
          .length,
        gone: outcomes.filter((entry) => entry.outcome === "gone").length,
        failed: outcomes.filter((entry) => entry.outcome === "failed").length,
      };
      // Keep production diagnosis possible without logging an endpoint, owner,
      // session identifier or notification payload.
      app.log.info(
        { push_delivery: deliveryCounts },
        "push delivery completed",
      );
      for (const { subscription, outcome } of outcomes) {
        if (outcome === "gone") {
          // The browser dropped this subscription; keeping it would retry a
          // dead endpoint on every future notification.
          await store.deletePushSubscription(ownerId, subscription.endpoint);
        }
      }
      live = outcomes
        .filter((entry) => entry.outcome !== "gone")
        .map((entry) => entry.subscription);
    }
  }

  // Server-sent events rather than WebSocket: the read-only phase only pushes
  // gateway -> panel, and the panel reaches the gateway through a same-origin
  // HTTP proxy. Adding WebSocket would require a separately authenticated
  // upgrade proxy, which is deferred until the product has a bidirectional
  // use case.
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
      // Ask reverse proxies not to buffer; buffering turns the stream into a
      // hung request that only flushes when the connection closes.
      "x-accel-buffering": "no",
    });
    request.raw.socket.setNoDelay(true);
    request.raw.socket.setTimeout(0);

    const write = (chunk: string): void => {
      if (!raw.writableEnded) {
        raw.write(chunk);
      }
    };
    write(`retry: ${STREAM_RETRY_MS}\n\n`);
    write("event: ready\ndata: {}\n\n");

    const unsubscribe = broker.subscribe(ownerId, (message) => {
      write(`event: sessions\ndata: ${JSON.stringify(message)}\n\n`);
    });
    const heartbeat = setInterval(
      () => write(": keepalive\n\n"),
      STREAM_HEARTBEAT_MS,
    );
    heartbeat.unref();

    let closed = false;
    const close = (): void => {
      if (closed) {
        return;
      }
      closed = true;
      clearInterval(heartbeat);
      unsubscribe();
      openStreams.delete(close);
      raw.end();
    };
    openStreams.add(close);
    raw.on("close", close);
    raw.on("error", close);
  });

  // Retired in-memory prototype is available only to explicitly injected fixtures.
  // Production remote traffic must use the authenticated, durable v3 routes.
  if (options.v2Manager) {
    const v2Manager = options.v2Manager;

    // v2 WebSocket Multiplexing Bus for Runners
    void app.register(async (v2WsApp) => {
      v2WsApp.get("/v2/ws", { websocket: true }, (socket, _request) => {
        let runnerConn: any;

        socket.on("message", (data: any) => {
          try {
            const frame = JSON.parse(data.toString("utf8"));
            if (!runnerConn && frame.type === "handshake") {
              const runnerId =
                frame.payload?.runner_id || `runner_${randomUUID()}`;
              runnerConn = v2Manager.registerRunner(runnerId, socket as any);
            }
            if (runnerConn) {
              v2Manager.handleIncomingFrame(runnerConn, frame);
            }
          } catch {
            // ignore parse error
          }
        });

        socket.on("close", () => {
          if (runnerConn) {
            v2Manager.unregisterRunner(runnerConn.runner_id);
          }
        });
      });
    });

    // v2 Dynamic Capabilities
    app.get("/v2/capabilities", async (request) => {
      await authenticator.authenticate(request);
      return { capabilities: v2Manager.listAllCapabilities() };
    });

    app.get("/v2/runners/:runnerId/capabilities", async (request, reply) => {
      await authenticator.authenticate(request);
      const { runnerId } = request.params as { runnerId: string };
      const caps = v2Manager.getCapabilities(runnerId);
      if (!caps) {
        void reply.status(404).send({ error: "runner_not_found" });
        return;
      }
      return { capabilities: caps };
    });

    app.post("/v2/capabilities", async (request) => {
      const report = validateCapabilityReport(request.body);
      v2Manager.saveCapabilities(report);
      return { status: "recorded" };
    });

    // v2 Workspace-First Sessions
    app.get("/v2/sessions", async (request) => {
      await authenticator.authenticate(request);
      const query = request.query as {
        runner_id?: string;
        workspace_path?: string;
      };
      return { sessions: v2Manager.eventStore.listSessions(query) };
    });

    app.get("/v2/sessions/:sessionId", async (request, reply) => {
      await authenticator.authenticate(request);
      const { sessionId } = request.params as { sessionId: string };
      const session = v2Manager.eventStore.getSession(sessionId);
      if (!session) {
        void reply.status(404).send({ error: "session_not_found" });
        return;
      }
      return { session };
    });

    // v2 Interactive Control Commands
    app.post("/v2/sessions/launch", async (request, reply) => {
      await authenticator.authenticate(request);
      const body = expectObject(request.body);
      const profile = validateLaunchProfile(body.launch_profile);
      const cmdId =
        typeof body.cmd_id === "string"
          ? body.cmd_id
          : `cmd_${Date.now().toString(36)}`;
      const hubSessionId = `hub_${randomUUID()}`;

      try {
        v2Manager.sendCommand(profile.target_runner_id, hubSessionId, cmdId, {
          action: "launch_session",
          launch_profile: profile,
        });
        return { hub_session_id: hubSessionId, cmd_id: cmdId };
      } catch (err: unknown) {
        const msg = err instanceof Error ? err.message : "launch_failed";
        void reply.status(400).send({ error: msg });
      }
    });

    app.post("/v2/sessions/:sessionId/prompt", async (request, reply) => {
      await authenticator.authenticate(request);
      const { sessionId } = request.params as { sessionId: string };
      const session = v2Manager.eventStore.getSession(sessionId);
      if (!session) {
        void reply.status(404).send({ error: "session_not_found" });
        return;
      }
      const body = expectObject(request.body);
      const text = typeof body.text === "string" ? body.text : "";
      if (!text.trim()) {
        void reply.status(400).send({ error: "prompt_text_required" });
        return;
      }
      const cmdId =
        typeof body.cmd_id === "string"
          ? body.cmd_id
          : `cmd_${Date.now().toString(36)}`;

      v2Manager.sendCommand(session.runner_id, sessionId, cmdId, {
        action: "send_prompt",
        text,
        client_turn_id:
          typeof body.client_turn_id === "string"
            ? body.client_turn_id
            : undefined,
      });
      return { status: "queued", cmd_id: cmdId };
    });

    app.post("/v2/sessions/:sessionId/approve", async (request, reply) => {
      await authenticator.authenticate(request);
      const { sessionId } = request.params as { sessionId: string };
      const session = v2Manager.eventStore.getSession(sessionId);
      if (!session) {
        void reply.status(404).send({ error: "session_not_found" });
        return;
      }
      const body = expectObject(request.body);
      const approvalId =
        typeof body.approval_id === "string" ? body.approval_id : "";
      const decision =
        body.decision === "approved" || body.decision === "rejected"
          ? body.decision
          : "rejected";
      const cmdId =
        typeof body.cmd_id === "string"
          ? body.cmd_id
          : `cmd_${Date.now().toString(36)}`;

      v2Manager.sendCommand(session.runner_id, sessionId, cmdId, {
        action: "resolve_approval",
        approval_id: approvalId,
        decision,
        custom_feedback:
          typeof body.custom_feedback === "string"
            ? body.custom_feedback
            : undefined,
      });
      return { status: "sent", cmd_id: cmdId };
    });

    app.post("/v2/sessions/:sessionId/interrupt", async (request, reply) => {
      await authenticator.authenticate(request);
      const { sessionId } = request.params as { sessionId: string };
      const session = v2Manager.eventStore.getSession(sessionId);
      if (!session) {
        void reply.status(404).send({ error: "session_not_found" });
        return;
      }
      const body = expectObject(request.body);
      const cmdId =
        typeof body.cmd_id === "string"
          ? body.cmd_id
          : `cmd_${Date.now().toString(36)}`;

      v2Manager.sendCommand(session.runner_id, sessionId, cmdId, {
        action: "interrupt_turn",
        reason:
          typeof body.reason === "string" ? body.reason : "user_cancelled",
      });
      return { status: "sent", cmd_id: cmdId };
    });

    // v2 Live Stream Delta SSE for Session
    app.get("/v2/sessions/:sessionId/stream", async (request, reply) => {
      await authenticator.authenticate(request);
      const { sessionId } = request.params as { sessionId: string };
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

      const write = (chunk: string): void => {
        if (!raw.writableEnded) {
          raw.write(chunk);
        }
      };
      write(`retry: ${STREAM_RETRY_MS}\n\n`);
      write("event: ready\ndata: {}\n\n");

      const unsubscribe = v2Manager.eventStore.subscribeDeltas(
        sessionId,
        (delta) => {
          write(`event: delta\ndata: ${JSON.stringify(delta)}\n\n`);
        },
      );

      const close = (): void => {
        unsubscribe();
        raw.end();
      };
      raw.on("close", close);
      raw.on("error", close);
    });
  }

  app.addHook("onClose", async () => {
    for (const close of [...openStreams]) {
      close();
    }
    await broker.close();
    await store.close?.();
  });

  return app;
}

function expectObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError("invalid_body", "request body must be an object");
  }
  return value as Record<string, unknown>;
}

function bindingTtlSeconds(value: unknown): number {
  if (value === undefined || value === null) {
    return DEFAULT_BINDING_TTL_SECONDS;
  }
  const body = expectObject(value);
  const unknownField = Object.keys(body).find(
    (key) => key !== "expires_in_seconds",
  );
  if (unknownField !== undefined) {
    throw new ContractError("unknown_field", "body field is invalid");
  }
  if (body.expires_in_seconds === undefined) {
    return DEFAULT_BINDING_TTL_SECONDS;
  }
  if (
    typeof body.expires_in_seconds !== "number" ||
    !Number.isSafeInteger(body.expires_in_seconds) ||
    body.expires_in_seconds < MIN_BINDING_TTL_SECONDS ||
    body.expires_in_seconds > MAX_BINDING_TTL_SECONDS
  ) {
    throw new ContractError(
      "invalid_binding_ttl",
      "binding lifetime is invalid",
    );
  }
  return body.expires_in_seconds;
}

/**
 * Accepts the shape `PushSubscription.toJSON()` produces in the browser, and
 * nothing else: unknown fields are rejected rather than stored, matching how
 * sync events are parsed.
 */
function parseSubscription(value: unknown): PushSubscription {
  const body = expectObject(value);
  const unknownField = Object.keys(body).find(
    (key) => !["endpoint", "keys", "expirationTime"].includes(key),
  );
  if (unknownField !== undefined) {
    throw new ContractError("unknown_field", "body field is invalid");
  }
  const keys = expectObject(body.keys);
  const unknownKey = Object.keys(keys).find(
    (key) => !["p256dh", "auth"].includes(key),
  );
  if (unknownKey !== undefined) {
    throw new ContractError("unknown_field", "keys field is invalid");
  }
  return {
    endpoint: parseEndpoint(body.endpoint),
    p256dh: parseKeyMaterial(keys.p256dh, "p256dh", 65),
    auth: parseKeyMaterial(keys.auth, "auth", 16),
  };
}

function parseEndpoint(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > MAX_PUSH_ENDPOINT_LENGTH
  ) {
    throw new ContractError("invalid_endpoint", "endpoint is invalid");
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new ContractError("invalid_endpoint", "endpoint is not a URL");
  }
  // Push services are always HTTPS. Requiring it also keeps a subscription
  // from steering the gateway at a plaintext internal address.
  if (
    url.protocol !== "https:" ||
    url.username !== "" ||
    url.password !== "" ||
    url.hash !== ""
  ) {
    throw new ContractError("invalid_endpoint", "endpoint is not allowed");
  }
  return url.toString();
}

function parseKeyMaterial(
  value: unknown,
  field: string,
  expectedBytes: number,
): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]{1,200}$/.test(value)) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  const decoded = Buffer.from(value, "base64url");
  if (decoded.length !== expectedBytes) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  if (field === "p256dh" && decoded[0] !== 0x04) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  return value;
}

function isOneOf<const T extends readonly string[]>(
  values: T,
  value: string,
): value is T[number] {
  return (values as readonly string[]).includes(value);
}

function assertFreshDeviceTimestamp(value: string, now: Date): void {
  if (Math.abs(Date.parse(value) - now.getTime()) > MAX_DEVICE_CLOCK_SKEW_MS) {
    throw new ContractError(
      "stale_device_request",
      "device timestamp is stale",
    );
  }
}

function requireAccounts(accounts: AccountStore | undefined): AccountStore {
  if (accounts === undefined) {
    throw new AccountError(
      "accounts_unavailable",
      "account service is not configured",
      503,
    );
  }
  return accounts;
}

async function requireSessionIdentity(
  accounts: AccountStore,
  request: Parameters<typeof currentSessionIdentity>[1],
  at: Date,
) {
  const identity = await currentSessionIdentity(accounts, request, at);
  if (identity === undefined) throw new AuthenticationError();
  return identity;
}

async function requireAccountIdentity(
  accounts: AccountStore,
  authenticator: OwnerAuthenticator,
  request: Parameters<typeof currentSessionIdentity>[1],
  at: Date,
) {
  const sessionIdentity = await currentSessionIdentity(accounts, request, at);
  if (sessionIdentity !== undefined) return sessionIdentity;
  const ownerId = await authenticator.authenticate(request);
  return {
    actor_user_id: ownerId,
    workspace_owner_id: ownerId,
    role: "owner" as const,
  };
}

async function mutationIdentity(
  accounts: AccountStore | undefined,
  authenticator: OwnerAuthenticator,
  request: Parameters<typeof currentSessionIdentity>[1],
  at: Date,
): Promise<{
  workspace_owner_id: string;
  actor_user_id?: string;
}> {
  if (accounts !== undefined) {
    const identity = await currentSessionIdentity(accounts, request, at);
    if (identity !== undefined) {
      return {
        workspace_owner_id: identity.workspace_owner_id,
        actor_user_id: identity.actor_user_id,
      };
    }
  }
  return {
    workspace_owner_id: await authenticator.authenticate(request),
  };
}

function inviteRole(value: unknown): InviteRole {
  if (value !== "admin" && value !== "member") {
    throw new AccountError("invalid_team_role", "team role is invalid", 400);
  }
  return value;
}

function paidPlan(value: unknown): PaidSubscriptionPlan {
  if (value !== "pro" && value !== "team") {
    throw new AccountError(
      "invalid_billing_plan",
      "billing plan is invalid",
      400,
    );
  }
  return value;
}

async function issueEmailVerification(
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
  } catch (error) {
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

function requireVerifiedEmail(account: AccountView, required: boolean): void {
  if (required && account.user.email_verified_at === undefined) {
    throw new AccountError(
      "email_verification_required",
      "email verification is required",
      403,
    );
  }
}
