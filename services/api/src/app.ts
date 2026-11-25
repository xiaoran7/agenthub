import { randomBytes } from "node:crypto";

import { ContractError } from "@agent-pulse/contracts";
import Fastify, { type FastifyInstance } from "fastify";
import fastifyWebsocket from "@fastify/websocket";

import { AccountError } from "./accounts.js";
import {
  dispatchWaitingPushNotifications,
  registerV1StreamRoute,
} from "./app-helpers.js";
import { AuthenticationError, RejectingAuthenticator } from "./auth.js";
import { ChangeBroker } from "./events.js";
import { RunnerPresence } from "./presence.js";
import {
  registerRemoteRoutes,
  type RemoteControlHooks,
} from "./remote-routes.js";
import { enforceCookieCsrf } from "./security-guards.js";
import {
  GatewayStoreError,
  InMemoryGatewayStore,
  type GatewayStore,
  type SessionStatusChange,
} from "./store.js";
import { registerAuthRoutes } from "./routes/auth-routes.js";
import { registerDevicePushRoutes } from "./routes/device-push-routes.js";
import type {
  GatewayAppOptions,
  GatewayPushOptions,
  GatewayRouteContext,
} from "./routes/gateway-context.js";
import { registerTeamBillingRoutes } from "./routes/team-billing-routes.js";

export type { GatewayAppOptions, GatewayPushOptions };

export function createGatewayApp(
  options: GatewayAppOptions = {},
): FastifyInstance {
  const store: GatewayStore = options.store ?? new InMemoryGatewayStore();
  const bindingCodes = options.bindingCodes;
  const now = options.now ?? (() => new Date());
  const bindingCodeFactory =
    options.bindingCodeFactory ?? (() => randomBytes(32).toString("base64url"));
  const broker = options.broker ?? new ChangeBroker();
  const authenticator = options.authenticator ?? new RejectingAuthenticator();
  const loginRateLimitSecret = options.loginRateLimitSecret ?? randomBytes(32);
  // One presence set for both surfaces: the v3 stream marks a Runner online,
  // and the v1 device list reads the same answer.
  const presence =
    options.remoteRoutes?.presence ?? options.presence ?? new RunnerPresence();
  if (options.accounts !== undefined) {
    store.setAuditRecorder?.(options.accounts);
    if (
      !Object.is(options.accounts, store) &&
      store.deletePushSubscriptionsForSubscriber !== undefined
    ) {
      options.accounts.setMemberRemovedHook?.(
        async (workspaceOwnerId, memberUserId) => {
          await store.deletePushSubscriptionsForSubscriber?.(
            workspaceOwnerId,
            memberUserId,
          );
        },
      );
    }
  }
  const openStreams = new Set<(destroySocket?: boolean) => void>();
  const streamAuthRevalidators = new Set<() => void>();
  const secureCookies = options.secureCookies ?? false;
  const trustProxyOption = options.trustProxy ?? false;
  const fastifyTrustProxy =
    typeof trustProxyOption === "number"
      ? trustProxyOption > 0
        ? (_address: string, hop: number) => hop < trustProxyOption
        : false
      : trustProxyOption;
  const app = Fastify({
    logger: options.logger ?? false,
    bodyLimit: 1024 * 1024,
    trustProxy: fastifyTrustProxy,
  });
  app.addHook("preHandler", async (request) => {
    enforceCookieCsrf(
      request,
      secureCookies,
      options.emailVerification?.baseUrl,
    );
  });

  app.setErrorHandler((error, _request, reply) => {
    if (error instanceof AccountError) {
      void reply.status(error.statusCode).send({ error: error.code });
      return;
    }
    if (error instanceof ContractError) {
      const status =
        error.code === "invalid_signature" ||
        error instanceof AuthenticationError
          ? 401
          : 400;
      if (error instanceof AuthenticationError) {
        void reply.header("www-authenticate", 'Bearer realm="agent-pulse"');
      }
      void reply.status(status).send({ error: error.code });
      return;
    }
    if (error instanceof GatewayStoreError) {
      const status = [
        "device_not_registered",
        "device_key_mismatch",
        "device_revoked",
        "invalid_binding_code",
        "device_not_owned",
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
      const code =
        error instanceof ContractError
          ? error.code
          : typeof error === "object" &&
              error !== null &&
              "code" in error &&
              typeof error.code === "string"
            ? error.code
            : "invalid_request";
      void reply.status(statusCode).send({
        error: code,
        code,
        message: error instanceof Error ? error.message : undefined,
      });
      return;
    }
    app.log.error(error);
    void reply.status(500).send({ error: "internal_error" });
  });

  // Native JSONL permits 16 MiB; leave room for the Remote envelope.
  void app.register(fastifyWebsocket, {
    options: { maxPayload: 17 * 1024 * 1024 },
  });
  let remoteHooks: RemoteControlHooks | undefined;
  const invalidateAllStreamAuth = (): void => {
    remoteHooks?.invalidateStreamAuth();
    for (const check of streamAuthRevalidators) check();
  };
  if (options.remoteContent) {
    const content = options.remoteContent;
    const externalHooks = options.remoteRoutes?.onControlHooks;
    void app.register(async (remoteApp) =>
      registerRemoteRoutes(remoteApp, store, authenticator, content, {
        ...options.remoteRoutes,
        accounts: options.accounts,
        secureCookies,
        now,
        presence,
        onControlHooks: (hooks) => {
          remoteHooks = hooks;
          externalHooks?.(hooks);
        },
      }),
    );
  }

  const singleAdmin = options.singleAdmin ?? true;

  app.get("/health", async () => ({ status: "ok" }));
  app.get("/healthz", async () => ({ status: "ok" }));

  async function notifyWaitingSessions(
    ownerId: string,
    changes: readonly SessionStatusChange[],
  ): Promise<void> {
    await dispatchWaitingPushNotifications(
      options.push,
      store,
      app,
      ownerId,
      changes,
    );
  }

  const routeContext: GatewayRouteContext = {
    app,
    options,
    store,
    bindingCodes,
    now,
    bindingCodeFactory,
    broker,
    authenticator,
    loginRateLimitSecret,
    presence,
    secureCookies,
    singleAdmin,
    invalidateAllStreamAuth,
    notifyWaitingSessions,
    getRemoteHooks: () => remoteHooks,
  };

  registerAuthRoutes(routeContext);
  registerTeamBillingRoutes(routeContext);
  registerDevicePushRoutes(routeContext);

  registerV1StreamRoute(
    app,
    authenticator,
    broker,
    openStreams,
    streamAuthRevalidators,
    options.remoteRoutes?.streamAuthRevalidateMs ?? 10_000,
  );

  app.addHook("preClose", async () => {
    for (const close of [...openStreams]) {
      close(true);
    }
    app.server.closeIdleConnections?.();
  });

  app.addHook("onClose", async () => {
    for (const close of [...openStreams]) {
      close(true);
    }
    await broker.close();
    await store.close?.();
    if (options.accounts && !Object.is(options.accounts, store)) {
      await options.accounts.close?.();
    }
  });

  return app;
}
