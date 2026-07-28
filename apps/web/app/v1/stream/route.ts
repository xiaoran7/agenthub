import {
  gatewayHeaders,
  gatewayOrigin,
} from "../../../src/server/gateway-proxy";

// Never cache or pre-render: this route is an open pipe, not a document.
export const dynamic = "force-dynamic";
export const revalidate = 0;

/**
 * Streaming proxy for the gateway change stream.
 *
 * The `/v1/:path*` rewrite in next.config.mjs buffers response bodies, so an
 * SSE response proxied through it never reaches the browser (verified: no
 * frame after two heartbeats, while a direct gateway connection delivers in
 * milliseconds). A route handler hands the upstream body back as a stream, so
 * the panel keeps talking to one origin and the gateway still needs no CORS.
 */
export async function GET(request: Request): Promise<Response> {
  const headers = gatewayHeaders(request.headers);
  headers.set("accept", "text/event-stream");

  let upstream: Response;
  try {
    upstream = await fetch(`${gatewayOrigin()}/v1/stream`, {
      headers,
      cache: "no-store",
      // Closing the browser tab must close the gateway stream too, otherwise
      // abandoned subscriptions accumulate against the per-owner cap.
      signal: request.signal,
    });
  } catch {
    return Response.json({ error: "gateway_unreachable" }, { status: 502 });
  }

  if (!upstream.ok || upstream.body === null) {
    return new Response(upstream.body, {
      status: upstream.status,
      headers: {
        "content-type":
          upstream.headers.get("content-type") ?? "application/json",
        "cache-control": "no-store",
      },
    });
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "content-type": "text/event-stream",
      "cache-control": "no-store",
      "x-accel-buffering": "no",
    },
  });
}
