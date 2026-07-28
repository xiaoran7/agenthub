import {
  createHash,
  createPrivateKey,
  createPublicKey,
  sign,
  type KeyObject,
} from "node:crypto";

import {
  deviceIdForPublicKey,
  type SyncBatch,
  type SyncEvent,
  unsignedBatchJson,
} from "@agent-platform/contracts";

export const M4_DEVICE_COUNT = 12;
export const M4_SESSIONS_PER_DEVICE = 250;

export interface M4TestDevice {
  index: number;
  ownerId: string;
  bindingCode: string;
  privateKey: KeyObject;
  publicKey: string;
  deviceId: string;
}

export function makeM4Device(index: number, ownerId: string): M4TestDevice {
  const seed = createHash("sha256").update(`m4-device-${index}`).digest();
  const privateKey = createPrivateKey({
    key: Buffer.concat([
      Buffer.from("302e020100300506032b657004220420", "hex"),
      seed,
    ]),
    format: "der",
    type: "pkcs8",
  });
  const publicKey = createPublicKey(
    privateKey.export({ format: "pem", type: "pkcs8" }),
  )
    .export({ format: "der", type: "spki" })
    .subarray(-32)
    .toString("hex");
  return {
    index,
    ownerId,
    bindingCode: `m4-binding-${index}`,
    privateKey,
    publicKey,
    deviceId: deviceIdForPublicKey(publicKey),
  };
}

export function m4RegistrationPayload(
  device: M4TestDevice,
): Record<string, unknown> {
  const registration = {
    binding_code: device.bindingCode,
    device_id: device.deviceId,
    public_key: device.publicKey,
  };
  return {
    ...registration,
    signature: sign(
      null,
      Buffer.from(JSON.stringify(registration), "utf8"),
      device.privateKey,
    ).toString("hex"),
  };
}

export function makeM4Batch(device: M4TestDevice, count: number): SyncBatch {
  const observedAt = new Date(
    Date.UTC(2026, 6, 28, 0, 0, device.index),
  ).toISOString();
  const events = Array.from({ length: count }, (_, index): SyncEvent => {
    const sequence = index + 1;
    const waiting = index % 10 === 0;
    const state: SyncEvent["payload"]["state"] = {
      presence: "LIVE",
      activity_status: waiting ? "WAITING_INPUT" : "WORKING",
      raw_status: waiting ? "blocked" : "active",
    };
    if (waiting) {
      state.waiting_reason = "user_input";
    }
    return {
      sequence,
      event_id: uuidFor(1, device.index * M4_SESSIONS_PER_DEVICE + sequence),
      schema_version: 1,
      device_id: device.deviceId,
      agent_type: index % 2 === 0 ? "claude_code" : "codex",
      agent_session_id: `load-session-${device.index}-${index}`,
      event_type: "session_observed",
      observed_at: observedAt,
      source: index % 2 === 0 ? "claude_agents_json" : "codex_app_server",
      confidence: "authoritative",
      adapter_version: "m4-load-fixture",
      payload: {
        project: {
          display_name: `project-${index % 25}`,
          fingerprint: createHash("sha256")
            .update(`project-${device.index}-${index % 25}`)
            .digest("hex"),
        },
        state,
      },
    };
  });
  const batch: SyncBatch = {
    protocol_version: 1,
    batch_id: uuidFor(2, device.index + 1),
    device_id: device.deviceId,
    public_key: device.publicKey,
    created_at: observedAt,
    first_sequence: 1,
    last_sequence: count,
    events,
    signature: "",
  };
  batch.signature = sign(
    null,
    Buffer.from(unsignedBatchJson(batch), "utf8"),
    device.privateKey,
  ).toString("hex");
  return batch;
}

function uuidFor(namespace: number, value: number): string {
  return `${namespace.toString(16).padStart(8, "0")}-0000-4000-8000-${value
    .toString(16)
    .padStart(12, "0")}`;
}
