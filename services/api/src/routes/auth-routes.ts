import { createHmac, randomBytes } from "node:crypto";

import { parseAvatarPayload } from "@agent-pulse/contracts";
import type { FastifyReply, FastifyRequest } from "fastify";

import {
  AccountError,
  accountBody,
  assertOnlyFields,
  clearLoginSession,
  createLoginSession,
  currentSessionDigest,
  currentSessionIdentity,
  hashPassword,
  newUserId,
  normalizeDisplayName,
  normalizeEmail,
  tokenDigest,
  validatePassword,
  verifyAccountPassword,
  type AccountView,
  type LoginAttemptResult,
} from "../accounts.js";
import {
  issueEmailVerification,
  requireAccountIdentity,
  requireAccounts,
  requireSessionIdentity,
} from "../app-helpers.js";
import { AuthenticationError } from "../auth.js";
import { passwordResetUrl } from "../email.js";
import type { GatewayRouteContext } from "./gateway-context.js";

const EMAIL_VERIFICATION_BYTES = 32;
const EMAIL_VERIFICATION_TTL_MS = 24 * 60 * 60_000;
const EMAIL_VERIFICATION_MIN_INTERVAL_SECONDS = 60;
const PASSWORD_RESET_BYTES = 32;
const PASSWORD_RESET_TTL_MS = 60 * 60_000;
const PASSWORD_RESET_MIN_INTERVAL_SECONDS = 60;
const LOGIN_WINDOW_SECONDS = 15 * 60;
const LOGIN_MAXIMUM_ATTEMPTS = 5;
const LOGIN_IP_MAXIMUM_ATTEMPTS = 60;
const PASSWORD_USER_MAXIMUM_ATTEMPTS = 10;
const LOGIN_BLOCK_SECONDS = 15 * 60;
const DEFAULT_AUDIT_LIMIT = 30;
const MAX_AUDIT_LIMIT = 100;
const SESSION_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function registerAuthRoutes(ctx: GatewayRouteContext): void {
  const {
    app,
    options,
    authenticator,
    now,
    loginRateLimitSecret,
    secureCookies,
    singleAdmin,
    invalidateAllStreamAuth,
  } = ctx;

  app.get("/v1/auth/status", async () => {
    if (options.accounts?.hasAnyAdmin !== undefined) {
      const hasAdmin = await options.accounts.hasAnyAdmin();
      return { needs_setup: !hasAdmin };
    }
    return { needs_setup: false };
  });

  let setupLock: Promise<void> = Promise.resolve();
  const setupAdminHandler = async (
    request: FastifyRequest,
    reply: FastifyReply,
  ) => {
    const accounts = requireAccounts(options.accounts);
    const setupIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`setup_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const setupIpLimit = await accounts.consumeLoginAttempt({
      scopeDigest: setupIpScope,
      now: now().toISOString(),
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: 20,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!setupIpLimit.allowed) {
      reply.header("retry-after", String(setupIpLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "password", "display_name"]);
    const email = normalizeEmail(body.email);
    const password = validatePassword(body.password);
    const passwordHash = await hashPassword(password);
    const runCreate = async (): Promise<AccountView> => {
      if (
        singleAdmin &&
        accounts.hasAnyAdmin !== undefined &&
        (await accounts.hasAnyAdmin())
      ) {
        throw new AccountError(
          "admin_already_exists",
          "administrator is already configured",
          409,
        );
      }
      return await accounts.createAccount({
        userId: newUserId(),
        email,
        displayName: normalizeDisplayName(body.display_name, email),
        passwordHash,
        singleAdmin,
      });
    };
    let account: AccountView;
    if (singleAdmin) {
      const resultPromise = setupLock.then(runCreate);
      setupLock = resultPromise.then(
        () => undefined,
        () => undefined,
      );
      account = await resultPromise;
    } else {
      account = await runCreate();
    }
    await createLoginSession(
      accounts,
      account.user.user_id,
      reply,
      secureCookies,
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
  };

  app.post("/v1/auth/setup", setupAdminHandler);
  app.post("/v1/auth/register", setupAdminHandler);

  app.post("/v1/auth/login", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "password"]);
    const email = normalizeEmail(body.email);
    const password = validatePassword(body.password);
    const currentNow = now().toISOString();
    const loginIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`login_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const ipLimit = await accounts.consumeLoginAttempt({
      scopeDigest: loginIpScope,
      now: currentNow,
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: LOGIN_IP_MAXIMUM_ATTEMPTS,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!ipLimit.allowed) {
      reply.header("retry-after", String(ipLimit.retry_after_seconds));
      throw new AccountError(
        "login_rate_limited",
        "too many login attempts",
        429,
      );
    }
    const loginScope = createHmac("sha256", loginRateLimitSecret)
      .update(`${email}\n${request.ip}`, "utf8")
      .digest("hex");
    const accountScope = createHmac("sha256", loginRateLimitSecret)
      .update(`account\n${email}`, "utf8")
      .digest("hex");
    const rateLimit = await accounts.consumeLoginAttempt({
      scopeDigest: loginScope,
      now: currentNow,
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: LOGIN_MAXIMUM_ATTEMPTS,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    const accountLimit = rateLimit.allowed
      ? await accounts.consumeLoginAttempt({
          scopeDigest: accountScope,
          now: currentNow,
          windowSeconds: LOGIN_WINDOW_SECONDS,
          maximumAttempts: LOGIN_MAXIMUM_ATTEMPTS * 4,
          blockSeconds: LOGIN_BLOCK_SECONDS,
        })
      : rateLimit;
    const effectiveLimit = !rateLimit.allowed ? rateLimit : accountLimit;
    if (!effectiveLimit.allowed) {
      reply.header("retry-after", String(effectiveLimit.retry_after_seconds));
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
    await accounts.clearLoginAttempts(accountScope);
    await createLoginSession(
      accounts,
      user.user_id,
      reply,
      secureCookies,
      now(),
      "login",
    );
    return reply.send(account);
  });

  app.post("/v1/auth/logout", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    await clearLoginSession(accounts, request, reply, secureCookies);
    invalidateAllStreamAuth();
    return reply.status(204).send();
  });

  app.get("/v1/auth/me", async (request, reply) => {
    const accounts = options.accounts;
    const identity = await requireAccountIdentity(
      accounts,
      authenticator,
      request,
      now(),
      secureCookies,
    );
    let account = accounts
      ? await accounts.accountView(identity.actor_user_id)
      : undefined;
    if (account === undefined && identity.role === "owner") {
      account = {
        user: {
          user_id: identity.actor_user_id,
          email: "owner@self-hosted.local",
          display_name: "Self-Host Owner",
          status: "active",
          email_verified_at: new Date(0).toISOString(),
          created_at: new Date(0).toISOString(),
        },
        entitlements: {
          max_devices: 999,
          max_concurrent_tasks: 999,
          history_days: 3650,
          remote_tasks: true,
          team_members: 1,
        },
        subscription: {
          plan: "pro",
          status: "active",
        },
      };
    }
    if (account === undefined) throw new AuthenticationError();
    const workspaceAccount = accounts
      ? await accounts.accountView(identity.workspace_owner_id)
      : undefined;
    const listedWorkspaces = accounts
      ? await accounts.listWorkspaces(
          identity.actor_user_id,
          identity.workspace_owner_id,
        )
      : [];
    const workspaces =
      listedWorkspaces.length > 0 || identity.role !== "owner"
        ? listedWorkspaces
        : [
            {
              workspace_owner_id: identity.workspace_owner_id,
              name: "Default Workspace",
              role: "owner" as const,
              current: true,
            },
          ];
    return reply.send({
      ...account,
      workspaces,
      workspace:
        workspaces.find((workspace) => workspace.current) ?? workspaces[0],
      workspace_subscription:
        workspaceAccount?.subscription ?? account.subscription,
      workspace_entitlements:
        workspaceAccount?.entitlements ?? account.entitlements,
    });
  });

  app.post("/v1/auth/email-verifications", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const verifyConfirmScope = createHmac("sha256", loginRateLimitSecret)
      .update(`email_verify_confirm_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const verifyConfirmLimit = await accounts.consumeLoginAttempt({
      scopeDigest: verifyConfirmScope,
      now: now().toISOString(),
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: 20,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!verifyConfirmLimit.allowed) {
      reply.header(
        "retry-after",
        String(verifyConfirmLimit.retry_after_seconds),
      );
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
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
    const resetIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`reset_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const resetLimit = await accounts.consumeLoginAttempt({
      scopeDigest: resetIpScope,
      now: now().toISOString(),
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: 20,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!resetLimit.allowed) {
      reply.header("retry-after", String(resetLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
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
      if (issued.allowed && delivery.sender.sendPasswordReset !== undefined) {
        const sendReset = delivery.sender.sendPasswordReset;
        void sendReset({
          user_id: user.user_id,
          email: user.email,
          display_name: user.display_name,
          reset_url: passwordResetUrl(delivery.baseUrl, token),
          expires_at: expiresAt.toISOString(),
        }).catch(async (error: unknown) => {
          try {
            await accounts.deletePendingPasswordReset(digest);
          } catch (cleanupErr) {
            app.log.error(cleanupErr, "password reset rollback failed");
          }
          app.log.error(error, "password reset delivery failed");
        });
      }
    }
    return reply.status(202).send({ accepted: true });
  });

  app.post("/v1/auth/password-resets/confirm", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const confirmScope = createHmac("sha256", loginRateLimitSecret)
      .update(`reset_confirm_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const confirmLimit = await accounts.consumeLoginAttempt({
      scopeDigest: confirmScope,
      now: now().toISOString(),
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: 20,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!confirmLimit.allowed) {
      reply.header("retry-after", String(confirmLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
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
    await clearLoginSession(accounts, request, reply, secureCookies);
    invalidateAllStreamAuth();
    return reply.send({ changed: true });
  });

  app.post("/v1/auth/password", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const userId = identity.actor_user_id;
    const currentDigest = currentSessionDigest(request, secureCookies);
    if (currentDigest === undefined) throw new AuthenticationError();
    const currentNow = now().toISOString();
    const passwordIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`password_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const passwordIpLimit = await accounts.consumeLoginAttempt({
      scopeDigest: passwordIpScope,
      now: currentNow,
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: LOGIN_IP_MAXIMUM_ATTEMPTS,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!passwordIpLimit.allowed) {
      reply.header("retry-after", String(passwordIpLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
    const passwordUserScope = createHmac("sha256", loginRateLimitSecret)
      .update(`password_user\n${userId}`, "utf8")
      .digest("hex");
    const passwordUserLimit = await accounts.consumeLoginAttempt({
      scopeDigest: passwordUserScope,
      now: currentNow,
      windowSeconds: LOGIN_WINDOW_SECONDS,
      maximumAttempts: PASSWORD_USER_MAXIMUM_ATTEMPTS,
      blockSeconds: LOGIN_BLOCK_SECONDS,
    });
    if (!passwordUserLimit.allowed) {
      reply.header(
        "retry-after",
        String(passwordUserLimit.retry_after_seconds),
      );
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
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
    await accounts.clearLoginAttempts(passwordUserScope);
    invalidateAllStreamAuth();
    return reply.send({ changed: true, revoked: result.revoked_sessions });
  });

  app.patch("/v1/auth/avatar", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const body = accountBody(request.body);
    assertOnlyFields(body, ["data_url"]);
    // `null` clears the picture; anything else has to be a real image.
    const avatar =
      body.data_url === null
        ? undefined
        : parseAvatarPayload({ data_url: body.data_url });
    const updated = await accounts.setAvatar(
      identity.actor_user_id,
      avatar?.data_url,
      now().toISOString(),
    );
    if (!updated) throw new AuthenticationError();
    return reply.send({ avatar_data_url: avatar?.data_url ?? null });
  });

  app.get("/v1/auth/sessions", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const digest = currentSessionDigest(request, secureCookies);
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const userId = identity.actor_user_id;
    const { sessionId } = request.params as { sessionId?: unknown };
    if (typeof sessionId !== "string" || !SESSION_ID_PATTERN.test(sessionId)) {
      throw new AccountError(
        "invalid_session_id",
        "session ID is invalid",
        400,
      );
    }
    const digest = currentSessionDigest(request, secureCookies);
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
      await clearLoginSession(accounts, request, reply, secureCookies);
    }
    invalidateAllStreamAuth();
    return reply.status(204).send();
  });

  app.delete("/v1/auth/sessions", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const digest = currentSessionDigest(request, secureCookies);
    if (digest === undefined) throw new AuthenticationError();
    const revoked = await accounts.revokeOtherWebSessions(
      identity.actor_user_id,
      digest,
    );
    if (revoked > 0) invalidateAllStreamAuth();
    return reply.send({ revoked });
  });

  app.get("/v1/audit", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
}
