import {
  createECDH,
  createCipheriv,
  createPrivateKey,
  hkdfSync,
  randomBytes,
  sign,
  type ECDH,
} from "node:crypto";

/**
 * Web Push delivery: RFC 8188 aes128gcm content encoding, RFC 8291 key
 * derivation and RFC 8292 VAPID authorization, implemented on node:crypto.
 *
 * Hand-rolled rather than pulled from npm to keep the gateway's dependency
 * surface at fastify + postgres; the encryption is pinned by the RFC 8291
 * example vector in the tests, so a mistake cannot pass silently.
 */
export interface PushSubscription {
  endpoint: string;
  /** Receiver P-256 public key, base64url, uncompressed point (65 bytes). */
  p256dh: string;
  /** Receiver auth secret, base64url (16 bytes). */
  auth: string;
}

export interface VapidKeys {
  /** base64url uncompressed P-256 point (65 bytes). */
  publicKey: string;
  /** base64url private scalar (32 bytes). */
  privateKey: string;
  /** `mailto:` or `https:` contact for the push service operator. */
  subject: string;
}

export type PushOutcome = "delivered" | "gone" | "failed";

/** Single-record content encoding: the whole payload fits in one record. */
const RECORD_SIZE = 4_096;
const MAX_PAYLOAD_BYTES = 3_000;
const VAPID_TTL_SECONDS = 12 * 60 * 60;
/** How long the push service should retain an undelivered notification. */
const PUSH_TTL_SECONDS = 300;

export function encryptPushPayload(
  payload: string,
  subscription: Pick<PushSubscription, "p256dh" | "auth">,
  overrides: { salt?: Buffer; ephemeral?: ECDH } = {},
): Buffer {
  const receiverKey = decodeBase64Url(subscription.p256dh, "p256dh");
  if (receiverKey.length !== 65 || receiverKey[0] !== 0x04) {
    throw new PushError("invalid_subscription", "p256dh is not a P-256 point");
  }
  const authSecret = decodeBase64Url(subscription.auth, "auth");
  if (authSecret.length !== 16) {
    throw new PushError("invalid_subscription", "auth secret must be 16 bytes");
  }
  const plaintext = Buffer.from(payload, "utf8");
  if (plaintext.length > MAX_PAYLOAD_BYTES) {
    throw new PushError("payload_too_large", "push payload is too large");
  }

  const ephemeral = overrides.ephemeral ?? createECDH("prime256v1");
  if (overrides.ephemeral === undefined) {
    ephemeral.generateKeys();
  }
  const senderKey = ephemeral.getPublicKey();
  const sharedSecret = ephemeral.computeSecret(receiverKey);
  const salt = overrides.salt ?? randomBytes(16);

  // RFC 8291 section 3.4: the auth secret is the extract salt, and the info
  // string binds the derived key to both parties' public keys.
  const keyInfo = Buffer.concat([
    Buffer.from("WebPush: info\0", "utf8"),
    receiverKey,
    senderKey,
  ]);
  const ikm = Buffer.from(
    hkdfSync("sha256", sharedSecret, authSecret, keyInfo, 32),
  );
  const contentEncryptionKey = Buffer.from(
    hkdfSync(
      "sha256",
      ikm,
      salt,
      Buffer.from("Content-Encoding: aes128gcm\0", "utf8"),
      16,
    ),
  );
  const nonce = Buffer.from(
    hkdfSync(
      "sha256",
      ikm,
      salt,
      Buffer.from("Content-Encoding: nonce\0", "utf8"),
      12,
    ),
  );

  const cipher = createCipheriv("aes-128-gcm", contentEncryptionKey, nonce);
  // 0x02 is the delimiter for the last record; there is only one record here.
  const ciphertext = Buffer.concat([
    cipher.update(Buffer.concat([plaintext, Buffer.from([0x02])])),
    cipher.final(),
    cipher.getAuthTag(),
  ]);

  const header = Buffer.alloc(21);
  salt.copy(header, 0);
  header.writeUInt32BE(RECORD_SIZE, 16);
  header.writeUInt8(senderKey.length, 20);
  return Buffer.concat([header, senderKey, ciphertext]);
}

export function vapidAuthorization(
  endpoint: string,
  keys: VapidKeys,
  now: Date = new Date(),
): string {
  const audience = new URL(endpoint).origin;
  const header = encodeBase64Url(
    Buffer.from(JSON.stringify({ typ: "JWT", alg: "ES256" }), "utf8"),
  );
  const claims = encodeBase64Url(
    Buffer.from(
      JSON.stringify({
        aud: audience,
        exp: Math.floor(now.getTime() / 1_000) + VAPID_TTL_SECONDS,
        sub: keys.subject,
      }),
      "utf8",
    ),
  );
  const signingInput = `${header}.${claims}`;
  const signature = sign(
    "sha256",
    Buffer.from(signingInput, "utf8"),
    // ES256 signatures are raw R||S, not the DER encoding node emits by default.
    { key: vapidPrivateKey(keys), dsaEncoding: "ieee-p1363" },
  );
  return `vapid t=${signingInput}.${encodeBase64Url(signature)}, k=${keys.publicKey}`;
}

export interface PushSender {
  send(subscription: PushSubscription, payload: string): Promise<PushOutcome>;
}

export class WebPushSender implements PushSender {
  readonly #keys: VapidKeys;
  readonly #fetch: typeof fetch;

  constructor(keys: VapidKeys, fetchImplementation: typeof fetch = fetch) {
    this.#keys = keys;
    this.#fetch = fetchImplementation;
  }

  async send(
    subscription: PushSubscription,
    payload: string,
  ): Promise<PushOutcome> {
    let body: Buffer;
    try {
      body = encryptPushPayload(payload, subscription);
    } catch {
      // A subscription we cannot encrypt for is unusable; drop it like a
      // subscription the push service has already forgotten.
      return "gone";
    }
    let response: Response;
    try {
      response = await this.#fetch(subscription.endpoint, {
        method: "POST",
        headers: {
          authorization: vapidAuthorization(subscription.endpoint, this.#keys),
          "content-encoding": "aes128gcm",
          "content-type": "application/octet-stream",
          ttl: String(PUSH_TTL_SECONDS),
          urgency: "high",
        },
        body: new Uint8Array(body),
      });
    } catch {
      return "failed";
    }
    if (response.status === 404 || response.status === 410) {
      return "gone";
    }
    return response.ok ? "delivered" : "failed";
  }
}

export class PushError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "PushError";
    this.code = code;
  }
}

/**
 * Reads VAPID configuration from the environment. Returns undefined when no
 * keys are configured, which leaves push disabled rather than half-enabled.
 */
export function vapidKeysFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): VapidKeys | undefined {
  const publicKey = env.AGENT_PLATFORM_VAPID_PUBLIC_KEY;
  const privateKey = env.AGENT_PLATFORM_VAPID_PRIVATE_KEY;
  const subject = env.AGENT_PLATFORM_VAPID_SUBJECT;
  if (
    publicKey === undefined ||
    privateKey === undefined ||
    subject === undefined
  ) {
    return undefined;
  }
  const keys = { publicKey, privateKey, subject };
  // Fail at startup instead of on the first undeliverable notification.
  vapidPrivateKey(keys);
  if (!/^(mailto:|https:)/.test(subject)) {
    throw new PushError(
      "invalid_vapid_subject",
      "VAPID subject must be a mailto: or https: URL",
    );
  }
  return keys;
}

function vapidPrivateKey(keys: VapidKeys): ReturnType<typeof createPrivateKey> {
  const publicKey = decodeBase64Url(keys.publicKey, "vapid public key");
  if (publicKey.length !== 65 || publicKey[0] !== 0x04) {
    throw new PushError(
      "invalid_vapid_key",
      "VAPID public key must be an uncompressed P-256 point",
    );
  }
  const privateKey = decodeBase64Url(keys.privateKey, "vapid private key");
  if (privateKey.length !== 32) {
    throw new PushError(
      "invalid_vapid_key",
      "VAPID private key must be a 32 byte scalar",
    );
  }
  return createPrivateKey({
    key: {
      kty: "EC",
      crv: "P-256",
      x: encodeBase64Url(publicKey.subarray(1, 33)),
      y: encodeBase64Url(publicKey.subarray(33, 65)),
      d: encodeBase64Url(privateKey),
    },
    format: "jwk",
  });
}

export function encodeBase64Url(value: Buffer): string {
  return value.toString("base64url");
}

function decodeBase64Url(value: string, field: string): Buffer {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new PushError("invalid_subscription", `${field} is not base64url`);
  }
  return Buffer.from(value, "base64url");
}
