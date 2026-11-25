import { randomUUID } from "node:crypto";

import {
  ContractError,
  activityStatuses,
  isValidAgentType,
  parseAuthSyncBatch,
  parseDeviceHeartbeat,
  parseDeviceRegistration,
  parseSyncBatch,
  presences,
  verifySyncBatch,
} from "@agent-pulse/contracts";

import { AccountError } from "../accounts.js";
import { AuthenticationError } from "../auth.js";
import {
  assertFreshDeviceTimestamp,
  isOneOf,
  mutationIdentity,
  requireVerifiedEmail,
} from "../app-helpers.js";
import {
  expectBodyObject,
  parseBindingTtlSeconds,
  parsePushEndpoint,
  parsePushSubscriptionPayload,
} from "../security-guards.js";
import {
  GatewayStoreError,
  bindingCodeDigest,
  type AuthFilters,
  type SessionFilters,
} from "../store.js";
import type { GatewayRouteContext } from "./gateway-context.js";

const DEFAULT_SESSION_EVENT_LIMIT = 100;
const MAX_SESSION_EVENT_LIMIT = 500;
const MAX_PUSH_SUBSCRIPTIONS_PER_OWNER = 16;
const DEVICE_ID_PATTERN = /^dev_[0-9a-f]{32}$/;
const MAX_DEVICE_CLOCK_SKEW_MS = 5 * 60_000;

export function registerDevicePushRoutes(ctx: GatewayRouteContext): void {
  const {
    app,
    options,
    store,
    bindingCodes,
    now,
    bindingCodeFactory,
    broker,
    authenticator,
    presence,
    secureCookies,
    notifyWaitingSessions,
    getRemoteHooks,
  } = ctx;

  app.post("/v1/device-bindings", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
      secureCookies,
    );
    if (identity.role === "member") {
      throw new AccountError(
        "workspace_permission_denied",
        "workspace role cannot bind devices",
        403,
      );
    }
    const ownerId = identity.workspace_owner_id;
    if (options.accounts !== undefined) {
      const account = await options.accounts.accountView(ownerId);
      if (account === undefined) {
        if (identity.actor_user_id !== undefined) {
          throw new AuthenticationError();
        }
      } else {
        requireVerifiedEmail(account, options.emailVerification !== undefined);
        const activeCount = await store.countActiveDevices(ownerId);
        const pendingCount =
          (await store.countPendingBindingCodes?.(
            ownerId,
            now().toISOString(),
          )) ?? 0;
        if (activeCount + pendingCount >= account.entitlements.max_devices) {
          throw new AccountError(
            "device_limit_reached",
            "subscription device limit reached",
            403,
          );
        }
      }
    }
    const expiresInSeconds = parseBindingTtlSeconds(request.body);
    const bindingCode = bindingCodeFactory();
    const expiresAt = new Date(
      now().getTime() + expiresInSeconds * 1_000,
    ).toISOString();
    await store.createBindingCode(
      ownerId,
      bindingCodeDigest(bindingCode),
      expiresAt,
      identity.actor_user_id,
    );
    return reply.status(201).send({
      binding_code: bindingCode,
      expires_at: expiresAt,
    });
  });

  app.post("/v1/devices/register", async (request, reply) => {
    const registration = parseDeviceRegistration(request.body);
    let result;
    if (bindingCodes === undefined) {
      result = await store.registerDeviceWithBindingCode(
        bindingCodeDigest(registration.binding_code),
        registration.device_id,
        registration.public_key,
      );
    } else {
      const claimResult = bindingCodes.claim(
        registration.binding_code,
        registration.device_id,
      );
      try {
        result = await store.registerDevice(
          claimResult.ownerId,
          registration.device_id,
          registration.public_key,
          undefined,
          claimResult.isFresh,
        );
      } catch (error) {
        if (claimResult.isFresh) {
          bindingCodes.release(
            registration.binding_code,
            registration.device_id,
          );
        }
        throw error;
      }
    }
    if (registration.device_name && options.remoteContent) {
      options.remoteContent.recordDeviceName(
        result.device.owner_id,
        registration.device_id,
        registration.device_name,
      );
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

  app.post("/v1/device-auth", async (request, reply) => {
    const batch = parseAuthSyncBatch(request.body);
    const result = await store.ingestAuthBatch(batch);
    if (result.accepted > 0) {
      broker.publish(result.owner_id, {
        device_id: batch.device_id,
        accepted: result.accepted,
        auth_snapshots: true,
      });
    }
    return reply.status(200).send({
      protocol_version: batch.protocol_version,
      device_id: batch.device_id,
      accepted: result.accepted,
    });
  });

  app.post("/v1/devices/heartbeat", async (request, reply) => {
    const heartbeat = parseDeviceHeartbeat(request.body);
    const receivedAt = now();
    assertFreshDeviceTimestamp(heartbeat.sent_at, receivedAt);
    const result = await store.recordHeartbeat(
      heartbeat,
      receivedAt.toISOString(),
    );
    if (result.accepted && heartbeat.device_name && options.remoteContent) {
      options.remoteContent.recordDeviceName(
        result.owner_id,
        heartbeat.device_id,
        heartbeat.device_name,
      );
    }
    return reply.send({
      protocol_version: heartbeat.protocol_version,
      device_id: heartbeat.device_id,
      accepted: result.accepted,
    });
  });

  app.get("/v1/devices", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const devices = await store.listDevices(ownerId);
    // A device id is a key, not a label. The name the Runner advertises is
    // what the settings list shows, so the id never has to.
    const names = options.remoteContent?.deviceNames(ownerId);
    return reply.send({
      devices: devices.map((device) => ({
        ...device,
        // A Runner that holds the v3 stream is online even when its HTTP
        // heartbeat stopped: `daemon`/`web` only open the WebSocket, so
        // `last_seen_at` is pairing time and the two surfaces disagreed.
        online: device.revoked
          ? false
          : device.online || presence.isOnline(device.device_id),
        ...(names ? { display_name: names.get(device.device_id) } : {}),
      })),
    });
  });

  app.get("/v1/device-auth", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find((key) => key !== "device_id");
    if (unknownQuery !== undefined) {
      throw new ContractError("invalid_query", "query field is invalid");
    }
    const filters: AuthFilters = {};
    if (query.device_id !== undefined) {
      if (
        typeof query.device_id !== "string" ||
        !DEVICE_ID_PATTERN.test(query.device_id)
      ) {
        throw new ContractError("invalid_query", "device_id is invalid");
      }
      filters.device_id = query.device_id;
    }
    return reply.send({ snapshots: await store.listAuth(ownerId, filters) });
  });

  app.delete("/v1/devices/:deviceId", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
      secureCookies,
    );
    if (identity.role === "member") {
      throw new AccountError(
        "workspace_permission_denied",
        "workspace role cannot revoke devices",
        403,
      );
    }
    const ownerId = identity.workspace_owner_id;
    const { deviceId } = request.params as { deviceId?: unknown };
    if (typeof deviceId !== "string" || !DEVICE_ID_PATTERN.test(deviceId)) {
      throw new ContractError("invalid_device_id", "device ID is invalid");
    }
    const revoked = await store.revokeDevice(
      ownerId,
      deviceId,
      identity.actor_user_id,
    );
    if (revoked) {
      getRemoteHooks()?.revokeRunner(deviceId, ownerId);
    }
    return reply.status(204).send();
  });

  app.get("/v1/sessions", async (request, reply) => {
    const ownerId = await authenticator.authenticate(request);
    const query = request.query as Record<string, unknown>;
    const unknownQuery = Object.keys(query).find(
      (key) =>
        !["device_id", "agent_type", "activity_status", "presence"].includes(
          key,
        ),
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
    if (query.agent_type !== undefined) {
      if (!isValidAgentType(query.agent_type)) {
        throw new ContractError("invalid_query", "agent_type is invalid");
      }
      filters.agent_type = query.agent_type;
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
    const ownerId = await authenticator.authenticate(request);
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
    if (!isValidAgentType(query.agent_type)) {
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
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
      secureCookies,
    );
    const ownerId = identity.workspace_owner_id;
    const subscriberUserId = identity.actor_user_id;
    if (options.push === undefined) {
      return reply.status(404).send({ error: "push_disabled" });
    }
    const subscription = parsePushSubscriptionPayload(
      request.body,
      options.allowedPushHosts,
    );
    const existing = await store.listPushSubscriptions(ownerId);
    const conflicting = existing.find(
      (item) => item.endpoint === subscription.endpoint,
    );
    if (
      conflicting !== undefined &&
      identity.role === "member" &&
      conflicting.subscriber_user_id !== subscriberUserId
    ) {
      throw new AccountError(
        "workspace_permission_denied",
        "workspace member cannot overwrite another subscriber's push subscription",
        403,
      );
    }
    const quotaScope =
      subscriberUserId !== undefined
        ? existing.filter(
            (item) => item.subscriber_user_id === subscriberUserId,
          )
        : existing;
    if (
      quotaScope.length >= MAX_PUSH_SUBSCRIPTIONS_PER_OWNER &&
      !quotaScope.some((item) => item.endpoint === subscription.endpoint)
    ) {
      throw new ContractError(
        "push_subscription_limit",
        "maximum push subscriptions reached",
      );
    }
    await store.savePushSubscription(ownerId, subscription, subscriberUserId);
    return reply.status(204).send();
  });

  app.delete("/v1/push/subscriptions", async (request, reply) => {
    const identity = await mutationIdentity(
      options.accounts,
      authenticator,
      request,
      now(),
      secureCookies,
    );
    const ownerId = identity.workspace_owner_id;
    const body = expectBodyObject(request.body);
    const unknownField = Object.keys(body).find((key) => key !== "endpoint");
    if (unknownField !== undefined) {
      throw new ContractError("unknown_field", "body field is invalid");
    }
    const subscriberFilter =
      identity.actor_user_id !== undefined && identity.role === "member"
        ? identity.actor_user_id
        : undefined;
    await store.deletePushSubscription(
      ownerId,
      parsePushEndpoint(body.endpoint, options.allowedPushHosts),
      subscriberFilter,
    );
    return reply.status(204).send();
  });
}
