import { isIP } from "node:net";

import { ContractError } from "@agent-pulse/contracts";
import type { FastifyRequest } from "fastify";
import { AccountError } from "./accounts.js";
import type { PushSubscription } from "./push.js";

const MAX_PUSH_ENDPOINT_LENGTH = 2_048;

const DEFAULT_ALLOWED_PUSH_EXACT_HOSTS = new Set([
  "fcm.googleapis.com",
  "android.googleapis.com",
  "updates.push.services.mozilla.com",
  "updates-autopush.stage.mozaws.net",
  "updates-autopush.dev.mozaws.net",
  "push.apple.com",
]);

const DEFAULT_ALLOWED_PUSH_SUFFIXES = [
  ".push.services.mozilla.com",
  ".notify.windows.com",
  ".wns.windows.com",
  ".push.apple.com",
];

export function allowedPushHostsFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): string[] {
  const raw = env.AGENT_PLATFORM_ALLOWED_PUSH_HOSTS;
  if (!raw) return [];
  return raw
    .split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter((entry) => entry.length > 0);
}

function hostMatchesAllowlist(
  hostname: string,
  extraAllowedHosts: readonly string[],
): boolean {
  if (DEFAULT_ALLOWED_PUSH_EXACT_HOSTS.has(hostname)) return true;
  if (
    DEFAULT_ALLOWED_PUSH_SUFFIXES.some(
      (suffix) => hostname.endsWith(suffix) && hostname.length > suffix.length,
    )
  ) {
    return true;
  }
  for (const allowed of extraAllowedHosts) {
    const normalized = allowed.toLowerCase();
    if (normalized.startsWith(".")) {
      if (
        hostname.endsWith(normalized) &&
        hostname.length > normalized.length
      ) {
        return true;
      }
    } else if (hostname === normalized) {
      return true;
    }
  }
  return false;
}

/**
 * Validates that a Web Push subscription endpoint is a well-formed HTTPS URL
 * pointing at a recognized browser push service rather than internal/SSRF targets.
 */
export function parsePushEndpoint(
  value: unknown,
  extraAllowedHosts: readonly string[] = [],
): string {
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
  const hostname = url.hostname.toLowerCase();
  if (
    url.protocol !== "https:" ||
    url.port !== "" ||
    url.username !== "" ||
    url.password !== "" ||
    url.hash !== "" ||
    !hostname ||
    hostname.endsWith(".") ||
    hostname.startsWith("[") ||
    isIP(hostname) !== 0 ||
    !hostname.includes(".") ||
    hostname === "localhost" ||
    hostname.endsWith(".localhost") ||
    hostname.endsWith(".local") ||
    hostname.endsWith(".internal") ||
    !hostMatchesAllowlist(hostname, extraAllowedHosts)
  ) {
    throw new ContractError("invalid_endpoint", "endpoint is not allowed");
  }
  return url.toString();
}

function hasSessionCookieHeader(
  request: FastifyRequest,
  secureCookies: boolean,
): boolean {
  const cookie = request.headers.cookie;
  if (typeof cookie !== "string" || cookie.length === 0) return false;
  for (const segment of cookie.split(";")) {
    const sep = segment.indexOf("=");
    if (sep < 0) continue;
    const name = segment.slice(0, sep).trim();
    if (name === "__Host-agent_pulse_session") return true;
    if (!secureCookies && name === "agent_pulse_session") return true;
  }
  return false;
}

function rejectCsrf(): never {
  throw new AccountError("csrf_rejected", "cross-origin mutation denied", 403);
}

/**
 * Blocks browser cross-site / sibling-site CSRF on cookie-authenticated mutations.
 */
export function enforceCookieCsrf(
  request: FastifyRequest,
  secureCookies: boolean,
  publicBaseUrl?: string,
): void {
  const method = request.method.toUpperCase();
  if (method === "GET" || method === "HEAD" || method === "OPTIONS") return;
  const authHeader = request.headers.authorization;
  if (
    typeof authHeader === "string" &&
    authHeader.toLowerCase().startsWith("bearer ")
  ) {
    return;
  }
  if (!hasSessionCookieHeader(request, secureCookies)) return;

  const contentTypeRaw = request.headers["content-type"];
  if (typeof contentTypeRaw === "string") {
    const mediaType = contentTypeRaw.split(";")[0]?.trim().toLowerCase() ?? "";
    if (
      mediaType === "application/x-www-form-urlencoded" ||
      mediaType === "multipart/form-data" ||
      mediaType === "text/plain"
    ) {
      rejectCsrf();
    }
  }

  const fetchSite = request.headers["sec-fetch-site"];
  if (fetchSite !== undefined) {
    if (
      typeof fetchSite !== "string" ||
      (fetchSite !== "same-origin" && fetchSite !== "none")
    ) {
      rejectCsrf();
    }
  }

  const allowedHosts = new Set<string>();
  if (typeof request.headers.host === "string" && request.headers.host) {
    allowedHosts.add(request.headers.host.toLowerCase());
  }
  if (typeof request.host === "string" && request.host) {
    allowedHosts.add(request.host.toLowerCase());
  }
  if (publicBaseUrl) {
    try {
      allowedHosts.add(new URL(publicBaseUrl).host.toLowerCase());
    } catch {
      // Ignore malformed optional public base URL.
    }
  }

  const origin = request.headers.origin;
  if (origin !== undefined) {
    if (typeof origin !== "string" || origin === "null") rejectCsrf();
    let parsed: URL;
    try {
      parsed = new URL(origin);
    } catch {
      rejectCsrf();
    }
    if (!allowedHosts.has(parsed.host.toLowerCase())) {
      rejectCsrf();
    }
    return;
  }

  const referer = request.headers.referer;
  if (typeof referer === "string" && referer.length > 0) {
    let parsed: URL;
    try {
      parsed = new URL(referer);
    } catch {
      rejectCsrf();
    }
    if (!allowedHosts.has(parsed.host.toLowerCase())) {
      rejectCsrf();
    }
  }
}

/**
 * Bounds concurrent `scrypt` key derivations so login bursts cannot saturate
 * libuv's worker pool across the API process.
 */
export class ScryptSemaphore {
  readonly #maxConcurrent: number;
  #active = 0;
  readonly #waiters: Array<() => void> = [];

  constructor(maxConcurrent = 8) {
    this.#maxConcurrent = Math.max(1, maxConcurrent);
  }

  async run<T>(task: () => Promise<T>): Promise<T> {
    if (this.#active >= this.#maxConcurrent) {
      await new Promise<void>((resolve) => {
        this.#waiters.push(resolve);
      });
    }
    this.#active += 1;
    try {
      return await task();
    } finally {
      this.#active -= 1;
      const next = this.#waiters.shift();
      if (next !== undefined) next();
    }
  }
}

export const scryptSemaphore = new ScryptSemaphore(8);

export function resolveTrustProxyConfig(
  host: string,
  rawHops: string | undefined,
  rawAddresses?: string,
): false | string[] {
  if (rawHops !== undefined && rawHops.trim() !== "") {
    const parsed = Number(rawHops.trim());
    if (!Number.isSafeInteger(parsed) || parsed < 0 || parsed > 8) {
      throw new Error("AGENT_PULSE_TRUST_PROXY_HOPS must be an integer 0..8");
    }
    if (parsed === 0) return false;
  }
  const addresses = (rawAddresses ?? "")
    .split(",")
    .map((address) => address.trim())
    .filter(Boolean);
  if (addresses.length > 0) return addresses;
  const normalized = host.trim().toLowerCase();
  const loopback =
    normalized === "localhost" ||
    normalized === "::1" ||
    (isIP(normalized) === 4 && normalized.startsWith("127."));
  return loopback ? ["127.0.0.1/8", "::1/128"] : false;
}

export async function mapWithConcurrency<T, R>(
  items: readonly T[],
  limit: number,
  worker: (item: T) => Promise<R>,
): Promise<R[]> {
  if (items.length === 0) return [];
  const results = new Array<R>(items.length);
  let cursor = 0;
  const workerCount = Math.min(Math.max(1, limit), items.length);
  await Promise.all(
    Array.from({ length: workerCount }, async () => {
      for (;;) {
        const index = cursor;
        cursor += 1;
        if (index >= items.length) return;
        results[index] = await worker(items[index]!);
      }
    }),
  );
  return results;
}

const DEFAULT_BINDING_TTL_SECONDS = 600;
const MIN_BINDING_TTL_SECONDS = 60;
const MAX_BINDING_TTL_SECONDS = 3_600;

export function expectBodyObject(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError("invalid_body", "request body must be an object");
  }
  return value as Record<string, unknown>;
}

export function parseBindingTtlSeconds(value: unknown): number {
  if (value === undefined || value === null) return DEFAULT_BINDING_TTL_SECONDS;
  const body = expectBodyObject(value);
  if (Object.keys(body).some((key) => key !== "expires_in_seconds")) {
    throw new ContractError("unknown_field", "body field is invalid");
  }
  if (body.expires_in_seconds === undefined) return DEFAULT_BINDING_TTL_SECONDS;
  if (
    typeof body.expires_in_seconds !== "number" ||
    !Number.isSafeInteger(body.expires_in_seconds) ||
    body.expires_in_seconds < MIN_BINDING_TTL_SECONDS ||
    body.expires_in_seconds > MAX_BINDING_TTL_SECONDS
  ) {
    throw new ContractError(
      "invalid_binding_ttl",
      "binding lifetime is invalid",
    );
  }
  return body.expires_in_seconds;
}

function parsePushKeyMaterial(
  value: unknown,
  field: string,
  expectedBytes: number,
): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]{1,200}$/.test(value)) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  const decoded = Buffer.from(value, "base64url");
  if (
    decoded.length !== expectedBytes ||
    (field === "p256dh" && decoded[0] !== 0x04)
  ) {
    throw new ContractError("invalid_subscription", `${field} is invalid`);
  }
  return value;
}

export function parsePushSubscriptionPayload(
  value: unknown,
  extraAllowedHosts: readonly string[] = [],
): PushSubscription {
  const body = expectBodyObject(value);
  if (
    Object.keys(body).some(
      (key) => !["endpoint", "keys", "expirationTime"].includes(key),
    )
  ) {
    throw new ContractError("unknown_field", "body field is invalid");
  }
  const keys = expectBodyObject(body.keys);
  if (Object.keys(keys).some((key) => !["p256dh", "auth"].includes(key))) {
    throw new ContractError("unknown_field", "keys field is invalid");
  }
  return {
    endpoint: parsePushEndpoint(body.endpoint, extraAllowedHosts),
    p256dh: parsePushKeyMaterial(keys.p256dh, "p256dh", 65),
    auth: parsePushKeyMaterial(keys.auth, "auth", 16),
  };
}

export function serializeCookieHeader(
  name: string,
  value: string,
  expiresAt: Date,
  maxAgeSeconds: number,
  secure: boolean,
): string {
  return [
    `${name}=${value}`,
    "Path=/",
    "HttpOnly",
    "SameSite=Lax",
    `Expires=${expiresAt.toUTCString()}`,
    `Max-Age=${maxAgeSeconds}`,
    ...(secure ? ["Secure"] : []),
  ].join("; ");
}

export function extractSessionToken(
  request: FastifyRequest,
  localName: string,
  secureName: string,
  secure: boolean,
): string | undefined {
  const cookie = request.headers.cookie;
  if (cookie === undefined) return undefined;
  let plainToken: string | undefined;
  for (const segment of cookie.split(";")) {
    const separator = segment.indexOf("=");
    if (separator < 0) continue;
    const name = segment.slice(0, separator).trim();
    const token = segment.slice(separator + 1);
    if (!/^[A-Za-z0-9_-]{43}$/.test(token)) continue;
    if (name === secureName) return token;
    if (!secure && name === localName && plainToken === undefined) {
      plainToken = token;
    }
  }
  return plainToken;
}
