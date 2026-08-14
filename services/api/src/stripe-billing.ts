import { createHash } from "node:crypto";

import Stripe from "stripe";

import {
  AccountError,
  type SubscriptionEvent,
  type SubscriptionStatus,
} from "./accounts.js";
import type { BillingProvider, PaidSubscriptionPlan } from "./billing.js";

const USER_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const PRICE_ID_PATTERN = /^price_[A-Za-z0-9]+$/;
const STRIPE_WEBHOOK_TOLERANCE_SECONDS = 300;
const SUBSCRIPTION_EVENTS = new Set([
  "customer.subscription.created",
  "customer.subscription.updated",
  "customer.subscription.deleted",
  "customer.subscription.paused",
  "customer.subscription.resumed",
]);

type StripeClient = Pick<Stripe, "billingPortal" | "checkout" | "webhooks">;

export interface StripeBillingOptions {
  secretKey: string;
  webhookSecret: string;
  priceIds: Record<PaidSubscriptionPlan, string>;
  baseUrl: string;
  client?: StripeClient;
}

export class StripeBillingProvider implements BillingProvider {
  readonly name = "stripe";
  readonly webhookPath = "/v1/billing/stripe/events";
  readonly webhookSignatureHeader = "stripe-signature";
  readonly #stripe: StripeClient;
  readonly #webhookSecret: string;
  readonly #priceIds: Record<PaidSubscriptionPlan, string>;
  readonly #baseUrl: URL;

  constructor(options: StripeBillingOptions) {
    if (!options.secretKey.startsWith("sk_")) {
      throw new Error("Stripe secret key must start with sk_");
    }
    if (!options.webhookSecret.startsWith("whsec_")) {
      throw new Error("Stripe webhook secret must start with whsec_");
    }
    if (
      !PRICE_ID_PATTERN.test(options.priceIds.pro) ||
      !PRICE_ID_PATTERN.test(options.priceIds.team) ||
      options.priceIds.pro === options.priceIds.team
    ) {
      throw new Error(
        "Stripe Pro and Team price IDs must be distinct price_ IDs",
      );
    }
    const baseUrl = new URL(options.baseUrl);
    if (baseUrl.protocol !== "https:") {
      throw new Error("Stripe billing base URL must use HTTPS");
    }
    this.#stripe =
      options.client ??
      new Stripe(options.secretKey, {
        maxNetworkRetries: 2,
        timeout: 10_000,
      });
    this.#webhookSecret = options.webhookSecret;
    this.#priceIds = { ...options.priceIds };
    this.#baseUrl = baseUrl;
  }

  async createCheckout(input: {
    userId: string;
    email: string;
    plan: PaidSubscriptionPlan;
    customerId?: string;
  }): Promise<string> {
    try {
      const metadata = {
        agent_pulse_user_id: input.userId,
        agent_pulse_plan: input.plan,
      };
      const session = await this.#stripe.checkout.sessions.create({
        mode: "subscription",
        client_reference_id: input.userId,
        ...(input.customerId === undefined
          ? { customer_email: input.email }
          : { customer: input.customerId }),
        line_items: [{ price: this.#priceIds[input.plan], quantity: 1 }],
        metadata,
        subscription_data: { metadata },
        allow_promotion_codes: true,
        success_url: new URL("/?billing=success", this.#baseUrl).toString(),
        cancel_url: new URL("/?billing=canceled", this.#baseUrl).toString(),
      });
      return stripeRedirectUrl(session.url, "checkout");
    } catch (error) {
      if (error instanceof AccountError) throw error;
      throw new AccountError(
        "billing_provider_failed",
        "Stripe checkout creation failed",
        502,
      );
    }
  }

  async createPortal(customerId: string): Promise<string> {
    try {
      const session = await this.#stripe.billingPortal.sessions.create({
        customer: customerId,
        return_url: this.#baseUrl.toString(),
      });
      return stripeRedirectUrl(session.url, "billing");
    } catch (error) {
      if (error instanceof AccountError) throw error;
      throw new AccountError(
        "billing_provider_failed",
        "Stripe portal creation failed",
        502,
      );
    }
  }

  parseWebhook(
    rawBody: Buffer,
    signature: string | undefined,
    now: Date,
  ): SubscriptionEvent | undefined {
    if (signature === undefined) {
      throw new AccountError(
        "invalid_billing_signature",
        "Stripe signature is missing",
        401,
      );
    }
    let event: Stripe.Event;
    try {
      event = this.#stripe.webhooks.constructEvent(
        rawBody,
        signature,
        this.#webhookSecret,
        STRIPE_WEBHOOK_TOLERANCE_SECONDS,
        undefined,
        now.getTime(),
      );
    } catch {
      throw new AccountError(
        "invalid_billing_signature",
        "Stripe signature is invalid",
        401,
      );
    }
    if (!SUBSCRIPTION_EVENTS.has(event.type)) return undefined;
    const subscription = event.data.object as Stripe.Subscription;
    if (subscription.object !== "subscription") {
      throw invalidStripeEvent("event object is not a subscription");
    }
    const userId = subscription.metadata.agent_pulse_user_id;
    if (userId === undefined || !USER_ID_PATTERN.test(userId)) {
      throw invalidStripeEvent("subscription user metadata is invalid");
    }
    const matchedItems = subscription.items.data.flatMap((item) => {
      const plan = planForPrice(item.price.id, this.#priceIds);
      return plan === undefined ? [] : [{ item, plan }];
    });
    const plans = new Set(matchedItems.map(({ plan }) => plan));
    if (plans.size !== 1 || matchedItems.length !== 1) {
      throw invalidStripeEvent("subscription must contain one known plan");
    }
    const plan = matchedItems[0]!.plan;
    const periodEnd = matchedItems[0]!.item.current_period_end;
    const customerId =
      typeof subscription.customer === "string"
        ? subscription.customer
        : subscription.customer.id;
    return {
      event_id: providerEventUuid(event.id),
      user_id: userId,
      provider: this.name,
      provider_customer_id: customerId,
      provider_subscription_id: subscription.id,
      plan,
      status: subscriptionStatus(subscription.status),
      current_period_end: stripeTimestamp(periodEnd, "current period end"),
      occurred_at: stripeTimestamp(event.created, "event creation time"),
    };
  }
}

function planForPrice(
  priceId: string,
  priceIds: Record<PaidSubscriptionPlan, string>,
): PaidSubscriptionPlan | undefined {
  if (priceId === priceIds.pro) return "pro";
  if (priceId === priceIds.team) return "team";
  return undefined;
}

function subscriptionStatus(
  status: Stripe.Subscription.Status,
): SubscriptionStatus {
  switch (status) {
    case "trialing":
      return "trialing";
    case "active":
      return "active";
    case "canceled":
      return "canceled";
    case "incomplete_expired":
      return "expired";
    case "incomplete":
    case "past_due":
    case "paused":
    case "unpaid":
      return "past_due";
    default:
      throw invalidStripeEvent("subscription status is unsupported");
  }
}

function stripeTimestamp(value: number, name: string): string {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw invalidStripeEvent(`${name} is invalid`);
  }
  return new Date(value * 1_000).toISOString();
}

function providerEventUuid(eventId: string): string {
  const bytes = createHash("sha256")
    .update(`stripe:${eventId}`, "utf8")
    .digest()
    .subarray(0, 16);
  bytes[6] = (bytes[6]! & 0x0f) | 0x50;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function stripeRedirectUrl(value: string | null, expectedHost: string): string {
  if (value === null) {
    throw new AccountError(
      "billing_provider_failed",
      "Stripe did not return a redirect URL",
      502,
    );
  }
  const url = new URL(value);
  if (
    url.protocol !== "https:" ||
    (url.hostname !== `${expectedHost}.stripe.com` &&
      !url.hostname.endsWith(`.${expectedHost}.stripe.com`))
  ) {
    throw new AccountError(
      "billing_provider_failed",
      "Stripe returned an invalid redirect URL",
      502,
    );
  }
  return url.toString();
}

function invalidStripeEvent(message: string): AccountError {
  return new AccountError("invalid_billing_event", message, 400);
}
