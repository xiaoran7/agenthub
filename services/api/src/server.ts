import { createGatewayApp, type GatewayAppOptions } from "./app.js";
import { PostgresGatewayStore } from "./postgres-store.js";
import { WebPushSender, vapidKeysFromEnv } from "./push.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "./store.js";

const port = parsePort(process.env.PORT);
const host = process.env.HOST ?? "127.0.0.1";
const bindingCode = process.env.AGENT_PLATFORM_BINDING_CODE;
const ownerId = process.env.AGENT_PLATFORM_OWNER_ID;
const databaseUrl = process.env.AGENT_PLATFORM_DATABASE_URL;

if (bindingCode === undefined || ownerId === undefined) {
  throw new Error(
    "AGENT_PLATFORM_BINDING_CODE and AGENT_PLATFORM_OWNER_ID are required",
  );
}

const store =
  databaseUrl === undefined
    ? new InMemoryGatewayStore()
    : await PostgresGatewayStore.connect(databaseUrl);
// Without a full VAPID triple the gateway starts with push disabled rather
// than half-configured; `npm run vapid-keys` prints a usable pair.
const vapidKeys = vapidKeysFromEnv();
const options: GatewayAppOptions = {
  store,
  bindingCodes: new OneTimeBindingCodes(new Map([[bindingCode, ownerId]])),
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
