import { isIP } from "node:net";

import { createGatewayApp, type GatewayAppOptions } from "./app.js";
import {
  InMemoryAccountStore,
  SessionAuthenticator,
  type AccountStore,
} from "./accounts.js";
import { SqliteAccountStore } from "./sqlite-account-store.js";
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
import {
  allowedPushHostsFromEnv,
  resolveTrustProxyConfig,
} from "./security-guards.js";

const port = parsePort(process.env.PORT);
const host = process.env.HOST ?? "127.0.0.1";
const databaseUrl = process.env.AGENT_PLATFORM_DATABASE_URL;
const remoteDbPath = process.env.AGENTHUB_REMOTE_DATABASE ?? "./data/remote.db";
const gatewayDbPath =
  process.env.AGENTHUB_GATEWAY_DATABASE ??
  (process.env.AGENTHUB_REMOTE_DATABASE
    ? process.env.AGENTHUB_REMOTE_DATABASE.replace(/\.db$/, "-gateway.db")
    : "./data/gateway.db");

let store: GatewayStore;
let accounts: AccountStore;
if (databaseUrl === undefined) {
  store = new SqliteGatewayStore(gatewayDbPath);
  accounts = new SqliteAccountStore(gatewayDbPath);
} else {
  const postgresStore = await PostgresGatewayStore.connect(databaseUrl);
  store = postgresStore;
  accounts = postgresStore;
}
const secureCookies = parseSecureCookies(
  process.env.AGENT_PULSE_COOKIE_SECURE,
  process.env.NODE_ENV,
);
const authenticator = ownerAuthenticator(accounts, host, secureCookies);
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
  secureCookies,
  allowedPushHosts: allowedPushHostsFromEnv(),
  trustProxy: resolveTrustProxyConfig(
    host,
    process.env.AGENT_PULSE_TRUST_PROXY_HOPS,
    process.env.AGENT_PULSE_TRUST_PROXY_ADDRESSES,
  ),
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
options.remoteContent = new RemoteContentStore(remoteDbPath);
options.remoteRoutes = {
  offlineGraceMs: parseGraceMs(
    process.env.AGENTHUB_RUNNER_OFFLINE_GRACE_MS,
    6_000,
  ),
  deviceRevalidateMs: parseGraceMs(
    process.env.AGENTHUB_DEVICE_REVALIDATE_MS,
    2_000,
  ),
  streamAuthRevalidateMs: parseGraceMs(
    process.env.AGENTHUB_STREAM_AUTH_REVALIDATE_MS,
    2_000,
  ),
};
const app = createGatewayApp(options);

const pruneTimer = setInterval(
  () => {
    const nowIso = new Date().toISOString();
    void Promise.all([
      Promise.resolve(store.pruneExpiredData?.(nowIso)).catch(() => {}),
      Promise.resolve(
        (
          accounts as {
            pruneExpiredData?: (now: string) => void | Promise<void>;
          }
        ).pruneExpiredData?.(nowIso),
      ).catch(() => {}),
    ]);
  },
  6 * 60 * 60_000,
);
pruneTimer.unref();

let shuttingDown = false;
const shutdown = (): void => {
  if (shuttingDown) return;
  shuttingDown = true;
  clearInterval(pruneTimer);
  void app.close().then(
    () => process.exit(0),
    () => process.exit(1),
  );
};
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

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
  secureCookies: boolean,
):
  | BearerTokenAuthenticator
  | DevelopmentIdentityAuthenticator
  | SessionAuthenticator {
  const ownerId = process.env.AGENT_PLATFORM_OWNER_ID ?? "default-owner";
  const rawAccessToken =
    process.env.AGENT_PLATFORM_ACCESS_TOKEN ??
    process.env.GATEWAY_SECRET_TOKEN ??
    process.env.GATEWAY_ACCESS_TOKEN;
  const accessToken =
    rawAccessToken !== undefined && rawAccessToken.trim() !== ""
      ? rawAccessToken
      : undefined;
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
      : new SessionAuthenticator(accounts, secureCookies);
  }
  const authenticator = new BearerTokenAuthenticator(ownerId, accessToken);
  // Child processes must not inherit a credential the API no longer needs in
  // plaintext; the authenticator retains only the digest.
  delete process.env.AGENT_PLATFORM_ACCESS_TOKEN;
  delete process.env.GATEWAY_SECRET_TOKEN;
  delete process.env.GATEWAY_ACCESS_TOKEN;
  return authenticator;
}

function parseSecureCookies(
  raw: string | undefined,
  nodeEnv: string | undefined,
): boolean {
  if (raw === undefined || raw.trim() === "") {
    return nodeEnv === "production";
  }
  if (raw === "true") return true;
  if (raw === "false") return false;
  throw new Error("AGENT_PULSE_COOKIE_SECURE must be true or false");
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

function parseGraceMs(value: string | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0) return fallback;
  return parsed;
}
