import assert from "node:assert/strict";
import test from "node:test";

import { ResendEmailSender } from "../src/email.js";

function messageSender() {
  const requests: Array<{ url: string; init: RequestInit | undefined }> = [];
  const sender = new ResendEmailSender(
    "re_test_agent_pulse_key",
    "AgentPulse <noreply@example.com>",
    async (url, init) => {
      requests.push({ url: String(url), init });
      return new Response(JSON.stringify({ id: "email_test" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    },
  );
  return { requests, sender };
}

test("Resend sender renders all account messages and escapes HTML", async () => {
  const { requests, sender } = messageSender();
  await sender.sendVerification({
    user_id: "user-1",
    email: "verify@example.com",
    display_name: "<Operator>",
    verification_url: "https://platform.example.com/verify#token=x",
    expires_at: "2026-08-15T00:00:00.000Z",
  });
  await sender.sendPasswordReset({
    user_id: "user-1",
    email: "reset@example.com",
    display_name: "Reset User",
    reset_url: "https://platform.example.com/reset#token=x",
    expires_at: "2026-08-15T00:00:00.000Z",
  });
  await sender.sendTeamInvitation({
    email: "team@example.com",
    display_name: "Team User",
    inviter_name: "Owner",
    invitation_url: "https://platform.example.com/#team_invitation=x",
    role: "member",
    expires_at: "2026-08-15T00:00:00.000Z",
  });
  assert.equal(requests.length, 3);
  const first = JSON.parse(String(requests[0]!.init?.body)) as {
    from: string;
    to: string;
    html: string;
    text: string;
  };
  assert.equal(first.from, "AgentPulse <noreply@example.com>");
  assert.equal(first.to, "verify@example.com");
  assert.ok(first.html.includes("&lt;Operator&gt;"));
  assert.ok(first.text.includes("https://platform.example.com/verify"));
  assert.equal(first.html.includes("<Operator>"), false);
  assert.equal(
    requests[0]!.init?.headers &&
      new Headers(requests[0]!.init?.headers).get("authorization"),
    "Bearer re_test_agent_pulse_key",
  );
});

test("Resend sender classifies provider rejection without exposing credentials", async () => {
  const sender = new ResendEmailSender(
    "re_test_agent_pulse_key",
    "noreply@example.com",
    async () => new Response("rejected", { status: 500 }),
  );
  await assert.rejects(
    sender.sendVerification({
      user_id: "user-1",
      email: "verify@example.com",
      display_name: "User",
      verification_url: "https://platform.example.com/verify#token=x",
      expires_at: "2026-08-15T00:00:00.000Z",
    }),
    (error: unknown) =>
      error instanceof Error &&
      error.message === "email delivery service rejected the message" &&
      !error.message.includes("re_test_agent_pulse_key"),
  );
});
