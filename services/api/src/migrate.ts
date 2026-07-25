import { PostgresGatewayStore } from "./postgres-store.js";

const databaseUrl = process.env.AGENT_PLATFORM_DATABASE_URL;
if (databaseUrl === undefined || databaseUrl.length === 0) {
  throw new Error("AGENT_PLATFORM_DATABASE_URL is required");
}

const store = await PostgresGatewayStore.connect(databaseUrl);
await store.close();
process.stdout.write('{"migrated":true}\n');
