import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";

import Stripe from "stripe";

import { createGatewayApp } from "../src/app.js";
import {
  AccountError,
  InMemoryAccountStore,
  SessionAuthenticator,
  type SubscriptionEvent,
} from "../src/accounts.js";
import type { BillingProvider, PaidSubscriptionPlan } from "../src/billing.js";
import {
  StripeBillingProvider,
  type StripeBillingOptions,
} from "../src/stripe-billing.js";
import { InMemoryGatewayStore } from "../src/store.js";

const priceIds = { pro: "price_pro", team: "price_team" } as const;
const baseUrl = "https://platform.example.com";

function stripeProvider(): StripeBillingProvider {
  return new StripeBillingProvider({
    secretKey: "sk_test_agent_pulse",
    webhookSecret: "whsec_agent_pulse",
    priceIds,
    baseUrl,
  });
}

test("Stripe Checkout and portal keep provider URLs and metadata bounded", async () => {
  let checkoutParams: Record<string, unknown> | undefined;
  let portalParams: Record<string, unknown> | undefined;
  const client = {
    checkout: {
      sessions: {
        create: async (params: Record<string, unknown>) => {
          checkoutParams = params;
          return { url: "https://checkout.stripe.com/cs_test" };
        },
      },
    },
    billingPortal: {
      sessions: {
        create: async (params: Record<string, unknown>) => {
          portalParams = params;
          return { url: "https://billing.stripe.com/p/session_test" };
        },
      },
    },
    webhooks: { constructEvent: () => assert.fail("not used") },
  } as unknown as NonNullable<StripeBillingOptions["client"]>;
  const provider = new StripeBillingProvider({
    secretKey: "sk_test_agent_pulse",
    webhookSecret: "whsec_agent_pulse",
    priceIds,
    baseUrl,
    client,
  });
  assert.equal(
    await provider.createCheckout({
      userId: "11111111-1111-4111-8111-111111111111",
      email: "buyer@example.com",
      plan: "pro",
    }),
    "https://checkout.stripe.com/cs_test",
  );
  assert.deepEqual(checkoutParams?.line_items, [
    { price: "price_pro", quantity: 1 },
  ]);
  assert.deepEqual(checkoutParams?.subscription_data, {
    metadata: {
      agent_pulse_user_id: "11111111-1111-4111-8111-111111111111",
      agent_pulse_plan: "pro",
    },
  });
  assert.equal(
    await provider.createPortal("cus_test"),
    "https://billing.stripe.com/p/session_test",
  );
  assert.deepEqual(portalParams, {
    customer: "cus_test",
    return_url: `${baseUrl}/`,
  });
});

test("Stripe subscription webhooks verify signatures and map the configured price", () => {
  const provider = stripeProvider();
  const timestamp = 1_786_622_400;
  const payload = JSON.stringify({
    id: "evt_agent_pulse_1",
    object: "event",
    api_version: "2025-06-30.basil",
    created: timestamp,
    livemode: false,
    pending_webhooks: 1,
    request: { id: null, idempotency_key: null },
    type: "customer.subscription.updated",
    data: {
      object: {
        object: "subscription",
        id: "sub_test",
        customer: "cus_test",
        metadata: {
          agent_pulse_user_id: "11111111-1111-4111-8111-111111111111",
        },
        status: "active",
        items: {
          object: "list",
          data: [
            {
              object: "subscription_item",
              id: "si_test",
              price: { id: "price_team" },
              current_period_end: timestamp + 86_400,
            },
          ],
        },
      },
    },
  });
  const signature = Stripe.webhooks.generateTestHeaderString({
    payload,
    secret: "whsec_agent_pulse",
    timestamp,
  });
  const event = provider.parseWebhook(
    Buffer.from(payload),
    signature,
    new Date(timestamp * 1_000),
  );
  assert.equal(event?.provider, "stripe");
  assert.equal(event?.plan, "team");
  assert.equal(event?.status, "active");
  assert.equal(event?.provider_subscription_id, "sub_test");
  assert.equal(event?.current_period_end, "2026-08-14T12:00:00.000Z");
  assert.match(event?.event_id ?? "", /^[0-9a-f-]{36}$/);
  assert.throws(
    () =>
      provider.parseWebhook(
        Buffer.from(payload),
        `${signature}x`,
        new Date(timestamp * 1_000),
      ),
    (error: unknown) =>
      error instanceof AccountError &&
      error.code === "invalid_billing_signature",
  );
});

class TestBillingProvider implements BillingProvider {
  readonly name = "test";
  readonly webhookPath = "/v1/billing/test/events";
  readonly webhookSignatureHeader = "x-test-signature";
  readonly checkoutCalls: Array<{
    userId: string;
    email: string;
    plan: PaidSubscriptionPlan;
    customerId?: string;
  }> = [];
  rawBody = "";
  webhookEvent: SubscriptionEvent | undefined;

  async createCheckout(input: {
    userId: string;
    email: string;
    plan: PaidSubscriptionPlan;
    customerId?: string;
  }): Promise<string> {
    this.checkoutCalls.push(input);
    return "https://checkout.stripe.com/test-adapter";
  }

  async createPortal(_customerId: string): Promise<string> {
    return "https://billing.stripe.com/test-adapter";
  }

  parseWebhook(
    rawBody: Buffer,
    signature: string | undefined,
  ): SubscriptionEvent {
    assert.equal(signature, "test-signature");
    this.rawBody = rawBody.toString("utf8");
    assert.ok(this.webhookEvent);
    return this.webhookEvent;
  }
}

test("billing HTTP routes are session scoped and expose only redirect URLs", async () => {
  const accounts = new InMemoryAccountStore();
  const provider = new TestBillingProvider();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
    billingProvider: provider,
    now: () => new Date("2026-08-13T12:00:00.000Z"),
  });
  const registered = await app.inject({
    method: "POST",
    url: "/v1/auth/register",
    payload: {
      email: "billing-routes@example.com",
      password: "correct horse battery staple",
    },
  });
  const cookie = (registered.headers["set-cookie"] as string).split(";", 1)[0];
  const unavailable = await app.inject({ method: "GET", url: "/v1/billing" });
  assert.equal(unavailable.statusCode, 401);
  const status = await app.inject({
    method: "GET",
    url: "/v1/billing",
    headers: { cookie },
  });
  assert.deepEqual(status.json(), {
    available: true,
    provider: "test",
    checkout: true,
    portal: false,
    subscription: { plan: "free", status: "active" },
  });
  const invalid = await app.inject({
    method: "POST",
    url: "/v1/billing/checkout",
    headers: { cookie },
    payload: { plan: "free" },
  });
  assert.equal(invalid.statusCode, 400);
  const checkout = await app.inject({
    method: "POST",
    url: "/v1/billing/checkout",
    headers: { cookie },
    payload: { plan: "pro" },
  });
  assert.equal(checkout.statusCode, 200);
  assert.deepEqual(checkout.json(), {
    provider: "test",
    url: "https://checkout.stripe.com/test-adapter",
  });
  assert.equal(provider.checkoutCalls[0]?.email, "billing-routes@example.com");
  const userId = registered.json().user.user_id as string;
  await accounts.applySubscriptionEvent({
    event_id: randomUUID(),
    user_id: userId,
    provider: "test",
    provider_customer_id: "cus_test",
    provider_subscription_id: "sub_test",
    plan: "pro",
    status: "active",
    occurred_at: "2026-08-13T12:00:00.000Z",
  });
  const portal = await app.inject({
    method: "POST",
    url: "/v1/billing/portal",
    headers: { cookie },
    payload: {},
  });
  assert.equal(portal.statusCode, 200);
  provider.webhookEvent = {
    event_id: randomUUID(),
    user_id: userId,
    provider: "test",
    provider_customer_id: "cus_test",
    provider_subscription_id: "sub_test",
    plan: "pro",
    status: "active",
    occurred_at: "2026-08-13T12:00:00.000Z",
  };
  const webhook = await app.inject({
    method: "POST",
    url: "/v1/billing/test/events",
    headers: { "x-test-signature": "test-signature" },
    payload: { event: "subscription.updated" },
  });
  assert.equal(webhook.statusCode, 200);
  assert.equal(webhook.json().duplicate, false);
  assert.ok(provider.rawBody.includes("subscription.updated"));
  await app.close();
});

test("unconfigured billing never bypasses authentication", async () => {
  const accounts = new InMemoryAccountStore();
  const app = createGatewayApp({
    accounts,
    store: new InMemoryGatewayStore(),
    authenticator: new SessionAuthenticator(accounts),
  });
  const unauthenticated = await app.inject({
    method: "POST",
    url: "/v1/billing/checkout",
    payload: { plan: "pro" },
  });
  assert.equal(unauthenticated.statusCode, 401);
  await app.close();
});
