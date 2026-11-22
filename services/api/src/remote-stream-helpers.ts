import { createPublicKey, verify } from "node:crypto";
import type { FastifyInstance, FastifyRequest } from "fastify";
import { RemoteContentStore, RemoteStoreError } from "./remote-store.js";
import type { GatewayStore } from "./store.js";

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
  streams: Set<() => void>,
  ownerStreams: Map<string, Set<() => void>>,
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
      let cancelDrain: (() => void) | undefined;
      const close = (): void => {
        if (closed) return;
        closed = true;
        cancelDrain?.();
        cancelDrain = undefined;
        clearInterval(timer);
        clearInterval(heartbeat);
        unsubscribe();
        unsubscribePlugins();
        streams.delete(close);
        const set = ownerStreams.get(owner);
        if (set) {
          set.delete(close);
          if (set.size === 0) ownerStreams.delete(owner);
        }
        if (!raw.writableEnded) raw.end();
      };
      const writeChunk = async (chunk: string): Promise<boolean> => {
        if (closed || raw.destroyed || raw.writableEnded) return false;
        if (raw.write(chunk)) return true;
        return await new Promise<boolean>((resolve) => {
          const drainTimer = setTimeout(onStop, 15_000);
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
      const pump = (): void => {
        if (closed) return;
        if (busy) {
          queued = true;
          return;
        }
        if (content.cursor(owner) <= cursor) return;
        busy = true;
        void (async () => {
          const checkNow = Date.now();
          const currentEpoch = getAuthEpoch();
          if (
            streamAuthRevalidateMs <= 0 ||
            lastAuthEpoch !== currentEpoch ||
            checkNow - lastAuthCheckedAt >= streamAuthRevalidateMs
          ) {
            if ((await resolveOwner(request)) !== owner) {
              close();
              return;
            }
            lastAuthCheckedAt = Date.now();
            lastAuthEpoch = currentEpoch;
          }
          for (;;) {
            if (closed) return;
            let wrote = 0;
            for (const event of content.events(owner, cursor)) {
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
        if (
          changed === owner &&
          !closed &&
          !raw.writableNeedDrain &&
          !raw.writableEnded
        ) {
          raw.write(`event: plugins\ndata: ${JSON.stringify({ runner })}\n\n`);
        }
      });
      const timer = setInterval(pump, 10_000);
      pump();
      const heartbeat = setInterval(() => {
        if (closed || busy || raw.writableNeedDrain || raw.writableEnded)
          return;
        raw.write(": heartbeat\n\n");
      }, 10_000);
      streams.add(close);
      let set = ownerStreams.get(owner);
      if (!set) {
        set = new Set();
        ownerStreams.set(owner, set);
      }
      set.add(close);
      raw.on("close", close);
      raw.on("error", close);
    },
  );
}
