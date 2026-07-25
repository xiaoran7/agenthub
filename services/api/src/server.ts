import { createGatewayApp } from "./app.js";
import { InMemoryGatewayStore, OneTimeBindingCodes } from "./store.js";

const port = parsePort(process.env.PORT);
const host = process.env.HOST ?? "127.0.0.1";
const bindingCode = process.env.AGENT_PLATFORM_BINDING_CODE;
const ownerId = process.env.AGENT_PLATFORM_OWNER_ID;

if (bindingCode === undefined || ownerId === undefined) {
  throw new Error(
    "AGENT_PLATFORM_BINDING_CODE and AGENT_PLATFORM_OWNER_ID are required",
  );
}

const app = createGatewayApp({
  store: new InMemoryGatewayStore(),
  bindingCodes: new OneTimeBindingCodes(new Map([[bindingCode, ownerId]])),
  logger: true,
});

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
