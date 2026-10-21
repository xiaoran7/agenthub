import { createPublicKey, randomBytes, verify } from "node:crypto";
import { parseRemoteCommand } from "@agent-pulse/contracts";
import type { FastifyInstance, FastifyRequest } from "fastify";
import type { WebSocket } from "ws";
import { AuthenticationError, type OwnerAuthenticator } from "./auth.js";
import {
  currentSessionDigest,
  currentSessionIdentity,
  type AccountStore,
} from "./accounts.js";
import type { GatewayStore } from "./store.js";
import { RemoteContentStore, RemoteStoreError } from "./remote-store.js";

interface Connection {
  socket: WebSocket;
  owner: string;
  runner: string;
  generation: string;
  offered: number;
  lastSeen: number;
}

export interface RemoteRouteOptions {
  isBackpressured?: (socket: WebSocket) => boolean;
  /** Account store used to resolve a browser session cookie to its owner. */
  accounts?: AccountStore | undefined;
  /** Clock injection point for the session-expiry check. */
  now?: (() => Date) | undefined;
}

export function registerRemoteRoutes(
  app: FastifyInstance,
  devices: GatewayStore,
  auth: OwnerAuthenticator,
  content: RemoteContentStore,
  options: RemoteRouteOptions = {},
): void {
  const connections = new Map<string, Connection>();
  const streams = new Set<() => void>();
  const sockets = new Set<WebSocket>();
  const now = options.now ?? (() => new Date());
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
  const resolveOwner = async (request: FastifyRequest): Promise<string> => {
    if (options.accounts !== undefined) {
      const identity = await currentSessionIdentity(
        options.accounts,
        request,
        now(),
      );
      if (identity !== undefined) return identity.workspace_owner_id;
      if (currentSessionDigest(request) !== undefined) {
        throw new AuthenticationError();
      }
    }
    return auth.authenticate(request);
  };
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
      transmit(connection, { type: "command", ...item });
      connection.offered = item.seq;
    }
  };
  app.setErrorHandler((error, _request, reply) => {
    if (error instanceof AuthenticationError) {
      void reply.status(401).send({ error: "authentication_required" });
      return;
    }
    const code =
      error instanceof RemoteStoreError ? error.code : "remote_request_failed";
    void reply
      .status(
        code === "device_not_found" || code === "session_not_found" ? 404 : 400,
      )
      .send({ error: code });
  });
  app.get("/v3/remote/ws", { websocket: true }, (socket) => {
    sockets.add(socket);
    const nonce = randomBytes(32).toString("hex");
    let connection: Connection | undefined;
    let chain = Promise.resolve();
    let queued = 0;
    let queuedBytes = 0;
    socket.send(JSON.stringify({ type: "challenge", version: 3, nonce }));
    const timeout = setTimeout(
      () => socket.close(1008, "handshake_timeout"),
      10_000,
    );
    socket.on("message", (data) => {
      const serialized = data.toString();
      const bytes = Buffer.byteLength(serialized);
      queuedBytes += bytes;
      if (
        ++queued > 128 ||
        queuedBytes > 32 * 1024 * 1024 ||
        bytes > (connection ? 17 * 1024 * 1024 : 16 * 1024)
      ) {
        socket.close(1009, "frame_too_large");
        return;
      }
      chain = chain
        .then(async () => {
          const frame = JSON.parse(serialized) as Record<string, unknown>;
          if (!connection) {
            if (
              frame.type !== "hello" ||
              typeof frame.runner_id !== "string" ||
              typeof frame.host_generation !== "string" ||
              frame.host_generation.length > 128 ||
              typeof frame.signature !== "string" ||
              !/^[0-9a-f]{128}$/u.test(frame.signature) ||
              !Number.isSafeInteger(frame.received_command_seq)
            )
              throw new RemoteStoreError("invalid_handshake");
            const device = await devices.getRegisteredDevice(frame.runner_id);
            if (!device || device.revoked_at)
              throw new RemoteStoreError("device_not_found");
            const signed = Buffer.from(
              `agenthub.remote.v3\n${device.device_id}\n${nonce}\n${frame.host_generation}\n${String(frame.received_command_seq)}`,
            );
            const key = createPublicKey({
              key: Buffer.concat([
                Buffer.from("302a300506032b6570032100", "hex"),
                Buffer.from(device.public_key, "hex"),
              ]),
              format: "der",
              type: "spki",
            });
            if (!verify(null, signed, key, Buffer.from(frame.signature, "hex")))
              throw new RemoteStoreError("invalid_device_signature");
            content.register(device.owner_id, device.device_id);
            const marks = content.watermarks(device.device_id);
            const received = Number(frame.received_command_seq);
            if (
              !Number.isSafeInteger(received) ||
              received < 0 ||
              received > marks.command_seq
            )
              throw new RemoteStoreError("command_journal_mismatch");
            if (received < marks.command_ack) {
              content.acknowledgeCommands(device.device_id, received, true);
            }
            const previous = connections.get(device.device_id);
            if (previous) previous.socket.close(1000, "connection_replaced");
            connection = {
              socket,
              owner: device.owner_id,
              runner: device.device_id,
              generation: frame.host_generation,
              offered: received,
              lastSeen: Date.now(),
            };
            connections.set(connection.runner, connection);
            clearTimeout(timeout);
            transmit(connection, {
              type: "hello_ack",
              version: 3,
              received_seq: marks.received,
            });
            pump(connection);
            return;
          }
          assertCurrentConnection(connection);
          await requireDevice(connection.owner, connection.runner);
          assertCurrentConnection(connection);
          connection.lastSeen = Date.now();
          if (frame.type === "data") {
            if (
              typeof frame.payload !== "object" ||
              frame.payload === null ||
              Array.isArray(frame.payload)
            )
              throw new RemoteStoreError("invalid_payload");
            try {
              content.receive(
                connection.owner,
                connection.runner,
                Number(frame.seq),
                frame.payload as Record<string, unknown>,
                connection.generation,
              );
            } catch (err) {
              if (
                err instanceof RemoteStoreError &&
                err.code === "sequence_conflict" &&
                Number(frame.seq) <=
                  content.watermarks(connection.runner).received
              ) {
                // Runner replayed an older sequence whose payload differed from Gateway canonical log
                // (e.g. after restart or generation rollover). Do not terminate the connection.
                // Transmit ACK of the current watermark so the runner can advance past it.
              } else {
                throw err;
              }
            }
            transmit(connection, {
              type: "ack",
              seq: content.watermarks(connection.runner).received,
            });
          } else if (frame.type === "command_ack") {
            const seq = Number(frame.seq);
            if (seq > connection.offered)
              throw new RemoteStoreError("invalid_command_ack");
            content.acknowledgeCommands(connection.runner, seq);
            pump(connection);
          } else if (frame.type === "heartbeat") {
            transmit(connection, { type: "heartbeat_ack" });
            pump(connection);
          } else throw new RemoteStoreError("invalid_frame");
        })
        .catch((error: unknown) => {
          const code =
            error instanceof RemoteStoreError
              ? error.code
              : "invalid_remote_frame";
          if (socket.readyState === 1)
            socket.send(JSON.stringify({ type: "error", code }));
          socket.close(1008, code);
        })
        .finally(() => {
          queued--;
          queuedBytes -= bytes;
        });
    });
    socket.on("close", () => {
      clearTimeout(timeout);
      sockets.delete(socket);
      if (connection && connections.get(connection.runner) === connection)
        connections.delete(connection.runner);
    });
  });
  const watchdog = setInterval(() => {
    for (const connection of connections.values())
      if (Date.now() - connection.lastSeen > 60_000)
        connection.socket.close(1001, "heartbeat_timeout");
  }, 10_000);
  watchdog.unref();

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
    const runners = content.runners(owner);
    const allowed = new Set<string>();
    // One batched lookup: this runs on every console poll, so a per-runner
    // await would add a database round trip per device.
    const known = await devices.getRegisteredDevices(
      runners.map((runner) => runner.runner_id),
    );
    for (const runner of runners) {
      const device = known.get(runner.runner_id);
      if (device?.owner_id === owner && !device.revoked_at)
        allowed.add(runner.runner_id);
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
    return {
      cursor: content.cursor(owner),
      runners: runners.map((runner) => ({
        ...runner,
        online:
          connections.has(runner.runner_id) && allowed.has(runner.runner_id),
        revoked: !allowed.has(runner.runner_id),
      })),
      sessions: content.sessions(owner).map((session) => {
        const conn = connections.get(session.runner_id);
        return {
          ...session,
          online: !!conn && allowed.has(session.runner_id),
          binding: {
            ...session.binding,
            control_connected:
              session.binding.control_connected &&
              conn?.generation === session.binding.host_generation &&
              allowed.has(session.runner_id),
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
      if (!binding) {
        const commands = content.commandHistory(owner, runner, hub);
        if (!commands.length) throw new RemoteStoreError("session_not_found");
        return { binding: null, cursor: content.cursor(owner), commands };
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
        cursor: content.cursor(owner),
        commands: content.commandHistory(owner, runner, hub),
      };
    },
  );
  app.post<{ Params: { runner: string } }>(
    "/v3/remote/runners/:runner/commands",
    async (request, reply) => {
      const owner = await resolveOwner(request);
      const { runner } = request.params;
      await requireDevice(owner, runner);
      const command = parseRemoteCommand(request.body);
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
          !binding.control_connected
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
  app.get<{ Querystring: { after?: string; runner?: string; hub?: string } }>(
    "/v3/remote/events",
    async (request) => {
      const owner = await resolveOwner(request);
      const { after, runner, hub } = request.query;
      const cursor = Number(after ?? 0);
      if (!Number.isSafeInteger(cursor) || cursor < 0)
        throw new RemoteStoreError("invalid_cursor");
      if (hub && !runner) throw new RemoteStoreError("runner_required");
      return { events: content.events(owner, cursor, runner, hub) };
    },
  );
  app.get<{ Querystring: { after?: string } }>(
    "/v3/remote/stream",
    async (request, reply) => {
      const owner = await resolveOwner(request);
      let cursor = Number(request.query.after ?? 0);
      if (!Number.isSafeInteger(cursor) || cursor < 0)
        throw new RemoteStoreError("invalid_cursor");
      if (streams.size >= 128) throw new RemoteStoreError("stream_limit");
      reply.hijack();
      const raw = reply.raw;
      raw.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-store",
        "x-accel-buffering": "no",
      });
      raw.write("retry: 1000\n\n");
      let busy = false;
      const timer = setInterval(() => {
        if (busy) return;
        busy = true;
        void (async () => {
          // Re-check the account cookie so revocation also closes content streams.
          if ((await resolveOwner(request)) !== owner) {
            close();
            return;
          }
          for (const event of content.events(owner, cursor)) {
            if (
              !raw.write(
                `id: ${event.cursor}\nevent: remote\ndata: ${JSON.stringify(event)}\n\n`,
              )
            ) {
              close();
              return;
            }
            cursor = event.cursor;
          }
        })()
          .catch(() => close())
          .finally(() => {
            busy = false;
          });
      }, 250);
      const heartbeat = setInterval(() => {
        if (!raw.write(": heartbeat\n\n")) close();
      }, 10_000);
      const close = (): void => {
        clearInterval(timer);
        clearInterval(heartbeat);
        streams.delete(close);
        if (!raw.writableEnded) raw.end();
      };
      streams.add(close);
      raw.on("close", close);
      raw.on("error", close);
    },
  );
  app.addHook("onClose", async () => {
    clearInterval(watchdog);
    for (const close of streams) close();
    for (const socket of sockets) socket.terminate();
    content.close();
  });
}
