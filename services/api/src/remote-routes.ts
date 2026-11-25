import { randomBytes } from "node:crypto";
import { parseRemoteCommand } from "@agent-pulse/contracts";
import type { FastifyInstance, FastifyRequest } from "fastify";
import type { WebSocket } from "ws";
import { AuthenticationError, type OwnerAuthenticator } from "./auth.js";
import {
  currentSessionDigest,
  currentSessionIdentity,
  type AccountStore,
} from "./accounts.js";
import { RunnerPresence } from "./presence.js";
import type { GatewayStore } from "./store.js";
import {
  PendingPluginCommandBroker,
  parsePluginStateFrame,
  registerRemotePluginRoutes,
} from "./remote-plugins.js";
import {
  RemoteContentStore,
  RemoteStoreError,
  projectRunnerHello,
} from "./remote-store.js";
import { extractDiscoveryProviders } from "./remote-store-validation.js";
import {
  createRemoteErrorHandler,
  registerRemoteStreamEndpoint,
  verifyRunnerHandshake,
} from "./remote-stream-helpers.js";

const MAX_WS_CONNECTIONS = 256;
const MAX_UNAUTH_WS_CONNECTIONS = 64;
const MAX_UNAUTH_WS_PER_IP = 8;

interface Connection {
  socket: WebSocket;
  owner: string;
  runner: string;
  generation: string;
  offered: number;
  lastSeen: number;
  verifiedAt: number;
  supportsPlugins?: boolean;
  supportsPluginsV2?: boolean;
  supportsCodexSessionControl?: boolean;
}

export interface RemoteControlHooks {
  revokeRunner: (runner: string, expectedOwner?: string) => void;
  invalidateStreamAuth: () => void;
}

export interface RemoteRouteOptions {
  isBackpressured?: (socket: WebSocket) => boolean;
  /** Account store used to resolve a browser session cookie to its owner. */
  accounts?: AccountStore | undefined;
  /** Clock injection point for the session-expiry check. */
  now?: (() => Date) | undefined;
  /** Live stream presence, shared with the device list. */
  presence?: RunnerPresence | undefined;
  /** Grace window before a dropped WebSocket flips runner presence offline. */
  offlineGraceMs?: number | undefined;
  /** Minimum interval between DB device-revocation re-checks on an active WebSocket. */
  deviceRevalidateMs?: number | undefined;
  /** Minimum interval between DB cookie re-checks when pumping SSE frames. */
  streamAuthRevalidateMs?: number | undefined;
  /** Whether secure `__Host-` session cookies are strictly required. */
  secureCookies?: boolean | undefined;
  /** Receives hooks so revocation routes can evict live sockets/caches immediately. */
  onControlHooks?: ((hooks: RemoteControlHooks) => void) | undefined;
}

export function registerRemoteRoutes(
  app: FastifyInstance,
  devices: GatewayStore,
  auth: OwnerAuthenticator,
  content: RemoteContentStore,
  options: RemoteRouteOptions = {},
): void {
  const connections = new Map<string, Connection>();
  const pluginBroker = new PendingPluginCommandBroker();
  const streams = new Set<(destroySocket?: boolean) => void>();
  const ownerStreams = new Map<
    string,
    Set<(destroySocket?: boolean) => void>
  >();
  const streamRevalidators = new Set<() => void>();
  const sockets = new Set<WebSocket>();
  const unauthSockets = new Set<WebSocket>();
  const unauthByIp = new Map<string, number>();
  const releaseUnauth = (socket: WebSocket, ip: string): void => {
    if (!unauthSockets.delete(socket)) return;
    const count = unauthByIp.get(ip) ?? 0;
    if (count <= 1) {
      unauthByIp.delete(ip);
    } else {
      unauthByIp.set(ip, count - 1);
    }
  };
  const now = options.now ?? (() => new Date());
  const presence = options.presence ?? new RunnerPresence();
  const isOnline = (runner: string): boolean => presence.isOnline(runner);
  const deviceRevalidateMs = options.deviceRevalidateMs ?? 0;
  const streamAuthRevalidateMs = options.streamAuthRevalidateMs ?? 0;
  let authEpoch = 0;
  options.onControlHooks?.({
    revokeRunner(runner: string, expectedOwner?: string): void {
      const conn = connections.get(runner);
      if (expectedOwner) {
        if (conn && conn.owner !== expectedOwner) return;
        if (!conn) {
          const recordedOwner = content.runnerOwner(runner);
          if (recordedOwner && recordedOwner !== expectedOwner) return;
        }
      }
      pluginBroker.rejectForRunner(runner, "runner_offline");
      content.deleteRunnerPlugins(runner, expectedOwner);
      if (conn) {
        conn.verifiedAt = 0;
        conn.socket.close(1008, "device_revoked");
        connections.delete(runner);
      }
      presence.markOffline(runner);
    },
    invalidateStreamAuth(): void {
      authEpoch += 1;
      for (const revalidate of [...streamRevalidators]) {
        revalidate();
      }
    },
  });
  /**
   * Resolves the owner of a console request.
   *
   * The account session cookie wins, exactly as it does for the account
   * routes: the workspace owner is the scope every device, session and
   * command belongs to. The development `x-user-id` header stays a fallback
   * for a Gateway running without accounts.
   *
   * A cookie that no longer resolves is a signed-out console, not a
   * credential-free one: falling through to the deployment owner would answer
   * the request from a different scope than the one the browser is signed in
   * to, so an expired session would quietly show another scope's devices
   * instead of asking the user to sign in again.
   */
  const resolveActor = async (
    request: FastifyRequest,
  ): Promise<{ owner: string; role: "owner" | "admin" | "member" }> => {
    if (options.accounts !== undefined) {
      const secure = options.secureCookies ?? false;
      const identity = await currentSessionIdentity(
        options.accounts,
        request,
        now(),
        secure,
      );
      if (identity !== undefined) {
        return { owner: identity.workspace_owner_id, role: identity.role };
      }
      if (currentSessionDigest(request, secure) !== undefined) {
        throw new AuthenticationError();
      }
    }
    return { owner: await auth.authenticate(request), role: "owner" };
  };
  const resolveOwner = async (request: FastifyRequest): Promise<string> =>
    (await resolveActor(request)).owner;
  const requireDevice = async (
    owner: string,
    runner: string,
  ): Promise<void> => {
    const device = await devices.getRegisteredDevice(runner);
    if (!device || device.owner_id !== owner || device.revoked_at)
      throw new RemoteStoreError("device_not_found");
  };
  const assertCurrentConnection = (connection: Connection): void => {
    if (connections.get(connection.runner) !== connection)
      throw new RemoteStoreError("stale_connection");
  };
  const transmit = (connection: Connection, value: unknown): void => {
    if (
      connection.socket.readyState !== 1 ||
      (options.isBackpressured?.(connection.socket) ??
        connection.socket.bufferedAmount > 1024 * 1024)
    )
      throw new RemoteStoreError("runner_backpressure");
    connection.socket.send(JSON.stringify(value));
  };
  const pump = (connection: Connection): void => {
    for (const item of content.commands(
      connection.runner,
      connection.offered,
    )) {
      try {
        transmit(connection, { type: "command", ...item });
        connection.offered = item.seq;
      } catch (error) {
        if (
          error instanceof RemoteStoreError &&
          error.code === "runner_backpressure"
        ) {
          break;
        }
        throw error;
      }
    }
  };
  app.setErrorHandler(createRemoteErrorHandler(app));
  app.get("/v3/remote/ws", { websocket: true }, (socket, request) => {
    const clientIp = request.ip || "unknown";
    const ipUnauthCount = unauthByIp.get(clientIp) ?? 0;
    if (
      sockets.size >= MAX_WS_CONNECTIONS ||
      unauthSockets.size >= MAX_UNAUTH_WS_CONNECTIONS ||
      ipUnauthCount >= MAX_UNAUTH_WS_PER_IP
    ) {
      socket.close(1013, "too_many_connections");
      const rejectTimer = setTimeout(() => socket.terminate?.(), 1_000);
      rejectTimer.unref?.();
      socket.once("close", () => clearTimeout(rejectTimer));
      return;
    }
    sockets.add(socket);
    unauthSockets.add(socket);
    unauthByIp.set(clientIp, ipUnauthCount + 1);
    const nonce = randomBytes(32).toString("hex");
    let connection: Connection | undefined;
    let closing = false;
    let terminateTimer: ReturnType<typeof setTimeout> | undefined;
    const scheduleForceTerminate = (): void => {
      if (terminateTimer !== undefined) return;
      terminateTimer = setTimeout(() => {
        socket.terminate?.();
      }, 1_000);
      terminateTimer.unref?.();
    };
    let chain = Promise.resolve();
    let queued = 0;
    let queuedBytes = 0;
    socket.send(JSON.stringify({ type: "challenge", version: 3, nonce }));
    const timeout = setTimeout(() => {
      closing = true;
      socket.close(1008, "handshake_timeout");
      scheduleForceTerminate();
    }, 10_000);
    timeout.unref?.();
    socket.on("pong", () => {
      if (connection) connection.lastSeen = Date.now();
    });
    socket.on("message", (data) => {
      if (closing) return;
      const bytes = Buffer.isBuffer(data)
        ? data.byteLength
        : Array.isArray(data)
          ? data.reduce((sum, part) => sum + part.byteLength, 0)
          : Buffer.byteLength(String(data));
      if (
        queued + 1 > 128 ||
        queuedBytes + bytes > 32 * 1024 * 1024 ||
        bytes > (connection ? 17 * 1024 * 1024 : 16 * 1024)
      ) {
        closing = true;
        socket.close(1009, "frame_too_large");
        if (!connection) scheduleForceTerminate();
        return;
      }
      const serialized = data.toString("utf8");
      queued += 1;
      queuedBytes += bytes;
      chain = chain
        .then(async () => {
          if (closing) return;
          const frame = JSON.parse(serialized) as Record<string, unknown>;
          if (!connection) {
            const handshake = await verifyRunnerHandshake(
              devices,
              frame,
              nonce,
            );
            if (closing) return;
            const device = handshake.device;
            content.register(device.owner_id, device.device_id);
            // A restarted Host publishes a new generation, and a command is
            // addressed to exactly one generation. Commands still waiting for
            // the previous one can never be applied, so they are dropped here
            // instead of being re-delivered on every reconnect.
            content.supersedeStaleCommands(
              device.device_id,
              handshake.hostGeneration,
            );
            const marks = content.watermarks(device.device_id);
            let received = handshake.receivedCommandSeq;
            if (!Number.isSafeInteger(received) || received < 0) {
              throw new RemoteStoreError("command_journal_mismatch");
            }
            if (received > marks.command_seq) {
              received = marks.command_seq;
            }
            if (received < marks.command_ack) {
              content.acknowledgeCommands(device.device_id, received, true);
            }
            const previous = connections.get(device.device_id);
            if (previous) previous.socket.close(1000, "connection_replaced");
            const caps = Array.isArray(frame.capabilities)
              ? frame.capabilities
              : [];
            const supportsPlugins =
              caps.includes("plugins") || frame.plugins === true;
            const supportsPluginsV2 = caps.includes("plugins_v2");
            const nextConnection: Connection = {
              socket,
              owner: device.owner_id,
              runner: device.device_id,
              generation: handshake.hostGeneration,
              offered: received,
              lastSeen: Date.now(),
              verifiedAt: Date.now(),
              supportsPlugins,
              supportsPluginsV2,
              supportsCodexSessionControl: caps.includes(
                "codex_session_control",
              ),
            };
            connection = nextConnection;
            releaseUnauth(socket, clientIp);
            connections.set(nextConnection.runner, nextConnection);
            presence.markOnline(nextConnection.runner);
            clearTimeout(timeout);
            transmit(nextConnection, {
              type: "hello_ack",
              version: 3,
              received_seq: marks.received,
              // The highest command this journal still holds. A Runner that has
              // applied more than this is remembering a journal that no longer
              // exists, so it has to pull its own watermark back instead of
              // rejecting every replayed command as already-seen.
              command_seq: marks.command_seq,
            });
            pump(nextConnection);
            return;
          }
          assertCurrentConnection(connection);
          const frameNow = Date.now();
          if (
            deviceRevalidateMs <= 0 ||
            frameNow - connection.verifiedAt >= deviceRevalidateMs
          ) {
            await requireDevice(connection.owner, connection.runner);
            if (closing) return;
            assertCurrentConnection(connection);
            connection.verifiedAt = Date.now();
          }
          connection.lastSeen = frameNow;
          if (frame.type === "data") {
            if (
              typeof frame.payload !== "object" ||
              frame.payload === null ||
              Array.isArray(frame.payload)
            )
              throw new RemoteStoreError("invalid_payload");
            let ackSeq: number;
            try {
              ackSeq = content.receive(
                connection.owner,
                connection.runner,
                Number(frame.seq),
                frame.payload as Record<string, unknown>,
                connection.generation,
              );
            } catch (err) {
              if (
                err instanceof RemoteStoreError &&
                err.code === "sequence_pruned"
              ) {
                ackSeq = content.watermarks(connection.runner).received;
              } else {
                throw err;
              }
            }
            transmit(connection, {
              type: "ack",
              seq: ackSeq,
            });
          } else if (frame.type === "command_ack") {
            const seq = Number(frame.seq);
            if (seq > connection.offered)
              throw new RemoteStoreError("invalid_command_ack");
            content.acknowledgeCommands(connection.runner, seq);
            pump(connection);
          } else if (frame.type === "plugin_state") {
            const state = parsePluginStateFrame(frame, connection.runner);
            content.saveRunnerPlugins(
              connection.owner,
              connection.runner,
              state.registry_url,
              state.plugins,
            );
            pluginBroker.resolve(connection.runner, state);
          } else if (frame.type === "heartbeat") {
            transmit(connection, { type: "heartbeat_ack" });
            pump(connection);
          } else throw new RemoteStoreError("invalid_frame");
        })
        .catch((error: unknown) => {
          if (closing) return;
          closing = true;
          if (connection && connections.get(connection.runner) === connection) {
            connections.delete(connection.runner);
            pluginBroker.rejectForRunner(connection.runner, "runner_offline");
            presence.scheduleOffline(
              connection.runner,
              options?.offlineGraceMs ?? 0,
            );
          }
          const code =
            error instanceof RemoteStoreError
              ? error.code
              : "invalid_remote_frame";
          if (socket.readyState === 1)
            socket.send(JSON.stringify({ type: "error", code }));
          socket.close(1008, code);
          if (!connection) scheduleForceTerminate();
        })
        .finally(() => {
          queued--;
          queuedBytes -= bytes;
        });
    });
    socket.on("close", () => {
      closing = true;
      clearTimeout(timeout);
      if (terminateTimer !== undefined) clearTimeout(terminateTimer);
      releaseUnauth(socket, clientIp);
      sockets.delete(socket);
      // A replaced socket closes after its successor is already registered;
      // only the current connection may clear presence.
      if (connection && connections.get(connection.runner) === connection) {
        connections.delete(connection.runner);
        pluginBroker.rejectForRunner(connection.runner, "runner_offline");
        presence.scheduleOffline(
          connection.runner,
          options?.offlineGraceMs ?? 0,
        );
      }
    });
  });
  const watchdog = setInterval(() => {
    const nowMs = Date.now();
    for (const connection of connections.values()) {
      const idleMs = nowMs - connection.lastSeen;
      if (idleMs > 60_000) {
        connection.socket.terminate?.();
        connection.socket.close(1001, "heartbeat_timeout");
      } else if (idleMs > 30_000) {
        try {
          connection.socket.ping?.();
        } catch {
          // ignore send errors on closing sockets
        }
      }
    }
  }, 10_000);
  watchdog.unref();

  registerRemotePluginRoutes({
    app,
    connections,
    pluginBroker,
    listRunnerPlugins: (owner, runner) =>
      content.listRunnerPlugins(owner, runner),
    listActiveRunners: async (owner) =>
      new Set(
        (await devices.listDevices(owner))
          .filter((d) => !d.revoked)
          .map((d) => d.device_id),
      ),
    resolveOwner,
    resolveActor,
    requireDevice,
    transmit,
  });

  app.get("/v3/remote/sessions", async (request) => {
    const owner = await resolveOwner(request);
    const registeredDevices = await devices.listDevices(owner);
    for (const reg of registeredDevices) {
      if (!reg.revoked) {
        try {
          content.register(owner, reg.device_id);
        } catch {
          // Ignore runner_owner_conflict
        }
      }
    }
    const runners = content.thinRunners(owner);
    const allowed = new Set<string>();
    // One batched lookup: this runs on every console poll, so a per-runner
    // await would add a database round trip per device.
    const known = await devices.getRegisteredDevices(
      runners.map((runner) => runner.runner_id),
    );
    for (const runner of runners) {
      const device = known.get(runner.runner_id);
      if (device?.owner_id === owner && !device.revoked_at) {
        allowed.add(runner.runner_id);
      } else {
        const conn = connections.get(runner.runner_id);
        if (conn && conn.owner === owner) {
          conn.socket.close(1008, "device_revoked");
          connections.delete(runner.runner_id);
          pluginBroker.rejectForRunner(runner.runner_id, "runner_offline");
          presence.markOffline(runner.runner_id);
        } else if (!conn && (!device || device.owner_id === owner)) {
          pluginBroker.rejectForRunner(runner.runner_id, "runner_offline");
          presence.markOffline(runner.runner_id);
        }
      }
    }
    // Include any active registered device that is not yet in content.runners
    const runnerSet = new Set(runners.map((r) => r.runner_id));
    for (const reg of registeredDevices) {
      if (!reg.revoked && !runnerSet.has(reg.device_id)) {
        runners.push({
          runner_id: reg.device_id,
          hello: null,
        });
        allowed.add(reg.device_id);
      }
    }
    const rawSessions = content
      .sessions(owner)
      .filter((session) => allowed.has(session.runner_id));
    const providersByRunner = new Map<string, Map<string, string>>();
    for (const session of rawSessions) {
      if (!providersByRunner.has(session.runner_id)) {
        providersByRunner.set(
          session.runner_id,
          extractDiscoveryProviders(
            content.runnerHello(owner, session.runner_id),
          ),
        );
      }
    }
    return {
      cursor: content.cursor(owner),
      runners: runners
        .filter((runner) => allowed.has(runner.runner_id))
        .map((runner) => ({
          runner_id: runner.runner_id,
          online: isOnline(runner.runner_id),
          revoked: false,
          // Thin on purpose. `hello` carries the model catalog and the whole
          // discovery candidate list, which is what made this response half a
          // megabyte per device: the console re-reads it on every Gateway
          // push, so the payload was being rebuilt and re-sent constantly.
          // What survives is what the list itself renders; anything that needs
          // the catalog or the candidates fetches them per device.
          hello: projectRunnerHello(runner.hello),
          ...(connections.get(runner.runner_id)?.supportsCodexSessionControl
            ? { codex_session_control: true }
            : {}),
        })),
      sessions: rawSessions.map((session) => {
        const conn = connections.get(session.runner_id);
        const provider = providersByRunner
          .get(session.runner_id)
          ?.get(session.binding.provider_session_id);
        return {
          ...session,
          ...(provider ? { provider } : {}),
          online: isOnline(session.runner_id),
          binding: {
            ...session.binding,
            control_connected:
              session.binding.control_connected &&
              conn?.generation === session.binding.host_generation,
          },
        };
      }),
    };
  });
  app.get<{ Params: { runner: string; hub: string } }>(
    "/v3/remote/sessions/:runner/:hub",
    async (request) => {
      const owner = await resolveOwner(request);
      const { runner, hub } = request.params;
      const binding = content.session(owner, runner, hub);
      // Session-scoped, not owner-wide: an unrelated runner publishing a frame
      // must not look like a change to the session the reader has open, or the
      // console re-reads its whole transcript for nothing.
      const cursor = content.sessionCursor(owner, runner, hub);
      const resume = content.resumePoint(owner, runner, hub);
      if (!binding) {
        const commands = content.commandHistory(owner, runner, hub);
        if (!commands.length) throw new RemoteStoreError("session_not_found");
        return { binding: null, cursor, resume, commands };
      }
      const conn = connections.get(runner);
      const device = await devices.getRegisteredDevice(runner);
      return {
        binding: {
          ...binding,
          control_connected:
            binding.control_connected &&
            conn?.generation === binding.host_generation &&
            !device?.revoked_at,
        },
        cursor,
        resume,
        commands: content.commandHistory(owner, runner, hub),
      };
    },
  );
  /**
   * The full `hello` for one device: the model catalog and the discovery
   * candidates the session list no longer carries.
   *
   * The list is thin because it is polled; this is the opposite — fetched when
   * a screen actually needs the catalog, which is once per device rather than
   * once per Gateway push. A device that has never published a `hello` answers
   * with an error rather than an empty object, so the caller can tell "nothing
   * to show yet" from "no such device".
   *
   * Owner-scoped through the same registration check the list uses: a runner id
   * is not a capability, and answering for a device the caller does not own
   * would expose its workspaces and its model ids.
   */
  app.get<{ Params: { runner: string } }>(
    "/v3/remote/runners/:runner/hello",
    async (request) => {
      const owner = await resolveOwner(request);
      const { runner } = request.params;
      await requireDevice(owner, runner);
      const hello = content.runnerHello(owner, runner);
      if (!hello) throw new RemoteStoreError("runner_not_ready");
      return {
        runner_id: runner,
        online: isOnline(runner),
        revoked: false,
        ...(connections.get(runner)?.supportsCodexSessionControl
          ? { codex_session_control: true }
          : {}),
        hello,
      };
    },
  );
  app.post<{ Params: { runner: string } }>(
    "/v3/remote/runners/:runner/commands",
    { bodyLimit: 10 * 1024 * 1024 },
    async (request, reply) => {
      const { owner, role } = await resolveActor(request);
      const { runner } = request.params;
      await requireDevice(owner, runner);
      const command = parseRemoteCommand(request.body);
      if (
        role === "member" &&
        ((command.command.action === "connect" &&
          (command.command.sandbox === "danger-full-access" ||
            command.command.approval_policy === "never" ||
            command.command.permission_mode === "bypassPermissions")) ||
          (command.command.action === "send_prompt" &&
            command.command.permission_mode === "bypassPermissions"))
      ) {
        throw new RemoteStoreError("workspace_permission_denied");
      }
      const existing = content.existingCommand(owner, runner, command);
      if (existing !== undefined)
        return reply.status(202).send({
          status: "queued",
          seq: existing,
          client_cmd_id: command.client_cmd_id,
        });
      const conn = connections.get(runner);
      if (!conn || conn.socket.readyState !== 1)
        throw new RemoteStoreError("runner_offline");
      if (command.host_generation !== conn.generation)
        throw new RemoteStoreError("host_generation_mismatch");
      const action = command.command;
      if (
        !conn.supportsCodexSessionControl &&
        (action.action === "release_control" ||
          (action.action === "send_prompt" &&
            action.acquire_control !== undefined) ||
          (action.action === "connect" &&
            ["preview", "take_control"].includes(action.mode)))
      ) {
        throw new RemoteStoreError("runner_upgrade_required");
      }
      if (command.command.action === "connect") {
        const workspaceStatus = content.workspaceStatus(
          owner,
          runner,
          conn.generation,
          command.command.workspace_path,
        );
        if (workspaceStatus !== "ready")
          throw new RemoteStoreError(workspaceStatus);
      } else {
        const binding = content.session(owner, runner, command.hub_session_id);
        if (!binding) throw new RemoteStoreError("session_not_found");
        if (
          binding.host_generation !== conn.generation ||
          command.host_generation !== conn.generation ||
          (!binding.control_connected &&
            command.command.action !== "release_control" &&
            !(
              command.command.action === "send_prompt" &&
              command.command.acquire_control
            ))
        )
          throw new RemoteStoreError("session_control_lost");
      }
      const queued = content.submit(owner, runner, command);
      try {
        pump(conn);
      } catch (error) {
        if (
          !(error instanceof RemoteStoreError) ||
          error.code !== "runner_backpressure"
        )
          throw error;
      }
      return reply.status(202).send({
        status: "queued",
        seq: queued.seq,
        client_cmd_id: command.client_cmd_id,
      });
    },
  );
  app.get<{
    Querystring: {
      after?: string;
      runner?: string;
      hub?: string;
      limit?: string;
    };
  }>("/v3/remote/events", async (request) => {
    const owner = await resolveOwner(request);
    const { after, runner, hub, limit } = request.query;
    const cursor = Number(after ?? 0);
    if (!Number.isSafeInteger(cursor) || cursor < 0)
      throw new RemoteStoreError("invalid_cursor");
    if (hub && !runner) throw new RemoteStoreError("runner_required");
    const parsedLimit = limit ? Number(limit) : undefined;
    if (
      parsedLimit !== undefined &&
      (!Number.isSafeInteger(parsedLimit) || parsedLimit <= 0)
    )
      throw new RemoteStoreError("invalid_cursor");
    const events = content.events(owner, cursor, runner, hub, parsedLimit);
    const minCursor = content.minCursor(owner, runner);
    const truncated =
      cursor > 0 &&
      events.length === 0 &&
      minCursor !== null &&
      cursor < minCursor;
    return truncated ? { events, truncated: true } : { events };
  });
  registerRemoteStreamEndpoint(
    app,
    content,
    streams,
    ownerStreams,
    streamRevalidators,
    resolveOwner,
    () => authEpoch,
    streamAuthRevalidateMs,
  );
  app.addHook("preClose", async () => {
    for (const close of [...streams]) close(true);
    for (const socket of [...sockets]) socket.terminate();
    app.server.closeIdleConnections?.();
  });
  app.addHook("onClose", async () => {
    clearInterval(watchdog);
    presence.close();
    for (const close of [...streams]) close(true);
    for (const socket of [...sockets]) socket.terminate();
    content.close();
  });
}
