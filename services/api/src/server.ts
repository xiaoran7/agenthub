import { isIP } from "node:net";

import { createGatewayApp, type GatewayAppOptions } from "./app.js";
import {
  InMemoryAccountStore,
  SessionAuthenticator,
  type AccountStore,
} from "./accounts.js";
import {
  BearerTokenAuthenticator,
  DevelopmentIdentityAuthenticator,
} from "./auth.js";
import { BillingEventVerifier } from "./billing.js";
import { HttpEmailVerificationSender, ResendEmailSender } from "./email.js";
import { ChangeBroker, PostgresChangeBroker } from "./events.js";
import { PostgresGatewayStore } from "./postgres-store.js";
import { WebPushSender, vapidKeysFromEnv } from "./push.js";
import { StripeBillingProvider } from "./stripe-billing.js";
import { InMemoryGatewayStore, type GatewayStore } from "./store.js";
import { SqliteGatewayStore } from "./sqlite-store.js";
import { RemoteContentStore } from "./remote-store.js";

const port = parsePort(process.env.PORT);
const host = process.env.HOST ?? "127.0.0.1";
const databaseUrl = process.env.AGENT_PLATFORM_DATABASE_URL;
const sqliteDbPath =
  process.env.AGENTHUB_REMOTE_DATABASE ?? "./data/agenthub.db";

let store: GatewayStore;
let accounts: AccountStore;
if (databaseUrl === undefined) {
  store = new SqliteGatewayStore(sqliteDbPath);
  accounts = new InMemoryAccountStore();
} else {
  const postgresStore = await PostgresGatewayStore.connect(databaseUrl);
  store = postgresStore;
  accounts = postgresStore;
}
const authenticator = ownerAuthenticator(accounts, host);
let broker: ChangeBroker = new ChangeBroker();
if (databaseUrl !== undefined) {
  try {
    broker = await PostgresChangeBroker.connect(databaseUrl);
  } catch {
    process.stderr.write(
      '{"level":"warn","code":"postgres_change_broker_unavailable"}\n',
    );
  }
}
// Without a full VAPID triple the gateway starts with push disabled rather
// than half-configured; `npm run vapid-keys` prints a usable pair.
const vapidKeys = vapidKeysFromEnv();
const options: GatewayAppOptions = {
  store,
  accounts,
  broker,
  authenticator,
  secureCookies: process.env.NODE_ENV === "production",
  trustProxy: isLoopbackHost(host),
  logger: true,
};
const loginRateLimitSecret = secretFromEnvironment(
  "AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET",
  process.env.NODE_ENV === "production",
);
if (loginRateLimitSecret !== undefined) {
  options.loginRateLimitSecret = loginRateLimitSecret;
}
const billingSecret = secretFromEnvironment(
  "AGENT_PULSE_BILLING_WEBHOOK_SECRET",
  false,
);
if (billingSecret !== undefined) {
  options.billingVerifier = new BillingEventVerifier(billingSecret);
}
const emailEndpoint = process.env.AGENT_PULSE_EMAIL_ENDPOINT;
const emailToken = process.env.AGENT_PULSE_EMAIL_TOKEN;
const publicBaseUrl = process.env.AGENT_PULSE_PUBLIC_BASE_URL;
const emailValues = [emailEndpoint, emailToken];
if (emailValues.some((value) => value !== undefined)) {
  if (
    emailEndpoint === undefined ||
    emailToken === undefined ||
    publicBaseUrl === undefined
  ) {
    throw new Error(
      "AGENT_PULSE_EMAIL_ENDPOINT, AGENT_PULSE_EMAIL_TOKEN and AGENT_PULSE_PUBLIC_BASE_URL must be configured together",
    );
  }
  options.emailVerification = {
    baseUrl: publicBaseUrl,
    sender: new HttpEmailVerificationSender(emailEndpoint, emailToken),
  };
  delete process.env.AGENT_PULSE_EMAIL_TOKEN;
}
const resendApiKey = process.env.AGENT_PULSE_RESEND_API_KEY;
const resendFrom = process.env.AGENT_PULSE_RESEND_FROM;
if (resendApiKey !== undefined || resendFrom !== undefined) {
  if (
    resendApiKey === undefined ||
    resendFrom === undefined ||
    publicBaseUrl === undefined ||
    emailEndpoint !== undefined ||
    emailToken !== undefined
  ) {
    throw new Error(
      "AGENT_PULSE_RESEND_API_KEY, AGENT_PULSE_RESEND_FROM and AGENT_PULSE_PUBLIC_BASE_URL must be configured together and cannot be combined with the HTTP email adapter",
    );
  }
  options.emailVerification = {
    baseUrl: publicBaseUrl,
    sender: new ResendEmailSender(resendApiKey, resendFrom),
  };
  delete process.env.AGENT_PULSE_RESEND_API_KEY;
}
const stripeSecretKey = process.env.AGENT_PULSE_STRIPE_SECRET_KEY;
const stripeWebhookSecret = process.env.AGENT_PULSE_STRIPE_WEBHOOK_SECRET;
const stripeProPrice = process.env.AGENT_PULSE_STRIPE_PRICE_PRO;
const stripeTeamPrice = process.env.AGENT_PULSE_STRIPE_PRICE_TEAM;
const stripeValues = [
  stripeSecretKey,
  stripeWebhookSecret,
  stripeProPrice,
  stripeTeamPrice,
];
if (stripeValues.some((value) => value !== undefined)) {
  if (
    stripeSecretKey === undefined ||
    stripeWebhookSecret === undefined ||
    stripeProPrice === undefined ||
    stripeTeamPrice === undefined ||
    publicBaseUrl === undefined
  ) {
    throw new Error(
      "AGENT_PULSE_STRIPE_SECRET_KEY, AGENT_PULSE_STRIPE_WEBHOOK_SECRET, AGENT_PULSE_STRIPE_PRICE_PRO, AGENT_PULSE_STRIPE_PRICE_TEAM and AGENT_PULSE_PUBLIC_BASE_URL must be configured together",
    );
  }
  options.billingProvider = new StripeBillingProvider({
    secretKey: stripeSecretKey,
    webhookSecret: stripeWebhookSecret,
    priceIds: { pro: stripeProPrice, team: stripeTeamPrice },
    baseUrl: publicBaseUrl,
  });
  delete process.env.AGENT_PULSE_STRIPE_SECRET_KEY;
  delete process.env.AGENT_PULSE_STRIPE_WEBHOOK_SECRET;
}
if (vapidKeys !== undefined) {
  options.push = {
    publicKey: vapidKeys.publicKey,
    sender: new WebPushSender(vapidKeys),
  };
}
const remoteDbPath = process.env.AGENTHUB_REMOTE_DATABASE ?? sqliteDbPath;
options.remoteContent = new RemoteContentStore(remoteDbPath);
const app = createGatewayApp(options);

await app.listen({ port, host });

function parsePort(value: string | undefined): number {
  if (value === undefined) {
    return 3001;
  }
  const port = Number(value);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65_535) {
    throw new Error("PORT must be an integer between 1 and 65535");
  }
  return port;
}

function isLoopbackHost(value: string): boolean {
  return (
    value === "localhost" ||
    value === "::1" ||
    (isIP(value) === 4 && value.split(".", 1)[0] === "127")
  );
}

function ownerAuthenticator(
  accounts: AccountStore,
  host: string,
):
  | BearerTokenAuthenticator
  | DevelopmentIdentityAuthenticator
  | SessionAuthenticator {
  const ownerId = process.env.AGENT_PLATFORM_OWNER_ID ?? "default-owner";
  const accessToken =
    process.env.AGENT_PLATFORM_ACCESS_TOKEN ??
    process.env.GATEWAY_SECRET_TOKEN ??
    process.env.GATEWAY_ACCESS_TOKEN;
  const allowDevelopmentAuth =
    process.env.AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH === "true";
  if (allowDevelopmentAuth && !isLoopbackHost(host)) {
    throw new Error(
      "AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH is only allowed on a loopback HOST",
    );
  }
  if (accessToken === undefined) {
    return allowDevelopmentAuth
      ? new DevelopmentIdentityAuthenticator()
      : new SessionAuthenticator(accounts);
  }
  const authenticator = new BearerTokenAuthenticator(ownerId, accessToken);
  // Child processes must not inherit a credential the API no longer needs in
  // plaintext; the authenticator retains only the digest.
  delete process.env.AGENT_PLATFORM_ACCESS_TOKEN;
  delete process.env.GATEWAY_SECRET_TOKEN;
  delete process.env.GATEWAY_ACCESS_TOKEN;
  return authenticator;
}

function secretFromEnvironment(
  name: string,
  required: boolean,
): Buffer | undefined {
  const value = process.env[name];
  if (value === undefined) {
    if (required) throw new Error(`${name} is required in production`);
    return undefined;
  }
  if (!/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new Error(`${name} must be base64url encoded`);
  }
  let decoded: Buffer;
  try {
    decoded = Buffer.from(value, "base64url");
  } catch {
    throw new Error(`${name} must be base64url encoded`);
  }
  if (decoded.length < 32) {
    throw new Error(`${name} must decode to at least 32 bytes`);
  }
  delete process.env[name];
  return decoded;
}
