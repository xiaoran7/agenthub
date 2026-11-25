import { createHmac, randomBytes, randomUUID } from "node:crypto";

import fastifyRawBody from "fastify-raw-body";

import {
  AccountError,
  accountBody,
  assertOnlyFields,
  currentSessionDigest,
  normalizeEmail,
  tokenDigest,
} from "../accounts.js";
import {
  inviteRole,
  mutationIdentity,
  paidPlan,
  requireAccounts,
  requireSessionIdentity,
  requireVerifiedEmail,
} from "../app-helpers.js";
import { AuthenticationError } from "../auth.js";
import { teamInvitationUrl } from "../email.js";
import type { GatewayRouteContext } from "./gateway-context.js";

const TEAM_INVITATION_BYTES = 32;
const TEAM_INVITATION_TTL_MS = 72 * 60 * 60_000;
const TEAM_RATE_WINDOW_SECONDS = 15 * 60;
const TEAM_RATE_MAX_ATTEMPTS = 20;
const TEAM_RATE_BLOCK_SECONDS = 15 * 60;
const SESSION_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function registerTeamBillingRoutes(ctx: GatewayRouteContext): void {
  const {
    app,
    options,
    store,
    now,
    loginRateLimitSecret,
    secureCookies,
    invalidateAllStreamAuth,
  } = ctx;

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

  app.get("/v1/team", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const inviteIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`team_invite_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const inviteIpLimit = await accounts.consumeLoginAttempt({
      scopeDigest: inviteIpScope,
      now: now().toISOString(),
      windowSeconds: TEAM_RATE_WINDOW_SECONDS,
      maximumAttempts: TEAM_RATE_MAX_ATTEMPTS,
      blockSeconds: TEAM_RATE_BLOCK_SECONDS,
    });
    if (!inviteIpLimit.allowed) {
      reply.header("retry-after", String(inviteIpLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    if (identity.role !== "owner" && identity.role !== "admin") {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
    const body = accountBody(request.body);
    assertOnlyFields(body, ["email", "role"]);
    const email = normalizeEmail(body.email);
    const role = inviteRole(body.role);
    if (identity.role === "admin" && role === "admin") {
      throw new AccountError(
        "team_permission_denied",
        "team permission denied",
        403,
      );
    }
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
    const acceptIpScope = createHmac("sha256", loginRateLimitSecret)
      .update(`team_invite_accept_ip\n${request.ip}`, "utf8")
      .digest("hex");
    const acceptIpLimit = await accounts.consumeLoginAttempt({
      scopeDigest: acceptIpScope,
      now: now().toISOString(),
      windowSeconds: TEAM_RATE_WINDOW_SECONDS,
      maximumAttempts: TEAM_RATE_MAX_ATTEMPTS,
      blockSeconds: TEAM_RATE_BLOCK_SECONDS,
    });
    if (!acceptIpLimit.allowed) {
      reply.header("retry-after", String(acceptIpLimit.retry_after_seconds));
      throw new AccountError("login_rate_limited", "too many attempts", 429);
    }
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    await store.deletePushSubscriptionsForSubscriber?.(
      identity.workspace_owner_id,
      userId,
    );
    invalidateAllStreamAuth();
    return reply.status(204).send();
  });

  app.post("/v1/team/workspace", async (request, reply) => {
    const accounts = requireAccounts(options.accounts);
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
    const digest = currentSessionDigest(request, secureCookies);
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
    invalidateAllStreamAuth();
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
    const hasManageableSubscription =
      reference?.subscription_id !== undefined &&
      (account.subscription.status === "active" ||
        account.subscription.status === "trialing" ||
        account.subscription.status === "past_due");
    if (hasManageableSubscription) {
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
    const identity = await requireSessionIdentity(
      accounts,
      request,
      now(),
      secureCookies,
    );
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
}
