import {
  ContractError,
  activityStatuses,
  agentTypes,
  parseDeviceRegistration,
  parseSyncBatch,
  presences,
  verifySyncBatch,
} from "@agent-platform/contracts";
import Fastify, { type FastifyInstance } from "fastify";

import { ChangeBroker } from "./events.js";
import {
  GatewayStoreError,
  InMemoryGatewayStore,
  OneTimeBindingCodes,
  type GatewayStore,
  type SessionFilters,
} from "./store.js";

const DEFAULT_SESSION_EVENT_LIMIT = 100;
const MAX_SESSION_EVENT_LIMIT = 500;
/** Reconnect delay advertised to EventSource clients. */
const STREAM_RETRY_MS = 5_000;
/** Keeps intermediaries from closing an idle stream, and detects dead peers. */
const STREAM_HEARTBEAT_MS = 20_000;
/** One panel opens one stream; the cap bounds a misbehaving or hostile client. */
const MAX_STREAMS_PER_OWNER = 8;

export interface GatewayAppOptions {
  store?: GatewayStore;
  bindingCodes?: OneTimeBindingCodes;
  broker?: ChangeBroker;
  logger?: boolean;
}

export function createGatewayApp(
  options: GatewayAppOptions = {},
): FastifyInstance {
  const store: GatewayStore = options.store ?? new InMemoryGatewayStore();
  const bindingCodes =
    options.bindingCodes ??
    new OneTimeBindingCodes(new Map([["local-development", "local-user"]]));
  const broker = options.broker ?? new ChangeBroker();
  const openStreams = new Set<() => void>();
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
    const result = await store.ingestBatch(batch);
    if (result.accepted > 0) {
      broker.publish(result.owner_id, {
        device_id: batch.device_id,
        accepted: result.accepted,
      });
    }
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

  app.get("/v1/sessions/events", async (request, reply) => {
    const ownerId = developmentOwner(request.headers["x-user-id"]);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) =>
        !["device_id", "agent_type", "agent_session_id", "limit"].includes(key),
    );
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    if (typeof query.device_id !== "string" || query.device_id.length === 0) {
      throw new ContractError("invalid_query", "device_id is required");
    }
    if (
      typeof query.agent_type !== "string" ||
      !isOneOf(agentTypes, query.agent_type)
    ) {
      throw new ContractError("invalid_query", "agent_type is invalid");
    }
    if (
      typeof query.agent_session_id !== "string" ||
      query.agent_session_id.length === 0 ||
      query.agent_session_id.length > 512
    ) {
      throw new ContractError("invalid_query", "agent_session_id is required");
    }
    let limit = DEFAULT_SESSION_EVENT_LIMIT;
    if (query.limit !== undefined) {
      if (
        typeof query.limit !== "string" ||
        !/^[1-9][0-9]{0,3}$/.test(query.limit)
      ) {
        throw new ContractError("invalid_query", "limit is invalid");
      }
      limit = Number(query.limit);
      if (limit > MAX_SESSION_EVENT_LIMIT) {
        throw new ContractError("invalid_query", "limit is invalid");
      }
    }
    const events = await store.listSessionEvents(ownerId, {
      device_id: query.device_id,
      agent_type: query.agent_type,
      agent_session_id: query.agent_session_id,
      limit,
    });
    return reply.send({ events });
  });

  // Server-sent events rather than WebSocket: the read-only phase only pushes
  // gateway -> panel, and the panel reaches the gateway through a same-origin
  // HTTP proxy that does not forward upgrade requests. Adding WebSocket would
  // mean opening CORS, which is deliberately deferred until real auth exists.
  app.get("/v1/stream", (request, reply) => {
    const ownerId = developmentOwner(request.headers["x-user-id"]);
    if (broker.subscriberCount(ownerId) >= MAX_STREAMS_PER_OWNER) {
      void reply.status(429).send({ error: "too_many_streams" });
      return;
    }
    reply.hijack();
    const raw = reply.raw;
    raw.writeHead(200, {
      "content-type": "text/event-stream",
      "cache-control": "no-store",
      connection: "keep-alive",
      // Ask reverse proxies not to buffer; buffering turns the stream into a
      // hung request that only flushes when the connection closes.
      "x-accel-buffering": "no",
    });
    request.raw.socket.setNoDelay(true);
    request.raw.socket.setTimeout(0);

    const write = (chunk: string): void => {
      if (!raw.writableEnded) {
        raw.write(chunk);
      }
    };
    write(`retry: ${STREAM_RETRY_MS}\n\n`);
    write("event: ready\ndata: {}\n\n");

    const unsubscribe = broker.subscribe(ownerId, (message) => {
      write(`event: sessions\ndata: ${JSON.stringify(message)}\n\n`);
    });
    const heartbeat = setInterval(
      () => write(": keepalive\n\n"),
      STREAM_HEARTBEAT_MS,
    );
    heartbeat.unref();

    let closed = false;
    const close = (): void => {
      if (closed) {
        return;
      }
      closed = true;
      clearInterval(heartbeat);
      unsubscribe();
      openStreams.delete(close);
      raw.end();
    };
    openStreams.add(close);
    raw.on("close", close);
    raw.on("error", close);
  });

  app.addHook("onClose", async () => {
    for (const close of [...openStreams]) {
      close();
    }
    await store.close?.();
  });

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
