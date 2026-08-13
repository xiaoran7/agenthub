import { createHash, timingSafeEqual } from "node:crypto";

import { ContractError } from "@agent-pulse/contracts";
import type { FastifyRequest } from "fastify";

const MIN_ACCESS_TOKEN_BYTES = 32;
const MAX_ACCESS_TOKEN_BYTES = 512;
const MAX_OWNER_ID_LENGTH = 128;

export interface OwnerAuthenticator {
  authenticate(request: FastifyRequest): string | Promise<string>;
}

/** Deny-by-default factory behaviour for callers that omit authentication. */
export class RejectingAuthenticator implements OwnerAuthenticator {
  authenticate(_request: FastifyRequest): string {
    throw new AuthenticationError();
  }
}

/**
 * Explicit local-development fallback. This must never be selected by the
 * production server unless the operator opts in.
 */
export class DevelopmentIdentityAuthenticator implements OwnerAuthenticator {
  authenticate(request: FastifyRequest): string {
    const value = request.headers["x-user-id"];
    if (typeof value !== "string" || !validOwnerId(value)) {
      throw new AuthenticationError();
    }
    return value;
  }
}

/**
 * Single-owner personal-deployment authentication.
 *
 * Only a SHA-256 digest is retained after construction. Authentication uses a
 * fixed-length timing-safe comparison so malformed and incorrect credentials
 * follow the same externally visible path.
 */
export class BearerTokenAuthenticator implements OwnerAuthenticator {
  readonly #ownerId: string;
  readonly #tokenDigest: Buffer;

  constructor(ownerId: string, accessToken: string) {
    if (!validOwnerId(ownerId)) {
      throw new Error(
        "AGENT_PLATFORM_OWNER_ID must contain 1 to 128 non-control characters",
      );
    }
    const tokenBytes = Buffer.byteLength(accessToken, "utf8");
    if (
      tokenBytes < MIN_ACCESS_TOKEN_BYTES ||
      tokenBytes > MAX_ACCESS_TOKEN_BYTES ||
      /\s/u.test(accessToken)
    ) {
      throw new Error(
        "AGENT_PLATFORM_ACCESS_TOKEN must contain 32 to 512 bytes without whitespace",
      );
    }
    this.#ownerId = ownerId;
    this.#tokenDigest = digest(accessToken);
  }

  authenticate(request: FastifyRequest): string {
    const candidate = parseBearerToken(request.headers.authorization);
    const candidateDigest = digest(candidate ?? "");
    if (
      candidate === undefined ||
      !timingSafeEqual(candidateDigest, this.#tokenDigest)
    ) {
      throw new AuthenticationError();
    }
    return this.#ownerId;
  }
}

export class AuthenticationError extends ContractError {
  constructor() {
    super("authentication_required", "a valid bearer access token is required");
    this.name = "AuthenticationError";
  }
}

function parseBearerToken(value: string | undefined): string | undefined {
  if (value === undefined || !value.startsWith("Bearer ")) {
    return undefined;
  }
  const token = value.slice("Bearer ".length);
  if (token.length === 0 || token.trim() !== token || /\s/u.test(token)) {
    return undefined;
  }
  return token;
}

function digest(value: string): Buffer {
  return createHash("sha256").update(value, "utf8").digest();
}

function validOwnerId(value: string): boolean {
  return (
    value.length > 0 &&
    value.length <= MAX_OWNER_ID_LENGTH &&
    value.trim() === value &&
    !/[\u0000-\u001f\u007f]/u.test(value)
  );
}
