import { createPrivateKey, createPublicKey, sign } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";

import {
  deviceIdForPublicKey,
  type SyncBatch,
  unsignedBatchJson,
} from "../src/index.js";

const seed = Buffer.from(
  "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
  "hex",
);
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
const deviceId = deviceIdForPublicKey(publicKey);

const batch: SyncBatch = {
  protocol_version: 1,
  batch_id: "123e4567-e89b-42d3-a456-426614174000",
  device_id: deviceId,
  public_key: publicKey,
  created_at: "2026-07-25T00:00:00Z",
  first_sequence: 1,
  last_sequence: 1,
  events: [
    {
      sequence: 1,
      event_id: "123e4567-e89b-42d3-a456-426614174001",
      schema_version: 1,
      device_id: deviceId,
      agent_type: "codex",
      agent_session_id: "contract-session",
      event_type: "session_observed",
      observed_at: "2026-07-25T00:00:00Z",
      source: "codex_app_server",
      confidence: "authoritative",
      adapter_version: "0.1.0",
      payload: {
        project: {
          display_name: "contract-project",
          fingerprint:
            "6c1f11bd12bd9b596bbad5091a198fd1b1b210002b86cd155b52b4b9b586e891",
        },
        state: {
          presence: "LIVE",
          activity_status: "WORKING",
          raw_status: "active",
        },
        last_activity_at: "2026-07-25T00:00:00Z",
      },
    },
  ],
  signature: "",
};
batch.signature = sign(
  null,
  Buffer.from(unsignedBatchJson(batch), "utf8"),
  privateKey,
).toString("hex");

const fixtureDirectory = new URL("../fixtures/", import.meta.url);
await mkdir(fixtureDirectory, { recursive: true });
await writeFile(
  new URL("sync-batch-v1.json", fixtureDirectory),
  `${JSON.stringify(batch, null, 2)}\n`,
  "utf8",
);
