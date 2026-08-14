import { createHmac, timingSafeEqual } from "node:crypto";

import type { FastifyRequest } from "fastify";

import {
  AccountError,
  subscriptionPlans,
  subscriptionStatuses,
  type AccountView,
  type SubscriptionPlan,
  type SubscriptionEvent,
} from "./accounts.js";

const EVENT_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const USER_ID_PATTERN = EVENT_ID_PATTERN;
const IDENTIFIER_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._:@/-]*$/;
const SIGNATURE_PATTERN = /^sha256=([0-9a-f]{64})$/;
const DEFAULT_MAX_CLOCK_SKEW_SECONDS = 300;

export type PaidSubscriptionPlan = Exclude<SubscriptionPlan, "free">;

export interface BillingProvider {
  readonly name: string;
  readonly webhookPath: string;
  readonly webhookSignatureHeader: string;
  createCheckout(input: {
    userId: string;
    email: string;
    plan: PaidSubscriptionPlan;
    customerId?: string;
  }): Promise<string>;
  createPortal(customerId: string): Promise<string>;
  parseWebhook(
    rawBody: Buffer,
    signature: string | undefined,
    now: Date,
  ): SubscriptionEvent | undefined;
}

export interface BillingStatus {
  available: boolean;
  provider?: string;
  checkout: boolean;
  portal: boolean;
  subscription: AccountView["subscription"];
}

export class BillingEventVerifier {
  readonly #secret: Buffer;
  readonly #maxClockSkewSeconds: number;

  constructor(
    secret: Buffer,
    maxClockSkewSeconds = DEFAULT_MAX_CLOCK_SKEW_SECONDS,
  ) {
    if (secret.length < 32) {
      throw new Error("billing webhook secret must contain at least 32 bytes");
    }
    if (
      !Number.isSafeInteger(maxClockSkewSeconds) ||
      maxClockSkewSeconds < 30
    ) {
      throw new Error("billing webhook clock skew must be at least 30 seconds");
    }
    this.#secret = Buffer.from(secret);
    this.#maxClockSkewSeconds = maxClockSkewSeconds;
  }

  verify(request: FastifyRequest, now: Date): SubscriptionEvent {
    const event = parseSubscriptionEvent(request.body);
    const timestamp = singleHeader(request.headers["x-agent-pulse-timestamp"]);
    const signature = singleHeader(request.headers["x-agent-pulse-signature"]);
    const timestampSeconds = Number(timestamp);
    if (
      timestamp === undefined ||
      !/^\d{10}$/.test(timestamp) ||
      !Number.isSafeInteger(timestampSeconds) ||
      Math.abs(Math.floor(now.getTime() / 1_000) - timestampSeconds) >
        this.#maxClockSkewSeconds
    ) {
      throw new AccountError(
        "invalid_billing_signature",
        "billing timestamp is invalid",
        401,
      );
    }
    const match = signature?.match(SIGNATURE_PATTERN);
    if (match?.[1] === undefined) {
      throw new AccountError(
        "invalid_billing_signature",
        "billing signature is invalid",
        401,
      );
    }
    const expected = createHmac("sha256", this.#secret)
      .update(`${timestamp}.${canonicalSubscriptionEvent(event)}`, "utf8")
      .digest();
    const actual = Buffer.from(match[1], "hex");
    if (
      actual.length !== expected.length ||
      !timingSafeEqual(actual, expected)
    ) {
      throw new AccountError(
        "invalid_billing_signature",
        "billing signature is invalid",
        401,
      );
    }
    return event;
  }
}

export function canonicalSubscriptionEvent(event: SubscriptionEvent): string {
  return JSON.stringify({
    event_id: event.event_id,
    user_id: event.user_id,
    provider: event.provider,
    provider_customer_id: event.provider_customer_id,
    provider_subscription_id: event.provider_subscription_id,
    plan: event.plan,
    status: event.status,
    ...(event.current_period_end === undefined
      ? {}
      : { current_period_end: event.current_period_end }),
    occurred_at: event.occurred_at,
  });
}

function parseSubscriptionEvent(value: unknown): SubscriptionEvent {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new AccountError(
      "invalid_billing_event",
      "event must be an object",
      400,
    );
  }
  const body = value as Record<string, unknown>;
  const allowed = [
    "event_id",
    "user_id",
    "provider",
    "provider_customer_id",
    "provider_subscription_id",
    "plan",
    "status",
    "current_period_end",
    "occurred_at",
  ];
  if (Object.keys(body).some((key) => !allowed.includes(key))) {
    throw new AccountError(
      "invalid_billing_event",
      "event field is invalid",
      400,
    );
  }
  const event: SubscriptionEvent = {
    event_id: identifier(body.event_id, "event_id", EVENT_ID_PATTERN, 36),
    user_id: identifier(body.user_id, "user_id", USER_ID_PATTERN, 36),
    provider: identifier(body.provider, "provider", IDENTIFIER_PATTERN, 40),
    provider_customer_id: identifier(
      body.provider_customer_id,
      "provider_customer_id",
      IDENTIFIER_PATTERN,
      255,
    ),
    provider_subscription_id: identifier(
      body.provider_subscription_id,
      "provider_subscription_id",
      IDENTIFIER_PATTERN,
      255,
    ),
    plan: enumValue(body.plan, subscriptionPlans, "plan"),
    status: enumValue(body.status, subscriptionStatuses, "status"),
    occurred_at: timestamp(body.occurred_at, "occurred_at"),
  };
  if (body.current_period_end !== undefined) {
    event.current_period_end = timestamp(
      body.current_period_end,
      "current_period_end",
    );
  }
  return event;
}

function identifier(
  value: unknown,
  name: string,
  pattern: RegExp,
  maximumLength: number,
): string {
  if (
    typeof value !== "string" ||
    value.length < 1 ||
    value.length > maximumLength ||
    !pattern.test(value)
  ) {
    throw new AccountError("invalid_billing_event", `${name} is invalid`, 400);
  }
  return value;
}

function enumValue<T extends string>(
  value: unknown,
  values: readonly T[],
  name: string,
): T {
  if (typeof value !== "string" || !values.includes(value as T)) {
    throw new AccountError("invalid_billing_event", `${name} is invalid`, 400);
  }
  return value as T;
}

function timestamp(value: unknown, name: string): string {
  if (typeof value !== "string") {
    throw new AccountError("invalid_billing_event", `${name} is invalid`, 400);
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime()) || date.toISOString() !== value) {
    throw new AccountError("invalid_billing_event", `${name} is invalid`, 400);
  }
  return value;
}

function singleHeader(
  value: string | string[] | undefined,
): string | undefined {
  return typeof value === "string" ? value : undefined;
}
