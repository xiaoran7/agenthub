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

import {
  AuthenticationError,
  RejectingAuthenticator,
  type OwnerAuthenticator,
} from "./auth.js";
import { ChangeBroker } from "./events.js";
import type { PushSender, PushSubscription } from "./push.js";
import {
  GatewayStoreError,
  InMemoryGatewayStore,
  OneTimeBindingCodes,
  type GatewayStore,
  type SessionFilters,
  type SessionStatusChange,
} from "./store.js";

const DEFAULT_SESSION_EVENT_LIMIT = 100;
const MAX_SESSION_EVENT_LIMIT = 500;
/** Reconnect delay advertised to EventSource clients. */
const STREAM_RETRY_MS = 5_000;
/** Keeps intermediaries from closing an idle stream, and detects dead peers. */
const STREAM_HEARTBEAT_MS = 20_000;
/** One panel opens one stream; the cap bounds a misbehaving or hostile client. */
const MAX_STREAMS_PER_OWNER = 8;
const MAX_PUSH_ENDPOINT_LENGTH = 2_048;
/**
 * Statuses that interrupt the user's attention. The panel is a monitor, not a
 * feed: notifying on every completion would train people to ignore it, and the
 * pain point in the PRD is specifically "waiting and nobody noticed".
 */
const NOTIFIED_STATUSES = new Set(["WAITING_INPUT", "WAITING_PERMISSION"]);

export interface GatewayPushOptions {
  /** VAPID application server key handed to the browser at subscribe time. */
  publicKey: string;
  sender: PushSender;
}

export interface GatewayAppOptions {
  store?: GatewayStore;
  bindingCodes?: OneTimeBindingCodes;
  broker?: ChangeBroker;
  push?: GatewayPushOptions;
  authenticator?: OwnerAuthenticator;
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
  const authenticator = options.authenticator ?? new RejectingAuthenticator();
  const openStreams = new Set<() => void>();
  const app = Fastify({
    logger: options.logger ?? false,
    bodyLimit: 1024 * 1024,
  });

  app.setErrorHandler((error, _request, reply) => {
    if (error instanceof ContractError) {
      const status =
        error.code === "invalid_signature" ||
        error instanceof AuthenticationError
          ? 401
          : 400;
      if (error instanceof AuthenticationError) {
        void reply.header("www-authenticate", 'Bearer realm="agent-platform"');
      }
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
      // Delivery happens off the request path: the Runner's upload must not
      // wait on a third-party push service, and a failed push must never turn
      // a stored batch into an error the Runner would retry.
      void notifyWaitingSessions(result.owner_id, result.changes).catch(
        (error: unknown) => {
          app.log.error(error);
        },
      );
    }
    return reply.status(200).send({
      protocol_version: batch.protocol_version,
      batch_id: batch.batch_id,
      device_id: batch.device_id,
      accepted_through: batch.last_sequence,
    });
  });

  app.get("/v1/devices", async (request, reply) => {
    const ownerId = authenticator.authenticate(request);
    return reply.send({ devices: await store.listDevices(ownerId) });
  });

  app.get("/v1/sessions", async (request, reply) => {
    const ownerId = authenticator.authenticate(request);
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
    const ownerId = authenticator.authenticate(request);
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

  app.get("/v1/push/key", async (_request, reply) => {
    if (options.push === undefined) {
      return reply.status(404).send({ error: "push_disabled" });
    }
    return reply.send({ public_key: options.push.publicKey });
  });

  app.post("/v1/push/subscriptions", async (request, reply) => {
    const ownerId = authenticator.authenticate(request);
    if (options.push === undefined) {
      return reply.status(404).send({ error: "push_disabled" });
    }
    await store.savePushSubscription(ownerId, parseSubscription(request.body));
    return reply.status(204).send();
  });

  app.delete("/v1/push/subscriptions", async (request, reply) => {
    const ownerId = authenticator.authenticate(request);
    const body = expectObject(request.body);
    const unknownField = Object.keys(body).find((key) => key !== "endpoint");
    if (unknownField !== undefined) {
      throw new ContractError("unknown_field", "body field is invalid");
    }
    await store.deletePushSubscription(ownerId, parseEndpoint(body.endpoint));
    return reply.status(204).send();
  });

  async function notifyWaitingSessions(
    ownerId: string,
    changes: readonly SessionStatusChange[],
  ): Promise<void> {
    const push = options.push;
    if (push === undefined) {
      return;
    }
    const waiting = changes.filter((change) =>
      NOTIFIED_STATUSES.has(change.activity_status),
    );
    if (waiting.length === 0) {
      return;
    }
    let live = await store.listPushSubscriptions(ownerId);
    for (const change of waiting) {
      if (live.length === 0) {
        return;
      }
      // Only fields the panel already shows: no task text, no working
      // directory, no raw agent status detail.
      const payload = JSON.stringify({
        device_id: change.device_id,
        agent_type: change.agent_type,
        agent_session_id: change.agent_session_id,
        project: change.project_display_name,
        activity_status: change.activity_status,
        waiting_reason: change.waiting_reason ?? "unknown",
        observed_at: change.observed_at,
      });
      const outcomes = await Promise.all(
        live.map(async (subscription) => ({
          subscription,
          outcome: await push.sender.send(subscription, payload),
        })),
      );
      for (const { subscription, outcome } of outcomes) {
        if (outcome === "gone") {
          // The browser dropped this subscription; keeping it would retry a
          // dead endpoint on every future notification.
          await store.deletePushSubscription(ownerId, subscription.endpoint);
        }
      }
      live = outcomes
        .filter((entry) => entry.outcome !== "gone")
        .map((entry) => entry.subscription);
    }
  }

  // Server-sent events rather than WebSocket: the read-only phase only pushes
  // gateway -> panel, and the panel reaches the gateway through a same-origin
  // HTTP proxy. Adding WebSocket would require a separately authenticated
  // upgrade proxy, which is deferred until the product has a bidirectional
  // use case.
  app.get("/v1/stream", (request, reply) => {
    const ownerId = authenticator.authenticate(request);
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

function expectObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError("invalid_body", "request body must be an object");
  }
  return value as Record<string, unknown>;
}

/**
 * Accepts the shape `PushSubscription.toJSON()` produces in the browser, and
 * nothing else: unknown fields are rejected rather than stored, matching how
 * sync events are parsed.
 */
function parseSubscription(value: unknown): PushSubscription {
  const body = expectObject(value);
  const unknownField = Object.keys(body).find(
    (key) => !["endpoint", "keys", "expirationTime"].includes(key),
  );
  if (unknownField !== undefined) {
    throw new ContractError("unknown_field", "body field is invalid");
  }
  const keys = expectObject(body.keys);
  const unknownKey = Object.keys(keys).find(
    (key) => !["p256dh", "auth"].includes(key),
  );
  if (unknownKey !== undefined) {
    throw new ContractError("unknown_field", "keys field is invalid");
  }
  return {
    endpoint: parseEndpoint(body.endpoint),
    p256dh: parseKeyMaterial(keys.p256dh, "p256dh", 65),
    auth: parseKeyMaterial(keys.auth, "auth", 16),
  };
}

function parseEndpoint(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > MAX_PUSH_ENDPOINT_LENGTH
  ) {
    throw new ContractError("invalid_endpoint", "endpoint is invalid");
  }
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new ContractError("invalid_endpoint", "endpoint is not a URL");
  }
  // Push services are always HTTPS. Requiring it also keeps a subscription
  // from steering the gateway at a plaintext internal address.
  if (
    url.protocol !== "https:" ||
    url.username !== "" ||
    url.password !== "" ||
    url.hash !== ""
  ) {
    throw new ContractError("invalid_endpoint", "endpoint is not allowed");
  }
  return url.toString();
}

function parseKeyMaterial(
  value: unknown,
  field: string,
  expectedBytes: number,
): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]{1,200}$/.test(value)) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  const decoded = Buffer.from(value, "base64url");
  if (decoded.length !== expectedBytes) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  if (field === "p256dh" && decoded[0] !== 0x04) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  return value;
}

function isOneOf<const T extends readonly string[]>(
  values: T,
  value: string,
): value is T[number] {
  return (values as readonly string[]).includes(value);
}
