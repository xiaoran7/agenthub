import type {
  ActivityStatus,
  AgentType,
  AuthSnapshot,
  AuthSyncBatch,
  DeviceHeartbeat,
  Presence,
  SyncBatch,
  SyncEvent,
  WaitingReason,
} from "@agent-pulse/contracts";

import type { PushSubscription } from "./push.js";
import type { AccountStore, AuditRecordInput } from "./account-types.js";

export interface StoredPushSubscription extends PushSubscription {
  subscriber_user_id?: string;
}

export interface RegisteredDevice {
  device_id: string;
  public_key: string;
  owner_id: string;
  created_at: string;
  last_seen_at: string;
  revoked_at?: string;
}

export interface DeviceView {
  device_id: string;
  created_at: string;
  last_seen_at: string;
  online: boolean;
  revoked: boolean;
}

export interface StoredSession extends SyncEvent {
  received_at: string;
}

export interface StoredAuthSnapshot extends AuthSnapshot {
  device_id: string;
  received_at: string;
}

export interface SessionFilters {
  device_id?: string;
  agent_type?: AgentType;
  activity_status?: ActivityStatus;
  presence?: Presence;
}

export interface AuthFilters {
  device_id?: string;
}

export interface SessionEventFilters {
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  limit: number;
}

export interface DeviceRegistrationResult {
  device: RegisteredDevice;
  created: boolean;
}

/**
 * A session whose stored snapshot changed activity status while ingesting one
 * batch. Only the whitelisted metadata already visible in the read panel is
 * carried here; it feeds the change stream and push notifications, which must
 * not widen the sync field policy.
 */
export interface SessionStatusChange {
  device_id: string;
  agent_type: AgentType;
  agent_session_id: string;
  project_display_name: string;
  presence: Presence;
  activity_status: ActivityStatus;
  waiting_reason?: WaitingReason;
  observed_at: string;
  /** Snapshot status before the batch; absent when the session is new. */
  previous_activity_status?: ActivityStatus;
}

export interface IngestResult {
  /** Number of events stored; repeated batches accept zero. */
  accepted: number;
  owner_id: string;
  changes: SessionStatusChange[];
}

export interface AuthIngestResult {
  /** Number of snapshots inserted or updated; repeated snapshots accept zero. */
  accepted: number;
  owner_id: string;
  snapshots: StoredAuthSnapshot[];
}

export interface HeartbeatResult {
  accepted: boolean;
  owner_id: string;
  last_seen_at: string;
}

export interface GatewayStore {
  getRegisteredDevice(deviceId: string): Promise<RegisteredDevice | undefined>;
  /**
   * Resolves many devices in one round trip. The remote console re-checks
   * device ownership on every poll, so this must not degrade to a serial loop
   * over {@link GatewayStore.getRegisteredDevice}.
   */
  getRegisteredDevices(
    deviceIds: readonly string[],
  ): Promise<Map<string, RegisteredDevice>>;
  setAuditRecorder?(recorder: Pick<AccountStore, "recordAuditEvent">): void;
  createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
    actorUserId?: string,
  ): Promise<void>;
  registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult>;
  revokeDevice(
    ownerId: string,
    deviceId: string,
    actorUserId?: string,
  ): Promise<boolean>;
  savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
    subscriberUserId?: string,
  ): Promise<void>;
  deletePushSubscription(
    ownerId: string,
    endpoint: string,
    subscriberUserId?: string,
  ): Promise<void>;
  deletePushSubscriptionsForSubscriber?(
    ownerId: string,
    subscriberUserId: string,
  ): Promise<void>;
  listPushSubscriptions(
    ownerId: string,
    subscriberUserId?: string,
  ): Promise<StoredPushSubscription[]>;
  countActiveDevices(ownerId: string): Promise<number>;
  countPendingBindingCodes?(ownerId: string, nowIso: string): Promise<number>;
  pruneExpiredData?(nowIso: string, historyDays?: number): Promise<void> | void;
  registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
    actorUserId?: string,
    allowReactivate?: boolean,
  ): Promise<DeviceRegistrationResult>;
  ingestBatch(batch: SyncBatch): Promise<IngestResult>;
  ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult>;
  recordHeartbeat(
    heartbeat: DeviceHeartbeat,
    receivedAt: string,
  ): Promise<HeartbeatResult>;
  listDevices(ownerId: string): Promise<DeviceView[]>;
  listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]>;
  listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]>;
  listAuth(
    ownerId: string,
    filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]>;
  close?(): Promise<void> | void;
}

export class GatewayStoreError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "GatewayStoreError";
    this.code = code;
  }
}

export class OneTimeBindingCodes {
  readonly #available: Map<string, string>;
  readonly #claims = new Map<string, { deviceId: string; ownerId: string }>();

  constructor(codes: ReadonlyMap<string, string>) {
    this.#available = new Map(codes);
  }

  claim(code: string, deviceId: string): { ownerId: string; isFresh: boolean } {
    const existing = this.#claims.get(code);
    if (existing !== undefined) {
      if (existing.deviceId !== deviceId) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      return { ownerId: existing.ownerId, isFresh: false };
    }
    const ownerId = this.#available.get(code);
    if (ownerId === undefined) {
      throw new GatewayStoreError(
        "invalid_binding_code",
        "binding code is invalid or expired",
      );
    }
    this.#available.delete(code);
    this.#claims.set(code, { deviceId, ownerId });
    return { ownerId, isFresh: true };
  }

  /**
   * Undo a claim whose registration failed so the code is not burned by a
   * rejected attempt (for example a device_conflict from the store).
   */
  release(code: string, deviceId: string): void {
    const existing = this.#claims.get(code);
    if (existing === undefined || existing.deviceId !== deviceId) {
      return;
    }
    this.#claims.delete(code);
    this.#available.set(code, existing.ownerId);
  }
}
