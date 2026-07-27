import assert from "node:assert/strict";
import {
  createDecipheriv,
  createECDH,
  createPublicKey,
  hkdfSync,
  verify,
} from "node:crypto";
import test from "node:test";

import {
  encodeBase64Url,
  encryptPushPayload,
  vapidAuthorization,
  vapidKeysFromEnv,
  WebPushSender,
  type PushSubscription,
} from "../src/push.js";

// RFC 8291 section 5, "Push Message Encryption Example".
const rfcExample = {
  plaintext: "When I grow up, I want to be a watermelon",
  uaPublic:
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
  uaPrivate: "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94",
  authSecret: "BTBZMqHH6r4Tts7J_aSIgg",
  asPrivate: "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw",
  asPublic:
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8",
  salt: "DGv6ra1nlYgDCS1FRnbzlw",
  body:
    "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vC" +
    "YLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXL" +
    "WyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
};

test("aes128gcm encryption reproduces the RFC 8291 example", () => {
  const ephemeral = createECDH("prime256v1");
  ephemeral.setPrivateKey(Buffer.from(rfcExample.asPrivate, "base64url"));
  assert.equal(
    ephemeral.getPublicKey().toString("base64url"),
    rfcExample.asPublic,
    "the ephemeral key pair must match the vector before comparing output",
  );

  const body = encryptPushPayload(
    rfcExample.plaintext,
    { p256dh: rfcExample.uaPublic, auth: rfcExample.authSecret },
    { salt: Buffer.from(rfcExample.salt, "base64url"), ephemeral },
  );
  assert.equal(body.toString("base64url"), rfcExample.body);
});

test("a random encryption round-trips through the receiver's key", () => {
  const receiver = createECDH("prime256v1");
  receiver.generateKeys();
  const authSecret = Buffer.from("0123456789abcdef", "utf8");
  const subscription = {
    p256dh: receiver.getPublicKey().toString("base64url"),
    auth: authSecret.toString("base64url"),
  };
  const payload = JSON.stringify({ activity_status: "WAITING_PERMISSION" });

  const first = encryptPushPayload(payload, subscription);
  const second = encryptPushPayload(payload, subscription);
  assert.notEqual(
    first.toString("base64url"),
    second.toString("base64url"),
    "salt and ephemeral key must be fresh per message",
  );
  assert.equal(decryptPushBody(first, receiver, authSecret), payload);
  assert.equal(decryptPushBody(second, receiver, authSecret), payload);
});

test("malformed subscription keys are rejected before any network call", () => {
  const receiver = createECDH("prime256v1");
  receiver.generateKeys();
  assert.throws(
    () =>
      encryptPushPayload("x", {
        p256dh: receiver.getPublicKey().toString("base64url"),
        auth: "c2hvcnQ",
      }),
    /auth secret must be 16 bytes/,
  );
  assert.throws(
    () =>
      encryptPushPayload("x", {
        p256dh: "AAAA",
        auth: "AAAAAAAAAAAAAAAAAAAAAA",
      }),
    /p256dh is not a P-256 point/,
  );
  assert.throws(
    () =>
      encryptPushPayload("x", {
        p256dh: "not base64url!",
        auth: "AAAAAAAAAAAAAAAAAAAAAA",
      }),
    /p256dh is not base64url/,
  );
});

test("VAPID authorization is a verifiable ES256 JWT scoped to the endpoint", () => {
  const keys = generateVapidKeys();
  const header = vapidAuthorization(
    "https://push.example/subscription/abc?token=1",
    keys,
    new Date("2026-07-27T00:00:00Z"),
  );
  const match = /^vapid t=([^,]+), k=(.+)$/.exec(header);
  assert.ok(match, "header must carry both the token and the public key");
  const [, token, publicKey] = match;
  assert.equal(publicKey, keys.publicKey);
  assert.ok(token !== undefined);

  const parts = token.split(".");
  assert.equal(parts.length, 3);
  const [encodedHeader, encodedClaims, encodedSignature] = parts as [
    string,
    string,
    string,
  ];
  assert.deepEqual(
    JSON.parse(Buffer.from(encodedHeader, "base64url").toString("utf8")),
    { typ: "JWT", alg: "ES256" },
  );
  const claims = JSON.parse(
    Buffer.from(encodedClaims, "base64url").toString("utf8"),
  );
  assert.deepEqual(claims, {
    // The audience is the endpoint origin only: paths and tokens must not leak
    // into a header the push service logs.
    aud: "https://push.example",
    exp: Math.floor(Date.parse("2026-07-27T12:00:00Z") / 1_000),
    sub: keys.subject,
  });

  const publicKeyBytes = Buffer.from(keys.publicKey, "base64url");
  const verified = verify(
    "sha256",
    Buffer.from(`${encodedHeader}.${encodedClaims}`, "utf8"),
    {
      key: createPublicKey({
        key: {
          kty: "EC",
          crv: "P-256",
          x: encodeBase64Url(publicKeyBytes.subarray(1, 33)),
          y: encodeBase64Url(publicKeyBytes.subarray(33, 65)),
        },
        format: "jwk",
      }),
      dsaEncoding: "ieee-p1363",
    },
    Buffer.from(encodedSignature, "base64url"),
  );
  assert.ok(verified, "the push service must be able to verify the token");
});

test("the sender reports gone for retired subscriptions and failed for errors", async () => {
  const keys = generateVapidKeys();
  const receiver = createECDH("prime256v1");
  receiver.generateKeys();
  const subscription: PushSubscription = {
    endpoint: "https://push.example/subscription/abc",
    p256dh: receiver.getPublicKey().toString("base64url"),
    auth: Buffer.from("0123456789abcdef", "utf8").toString("base64url"),
  };

  const requests: Array<{ url: string; headers: Headers }> = [];
  const sender = (status: number): WebPushSender =>
    new WebPushSender(keys, async (input, init) => {
      requests.push({
        url: String(input),
        headers: new Headers(init?.headers),
      });
      return new Response(null, { status });
    });

  assert.equal(await sender(201).send(subscription, "{}"), "delivered");
  assert.equal(await sender(410).send(subscription, "{}"), "gone");
  assert.equal(await sender(404).send(subscription, "{}"), "gone");
  assert.equal(await sender(500).send(subscription, "{}"), "failed");

  const first = requests[0];
  assert.ok(first);
  assert.equal(first.url, subscription.endpoint);
  assert.equal(first.headers.get("content-encoding"), "aes128gcm");
  assert.equal(first.headers.get("ttl"), "300");
  assert.match(first.headers.get("authorization") ?? "", /^vapid t=/);

  const unreachable = new WebPushSender(keys, async () => {
    throw new Error("connection reset");
  });
  assert.equal(await unreachable.send(subscription, "{}"), "failed");

  // An undecryptable subscription can never be delivered to; treat it as gone
  // so it is pruned instead of retried forever.
  assert.equal(
    await sender(201).send({ ...subscription, auth: "AAAA" }, "{}"),
    "gone",
  );
});

test("push stays disabled unless the whole VAPID triple is configured", () => {
  const keys = generateVapidKeys();
  assert.equal(vapidKeysFromEnv({}), undefined);
  assert.equal(
    vapidKeysFromEnv({
      AGENT_PLATFORM_VAPID_PUBLIC_KEY: keys.publicKey,
      AGENT_PLATFORM_VAPID_PRIVATE_KEY: keys.privateKey,
    }),
    undefined,
  );
  assert.throws(
    () =>
      vapidKeysFromEnv({
        AGENT_PLATFORM_VAPID_PUBLIC_KEY: keys.publicKey,
        AGENT_PLATFORM_VAPID_PRIVATE_KEY: "short",
        AGENT_PLATFORM_VAPID_SUBJECT: keys.subject,
      }),
    /32 byte scalar/,
  );
  assert.throws(
    () =>
      vapidKeysFromEnv({
        AGENT_PLATFORM_VAPID_PUBLIC_KEY: keys.publicKey,
        AGENT_PLATFORM_VAPID_PRIVATE_KEY: keys.privateKey,
        AGENT_PLATFORM_VAPID_SUBJECT: "operator@example.com",
      }),
    /mailto: or https:/,
  );
  assert.deepEqual(
    vapidKeysFromEnv({
      AGENT_PLATFORM_VAPID_PUBLIC_KEY: keys.publicKey,
      AGENT_PLATFORM_VAPID_PRIVATE_KEY: keys.privateKey,
      AGENT_PLATFORM_VAPID_SUBJECT: keys.subject,
    }),
    keys,
  );
});

function generateVapidKeys(): {
  publicKey: string;
  privateKey: string;
  subject: string;
} {
  const key = createECDH("prime256v1");
  key.generateKeys();
  return {
    publicKey: key.getPublicKey().toString("base64url"),
    privateKey: key.getPrivateKey().toString("base64url"),
    subject: "mailto:operator@example.com",
  };
}

/** The receiver half of RFC 8291, used to prove the encryption round-trips. */
function decryptPushBody(
  body: Buffer,
  receiver: ReturnType<typeof createECDH>,
  authSecret: Buffer,
): string {
  const salt = body.subarray(0, 16);
  const keyLength = body.readUInt8(20);
  const senderKey = body.subarray(21, 21 + keyLength);
  const ciphertext = body.subarray(21 + keyLength);
  const sharedSecret = receiver.computeSecret(senderKey);
  const keyInfo = Buffer.concat([
    Buffer.from("WebPush: info\0", "utf8"),
    receiver.getPublicKey(),
    senderKey,
  ]);
  const ikm = Buffer.from(
    hkdfSync("sha256", sharedSecret, authSecret, keyInfo, 32),
  );
  const key = Buffer.from(
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
  const decipher = createDecipheriv("aes-128-gcm", key, nonce);
  decipher.setAuthTag(ciphertext.subarray(ciphertext.length - 16));
  const padded = Buffer.concat([
    decipher.update(ciphertext.subarray(0, ciphertext.length - 16)),
    decipher.final(),
  ]);
  return padded.subarray(0, padded.length - 1).toString("utf8");
}
