import { createECDH } from "node:crypto";

/**
 * Prints a fresh VAPID key pair for local development.
 *
 * The keys identify this gateway to push services; treat the private key like
 * any other server secret and do not commit it.
 */
const key = createECDH("prime256v1");
key.generateKeys();

process.stdout.write(
  [
    `AGENT_PLATFORM_VAPID_PUBLIC_KEY=${key.getPublicKey().toString("base64url")}`,
    `AGENT_PLATFORM_VAPID_PRIVATE_KEY=${key.getPrivateKey().toString("base64url")}`,
    "AGENT_PLATFORM_VAPID_SUBJECT=mailto:you@example.com",
    "",
  ].join("\n"),
);
