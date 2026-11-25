import {
  createHash,
  createPublicKey,
  verify as verifySignature,
} from "node:crypto";

import { ContractError } from "./contract-error.js";
import { expectKeys, expectObject } from "./expect.js";
import type { DeviceHeartbeat, DeviceRegistration } from "./sync-types.js";
import {
  expectDeviceId,
  expectHex,
  expectProtocolVersion,
  expectString,
  expectTimestamp,
} from "./sync-validators.js";

export function deviceIdForPublicKey(publicKey: string): string {
  const digest = createHash("sha256")
    .update(Buffer.from(publicKey, "hex"))
    .digest("hex");
  return `dev_${digest.slice(0, 32)}`;
}

export function verifyEd25519(
  publicKeyHex: string,
  data: Buffer,
  signatureHex: string,
): boolean {
  try {
    const publicKey = createPublicKey({
      key: Buffer.concat([
        Buffer.from("302a300506032b6570032100", "hex"),
        Buffer.from(publicKeyHex, "hex"),
      ]),
      format: "der",
      type: "spki",
    });
    return verifySignature(
      null,
      data,
      publicKey,
      Buffer.from(signatureHex, "hex"),
    );
  } catch {
    return false;
  }
}

export function parseDeviceHeartbeat(value: unknown): DeviceHeartbeat {
  const object = expectObject(value, "device heartbeat");
  expectKeys(
    object,
    ["protocol_version", "device_id", "public_key", "sent_at", "signature"],
    ["device_name"],
  );
  const heartbeat: DeviceHeartbeat = {
    protocol_version: expectProtocolVersion(object.protocol_version),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    sent_at: expectTimestamp(object.sent_at, "sent_at"),
    signature: expectHex(object.signature, "signature", 128),
  };
  if (object.device_name !== undefined) {
    heartbeat.device_name = expectString(
      object.device_name,
      "device_name",
      128,
    );
  }
  verifyDeviceSignature(
    heartbeat.device_id,
    heartbeat.public_key,
    unsignedDeviceHeartbeatJson(heartbeat),
    heartbeat.signature,
  );
  return heartbeat;
}

export function unsignedDeviceHeartbeatJson(
  heartbeat: DeviceHeartbeat,
): string {
  return JSON.stringify({
    protocol_version: heartbeat.protocol_version,
    device_id: heartbeat.device_id,
    public_key: heartbeat.public_key,
    sent_at: heartbeat.sent_at,
  });
}

export function verifyDeviceSignature(
  deviceId: string,
  publicKey: string,
  unsignedJson: string,
  signature: string,
): void {
  if (deviceIdForPublicKey(publicKey) !== deviceId) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  if (!verifyEd25519(publicKey, Buffer.from(unsignedJson, "utf8"), signature)) {
    throw new ContractError("invalid_signature", "device signature is invalid");
  }
}

export function parseDeviceRegistration(value: unknown): DeviceRegistration {
  const object = expectObject(value, "device registration");
  expectKeys(
    object,
    ["binding_code", "device_id", "public_key", "signature"],
    ["device_name"],
  );
  const registration: DeviceRegistration = {
    binding_code: expectString(object.binding_code, "binding_code", 128),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    signature: expectHex(object.signature, "signature", 128),
  };
  if (object.device_name !== undefined) {
    registration.device_name = expectString(
      object.device_name,
      "device_name",
      128,
    );
  }
  if (
    deviceIdForPublicKey(registration.public_key) !== registration.device_id
  ) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  // Proof of possession: without it, anyone could register a public key they
  // observed in transit and bind another user's device to their own account.
  const signingBytes = Buffer.from(
    JSON.stringify({
      binding_code: registration.binding_code,
      device_id: registration.device_id,
      public_key: registration.public_key,
    }),
    "utf8",
  );
  if (
    !verifyEd25519(
      registration.public_key,
      signingBytes,
      registration.signature,
    )
  ) {
    throw new ContractError(
      "invalid_signature",
      "registration signature is invalid",
    );
  }
  return registration;
}
