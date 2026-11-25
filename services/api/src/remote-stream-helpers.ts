import { createPublicKey, verify } from "node:crypto";
import { ContractError } from "@agent-pulse/contracts";
import type { FastifyInstance, FastifyReply, FastifyRequest } from "fastify";
import { AccountError } from "./accounts.js";
import { AuthenticationError } from "./auth.js";
import { RemoteContentStore, RemoteStoreError } from "./remote-store.js";
import type { GatewayStore } from "./store.js";

export function createRemoteErrorHandler(app: FastifyInstance) {
  return (error: unknown, _request: FastifyRequest, reply: FastifyReply) => {
    if (error instanceof AccountError) {
      void reply.status(error.statusCode).send({ error: error.code });
      return;
    }
    if (error instanceof AuthenticationError) {
      void reply.status(401).send({ error: "authentication_required" });
      return;
    }
    if (error instanceof ContractError) {
      void reply.status(400).send({ error: error.code });
      return;
    }
    if (error instanceof RemoteStoreError) {
      const code = error.code;
      const status =
        code === "device_not_found" ||
        code === "session_not_found" ||
        code === "plugin_not_found"
          ? 404
          : code === "runner_offline"
            ? 503
            : code === "plugin_action_timeout"
              ? 504
              : code === "stream_limit"
                ? 429
                : code === "plugin_permission_denied" ||
                    code === "workspace_permission_denied"
                  ? 403
                  : 400;
      void reply.status(status).send({ error: code });
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
        typeof error === "object" &&
        error !== null &&
        "code" in error &&
        typeof error.code === "string"
          ? error.code
          : "remote_request_failed";
      void reply.status(statusCode).send({ error: code });
      return;
    }
    app.log.error(error);
    void reply.status(500).send({ error: "internal_error" });
  };
}

export async function verifyRunnerHandshake(
  devices: GatewayStore,
  frame: Record<string, unknown>,
  nonce: string,
) {
  if (
    frame.type !== "hello" ||
    typeof frame.runner_id !== "string" ||
    typeof frame.host_generation !== "string" ||
    frame.host_generation.length > 128 ||
    typeof frame.signature !== "string" ||
    !/^[0-9a-f]{128}$/u.test(frame.signature) ||
    !Number.isSafeInteger(frame.received_command_seq)
  ) {
    throw new RemoteStoreError("invalid_handshake");
  }
  const device = await devices.getRegisteredDevice(frame.runner_id);
  if (!device || device.revoked_at) {
    throw new RemoteStoreError("device_not_found");
  }
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
  if (!verify(null, signed, key, Buffer.from(frame.signature, "hex"))) {
    throw new RemoteStoreError("invalid_device_signature");
  }
  return {
    device,
    hostGeneration: frame.host_generation,
    receivedCommandSeq: Number(frame.received_command_seq),
  };
}

export function registerRemoteStreamEndpoint(
  app: FastifyInstance,
  content: RemoteContentStore,
  streams: Set<(destroySocket?: boolean) => void>,
  ownerStreams: Map<string, Set<(destroySocket?: boolean) => void>>,
  streamRevalidators: Set<() => void>,
  resolveOwner: (request: FastifyRequest) => Promise<string>,
  getAuthEpoch: () => number,
  streamAuthRevalidateMs: number,
): void {
  app.get<{ Querystring: { after?: string } }>(
    "/v3/remote/stream",
    async (request, reply) => {
      const owner = await resolveOwner(request);
      if (request.query.after !== undefined) {
        const parsedAfter = Number(request.query.after);
        if (!Number.isSafeInteger(parsedAfter) || parsedAfter < 0)
          throw new RemoteStoreError("invalid_cursor");
      }
      let cursor =
        request.query.after === undefined
          ? content.cursor(owner)
          : Number(request.query.after);
      const ownerSet = ownerStreams.get(owner);
      if (streams.size >= 128 || (ownerSet?.size ?? 0) >= 8) {
        throw new RemoteStoreError("stream_limit");
      }
      reply.hijack();
      const raw = reply.raw;
      raw.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-store",
        "x-accel-buffering": "no",
      });
      let closed = false;
      let busy = false;
      let queued = false;
      let pendingHeartbeat = false;
      const pendingPluginRunners = new Set<string>();
      let cancelDrain: (() => void) | undefined;
      const close = (destroySocket = false): void => {
        if (closed) {
          if (destroySocket && !raw.destroyed) raw.destroy();
          return;
        }
        closed = true;
        cancelDrain?.();
        cancelDrain = undefined;
        clearInterval(timer);
        clearInterval(heartbeat);
        unsubscribe();
        unsubscribePlugins();
        streamRevalidators.delete(pump);
        streams.delete(close);
        const set = ownerStreams.get(owner);
        if (set) {
          set.delete(close);
          if (set.size === 0) ownerStreams.delete(owner);
        }
        if (!raw.writableEnded) raw.end();
        if (destroySocket && !raw.destroyed) raw.destroy();
      };
      const writeChunk = async (chunk: string): Promise<boolean> => {
        if (closed || raw.destroyed || raw.writableEnded) return false;
        if (raw.write(chunk)) return true;
        return await new Promise<boolean>((resolve) => {
          const drainTimer = setTimeout(onTimeout, 15_000);
          drainTimer.unref?.();
          function cleanup() {
            clearTimeout(drainTimer);
            raw.off("drain", onDrain);
            raw.off("close", onStop);
            raw.off("error", onStop);
            cancelDrain = undefined;
          }
          function onDrain() {
            cleanup();
            resolve(!closed && !raw.destroyed && !raw.writableEnded);
          }
          function onTimeout() {
            cleanup();
            if (!raw.destroyed) raw.destroy();
            resolve(false);
          }
          function onStop() {
            cleanup();
            resolve(false);
          }
          cancelDrain = onStop;
          raw.once("drain", onDrain);
          raw.once("close", onStop);
          raw.once("error", onStop);
        });
      };
      raw.write(`retry: 1000\n: cursor ${cursor}\n\n`);
      let lastAuthCheckedAt = Date.now();
      let lastAuthEpoch = getAuthEpoch();
      const needsAuthRevalidation = (): boolean =>
        streamAuthRevalidateMs <= 0 ||
        lastAuthEpoch !== getAuthEpoch() ||
        Date.now() - lastAuthCheckedAt >= streamAuthRevalidateMs;
      const verifyAuth = async (): Promise<boolean> => {
        const epochBefore = getAuthEpoch();
        const resolved = await resolveOwner(request);
        if (closed || resolved !== owner) {
          close();
          return false;
        }
        lastAuthCheckedAt = Date.now();
        lastAuthEpoch = epochBefore;
        return true;
      };
      const pump = (): void => {
        if (closed) return;
        if (busy) {
          queued = true;
          return;
        }
        const mustRevalidate = needsAuthRevalidation();
        const hasWork =
          content.cursor(owner) > cursor ||
          pendingPluginRunners.size > 0 ||
          pendingHeartbeat;
        if (!hasWork && !mustRevalidate) return;
        busy = true;
        void (async () => {
          if (mustRevalidate && !(await verifyAuth())) return;
          for (const pluginRunner of [...pendingPluginRunners]) {
            pendingPluginRunners.delete(pluginRunner);
            if (closed) return;
            if (lastAuthEpoch !== getAuthEpoch() && !(await verifyAuth())) {
              return;
            }
            const ok = await writeChunk(
              `event: plugins\ndata: ${JSON.stringify({ runner: pluginRunner })}\n\n`,
            );
            if (!ok) {
              close();
              return;
            }
          }
          if (pendingHeartbeat) {
            pendingHeartbeat = false;
            if (closed) return;
            if (lastAuthEpoch !== getAuthEpoch() && !(await verifyAuth())) {
              return;
            }
            if (!raw.writableNeedDrain && !raw.writableEnded) {
              raw.write(": heartbeat\n\n");
            }
          }
          if (content.cursor(owner) <= cursor) return;
          for (;;) {
            if (closed) return;
            let wrote = 0;
            for (const event of content.events(owner, cursor)) {
              if (closed) return;
              if (lastAuthEpoch !== getAuthEpoch() && !(await verifyAuth())) {
                return;
              }
              const ok = await writeChunk(
                `id: ${event.cursor}\nevent: remote\ndata: ${JSON.stringify(event)}\n\n`,
              );
              if (!ok) {
                close();
                return;
              }
              cursor = event.cursor;
              wrote += 1;
            }
            if (wrote < 256) break;
          }
        })()
          .catch(() => close())
          .finally(() => {
            busy = false;
            if (!closed && queued) {
              queued = false;
              pump();
            }
          });
      };
      const unsubscribe = content.onChange((changed) => {
        if (changed === owner) pump();
      });
      const unsubscribePlugins = content.onPluginChange((changed, runner) => {
        if (changed === owner && !closed) {
          pendingPluginRunners.add(runner);
          pump();
        }
      });
      const pollIntervalMs =
        streamAuthRevalidateMs > 0
          ? Math.min(10_000, Math.max(streamAuthRevalidateMs, 1_000))
          : 10_000;
      const timer = setInterval(pump, pollIntervalMs);
      timer.unref?.();
      const heartbeat = setInterval(() => {
        if (closed || busy || raw.writableNeedDrain || raw.writableEnded)
          return;
        if (needsAuthRevalidation()) {
          pendingHeartbeat = true;
          pump();
          return;
        }
        raw.write(": heartbeat\n\n");
      }, 10_000);
      heartbeat.unref?.();
      streamRevalidators.add(pump);
      streams.add(close);
      let set = ownerStreams.get(owner);
      if (!set) {
        set = new Set();
        ownerStreams.set(owner, set);
      }
      set.add(close);
      raw.on("close", () => close());
      raw.on("error", () => close());
      pump();
    },
  );
}
