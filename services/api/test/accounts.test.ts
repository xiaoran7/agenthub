import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import test from "node:test";

process.env.AGENT_PULSE_ENFORCE_ENTITLEMENTS = "true";

const TEST_NOW = new Date();
const testTime = (offsetMs = 0) =>
  new Date(TEST_NOW.getTime() + offsetMs).toISOString();

import { createGatewayApp } from "../src/app.js";
import {
  InMemoryAccountStore,
  SessionAuthenticator,
  auditMetadata,
  entitlementsFor,
  hashPassword,
  type SubscriptionEvent,
  verifyPassword,
} from "../src/accounts.js";
import {
  BillingEventVerifier,
  canonicalSubscriptionEvent,
} from "../src/billing.js";
import type {
  EmailVerificationMessage,
  PasswordResetMessage,
} from "../src/email.js";
import { InMemoryGatewayStore } from "../src/store.js";

function sessionCookie(response: { headers: Record<string, unknown> }): string {
  const value = response.headers["set-cookie"];
  if (typeof value !== "string") {
    assert.fail("response did not set a session cookie");
  }
  return value.split(";", 1)[0] ?? "";
}

function accountApp() {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  return {
    store,
    app: createGatewayApp({
      accounts,
      store,
      authenticator: new SessionAuthenticator(accounts),
      now: () => TEST_NOW,
      singleAdmin: false,
    }),
  };
}

test("password hashes round-trip without retaining plaintext", async () => {
  const encoded = await hashPassword("correct horse battery staple");
  assert.match(encoded, /^scrypt\$16384\$8\$1\$/);
  assert.equal(encoded.includes("correct horse"), false);
  assert.equal(
    await verifyPassword("correct horse battery staple", encoded),
    true,
  );
  assert.equal(await verifyPassword("wrong password", encoded), false);
});

test("subscription plans resolve to server-side entitlements", () => {
  assert.deepEqual(entitlementsFor({ plan: "free", status: "active" }), {
    max_devices: 1,
    max_concurrent_tasks: 0,
    history_days: 7,
    remote_tasks: false,
    team_members: 1,
  });
  assert.equal(
    entitlementsFor({ plan: "team", status: "past_due" }).max_devices,
    1,
  );
});

test("audit metadata only accepts bounded public fields", () => {
  assert.deepEqual(auditMetadata({ role: "member", revoked_sessions: 2 }), {
    role: "member",
    revoked_sessions: 2,
  });
  assert.throws(
    () =>
      auditMetadata({
        email: "secret@example.com",
      } as unknown as Parameters<typeof auditMetadata>[0]),
    /key is not allowed/u,
  );
  assert.throws(
    () => auditMetadata({ provider: "x".repeat(129) }),
    /value is invalid/u,
  );
});

test("registration creates a free account and an HttpOnly session", async () => {
  const { app } = accountApp();
  const response = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "User@Example.COM",
      password: "correct horse battery staple",
      display_name: "Tanner",
    },
  });
  assert.equal(response.statusCode, 201);
  const body = response.json();
  assert.equal(body.user.email, "user@example.com");
  assert.equal(body.user.password_hash, undefined);
  assert.equal(body.subscription.plan, "free");
  assert.equal(body.entitlements.max_devices, 1);
  assert.match(
    String(response.headers["set-cookie"]),
    /HttpOnly; SameSite=Lax/,
  );

  const me = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: sessionCookie(response) },
  });
  assert.equal(me.statusCode, 200);
  assert.equal(me.json().user.user_id, body.user.user_id);
  await app.close();
});

test("login, logout and duplicate-email failures use stable errors", async () => {
  const { app } = accountApp();
  const payload = {
    email: "owner@example.com",
    password: "correct horse battery staple",
  };
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload,
  });
  assert.equal(registered.statusCode, 201);
  const duplicate = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload,
  });
  assert.equal(duplicate.statusCode, 409);
  assert.equal(duplicate.json().error, "email_already_registered");

  const rejected = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload: { ...payload, password: "definitely incorrect" },
  });
  assert.equal(rejected.statusCode, 401);
  assert.equal(rejected.json().error, "invalid_credentials");

  const loggedIn = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  assert.equal(loggedIn.statusCode, 200);
  const cookie = sessionCookie(loggedIn);
  const loggedOut = await app.inject({
    method: "POST",
    url: "/v1/auth/logout",
    headers: { cookie },
  });
  assert.equal(loggedOut.statusCode, 204);
  const me = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie },
  });
  assert.equal(me.statusCode, 401);
  await app.close();
});

test("registered users cannot spoof another tenant and free limits devices", async () => {
  const { app, store } = accountApp();
  const first = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "first@example.com",
      password: "correct horse battery staple",
    },
  });
  const second = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "second@example.com",
      password: "correct horse battery staple",
    },
  });
  const firstCookie = sessionCookie(first);
  const secondCookie = sessionCookie(second);
  const firstUserId = first.json().user.user_id as string;
  await store.registerDevice(
    firstUserId,
    "dev_11111111111111111111111111111111",
    "22".repeat(32),
  );
  const firstBinding = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { cookie: firstCookie },
  });
  assert.equal(firstBinding.statusCode, 403);
  assert.equal(firstBinding.json().error, "device_limit_reached");

  const secondDevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: {
      cookie: secondCookie,
      "x-user-id": firstUserId,
    },
  });
  assert.equal(secondDevices.statusCode, 200);
  assert.deepEqual(secondDevices.json(), { devices: [] });
  await app.close();
});

test("email verification is one-time, rate limited, and gates write capabilities", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const messages: EmailVerificationMessage[] = [];
  let current = TEST_NOW;
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
    now: () => current,
    emailVerification: {
      baseUrl: "https://pulse.example",
      sender: {
        async sendVerification(message) {
          messages.push(message);
        },
      },
    },
  });
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "verify@example.com",
      password: "correct horse battery staple",
    },
  });
  assert.equal(registered.statusCode, 201);
  assert.equal(registered.json().user.email_verified_at, undefined);
  assert.equal(messages.length, 1);
  const cookie = sessionCookie(registered);

  const binding = await app.inject({
    method: "POST",
    url: "/v1/device-bindings",
    headers: { cookie },
  });
  assert.equal(binding.statusCode, 403);
  assert.equal(binding.json().error, "email_verification_required");

  const resend = await app.inject({
    method: "POST",
    url: "/v1/auth/email-verifications",
    headers: { cookie },
  });
  assert.equal(resend.statusCode, 429);
  assert.equal(resend.headers["retry-after"], "60");
  assert.equal(messages.length, 1);

  const verificationUrl = new URL(messages[0]?.verification_url ?? "");
  assert.equal(verificationUrl.search, "");
  const token = new URLSearchParams(verificationUrl.hash.slice(1)).get("token");
  assert.match(token ?? "", /^[A-Za-z0-9_-]{43}$/);
  const confirmed = await app.inject({
    method: "POST",
    url: "/v1/auth/email-verifications/confirm",
    payload: { token },
  });
  assert.equal(confirmed.statusCode, 200);
  assert.equal(confirmed.json().user.email_verified_at, testTime());
  const replayed = await app.inject({
    method: "POST",
    url: "/v1/auth/email-verifications/confirm",
    payload: { token },
  });
  assert.equal(replayed.statusCode, 400);
  assert.equal(replayed.json().error, "invalid_email_verification");

  current = new Date(TEST_NOW.getTime() + 24 * 60 * 60 * 1_000 + 1_000);
  const alreadyVerified = await app.inject({
    method: "POST",
    url: "/v1/auth/email-verifications",
    headers: { cookie },
  });
  assert.equal(alreadyVerified.statusCode, 202);
  assert.deepEqual(alreadyVerified.json(), { accepted: false, verified: true });
  assert.equal(messages.length, 1);
  await app.close();
});

test("failed verification delivery does not strand the account or rate limit retry", async () => {
  const accounts = new InMemoryAccountStore();
  let deliveries = 0;
  let delivered: EmailVerificationMessage | undefined;
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    now: () => TEST_NOW,
    emailVerification: {
      baseUrl: "https://pulse.example",
      sender: {
        async sendVerification(message) {
          deliveries += 1;
          if (deliveries === 1) throw new Error("provider unavailable");
          delivered = message;
        },
      },
    },
  });
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "delivery-retry@example.com",
      password: "correct horse battery staple",
    },
  });
  assert.equal(registered.statusCode, 201);
  const resent = await app.inject({
    method: "POST",
    url: "/v1/auth/email-verifications",
    headers: { cookie: sessionCookie(registered) },
  });
  assert.equal(resent.statusCode, 202);
  assert.deepEqual(resent.json(), { accepted: true, verified: false });
  assert.equal(deliveries, 2);
  assert.ok(delivered);
  await app.close();
});

test("login attempts are bounded per email and client scope", async () => {
  const { app } = accountApp();
  const payload = {
    email: "limited@example.com",
    password: "correct horse battery staple",
  };
  await app.inject({ method: "POST", url: "/v1/auth/register", payload });
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const response = await app.inject({
      method: "POST",
      url: "/v1/auth/login",
      payload: { ...payload, password: "definitely incorrect" },
    });
    assert.equal(response.statusCode, 401);
  }
  const limited = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  assert.equal(limited.statusCode, 429);
  assert.equal(limited.json().error, "login_rate_limited");
  assert.equal(limited.headers["retry-after"], "900");
  await app.close();
});

test("users can inspect and revoke individual web sessions", async () => {
  const { app } = accountApp();
  const payload = {
    email: "sessions@example.com",
    password: "correct horse battery staple",
  };
  const first = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload,
  });
  const second = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  const firstCookie = sessionCookie(first);
  const secondCookie = sessionCookie(second);
  const listed = await app.inject({
    method: "GET",
    url: "/v1/auth/sessions",
    headers: { cookie: secondCookie },
  });
  assert.equal(listed.statusCode, 200);
  const sessions = listed.json().sessions as Array<{
    session_id: string;
    current: boolean;
  }>;
  assert.equal(sessions.length, 2);
  assert.equal(sessions.filter((session) => session.current).length, 1);

  const revokedOthers = await app.inject({
    method: "DELETE",
    url: "/v1/auth/sessions",
    headers: { cookie: secondCookie },
  });
  assert.deepEqual(revokedOthers.json(), { revoked: 1 });
  const firstMe = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: firstCookie },
  });
  assert.equal(firstMe.statusCode, 401);

  const current = sessions.find((session) => session.current);
  assert.ok(current);
  const otherAccount = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "other-sessions@example.com",
      password: "correct horse battery staple",
    },
  });
  const hidden = await app.inject({
    method: "DELETE",
    url: `/v1/auth/sessions/${current.session_id}`,
    headers: { cookie: sessionCookie(otherAccount) },
  });
  assert.equal(hidden.statusCode, 404);
  const revokeCurrent = await app.inject({
    method: "DELETE",
    url: `/v1/auth/sessions/${current.session_id}`,
    headers: { cookie: secondCookie },
  });
  assert.equal(revokeCurrent.statusCode, 204);
  assert.match(String(revokeCurrent.headers["set-cookie"]), /Max-Age=0/);
  const secondMe = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: secondCookie },
  });
  assert.equal(secondMe.statusCode, 401);
  await app.close();
});

test("password reset is non-enumerating, one-time, and revokes every session", async () => {
  const accounts = new InMemoryAccountStore();
  const resets: PasswordResetMessage[] = [];
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    now: () => TEST_NOW,
    emailVerification: {
      baseUrl: "https://pulse.example",
      sender: {
        async sendVerification() {},
        async sendPasswordReset(message) {
          resets.push(message);
        },
      },
    },
  });
  const payload = {
    email: "reset@example.com",
    password: "correct horse battery staple",
  };
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload,
  });
  const second = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  const unknown = await app.inject({
    method: "POST",
    url: "/v1/auth/password-resets",
    payload: { email: "unknown@example.com" },
  });
  const requested = await app.inject({
    method: "POST",
    url: "/v1/auth/password-resets",
    payload: { email: payload.email },
  });
  assert.equal(unknown.statusCode, 202);
  assert.deepEqual(unknown.json(), { accepted: true });
  assert.deepEqual(requested.json(), { accepted: true });
  assert.equal(resets.length, 1);
  const url = new URL(resets[0]?.reset_url ?? "");
  assert.equal(url.pathname, "/reset-password");
  assert.equal(url.search, "");
  const token = new URLSearchParams(url.hash.slice(1)).get("token");
  assert.match(token ?? "", /^[A-Za-z0-9_-]{43}$/);

  const changed = await app.inject({
    method: "POST",
    url: "/v1/auth/password-resets/confirm",
    payload: { token, password: "a completely new password" },
  });
  assert.deepEqual(changed.json(), { changed: true });
  assert.match(String(changed.headers["set-cookie"]), /Max-Age=0/);
  const replay = await app.inject({
    method: "POST",
    url: "/v1/auth/password-resets/confirm",
    payload: { token, password: "another completely new password" },
  });
  assert.equal(replay.statusCode, 400);
  assert.equal(replay.json().error, "invalid_password_reset");
  for (const cookie of [sessionCookie(registered), sessionCookie(second)]) {
    const me = await app.inject({
      method: "GET",
      url: "/v1/auth/me",
      headers: { cookie },
    });
    assert.equal(me.statusCode, 401);
  }
  const oldLogin = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  assert.equal(oldLogin.statusCode, 401);
  const newLogin = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload: { ...payload, password: "a completely new password" },
  });
  assert.equal(newLogin.statusCode, 200);
  await app.close();
});

test("authenticated password change preserves only the current session", async () => {
  const { app } = accountApp();
  const payload = {
    email: "change-password@example.com",
    password: "correct horse battery staple",
  };
  const first = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload,
  });
  const second = await app.inject({
    method: "POST",
    url: "/v1/auth/login",
    payload,
  });
  const firstCookie = sessionCookie(first);
  const secondCookie = sessionCookie(second);
  const wrong = await app.inject({
    method: "POST",
    url: "/v1/auth/password",
    headers: { cookie: secondCookie },
    payload: {
      current_password: "this is not the password",
      new_password: "a completely new password",
    },
  });
  assert.equal(wrong.statusCode, 401);
  assert.equal(wrong.json().error, "invalid_current_password");
  const changed = await app.inject({
    method: "POST",
    url: "/v1/auth/password",
    headers: { cookie: secondCookie },
    payload: {
      current_password: payload.password,
      new_password: "a completely new password",
    },
  });
  assert.deepEqual(changed.json(), { changed: true, revoked: 1 });
  const firstMe = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: firstCookie },
  });
  const secondMe = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: secondCookie },
  });
  assert.equal(firstMe.statusCode, 401);
  assert.equal(secondMe.statusCode, 200);
  await app.close();
});

test("team workspaces enforce roles, invitations, and actor privacy", async () => {
  const accounts = new InMemoryAccountStore();
  const store = new InMemoryGatewayStore();
  const app = createGatewayApp({
    accounts,
    store,
    authenticator: new SessionAuthenticator(accounts),
    now: () => TEST_NOW,
    singleAdmin: false,
  });
  const register = async (email: string, displayName: string) =>
    app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: {
        email,
        display_name: displayName,
        password: "correct horse battery staple",
      },
    });
  const owner = await register("team-owner@example.com", "Team Owner");
  const admin = await register("team-admin@example.com", "Team Admin");
  const member = await register("team-member@example.com", "Team Member");
  const outsider = await register("team-outsider@example.com", "Outsider");
  const ownerId = owner.json().user.user_id as string;
  const adminId = admin.json().user.user_id as string;
  const memberId = member.json().user.user_id as string;
  accounts.setSubscription(ownerId, { plan: "team", status: "active" });
  await store.registerDevice(
    ownerId,
    "dev_33333333333333333333333333333333",
    "44".repeat(32),
  );

  const invite = async (
    cookie: string,
    email: string,
    role: "admin" | "member",
  ) =>
    app.inject({
      method: "POST",
      url: "/v1/team/invitations",
      headers: { cookie },
      payload: { email, role },
    });
  const accept = async (cookie: string, invitationUrl: string) => {
    const token = new URLSearchParams(invitationUrl.slice(1)).get("token");
    assert.match(token ?? "", /^[A-Za-z0-9_-]{43}$/);
    return app.inject({
      method: "POST",
      url: "/v1/team/invitations/accept",
      headers: { cookie },
      payload: { token },
    });
  };

  const adminInvitation = await invite(
    sessionCookie(owner),
    "team-admin@example.com",
    "admin",
  );
  assert.equal(adminInvitation.statusCode, 201);
  const wrongRecipient = await accept(
    sessionCookie(outsider),
    adminInvitation.json().invitation_url,
  );
  assert.equal(wrongRecipient.statusCode, 400);
  const acceptedAdmin = await accept(
    sessionCookie(admin),
    adminInvitation.json().invitation_url,
  );
  assert.deepEqual(acceptedAdmin.json(), {
    accepted: true,
    workspace_owner_id: ownerId,
  });
  const switchAdmin = await app.inject({
    method: "POST",
    url: "/v1/team/workspace",
    headers: { cookie: sessionCookie(admin) },
    payload: { workspace_owner_id: ownerId },
  });
  assert.equal(switchAdmin.statusCode, 200);

  const memberInvitation = await invite(
    sessionCookie(admin),
    "team-member@example.com",
    "member",
  );
  assert.equal(memberInvitation.statusCode, 201);
  const adminCannotInviteAdmin = await invite(
    sessionCookie(admin),
    "team-outsider@example.com",
    "admin",
  );
  assert.equal(adminCannotInviteAdmin.statusCode, 403);
  await accept(sessionCookie(member), memberInvitation.json().invitation_url);

  const switchWorkspace = await app.inject({
    method: "POST",
    url: "/v1/team/workspace",
    headers: { cookie: sessionCookie(member) },
    payload: { workspace_owner_id: ownerId },
  });
  assert.equal(switchWorkspace.statusCode, 200);
  assert.equal(
    switchWorkspace
      .json()
      .workspaces.find((workspace: { current: boolean }) => workspace.current)
      .workspace_owner_id,
    ownerId,
  );
  const memberMe = await app.inject({
    method: "GET",
    url: "/v1/auth/me",
    headers: { cookie: sessionCookie(member) },
  });
  assert.equal(memberMe.json().user.email, "team-member@example.com");
  assert.equal(memberMe.json().subscription.plan, "free");
  const sharedDevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { cookie: sessionCookie(member) },
  });
  assert.equal(sharedDevices.json().devices.length, 1);

  const memberCannotInvite = await invite(
    sessionCookie(member),
    "team-outsider@example.com",
    "member",
  );
  assert.equal(memberCannotInvite.statusCode, 403);
  const memberCannotChangeAdmin = await app.inject({
    method: "PATCH",
    url: `/v1/team/members/${adminId}`,
    headers: { cookie: sessionCookie(member) },
    payload: { role: "member" },
  });
  assert.equal(memberCannotChangeAdmin.statusCode, 403);
  const memberCannotRemoveAdmin = await app.inject({
    method: "DELETE",
    url: `/v1/team/members/${adminId}`,
    headers: { cookie: sessionCookie(member) },
  });
  assert.equal(memberCannotRemoveAdmin.statusCode, 403);
  const memberRemoved = await app.inject({
    method: "DELETE",
    url: `/v1/team/members/${memberId}`,
    headers: { cookie: sessionCookie(admin) },
  });
  assert.equal(memberRemoved.statusCode, 204);
  const personalDevices = await app.inject({
    method: "GET",
    url: "/v1/devices",
    headers: { cookie: sessionCookie(member) },
  });
  assert.deepEqual(personalDevices.json(), { devices: [] });

  const ownerPromotes = await app.inject({
    method: "PATCH",
    url: `/v1/team/members/${adminId}`,
    headers: { cookie: sessionCookie(owner) },
    payload: { role: "member" },
  });
  assert.equal(ownerPromotes.statusCode, 200);
  const team = await app.inject({
    method: "GET",
    url: "/v1/team",
    headers: { cookie: sessionCookie(owner) },
  });
  assert.equal(team.statusCode, 200);
  assert.equal(team.json().member_limit, 10);
  assert.deepEqual(
    team.json().members.map((entry: { email: string; role: string }) => ({
      email: entry.email,
      role: entry.role,
    })),
    [
      { email: "team-owner@example.com", role: "owner" },
      { email: "team-admin@example.com", role: "member" },
    ],
  );
  await app.close();
});

test("audit events are scoped, paginated, and exclude sensitive request data", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    now: () => TEST_NOW,
    singleAdmin: false,
  });
  const password = "correct horse battery staple";
  const register = async (email: string) =>
    app.inject({
      method: "POST",
      url: "/v1/auth/register",
      payload: { email, password },
    });
  const owner = await register("audit-owner@example.com");
  const member = await register("audit-member@example.com");
  const outsider = await register("audit-outsider@example.com");
  const ownerId = owner.json().user.user_id as string;
  const memberId = member.json().user.user_id as string;
  accounts.setSubscription(ownerId, { plan: "team", status: "active" });

  const firstPage = await app.inject({
    method: "GET",
    url: "/v1/audit?scope=account&limit=1",
    headers: { cookie: sessionCookie(owner) },
  });
  assert.equal(firstPage.statusCode, 200);
  assert.equal(firstPage.json().events.length, 1);
  assert.match(firstPage.json().next_before, /^[0-9a-f-]{36}$/);
  const secondPage = await app.inject({
    method: "GET",
    url: `/v1/audit?scope=account&limit=1&before=${firstPage.json().next_before}`,
    headers: { cookie: sessionCookie(owner) },
  });
  assert.equal(secondPage.statusCode, 200);
  assert.equal(secondPage.json().events.length, 1);
  assert.notEqual(
    firstPage.json().events[0].event_id,
    secondPage.json().events[0].event_id,
  );
  const invalidCursor = await app.inject({
    method: "GET",
    url: "/v1/audit?before=not-a-uuid",
    headers: { cookie: sessionCookie(owner) },
  });
  assert.equal(invalidCursor.statusCode, 400);
  assert.equal(invalidCursor.json().error, "invalid_audit_query");

  const invitation = await app.inject({
    method: "POST",
    url: "/v1/team/invitations",
    headers: { cookie: sessionCookie(owner) },
    payload: { email: "audit-member@example.com", role: "member" },
  });
  assert.equal(invitation.statusCode, 201);
  const token = new URLSearchParams(
    String(invitation.json().invitation_url).slice(1),
  ).get("token");
  assert.ok(token);
  const accepted = await app.inject({
    method: "POST",
    url: "/v1/team/invitations/accept",
    headers: { cookie: sessionCookie(member) },
    payload: { token },
  });
  assert.equal(accepted.statusCode, 200);
  await app.inject({
    method: "POST",
    url: "/v1/team/workspace",
    headers: { cookie: sessionCookie(member) },
    payload: { workspace_owner_id: ownerId },
  });

  const ownerWorkspaceAudit = await app.inject({
    method: "GET",
    url: "/v1/audit?scope=workspace",
    headers: { cookie: sessionCookie(owner) },
  });
  const memberWorkspaceAudit = await app.inject({
    method: "GET",
    url: "/v1/audit?scope=workspace",
    headers: { cookie: sessionCookie(member) },
  });
  assert.equal(ownerWorkspaceAudit.statusCode, 200);
  assert.deepEqual(
    memberWorkspaceAudit.json().events,
    ownerWorkspaceAudit.json().events,
  );
  assert.deepEqual(
    new Set(
      ownerWorkspaceAudit
        .json()
        .events.map((event: { action: string }) => event.action),
    ),
    new Set(["team.invitation_created", "team.invitation_accepted"]),
  );
  const serialized = JSON.stringify({
    account: firstPage.json().events,
    workspace: ownerWorkspaceAudit.json().events,
  });
  for (const forbidden of [
    "audit-owner@example.com",
    "audit-member@example.com",
    password,
    token,
    "token_digest",
    "request",
    "127.0.0.1",
  ]) {
    assert.equal(serialized.includes(forbidden), false);
  }

  const outsiderWorkspaceAudit = await app.inject({
    method: "GET",
    url: "/v1/audit?scope=workspace",
    headers: { cookie: sessionCookie(outsider) },
  });
  assert.deepEqual(outsiderWorkspaceAudit.json().events, []);
  const removed = await app.inject({
    method: "DELETE",
    url: `/v1/team/members/${memberId}`,
    headers: { cookie: sessionCookie(owner) },
  });
  assert.equal(removed.statusCode, 204);
  const formerMemberAudit = await app.inject({
    method: "GET",
    url: "/v1/audit?scope=workspace",
    headers: { cookie: sessionCookie(member) },
  });
  assert.deepEqual(formerMemberAudit.json().events, []);
  await app.close();
});

test("signed billing events are idempotent, ordered, and preserve paid-through access", async () => {
  const secret = Buffer.alloc(32, 7);
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    now: () => TEST_NOW,
    billingVerifier: new BillingEventVerifier(secret),
  });
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "billing@example.com",
      password: "correct horse battery staple",
    },
  });
  const userId = registered.json().user.user_id as string;
  const event: SubscriptionEvent = {
    event_id: "11111111-1111-4111-8111-111111111111",
    user_id: userId,
    provider: "billing-adapter",
    provider_customer_id: "customer_1",
    provider_subscription_id: "subscription_1",
    plan: "pro",
    status: "canceled",
    current_period_end: testTime(31 * 24 * 60 * 60 * 1_000),
    occurred_at: testTime(-60 * 1_000),
  };
  const send = (body: SubscriptionEvent, signatureBody = body) => {
    const timestamp = String(Math.floor(TEST_NOW.getTime() / 1_000));
    const signature = createHmac("sha256", secret)
      .update(`${timestamp}.${canonicalSubscriptionEvent(signatureBody)}`)
      .digest("hex");
    return app.inject({
      method: "POST",
      url: "/v1/billing/events",
      headers: {
        "x-agent-pulse-timestamp": timestamp,
        "x-agent-pulse-signature": `sha256=${signature}`,
      },
      payload: body,
    });
  };

  const applied = await send(event);
  assert.equal(applied.statusCode, 200);
  assert.equal(applied.json().applied, true);
  assert.equal(applied.json().entitlements.max_devices, 5);
  assert.equal(applied.json().subscription.provider_customer_id, undefined);
  const duplicate = await send(event);
  assert.deepEqual(
    {
      applied: duplicate.json().applied,
      duplicate: duplicate.json().duplicate,
    },
    { applied: true, duplicate: true },
  );

  const older: SubscriptionEvent = {
    ...event,
    event_id: "22222222-2222-4222-8222-222222222222",
    plan: "free",
    status: "expired",
    current_period_end: testTime(-12 * 24 * 60 * 60 * 1_000),
    occurred_at: testTime(-2 * 60 * 1_000),
  };
  const ignored = await send(older);
  assert.equal(ignored.json().applied, false);
  assert.equal(ignored.json().subscription.plan, "pro");

  const tampered = await send({ ...event, plan: "team" }, event);
  assert.equal(tampered.statusCode, 401);
  assert.equal(tampered.json().error, "invalid_billing_signature");
  await app.close();
});
