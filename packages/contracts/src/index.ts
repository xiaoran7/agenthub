import {
  createHash,
  createPublicKey,
  verify as verifySignature,
} from "node:crypto";

export const SYNC_PROTOCOL_VERSION = 1;
export const MAX_SYNC_BATCH_EVENTS = 1_000;

export const agentTypes = ["claude_code", "codex", "grok_build"] as const;
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
  "grok_sessions",
  "grok_hook",
  "grok_active_sessions",
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

export const remoteTaskStatuses = [
  "queued",
  "delivered",
  "accepted",
  "running",
  "waiting_user",
  "succeeded",
  "failed",
  "canceled",
  "expired",
] as const;
export const taskExecutionPolicies = [
  "observe_only",
  "local_confirmation",
  "trusted_execution",
] as const;

export type RemoteTaskStatus = (typeof remoteTaskStatuses)[number];
export type TaskExecutionPolicy = (typeof taskExecutionPolicies)[number];

export interface CreateRemoteTaskInput {
  target_device_id: string;
  agent_type: AgentType;
  prompt: string;
  project_fingerprint?: string;
  idempotency_key: string;
  execution_policy: TaskExecutionPolicy;
  expires_in_seconds: number;
}

export interface RemoteTask {
  task_id: string;
  target_device_id: string;
  agent_type: AgentType;
  prompt: string;
  project_fingerprint?: string;
  idempotency_key: string;
  execution_policy: TaskExecutionPolicy;
  status: RemoteTaskStatus;
  version: number;
  created_at: string;
  expires_at: string;
  updated_at: string;
  delivered_at?: string;
  started_at?: string;
  completed_at?: string;
  agent_session_id?: string;
  result_summary?: string;
  error_code?: string;
  execution_started_at?: string;
  execution_finished_at?: string;
  receipt_digest?: string;
  exit_code?: number;
}

export interface DeviceTaskClaim {
  protocol_version: number;
  device_id: string;
  public_key: string;
  requested_at: string;
  signature: string;
}

export interface DeviceHeartbeat {
  protocol_version: number;
  device_id: string;
  public_key: string;
  sent_at: string;
  signature: string;
}

export interface DeviceTaskUpdate {
  protocol_version: number;
  task_id: string;
  device_id: string;
  public_key: string;
  expected_version: number;
  status: Exclude<RemoteTaskStatus, "queued" | "delivered" | "expired">;
  updated_at: string;
  agent_session_id?: string;
  result_summary?: string;
  error_code?: string;
  execution_started_at?: string;
  execution_finished_at?: string;
  receipt_digest?: string;
  exit_code?: number;
  signature: string;
}

export const authStates = [
  "authenticated",
  "unauthenticated",
  "reauth_required",
  "unknown",
] as const;
export const credentialSources = [
  "official_cli",
  "app_server",
  "environment",
  "file",
  "keyring",
  "cache",
  "unknown",
] as const;
export const quotaStates = [
  "available",
  "exhausted",
  "unavailable",
  "unknown",
] as const;

export type AuthState = (typeof authStates)[number];
export type CredentialSource = (typeof credentialSources)[number];
export type QuotaState = (typeof quotaStates)[number];

export interface QuotaWindow {
  name: string;
  used_percent?: number;
  window_duration_minutes?: number;
  resets_at?: string;
}

export interface QuotaSnapshot {
  state: QuotaState;
  source: CredentialSource;
  refreshed_at: string;
  windows: QuotaWindow[];
  available_reset_credits?: number;
}

/** Safe-to-sync authentication view. It intentionally has no token fields. */
export interface AuthSnapshot {
  provider: AgentType;
  state: AuthState;
  source: CredentialSource;
  checked_at: string;
  account_hint?: string;
  plan_type?: string;
  auth_mode?: string;
  access_token_present: boolean;
  refresh_token_present: boolean;
  access_token_expires_at?: string;
  quota?: QuotaSnapshot;
  error_code?: string;
}

export interface AuthSyncBatch {
  protocol_version: number;
  device_id: string;
  public_key: string;
  checked_at: string;
  snapshots: AuthSnapshot[];
  signature: string;
}

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

export function parseCreateRemoteTask(value: unknown): CreateRemoteTaskInput {
  const object = expectObject(value, "remote task");
  expectKeys(
    object,
    [
      "target_device_id",
      "agent_type",
      "prompt",
      "idempotency_key",
      "execution_policy",
      "expires_in_seconds",
    ],
    ["project_fingerprint"],
  );
  const prompt = expectString(object.prompt, "prompt", 32_000);
  if (Buffer.byteLength(prompt, "utf8") > 64_000) {
    throw new ContractError("invalid_field", "prompt exceeds 64000 bytes");
  }
  const input: CreateRemoteTaskInput = {
    target_device_id: expectDeviceId(object.target_device_id),
    agent_type: expectEnum(object.agent_type, agentTypes, "agent_type"),
    prompt,
    idempotency_key: expectIdempotencyKey(object.idempotency_key),
    execution_policy: expectEnum(
      object.execution_policy,
      taskExecutionPolicies,
      "execution_policy",
    ),
    expires_in_seconds: expectInteger(
      object.expires_in_seconds,
      "expires_in_seconds",
      60,
    ),
  };
  if (input.expires_in_seconds > 86_400) {
    throw new ContractError(
      "invalid_field",
      "expires_in_seconds must be at most 86400",
    );
  }
  if (object.project_fingerprint !== undefined) {
    input.project_fingerprint = expectHex(
      object.project_fingerprint,
      "project_fingerprint",
      64,
    );
  }
  return input;
}

export function parseDeviceTaskClaim(value: unknown): DeviceTaskClaim {
  const object = expectObject(value, "device task claim");
  expectKeys(object, [
    "protocol_version",
    "device_id",
    "public_key",
    "requested_at",
    "signature",
  ]);
  const claim: DeviceTaskClaim = {
    protocol_version: expectProtocolVersion(object.protocol_version),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    requested_at: expectTimestamp(object.requested_at, "requested_at"),
    signature: expectHex(object.signature, "signature", 128),
  };
  verifyDeviceSignature(
    claim.device_id,
    claim.public_key,
    unsignedDeviceTaskClaimJson(claim),
    claim.signature,
  );
  return claim;
}

export function parseDeviceHeartbeat(value: unknown): DeviceHeartbeat {
  const object = expectObject(value, "device heartbeat");
  expectKeys(object, [
    "protocol_version",
    "device_id",
    "public_key",
    "sent_at",
    "signature",
  ]);
  const heartbeat: DeviceHeartbeat = {
    protocol_version: expectProtocolVersion(object.protocol_version),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    sent_at: expectTimestamp(object.sent_at, "sent_at"),
    signature: expectHex(object.signature, "signature", 128),
  };
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

export function unsignedDeviceTaskClaimJson(claim: DeviceTaskClaim): string {
  return JSON.stringify({
    protocol_version: claim.protocol_version,
    device_id: claim.device_id,
    public_key: claim.public_key,
    requested_at: claim.requested_at,
  });
}

export function parseDeviceTaskUpdate(value: unknown): DeviceTaskUpdate {
  const object = expectObject(value, "device task update");
  expectKeys(
    object,
    [
      "protocol_version",
      "task_id",
      "device_id",
      "public_key",
      "expected_version",
      "status",
      "updated_at",
      "signature",
    ],
    [
      "agent_session_id",
      "result_summary",
      "error_code",
      "execution_started_at",
      "execution_finished_at",
      "receipt_digest",
      "exit_code",
    ],
  );
  const status = expectEnum(object.status, remoteTaskStatuses, "status");
  if (status === "queued" || status === "delivered" || status === "expired") {
    throw new ContractError("invalid_field", "device task status is invalid");
  }
  const update: DeviceTaskUpdate = {
    protocol_version: expectProtocolVersion(object.protocol_version),
    task_id: expectUuid(object.task_id, "task_id"),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    expected_version: expectInteger(
      object.expected_version,
      "expected_version",
      1,
    ),
    status,
    updated_at: expectTimestamp(object.updated_at, "updated_at"),
    signature: expectHex(object.signature, "signature", 128),
  };
  if (object.agent_session_id !== undefined) {
    update.agent_session_id = expectString(
      object.agent_session_id,
      "agent_session_id",
      512,
    );
  }
  if (object.result_summary !== undefined) {
    update.result_summary = expectString(
      object.result_summary,
      "result_summary",
      2_000,
    );
  }
  if (object.error_code !== undefined) {
    update.error_code = expectSafeCode(object.error_code, "error_code");
  }
  if (object.execution_started_at !== undefined) {
    update.execution_started_at = expectTimestamp(
      object.execution_started_at,
      "execution_started_at",
    );
  }
  if (object.execution_finished_at !== undefined) {
    update.execution_finished_at = expectTimestamp(
      object.execution_finished_at,
      "execution_finished_at",
    );
  }
  if (object.receipt_digest !== undefined) {
    update.receipt_digest = expectHex(
      object.receipt_digest,
      "receipt_digest",
      64,
    );
  }
  if (object.exit_code !== undefined) {
    update.exit_code = expectInteger(object.exit_code, "exit_code", -1);
    if (update.exit_code > 255) {
      throw new ContractError("invalid_field", "exit_code must be at most 255");
    }
  }
  verifyDeviceSignature(
    update.device_id,
    update.public_key,
    unsignedDeviceTaskUpdateJson(update),
    update.signature,
  );
  return update;
}

export function unsignedDeviceTaskUpdateJson(update: DeviceTaskUpdate): string {
  return JSON.stringify({
    protocol_version: update.protocol_version,
    task_id: update.task_id,
    device_id: update.device_id,
    public_key: update.public_key,
    expected_version: update.expected_version,
    status: update.status,
    updated_at: update.updated_at,
    ...(update.agent_session_id === undefined
      ? {}
      : { agent_session_id: update.agent_session_id }),
    ...(update.result_summary === undefined
      ? {}
      : { result_summary: update.result_summary }),
    ...(update.error_code === undefined
      ? {}
      : { error_code: update.error_code }),
    ...(update.execution_started_at === undefined
      ? {}
      : { execution_started_at: update.execution_started_at }),
    ...(update.execution_finished_at === undefined
      ? {}
      : { execution_finished_at: update.execution_finished_at }),
    ...(update.receipt_digest === undefined
      ? {}
      : { receipt_digest: update.receipt_digest }),
    ...(update.exit_code === undefined ? {} : { exit_code: update.exit_code }),
  });
}

function verifyDeviceSignature(
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

export function parseAuthSyncBatch(value: unknown): AuthSyncBatch {
  const object = expectObject(value, "auth sync batch");
  expectKeys(object, [
    "protocol_version",
    "device_id",
    "public_key",
    "checked_at",
    "snapshots",
    "signature",
  ]);
  const snapshots = object.snapshots;
  if (
    !Array.isArray(snapshots) ||
    snapshots.length === 0 ||
    snapshots.length > 8
  ) {
    throw new ContractError(
      "invalid_auth_snapshots",
      "snapshots must contain between 1 and 8 items",
    );
  }
  const providers = snapshots.map((snapshot, index) =>
    expectEnum(
      expectObject(snapshot, `snapshots[${index}]`).provider,
      agentTypes,
      `snapshots[${index}].provider`,
    ),
  );
  if (new Set(providers).size !== providers.length) {
    throw new ContractError(
      "duplicate_auth_provider",
      "auth batch contains duplicate providers",
    );
  }
  const batch: AuthSyncBatch = {
    protocol_version: expectInteger(
      object.protocol_version,
      "protocol_version",
      1,
    ),
    device_id: expectDeviceId(object.device_id),
    public_key: expectHex(object.public_key, "public_key", 64),
    checked_at: expectTimestamp(object.checked_at, "checked_at"),
    snapshots: snapshots.map((snapshot, index) =>
      parseAuthSnapshot(snapshot, index),
    ),
    signature: expectHex(object.signature, "signature", 128),
  };
  if (batch.protocol_version !== 1) {
    throw new ContractError(
      "unsupported_protocol",
      `unsupported auth protocol version ${batch.protocol_version}`,
    );
  }
  if (deviceIdForPublicKey(batch.public_key) !== batch.device_id) {
    throw new ContractError(
      "device_id_mismatch",
      "device ID does not match public key",
    );
  }
  verifyAuthSyncBatch(batch);
  return batch;
}

export function unsignedAuthSyncBatchJson(batch: AuthSyncBatch): string {
  return JSON.stringify({
    protocol_version: batch.protocol_version,
    device_id: batch.device_id,
    public_key: batch.public_key,
    checked_at: batch.checked_at,
    snapshots: batch.snapshots.map((snapshot) =>
      canonicalAuthSnapshot(snapshot),
    ),
  });
}

export function verifyAuthSyncBatch(batch: AuthSyncBatch): void {
  if (
    !verifyEd25519(
      batch.public_key,
      Buffer.from(unsignedAuthSyncBatchJson(batch), "utf8"),
      batch.signature,
    )
  ) {
    throw new ContractError(
      "invalid_signature",
      "auth sync signature is invalid",
    );
  }
}

function parseAuthSnapshot(value: unknown, index: number): AuthSnapshot {
  const object = expectObject(value, `snapshots[${index}]`);
  expectKeys(
    object,
    [
      "provider",
      "state",
      "source",
      "checked_at",
      "access_token_present",
      "refresh_token_present",
    ],
    [
      "account_hint",
      "plan_type",
      "auth_mode",
      "access_token_expires_at",
      "quota",
      "error_code",
    ],
  );
  const snapshot: AuthSnapshot = {
    provider: expectEnum(
      object.provider,
      agentTypes,
      `snapshots[${index}].provider`,
    ),
    state: expectEnum(object.state, authStates, `snapshots[${index}].state`),
    source: expectEnum(
      object.source,
      credentialSources,
      `snapshots[${index}].source`,
    ),
    checked_at: expectTimestamp(
      object.checked_at,
      `snapshots[${index}].checked_at`,
    ),
    access_token_present: expectBoolean(
      object.access_token_present,
      `snapshots[${index}].access_token_present`,
    ),
    refresh_token_present: expectBoolean(
      object.refresh_token_present,
      `snapshots[${index}].refresh_token_present`,
    ),
  };
  if (object.account_hint !== undefined) {
    snapshot.account_hint = expectString(
      object.account_hint,
      `snapshots[${index}].account_hint`,
      256,
    );
  }
  if (object.plan_type !== undefined) {
    snapshot.plan_type = expectString(
      object.plan_type,
      `snapshots[${index}].plan_type`,
      128,
    );
  }
  if (object.auth_mode !== undefined) {
    snapshot.auth_mode = expectString(
      object.auth_mode,
      `snapshots[${index}].auth_mode`,
      128,
    );
  }
  if (object.access_token_expires_at !== undefined) {
    snapshot.access_token_expires_at = expectTimestamp(
      object.access_token_expires_at,
      `snapshots[${index}].access_token_expires_at`,
    );
  }
  if (object.quota !== undefined) {
    snapshot.quota = parseQuotaSnapshot(
      object.quota,
      `snapshots[${index}].quota`,
    );
  }
  if (object.error_code !== undefined) {
    snapshot.error_code = expectString(
      object.error_code,
      `snapshots[${index}].error_code`,
      128,
    );
  }
  return snapshot;
}

function parseQuotaSnapshot(value: unknown, name: string): QuotaSnapshot {
  const object = expectObject(value, name);
  expectKeys(
    object,
    ["state", "source", "refreshed_at", "windows"],
    ["available_reset_credits"],
  );
  if (!Array.isArray(object.windows) || object.windows.length > 16) {
    throw new ContractError(
      "invalid_field",
      `${name}.windows must contain at most 16 items`,
    );
  }
  const quota: QuotaSnapshot = {
    state: expectEnum(object.state, quotaStates, `${name}.state`),
    source: expectEnum(object.source, credentialSources, `${name}.source`),
    refreshed_at: expectTimestamp(object.refreshed_at, `${name}.refreshed_at`),
    windows: object.windows.map((window, index) =>
      parseQuotaWindow(window, `${name}.windows[${index}]`),
    ),
  };
  if (object.available_reset_credits !== undefined) {
    quota.available_reset_credits = expectInteger(
      object.available_reset_credits,
      `${name}.available_reset_credits`,
      0,
    );
  }
  return quota;
}

function parseQuotaWindow(value: unknown, name: string): QuotaWindow {
  const object = expectObject(value, name);
  expectKeys(
    object,
    ["name"],
    ["used_percent", "window_duration_minutes", "resets_at"],
  );
  const window: QuotaWindow = {
    name: expectString(object.name, `${name}.name`, 64),
  };
  if (object.used_percent !== undefined) {
    const used = expectInteger(object.used_percent, `${name}.used_percent`, 0);
    if (used > 100) {
      throw new ContractError(
        "invalid_field",
        `${name}.used_percent must be <= 100`,
      );
    }
    window.used_percent = used;
  }
  if (object.window_duration_minutes !== undefined) {
    window.window_duration_minutes = expectInteger(
      object.window_duration_minutes,
      `${name}.window_duration_minutes`,
      0,
    );
  }
  if (object.resets_at !== undefined) {
    window.resets_at = expectTimestamp(object.resets_at, `${name}.resets_at`);
  }
  return window;
}

function canonicalAuthSnapshot(snapshot: AuthSnapshot): AuthSnapshot {
  const canonical = {
    provider: snapshot.provider,
    state: snapshot.state,
    source: snapshot.source,
    checked_at: snapshot.checked_at,
    ...(snapshot.account_hint !== undefined
      ? { account_hint: snapshot.account_hint }
      : {}),
    ...(snapshot.plan_type !== undefined
      ? { plan_type: snapshot.plan_type }
      : {}),
    ...(snapshot.auth_mode !== undefined
      ? { auth_mode: snapshot.auth_mode }
      : {}),
    access_token_present: snapshot.access_token_present,
    refresh_token_present: snapshot.refresh_token_present,
    ...(snapshot.access_token_expires_at !== undefined
      ? { access_token_expires_at: snapshot.access_token_expires_at }
      : {}),
    ...(snapshot.quota !== undefined
      ? {
          quota: {
            state: snapshot.quota.state,
            source: snapshot.quota.source,
            refreshed_at: snapshot.quota.refreshed_at,
            windows: snapshot.quota.windows.map((window) => {
              const canonicalWindow: QuotaWindow = { name: window.name };
              if (window.used_percent !== undefined) {
                canonicalWindow.used_percent = window.used_percent;
              }
              if (window.window_duration_minutes !== undefined) {
                canonicalWindow.window_duration_minutes =
                  window.window_duration_minutes;
              }
              if (window.resets_at !== undefined) {
                canonicalWindow.resets_at = window.resets_at;
              }
              return canonicalWindow;
            }),
            ...(snapshot.quota.available_reset_credits !== undefined
              ? {
                  available_reset_credits:
                    snapshot.quota.available_reset_credits,
                }
              : {}),
          },
        }
      : {}),
    ...(snapshot.error_code !== undefined
      ? { error_code: snapshot.error_code }
      : {}),
  } as AuthSnapshot;
  return canonical;
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

function expectProtocolVersion(value: unknown): number {
  const version = expectInteger(value, "protocol_version", 1);
  if (version !== SYNC_PROTOCOL_VERSION) {
    throw new ContractError(
      "unsupported_protocol",
      `unsupported protocol version ${version}`,
    );
  }
  return version;
}

function expectIdempotencyKey(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length < 8 ||
    value.length > 128 ||
    !/^[A-Za-z0-9][A-Za-z0-9._:-]+$/.test(value)
  ) {
    throw new ContractError("invalid_field", "idempotency_key is invalid");
  }
  return value;
}

function expectSafeCode(value: unknown, name: string): string {
  if (
    typeof value !== "string" ||
    value.length > 128 ||
    !/^[a-z][a-z0-9_]*$/.test(value)
  ) {
    throw new ContractError("invalid_field", `${name} is invalid`);
  }
  return value;
}

function expectBoolean(value: unknown, name: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError("invalid_field", `${name} must be a boolean`);
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
