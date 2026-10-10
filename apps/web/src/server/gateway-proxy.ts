const DEFAULT_GATEWAY_ORIGIN = "http://127.0.0.1:3001";
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

function isLoopbackOrContainerHost(hostname: string): boolean {
  return (
    LOOPBACK_HOSTS.has(hostname) ||
    (!hostname.includes(".") && !hostname.startsWith("["))
  );
}

export function gatewayOrigin(
  input: string | undefined = process.env.GATEWAY_ORIGIN,
): string {
  const value = input ?? DEFAULT_GATEWAY_ORIGIN;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error("GATEWAY_ORIGIN must be a valid URL");
  }
  if (
    (url.protocol !== "https:" &&
      !(url.protocol === "http:" && isLoopbackOrContainerHost(url.hostname))) ||
    url.username !== "" ||
    url.password !== "" ||
    url.search !== "" ||
    url.hash !== "" ||
    (url.pathname !== "/" && url.pathname !== "")
  ) {
    throw new Error(
      "GATEWAY_ORIGIN must be an HTTPS origin or a loopback HTTP origin",
    );
  }
  return url.origin;
}

/**
 * Builds the only headers the same-origin proxy may forward. A configured
 * server-side token always wins; browser-provided Authorization is ignored so
 * it cannot override or exfiltrate the deployment credential.
 */
export function gatewayHeaders(
  incoming: Headers,
  accessToken: string | undefined = process.env.GATEWAY_ACCESS_TOKEN,
): Headers {
  const headers = new Headers();
  const accept = incoming.get("accept");
  const contentType = incoming.get("content-type");
  const cookie = incoming.get("cookie");
  if (accept !== null) {
    headers.set("accept", accept);
  }
  if (contentType !== null) {
    headers.set("content-type", contentType);
  }
  if (cookie !== null) {
    const sessionCookies = cookie
      .split(";")
      .map((part) => part.trim())
      .filter(
        (part) =>
          part.startsWith("agent_pulse_session=") ||
          part.startsWith("__Host-agent_pulse_session="),
      );
    if (sessionCookies.length > 0) {
      headers.set("cookie", sessionCookies.join("; "));
    }
  }
  if (accessToken !== undefined) {
    validateAccessToken(accessToken);
    headers.set("authorization", `Bearer ${accessToken}`);
  } else {
    const developmentIdentity = incoming.get("x-user-id");
    if (developmentIdentity !== null) {
      headers.set("x-user-id", developmentIdentity);
    }
  }
  return headers;
}

export async function proxyGatewayRequest(
  request: Request,
  path: readonly string[],
  options: {
    origin?: string;
    accessToken?: string | undefined;
    fetch?: typeof fetch;
    version?: "v1" | "v3";
  } = {},
): Promise<Response> {
  const requestUrl = new URL(request.url);
  const encodedPath = path.map(encodeURIComponent).join("/");
  const upstreamUrl = `${gatewayOrigin(options.origin)}/${options.version ?? "v1"}/${encodedPath}${requestUrl.search}`;
  const method = request.method.toUpperCase();
  const hasBody = method !== "GET" && method !== "HEAD";
  const accessToken =
    "accessToken" in options
      ? options.accessToken
      : process.env.GATEWAY_ACCESS_TOKEN;
  const requestInit: RequestInit = {
    method,
    headers: gatewayHeaders(request.headers, accessToken),
    cache: "no-store",
    signal: request.signal,
  };
  if (hasBody) {
    requestInit.body = await request.arrayBuffer();
  }
  let upstream: Response;
  try {
    upstream = await (options.fetch ?? fetch)(upstreamUrl, requestInit);
  } catch {
    return Response.json({ error: "gateway_unreachable" }, { status: 502 });
  }

  const setCookie = upstream.headers.get("set-cookie");
  return new Response(upstream.status === 204 ? null : upstream.body, {
    status: upstream.status,
    headers: {
      "content-type":
        upstream.headers.get("content-type") ?? "application/json",
      "cache-control": "no-store",
      ...(upstream.headers.get("www-authenticate") === null
        ? {}
        : {
            "www-authenticate":
              upstream.headers.get("www-authenticate") ?? "Bearer",
          }),
      ...(setCookie === null ? {} : { "set-cookie": setCookie }),
    },
  });
}

function validateAccessToken(value: string): void {
  const length = Buffer.byteLength(value, "utf8");
  if (length < 32 || length > 512 || /\s/u.test(value)) {
    throw new Error(
      "GATEWAY_ACCESS_TOKEN must contain 32 to 512 bytes without whitespace",
    );
  }
}
