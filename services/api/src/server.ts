import { isIP } from "node:net";

import { createGatewayApp, type GatewayAppOptions } from "./app.js";
import {
  BearerTokenAuthenticator,
  DevelopmentIdentityAuthenticator,
} from "./auth.js";
import { PostgresGatewayStore } from "./postgres-store.js";
import { WebPushSender, vapidKeysFromEnv } from "./push.js";
import { InMemoryGatewayStore } from "./store.js";

const port = parsePort(process.env.PORT);
const host = process.env.HOST ?? "127.0.0.1";
const ownerId = process.env.AGENT_PLATFORM_OWNER_ID;
const databaseUrl = process.env.AGENT_PLATFORM_DATABASE_URL;

if (ownerId === undefined) {
  throw new Error("AGENT_PLATFORM_OWNER_ID is required");
}
const authenticator = ownerAuthenticator(ownerId, host);

const store =
  databaseUrl === undefined
    ? new InMemoryGatewayStore()
    : await PostgresGatewayStore.connect(databaseUrl);
// Without a full VAPID triple the gateway starts with push disabled rather
// than half-configured; `npm run vapid-keys` prints a usable pair.
const vapidKeys = vapidKeysFromEnv();
const options: GatewayAppOptions = {
  store,
  authenticator,
  logger: true,
};
if (vapidKeys !== undefined) {
  options.push = {
    publicKey: vapidKeys.publicKey,
    sender: new WebPushSender(vapidKeys),
  };
}
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
  ownerId: string,
  host: string,
): BearerTokenAuthenticator | DevelopmentIdentityAuthenticator {
  const accessToken = process.env.AGENT_PLATFORM_ACCESS_TOKEN;
  const allowDevelopmentAuth =
    process.env.AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH === "true";
  if (accessToken === undefined && !allowDevelopmentAuth) {
    throw new Error(
      "AGENT_PLATFORM_ACCESS_TOKEN is required; set " +
        "AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true only for local development",
    );
  }
  if (allowDevelopmentAuth && !isLoopbackHost(host)) {
    throw new Error(
      "AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH is only allowed on a loopback HOST",
    );
  }
  if (accessToken === undefined) {
    return new DevelopmentIdentityAuthenticator();
  }
  const authenticator = new BearerTokenAuthenticator(ownerId, accessToken);
  // Child processes must not inherit a credential the API no longer needs in
  // plaintext; the authenticator retains only the digest.
  delete process.env.AGENT_PLATFORM_ACCESS_TOKEN;
  return authenticator;
}
