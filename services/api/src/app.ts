import {
  ContractError,
  activityStatuses,
  parseDeviceRegistration,
  parseSyncBatch,
  presences,
  verifySyncBatch,
} from "@agent-platform/contracts";
import Fastify, { type FastifyInstance } from "fastify";

import {
  GatewayStoreError,
  InMemoryGatewayStore,
  OneTimeBindingCodes,
  type GatewayStore,
  type SessionFilters,
} from "./store.js";

export interface GatewayAppOptions {
  store?: GatewayStore;
  bindingCodes?: OneTimeBindingCodes;
  logger?: boolean;
}

export function createGatewayApp(
  options: GatewayAppOptions = {},
): FastifyInstance {
  const store: GatewayStore = options.store ?? new InMemoryGatewayStore();
  const bindingCodes =
    options.bindingCodes ??
    new OneTimeBindingCodes(new Map([["local-development", "local-user"]]));
  const app = Fastify({
    logger: options.logger ?? false,
    bodyLimit: 1024 * 1024,
  });

  app.setErrorHandler((error, _request, reply) => {
    if (error instanceof ContractError) {
      const status =
        error.code === "invalid_signature" ||
        error.code === "missing_development_identity"
          ? 401
          : 400;
      void reply.status(status).send({ error: error.code });
      return;
    }
    if (error instanceof GatewayStoreError) {
      const status = [
        "device_not_registered",
        "device_key_mismatch",
        "invalid_binding_code",
      ].includes(error.code)
        ? 403
        : 409;
      void reply.status(status).send({ error: error.code });
      return;
    }
    const statusCode =
      typeof error === "object" &&
      error !== null &&
      "statusCode" in error &&
      typeof error.statusCode === "number"
        ? error.statusCode
        : undefined;
    if (statusCode !== undefined && statusCode >= 400 && statusCode < 500) {
      void reply.status(statusCode).send({ error: "invalid_request" });
      return;
    }
    app.log.error(error);
    void reply.status(500).send({ error: "internal_error" });
  });

  app.get("/health", async () => ({ status: "ok" }));

  app.post("/v1/devices/register", async (request, reply) => {
    const registration = parseDeviceRegistration(request.body);
    const ownerId = bindingCodes.claim(
      registration.binding_code,
      registration.device_id,
    );
    let result;
    try {
      result = await store.registerDevice(
        ownerId,
        registration.device_id,
        registration.public_key,
      );
    } catch (error) {
      bindingCodes.release(registration.binding_code, registration.device_id);
      throw error;
    }
    return reply.status(result.created ? 201 : 200).send({
      device_id: result.device.device_id,
      owner_id: result.device.owner_id,
      registered: true,
    });
  });

  app.post("/v1/sync/batches", async (request, reply) => {
    const batch = parseSyncBatch(request.body);
    verifySyncBatch(batch);
    await store.ingestBatch(batch);
    return reply.status(200).send({
      protocol_version: batch.protocol_version,
      batch_id: batch.batch_id,
      device_id: batch.device_id,
      accepted_through: batch.last_sequence,
    });
  });

  app.get("/v1/devices", async (request, reply) => {
    const ownerId = developmentOwner(request.headers["x-user-id"]);
    return reply.send({ devices: await store.listDevices(ownerId) });
  });

  app.get("/v1/sessions", async (request, reply) => {
    const ownerId = developmentOwner(request.headers["x-user-id"]);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) => !["device_id", "activity_status", "presence"].includes(key),
    );
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    const filters: SessionFilters = {};
    if (query.device_id !== undefined) {
      if (typeof query.device_id !== "string") {
        throw new ContractError("invalid_query", "device_id is invalid");
      }
      filters.device_id = query.device_id;
    }
    if (query.activity_status !== undefined) {
      if (
        typeof query.activity_status !== "string" ||
        !isOneOf(activityStatuses, query.activity_status)
      ) {
        throw new ContractError("invalid_query", "activity_status is invalid");
      }
      filters.activity_status = query.activity_status;
    }
    if (query.presence !== undefined) {
      if (
        typeof query.presence !== "string" ||
        !isOneOf(presences, query.presence)
      ) {
        throw new ContractError("invalid_query", "presence is invalid");
      }
      filters.presence = query.presence;
    }
    const sessions = await store.listSessions(ownerId, filters);
    return reply.send({ sessions });
  });

  if (store.close !== undefined) {
    app.addHook("onClose", async () => {
      await store.close?.();
    });
  }

  return app;
}

function developmentOwner(value: string | string[] | undefined): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 128) {
    throw new ContractError(
      "missing_development_identity",
      "x-user-id is required",
    );
  }
  return value;
}

function isOneOf<const T extends readonly string[]>(
  values: T,
  value: string,
): value is T[number] {
  return (values as readonly string[]).includes(value);
}
