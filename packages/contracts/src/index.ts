import {
  createHash,
  createPublicKey,
  verify as verifySignature,
} from "node:crypto";

export const SYNC_PROTOCOL_VERSION = 1;
export const MAX_SYNC_BATCH_EVENTS = 1_000;

export const agentTypes = ["claude_code", "codex"] as const;
export const presences = ["LIVE", "PERSISTED", "UNKNOWN"] as const;
export const activityStatuses = [
  "WORKING",
  "WAITING_INPUT",
  "WAITING_PERMISSION",
  "IDLE",
  "COMPLETED",
  "FAILED",
  "STOPPED",
  "UNKNOWN",
] as const;
export const observationSources = [
  "claude_agents_json",
  "claude_hook",
  "codex_app_server",
  "codex_hook",
  "process_probe",
  "local_history",
] as const;
export const confidences = [
  "unknown",
  "inferred",
  "observed",
  "authoritative",
] as const;
export const waitingReasons = [
  "user_input",
  "permission",
  "sandbox",
  "worker",
  "dialog",
  "mcp_elicitation",
  "unknown",
] as const;

export type AgentType = (typeof agentTypes)[number];
export type Presence = (typeof presences)[number];
export type ActivityStatus = (typeof activityStatuses)[number];
export type ObservationSource = (typeof observationSources)[number];
export type Confidence = (typeof confidences)[number];
export type WaitingReason = (typeof waitingReasons)[number];

export interface ProjectRef {
  display_name: string;
  fingerprint: string;
}

export interface SessionState {
  presence: Presence;
  activity_status: ActivityStatus;
  raw_status: string;
  raw_status_detail?: Record<string, string>;
  waiting_reason?: WaitingReason;
}

export interface SyncSessionPayload {
  project: ProjectRef;
  state: SessionState;
  started_at?: string;
  last_activity_at?: string;
}

export interface SyncEvent {
  sequence: number;
  event_id: string;
  schema_version: number;
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  event_type: "session_observed";
  observed_at: string;
  source: ObservationSource;
  confidence: Confidence;
  adapter_version: string;
  payload: SyncSessionPayload;
}

export interface SyncBatch {
  protocol_version: number;
  batch_id: string;
  device_id: string;
  public_key: string;
  created_at: string;
  first_sequence: number;
  last_sequence: number;
  events: SyncEvent[];
  signature: string;
}

export interface SyncAck {
  protocol_version: number;
  batch_id: string;
  device_id: string;
  accepted_through: number;
}

export interface DeviceRegistration {
  binding_code: string;
  device_id: string;
  public_key: string;
  signature: string;
}

export class ContractError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ContractError";
    this.code = code;
  }
}

export function parseDeviceRegistration(value: unknown): DeviceRegistration {
  const object = expectObject(value, "device registration");
  expectKeys(object, ["binding_code", "device_id", "public_key", "signature"]);
  const registration = {
    binding_code: expectString(object.binding_code, "binding_code", 128),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    signature: expectHex(object.signature, "signature", 128),
  };
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

export function deviceIdForPublicKey(publicKey: string): string {
  const digest = createHash("sha256")
    .update(Buffer.from(publicKey, "hex"))
    .digest("hex");
  return `dev_${digest.slice(0, 32)}`;
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

function verifyEd25519(
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
    agent_type: expectEnum(
      object.agent_type,
      agentTypes,
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

function expectObject(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new ContractError("invalid_field", `${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

function expectKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = [],
): void {
  for (const key of required) {
    if (!(key in value)) {
      throw new ContractError("missing_field", `missing field ${key}`);
    }
  }
  const allowed = new Set([...required, ...optional]);
  const unknown = Object.keys(value).find((key) => !allowed.has(key));
  if (unknown !== undefined) {
    throw new ContractError("unknown_field", `unknown field ${unknown}`);
  }
}

function expectString(value: unknown, name: string, maxLength: number): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > maxLength ||
    /[\u0000-\u001f\u007f]/.test(value) ||
    // Lone surrogates cannot exist in the Rust runner's UTF-8 strings and are
    // rejected by PostgreSQL jsonb; refusing them keeps both stores and both
    // languages inside the same contract.
    !value.isWellFormed()
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a non-empty string up to ${maxLength} characters`,
    );
  }
  return value;
}

function expectInteger(value: unknown, name: string, minimum: number): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < minimum
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a safe integer >= ${minimum}`,
    );
  }
  return value;
}

function expectSchemaVersion(value: unknown, name: string): number {
  const version = expectInteger(value, name, 1);
  if (version !== 1) {
    throw new ContractError(
      "unsupported_schema",
      `unsupported event schema version ${version}`,
    );
  }
  return version;
}

function expectEnum<const T extends readonly string[]>(
  value: unknown,
  allowed: T,
  name: string,
): T[number] {
  if (typeof value !== "string" || !allowed.includes(value)) {
    throw new ContractError("invalid_field", `${name} is invalid`);
  }
  return value as T[number];
}

function expectHex(value: unknown, name: string, length: number): string {
  if (
    typeof value !== "string" ||
    value.length !== length ||
    !/^[0-9a-f]+$/.test(value)
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be ${length} lowercase hex characters`,
    );
  }
  return value;
}

// Version 1 signing bytes sort map keys by Unicode code point (Rust BTreeMap
// order). String comparison must not be locale-sensitive, and detail keys are
// restricted to a charset where UTF-16 code-unit order equals code-point order.
function compareCodeUnits(left: string, right: string): number {
  if (left === right) {
    return 0;
  }
  return left < right ? -1 : 1;
}

function expectDetailKey(value: unknown): string {
  if (typeof value !== "string" || !/^[a-z][a-z0-9_]{0,63}$/.test(value)) {
    throw new ContractError(
      "invalid_field",
      "raw status detail keys must be snake_case ASCII up to 64 characters",
    );
  }
  return value;
}

function expectDeviceId(value: unknown): string {
  if (typeof value !== "string" || !/^dev_[0-9a-f]{32}$/.test(value)) {
    throw new ContractError("invalid_field", "device_id is invalid");
  }
  return value;
}

function expectUuid(value: unknown, name: string): string {
  // Lowercase only: PostgreSQL normalizes uuid columns to lowercase, so an
  // uppercase variant would break replay idempotency between parse and store.
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be a lowercase UUID`,
    );
  }
  return value;
}

function expectTimestamp(value: unknown, name: string): string {
  if (
    typeof value !== "string" ||
    value.length > 64 ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/.test(
      value,
    ) ||
    Number.isNaN(Date.parse(value))
  ) {
    throw new ContractError(
      "invalid_field",
      `${name} must be an RFC 3339 time`,
    );
  }
  return value;
}
