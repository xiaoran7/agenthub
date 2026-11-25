import { ContractError } from "./contract-error.js";
import { expectKeys, expectObject } from "./expect.js";
import { expectAgentType } from "./plugins.js";
import { deviceIdForPublicKey, verifyEd25519 } from "./device-sync.js";
import {
  MAX_SYNC_BATCH_EVENTS,
  SYNC_PROTOCOL_VERSION,
  activityStatuses,
  confidences,
  observationSources,
  presences,
  waitingReasons,
  type ProjectRef,
  type SessionState,
  type SyncBatch,
  type SyncEvent,
  type SyncSessionPayload,
} from "./sync-types.js";
import {
  compareCodeUnits,
  expectBoolean,
  expectDetailKey,
  expectDeviceId,
  expectEnum,
  expectHex,
  expectIdempotencyKey,
  expectInteger,
  expectProtocolVersion,
  expectSafeCode,
  expectSchemaVersion,
  expectString,
  expectTimestamp,
  expectUuid,
} from "./sync-validators.js";

export function parseSyncBatch(value: unknown): SyncBatch {
  const object = expectObject(value, "sync batch");
  expectKeys(object, [
    "protocol_version",
    "batch_id",
    "device_id",
    "public_key",
    "created_at",
    "first_sequence",
    "last_sequence",
    "events",
    "signature",
  ]);
  const protocolVersion = expectInteger(
    object.protocol_version,
    "protocol_version",
    1,
  );
  if (protocolVersion !== SYNC_PROTOCOL_VERSION) {
    throw new ContractError(
      "unsupported_protocol",
      `unsupported protocol version ${protocolVersion}`,
    );
  }
  if (!Array.isArray(object.events)) {
    throw new ContractError("invalid_events", "events must be an array");
  }
  if (
    object.events.length === 0 ||
    object.events.length > MAX_SYNC_BATCH_EVENTS
  ) {
    throw new ContractError(
      "invalid_events",
      `events must contain between 1 and ${MAX_SYNC_BATCH_EVENTS} items`,
    );
  }

  const batch: SyncBatch = {
    protocol_version: protocolVersion,
    batch_id: expectUuid(object.batch_id, "batch_id"),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    created_at: expectTimestamp(object.created_at, "created_at"),
    first_sequence: expectInteger(object.first_sequence, "first_sequence", 1),
    last_sequence: expectInteger(object.last_sequence, "last_sequence", 1),
    events: object.events.map((event, index) => parseSyncEvent(event, index)),
    signature: expectHex(object.signature, "signature", 128),
  };
  validateBatchRelations(batch);
  return batch;
}

export function unsignedBatchJson(batch: SyncBatch): string {
  return JSON.stringify({
    protocol_version: batch.protocol_version,
    batch_id: batch.batch_id,
    device_id: batch.device_id,
    public_key: batch.public_key,
    created_at: batch.created_at,
    first_sequence: batch.first_sequence,
    last_sequence: batch.last_sequence,
    events: batch.events.map(canonicalEvent),
  });
}

export function verifySyncBatch(batch: SyncBatch): void {
  if (deviceIdForPublicKey(batch.public_key) !== batch.device_id) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  if (
    !verifyEd25519(
      batch.public_key,
      Buffer.from(unsignedBatchJson(batch), "utf8"),
      batch.signature,
    )
  ) {
    throw new ContractError("invalid_signature", "batch signature is invalid");
  }
}

function parseSyncEvent(value: unknown, index: number): SyncEvent {
  const object = expectObject(value, `events[${index}]`);
  expectKeys(object, [
    "sequence",
    "event_id",
    "schema_version",
    "device_id",
    "agent_type",
    "agent_session_id",
    "event_type",
    "observed_at",
    "source",
    "confidence",
    "adapter_version",
    "payload",
  ]);
  if (object.event_type !== "session_observed") {
    throw new ContractError(
      "invalid_event_type",
      `events[${index}].event_type is unsupported`,
    );
  }
  return {
    sequence: expectInteger(object.sequence, `events[${index}].sequence`, 1),
    event_id: expectUuid(object.event_id, `events[${index}].event_id`),
    schema_version: expectSchemaVersion(
      object.schema_version,
      `events[${index}].schema_version`,
    ),
    device_id: expectDeviceId(object.device_id),
    agent_type: expectAgentType(
      object.agent_type,
      `events[${index}].agent_type`,
    ),
    agent_session_id: expectString(
      object.agent_session_id,
      `events[${index}].agent_session_id`,
      512,
    ),
    event_type: "session_observed",
    observed_at: expectTimestamp(
      object.observed_at,
      `events[${index}].observed_at`,
    ),
    source: expectEnum(
      object.source,
      observationSources,
      `events[${index}].source`,
    ),
    confidence: expectEnum(
      object.confidence,
      confidences,
      `events[${index}].confidence`,
    ),
    adapter_version: expectString(
      object.adapter_version,
      `events[${index}].adapter_version`,
      64,
    ),
    payload: parsePayload(object.payload, index),
  };
}

function parsePayload(value: unknown, eventIndex: number): SyncSessionPayload {
  const object = expectObject(value, `events[${eventIndex}].payload`);
  expectKeys(object, ["project", "state"], ["started_at", "last_activity_at"]);
  const project = expectObject(
    object.project,
    `events[${eventIndex}].payload.project`,
  );
  expectKeys(project, ["display_name", "fingerprint"]);
  const state = expectObject(
    object.state,
    `events[${eventIndex}].payload.state`,
  );
  expectKeys(
    state,
    ["presence", "activity_status", "raw_status"],
    ["raw_status_detail", "waiting_reason"],
  );

  const payload: SyncSessionPayload = {
    project: {
      display_name: expectString(
        project.display_name,
        "project.display_name",
        256,
      ),
      fingerprint: expectHex(project.fingerprint, "project.fingerprint", 64),
    },
    state: {
      presence: expectEnum(state.presence, presences, "state.presence"),
      activity_status: expectEnum(
        state.activity_status,
        activityStatuses,
        "state.activity_status",
      ),
      raw_status: expectString(state.raw_status, "state.raw_status", 128),
    },
  };
  if (state.raw_status_detail !== undefined) {
    const details = expectObject(
      state.raw_status_detail,
      "state.raw_status_detail",
    );
    const entryCount = Object.keys(details).length;
    if (entryCount === 0 || entryCount > 16) {
      throw new ContractError(
        "invalid_field",
        "state.raw_status_detail must have between 1 and 16 entries",
      );
    }
    payload.state.raw_status_detail = Object.fromEntries(
      Object.entries(details)
        .sort(([left], [right]) => compareCodeUnits(left, right))
        .map(([key, detail]) => [
          expectDetailKey(key),
          expectString(detail, `raw status detail ${key}`, 256),
        ]),
    );
  }
  if (state.waiting_reason !== undefined) {
    payload.state.waiting_reason = expectEnum(
      state.waiting_reason,
      waitingReasons,
      "state.waiting_reason",
    );
  }
  if (object.started_at !== undefined) {
    payload.started_at = expectTimestamp(object.started_at, "started_at");
  }
  if (object.last_activity_at !== undefined) {
    payload.last_activity_at = expectTimestamp(
      object.last_activity_at,
      "last_activity_at",
    );
  }
  return payload;
}

function validateBatchRelations(batch: SyncBatch): void {
  if (deviceIdForPublicKey(batch.public_key) !== batch.device_id) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  if (
    batch.first_sequence !== batch.events[0]?.sequence ||
    batch.last_sequence !== batch.events.at(-1)?.sequence
  ) {
    throw new ContractError(
      "invalid_sequence",
      "batch sequence bounds do not match events",
    );
  }
  const eventIds = new Set<string>();
  for (let index = 0; index < batch.events.length; index += 1) {
    const event = batch.events[index];
    if (event === undefined) {
      throw new ContractError("invalid_events", "batch event is missing");
    }
    if (event.device_id !== batch.device_id) {
      throw new ContractError(
        "device_id_mismatch",
        "batch contains an event for another device",
      );
    }
    if (event.sequence !== batch.first_sequence + index) {
      throw new ContractError(
        "invalid_sequence",
        "batch event sequences are not contiguous",
      );
    }
    if (eventIds.has(event.event_id)) {
      throw new ContractError(
        "duplicate_event",
        "batch contains a duplicate event ID",
      );
    }
    eventIds.add(event.event_id);
  }
}

function canonicalEvent(event: SyncEvent): SyncEvent {
  const rawStatusDetail = event.payload.state.raw_status_detail;
  const state: SessionState = {
    presence: event.payload.state.presence,
    activity_status: event.payload.state.activity_status,
    raw_status: event.payload.state.raw_status,
  };
  if (
    rawStatusDetail !== undefined &&
    Object.keys(rawStatusDetail).length > 0
  ) {
    state.raw_status_detail = Object.fromEntries(
      Object.entries(rawStatusDetail).sort(([left], [right]) =>
        compareCodeUnits(left, right),
      ),
    );
  }
  if (event.payload.state.waiting_reason !== undefined) {
    state.waiting_reason = event.payload.state.waiting_reason;
  }
  const payload: SyncSessionPayload = {
    project: {
      display_name: event.payload.project.display_name,
      fingerprint: event.payload.project.fingerprint,
    },
    state,
  };
  if (event.payload.started_at !== undefined) {
    payload.started_at = event.payload.started_at;
  }
  if (event.payload.last_activity_at !== undefined) {
    payload.last_activity_at = event.payload.last_activity_at;
  }
  return {
    sequence: event.sequence,
    event_id: event.event_id,
    schema_version: event.schema_version,
    device_id: event.device_id,
    agent_type: event.agent_type,
    agent_session_id: event.agent_session_id,
    event_type: event.event_type,
    observed_at: event.observed_at,
    source: event.source,
    confidence: event.confidence,
    adapter_version: event.adapter_version,
    payload,
  };
}
