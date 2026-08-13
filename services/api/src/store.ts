import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import type {
  ActivityStatus,
  AgentType,
  AuthSnapshot,
  AuthSyncBatch,
  Presence,
  CreateRemoteTaskInput,
  DeviceTaskClaim,
  DeviceTaskUpdate,
  RemoteTask,
  RemoteTaskStatus,
  SyncBatch,
  SyncEvent,
  WaitingReason,
} from "@agent-pulse/contracts";

import type { PushSubscription } from "./push.js";

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

export interface RemoteTaskFilters {
  target_device_id?: string;
  status?: RemoteTaskStatus;
}

export interface CreateRemoteTaskRecord extends CreateRemoteTaskInput {
  task_id: string;
  created_at: string;
  expires_at: string;
  max_concurrent_tasks: number;
}

export interface RemoteTaskCreateResult {
  task: RemoteTask;
  created: boolean;
}

export interface RemoteTaskMutationResult {
  task: RemoteTask;
  owner_id: string;
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

export interface GatewayStore {
  createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
  ): Promise<void>;
  registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult>;
  revokeDevice(ownerId: string, deviceId: string): Promise<boolean>;
  savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
  ): Promise<void>;
  deletePushSubscription(ownerId: string, endpoint: string): Promise<void>;
  listPushSubscriptions(ownerId: string): Promise<PushSubscription[]>;
  countActiveDevices(ownerId: string): Promise<number>;
  registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult>;
  ingestBatch(batch: SyncBatch): Promise<IngestResult>;
  ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult>;
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
  createRemoteTask(
    ownerId: string,
    input: CreateRemoteTaskRecord,
  ): Promise<RemoteTaskCreateResult>;
  countActiveRemoteTasks(ownerId: string): Promise<number>;
  listRemoteTasks(
    ownerId: string,
    filters: RemoteTaskFilters,
  ): Promise<RemoteTask[]>;
  cancelRemoteTask(ownerId: string, taskId: string): Promise<RemoteTask>;
  claimRemoteTask(
    claim: DeviceTaskClaim,
  ): Promise<RemoteTaskMutationResult | undefined>;
  updateRemoteTask(update: DeviceTaskUpdate): Promise<RemoteTaskMutationResult>;
  close?(): Promise<void>;
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

  claim(code: string, deviceId: string): string {
    const existing = this.#claims.get(code);
    if (existing !== undefined) {
      if (existing.deviceId !== deviceId) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      return existing.ownerId;
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
    return ownerId;
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

export class InMemoryGatewayStore implements GatewayStore {
  readonly #devices = new Map<string, RegisteredDevice>();
  readonly #bindingCodes = new Map<
    string,
    {
      ownerId: string;
      expiresAt: string;
      claimedDeviceId?: string;
    }
  >();
  readonly #pushSubscriptions = new Map<
    string,
    PushSubscription & { owner_id: string }
  >();
  readonly #eventsById = new Map<string, StoredSession>();
  readonly #eventIdsBySequence = new Map<string, string>();
  readonly #lastSequenceByDevice = new Map<string, number>();
  readonly #sessions = new Map<string, StoredSession>();
  readonly #authSnapshots = new Map<string, StoredAuthSnapshot>();
  readonly #remoteTasks = new Map<string, RemoteTask & { owner_id: string }>();
  readonly #taskIdsByIdempotencyKey = new Map<string, string>();
  readonly #now: () => Date;

  constructor(now: () => Date = () => new Date()) {
    this.#now = now;
  }

  async countActiveDevices(ownerId: string): Promise<number> {
    return [...this.#devices.values()].filter(
      (device) =>
        device.owner_id === ownerId && device.revoked_at === undefined,
    ).length;
  }

  async createBindingCode(
    ownerId: string,
    codeDigest: string,
    expiresAt: string,
  ): Promise<void> {
    const now = this.#now().getTime();
    for (const [digest, binding] of this.#bindingCodes) {
      if (
        binding.claimedDeviceId === undefined &&
        Date.parse(binding.expiresAt) <= now
      ) {
        this.#bindingCodes.delete(digest);
      }
    }
    if (this.#bindingCodes.has(codeDigest)) {
      throw new GatewayStoreError(
        "binding_code_conflict",
        "binding code digest already exists",
      );
    }
    this.#bindingCodes.set(codeDigest, { ownerId, expiresAt });
  }

  async registerDeviceWithBindingCode(
    codeDigest: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    const binding = this.#bindingCodes.get(codeDigest);
    if (binding === undefined) {
      throw new GatewayStoreError(
        "invalid_binding_code",
        "binding code is invalid or expired",
      );
    }
    if (binding.claimedDeviceId !== undefined) {
      if (binding.claimedDeviceId !== deviceId) {
        throw new GatewayStoreError(
          "binding_code_used",
          "binding code has already been used",
        );
      }
      return this.registerDevice(binding.ownerId, deviceId, publicKey);
    }
    if (Date.parse(binding.expiresAt) <= this.#now().getTime()) {
      throw new GatewayStoreError(
        "invalid_binding_code",
        "binding code is invalid or expired",
      );
    }
    binding.claimedDeviceId = deviceId;
    try {
      return await this.registerDevice(binding.ownerId, deviceId, publicKey);
    } catch (error) {
      delete binding.claimedDeviceId;
      throw error;
    }
  }

  async revokeDevice(ownerId: string, deviceId: string): Promise<boolean> {
    const device = this.#devices.get(deviceId);
    if (
      device === undefined ||
      device.owner_id !== ownerId ||
      device.revoked_at !== undefined
    ) {
      return false;
    }
    device.revoked_at = this.#now().toISOString();
    return true;
  }

  async savePushSubscription(
    ownerId: string,
    subscription: PushSubscription,
  ): Promise<void> {
    // The endpoint identifies one browser installation. Re-subscribing under a
    // different user moves it: the browser now belongs to that user.
    this.#pushSubscriptions.set(subscription.endpoint, {
      ...subscription,
      owner_id: ownerId,
    });
  }

  async deletePushSubscription(
    ownerId: string,
    endpoint: string,
  ): Promise<void> {
    if (this.#pushSubscriptions.get(endpoint)?.owner_id === ownerId) {
      this.#pushSubscriptions.delete(endpoint);
    }
  }

  async listPushSubscriptions(ownerId: string): Promise<PushSubscription[]> {
    return [...this.#pushSubscriptions.values()]
      .filter((subscription) => subscription.owner_id === ownerId)
      .map(({ owner_id: _ownerId, ...subscription }) => subscription);
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    const existing = this.#devices.get(deviceId);
    if (existing !== undefined) {
      if (existing.revoked_at !== undefined) {
        throw new GatewayStoreError("device_revoked", "device is revoked");
      }
      if (existing.public_key !== publicKey || existing.owner_id !== ownerId) {
        throw new GatewayStoreError(
          "device_conflict",
          "device is already registered with different credentials",
        );
      }
      return { device: existing, created: false };
    }
    const now = this.#now().toISOString();
    const device = {
      device_id: deviceId,
      public_key: publicKey,
      owner_id: ownerId,
      created_at: now,
      last_seen_at: now,
    };
    this.#devices.set(deviceId, device);
    return { device, created: true };
  }

  async ingestBatch(batch: SyncBatch): Promise<IngestResult> {
    const device = this.#devices.get(batch.device_id);
    if (device === undefined) {
      throw new GatewayStoreError(
        "device_not_registered",
        "device is not registered",
      );
    }
    if (device.revoked_at !== undefined) {
      throw new GatewayStoreError("device_revoked", "device is revoked");
    }
    if (device.public_key !== batch.public_key) {
      throw new GatewayStoreError(
        "device_key_mismatch",
        "batch public key does not match registered device",
      );
    }

    const newEvents: SyncEvent[] = [];
    for (const event of batch.events) {
      const existingById = this.#eventsById.get(event.event_id);
      if (existingById !== undefined) {
        const { received_at: _receivedAt, ...existingEvent } = existingById;
        if (!isDeepStrictEqual(existingEvent, event)) {
          throw new GatewayStoreError(
            "event_conflict",
            "event ID was reused with different data",
          );
        }
        continue;
      }
      const sequenceKey = `${event.device_id}:${event.sequence}`;
      const eventIdAtSequence = this.#eventIdsBySequence.get(sequenceKey);
      if (
        eventIdAtSequence !== undefined &&
        eventIdAtSequence !== event.event_id
      ) {
        throw new GatewayStoreError(
          "sequence_conflict",
          "device sequence was reused by another event",
        );
      }
      newEvents.push(event);
    }

    const lastSequence = this.#lastSequenceByDevice.get(batch.device_id) ?? 0;
    if (newEvents.length > 0 && newEvents[0]?.sequence !== lastSequence + 1) {
      throw new GatewayStoreError(
        "sequence_gap",
        "new events do not continue the stored device sequence",
      );
    }

    // Snapshot the pre-batch status of every touched session so the reported
    // transition is "before this batch -> after this batch", not one entry per
    // intermediate event.
    const before = new Map<string, ActivityStatus>();
    for (const event of newEvents) {
      const key = sessionKey(event);
      if (before.has(key)) {
        continue;
      }
      const current = this.#sessions.get(key);
      if (current !== undefined) {
        before.set(key, current.payload.state.activity_status);
      }
    }

    const receivedAt = this.#now().toISOString();
    const touched = new Set<string>();
    for (const event of newEvents) {
      this.#eventsById.set(event.event_id, {
        ...event,
        received_at: receivedAt,
      });
      this.#eventIdsBySequence.set(
        `${event.device_id}:${event.sequence}`,
        event.event_id,
      );
      this.#lastSequenceByDevice.set(event.device_id, event.sequence);
      const key = sessionKey(event);
      touched.add(key);
      const current = this.#sessions.get(key);
      if (current === undefined || shouldReplaceSnapshot(current, event)) {
        this.#sessions.set(key, { ...event, received_at: receivedAt });
      }
    }
    device.last_seen_at = receivedAt;

    const changes: SessionStatusChange[] = [];
    for (const key of touched) {
      const snapshot = this.#sessions.get(key);
      if (snapshot === undefined) {
        continue;
      }
      const previous = before.get(key);
      if (previous === snapshot.payload.state.activity_status) {
        continue;
      }
      changes.push(sessionStatusChange(snapshot, previous));
    }
    return {
      accepted: newEvents.length,
      owner_id: device.owner_id,
      changes,
    };
  }

  async ingestAuthBatch(batch: AuthSyncBatch): Promise<AuthIngestResult> {
    const device = this.#devices.get(batch.device_id);
    if (device === undefined) {
      throw new GatewayStoreError(
        "device_not_registered",
        "device is not registered",
      );
    }
    if (device.revoked_at !== undefined) {
      throw new GatewayStoreError("device_revoked", "device is revoked");
    }
    if (device.public_key !== batch.public_key) {
      throw new GatewayStoreError(
        "device_key_mismatch",
        "auth batch public key does not match registered device",
      );
    }

    const receivedAt = this.#now().toISOString();
    const accepted: StoredAuthSnapshot[] = [];
    let acceptedCount = 0;
    for (const snapshot of batch.snapshots) {
      const key = `${batch.device_id}:${snapshot.provider}`;
      const existing = this.#authSnapshots.get(key);
      if (existing !== undefined) {
        const {
          device_id: _deviceId,
          received_at: _receivedAt,
          ...existingSnapshot
        } = existing;
        const checkedAt = Date.parse(existing.checked_at);
        const incomingCheckedAt = Date.parse(snapshot.checked_at);
        if (
          checkedAt > incomingCheckedAt ||
          (checkedAt === incomingCheckedAt &&
            isDeepStrictEqual(existingSnapshot, snapshot))
        ) {
          accepted.push(existing);
          continue;
        }
      }
      const stored: StoredAuthSnapshot = {
        ...snapshot,
        device_id: batch.device_id,
        received_at: receivedAt,
      };
      this.#authSnapshots.set(key, stored);
      accepted.push(stored);
      acceptedCount += 1;
    }
    device.last_seen_at = receivedAt;
    return {
      accepted: acceptedCount,
      owner_id: device.owner_id,
      snapshots: accepted,
    };
  }

  async listDevices(ownerId: string): Promise<DeviceView[]> {
    const offlineBefore = this.#now().getTime() - 45_000;
    return [...this.#devices.values()]
      .filter((device) => device.owner_id === ownerId)
      .sort((left, right) => left.device_id.localeCompare(right.device_id))
      .map((device) => ({
        device_id: device.device_id,
        created_at: device.created_at,
        last_seen_at: device.last_seen_at,
        online:
          device.revoked_at === undefined &&
          Date.parse(device.last_seen_at) >= offlineBefore,
        revoked: device.revoked_at !== undefined,
      }));
  }

  async listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]> {
    const ownedDevices = new Set(
      [...this.#devices.values()]
        .filter((device) => device.owner_id === ownerId)
        .map((device) => device.device_id),
    );
    return [...this.#sessions.values()]
      .filter(
        (session) =>
          ownedDevices.has(session.device_id) &&
          (filters.device_id === undefined ||
            session.device_id === filters.device_id) &&
          (filters.agent_type === undefined ||
            session.agent_type === filters.agent_type) &&
          (filters.activity_status === undefined ||
            session.payload.state.activity_status ===
              filters.activity_status) &&
          (filters.presence === undefined ||
            session.payload.state.presence === filters.presence),
      )
      .sort((left, right) => {
        const time =
          Date.parse(right.observed_at) - Date.parse(left.observed_at);
        return time === 0 ? right.sequence - left.sequence : time;
      });
  }

  async listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]> {
    const device = this.#devices.get(filters.device_id);
    if (device === undefined || device.owner_id !== ownerId) {
      return [];
    }
    return [...this.#eventsById.values()]
      .filter(
        (event) =>
          event.device_id === filters.device_id &&
          event.agent_type === filters.agent_type &&
          event.agent_session_id === filters.agent_session_id,
      )
      .sort((left, right) => right.sequence - left.sequence)
      .slice(0, filters.limit);
  }

  async listAuth(
    ownerId: string,
    filters: AuthFilters,
  ): Promise<StoredAuthSnapshot[]> {
    const ownedDevices = new Set(
      [...this.#devices.values()]
        .filter((device) => device.owner_id === ownerId)
        .map((device) => device.device_id),
    );
    return [...this.#authSnapshots.values()]
      .filter(
        (snapshot) =>
          ownedDevices.has(snapshot.device_id) &&
          (filters.device_id === undefined ||
            snapshot.device_id === filters.device_id),
      )
      .sort((left, right) => {
        const time = Date.parse(right.checked_at) - Date.parse(left.checked_at);
        return time === 0 ? left.provider.localeCompare(right.provider) : time;
      });
  }

  async createRemoteTask(
    ownerId: string,
    input: CreateRemoteTaskRecord,
  ): Promise<RemoteTaskCreateResult> {
    this.#expireRemoteTasks();
    const device = this.#devices.get(input.target_device_id);
    if (
      device === undefined ||
      device.owner_id !== ownerId ||
      device.revoked_at !== undefined
    ) {
      throw new GatewayStoreError(
        "device_not_owned",
        "target device is unavailable",
      );
    }
    const idempotencyIndex = `${ownerId}:${input.idempotency_key}`;
    const existingId = this.#taskIdsByIdempotencyKey.get(idempotencyIndex);
    if (existingId !== undefined) {
      const existing = this.#remoteTasks.get(existingId);
      if (existing === undefined) throw new Error("task index is inconsistent");
      if (!sameTaskCreation(existing, input)) {
        throw new GatewayStoreError(
          "idempotency_conflict",
          "idempotency key was reused with different task data",
        );
      }
      return { task: publicRemoteTask(existing), created: false };
    }
    if (
      [...this.#remoteTasks.values()].filter(
        (task) => task.owner_id === ownerId && isActiveTaskStatus(task.status),
      ).length >= input.max_concurrent_tasks
    ) {
      throw new GatewayStoreError(
        "task_limit_reached",
        "subscription concurrent task limit reached",
      );
    }
    const task: RemoteTask & { owner_id: string } = {
      task_id: input.task_id,
      owner_id: ownerId,
      target_device_id: input.target_device_id,
      agent_type: input.agent_type,
      prompt: input.prompt,
      ...(input.project_fingerprint === undefined
        ? {}
        : { project_fingerprint: input.project_fingerprint }),
      idempotency_key: input.idempotency_key,
      execution_policy: input.execution_policy,
      status: "queued",
      version: 1,
      created_at: input.created_at,
      expires_at: input.expires_at,
      updated_at: input.created_at,
    };
    this.#remoteTasks.set(task.task_id, task);
    this.#taskIdsByIdempotencyKey.set(idempotencyIndex, task.task_id);
    return { task: publicRemoteTask(task), created: true };
  }

  async countActiveRemoteTasks(ownerId: string): Promise<number> {
    this.#expireRemoteTasks();
    return [...this.#remoteTasks.values()].filter(
      (task) => task.owner_id === ownerId && isActiveTaskStatus(task.status),
    ).length;
  }

  async listRemoteTasks(
    ownerId: string,
    filters: RemoteTaskFilters,
  ): Promise<RemoteTask[]> {
    this.#expireRemoteTasks();
    return [...this.#remoteTasks.values()]
      .filter(
        (task) =>
          task.owner_id === ownerId &&
          (filters.target_device_id === undefined ||
            task.target_device_id === filters.target_device_id) &&
          (filters.status === undefined || task.status === filters.status),
      )
      .sort((left, right) => right.created_at.localeCompare(left.created_at))
      .map(publicRemoteTask);
  }

  async cancelRemoteTask(ownerId: string, taskId: string): Promise<RemoteTask> {
    this.#expireRemoteTasks();
    const task = this.#remoteTasks.get(taskId);
    if (task === undefined || task.owner_id !== ownerId) {
      throw new GatewayStoreError("task_not_found", "task was not found");
    }
    if (task.status === "canceled") return publicRemoteTask(task);
    if (!isActiveTaskStatus(task.status)) {
      throw new GatewayStoreError("task_terminal", "task is already terminal");
    }
    transitionRemoteTask(task, "canceled", this.#now().toISOString());
    return publicRemoteTask(task);
  }

  async claimRemoteTask(
    claim: DeviceTaskClaim,
  ): Promise<RemoteTaskMutationResult | undefined> {
    const device = this.#requireTaskDevice(claim.device_id, claim.public_key);
    this.#expireRemoteTasks();
    const existing = [...this.#remoteTasks.values()]
      .filter(
        (task) =>
          task.owner_id === device.owner_id &&
          task.target_device_id === device.device_id &&
          ["delivered", "accepted", "waiting_user", "running"].includes(
            task.status,
          ),
      )
      .sort((left, right) =>
        left.created_at.localeCompare(right.created_at),
      )[0];
    if (existing !== undefined) {
      return { task: publicRemoteTask(existing), owner_id: device.owner_id };
    }
    const queued = [...this.#remoteTasks.values()]
      .filter(
        (task) =>
          task.owner_id === device.owner_id &&
          task.target_device_id === device.device_id &&
          task.status === "queued",
      )
      .sort((left, right) =>
        left.created_at.localeCompare(right.created_at),
      )[0];
    if (queued === undefined) return undefined;
    transitionRemoteTask(queued, "delivered", this.#now().toISOString());
    return { task: publicRemoteTask(queued), owner_id: device.owner_id };
  }

  async updateRemoteTask(
    update: DeviceTaskUpdate,
  ): Promise<RemoteTaskMutationResult> {
    const device = this.#requireTaskDevice(update.device_id, update.public_key);
    this.#expireRemoteTasks();
    const task = this.#remoteTasks.get(update.task_id);
    if (
      task === undefined ||
      task.owner_id !== device.owner_id ||
      task.target_device_id !== device.device_id
    ) {
      throw new GatewayStoreError("task_not_found", "task was not found");
    }
    if (
      task.version === update.expected_version + 1 &&
      sameTaskUpdate(task, update)
    ) {
      return { task: publicRemoteTask(task), owner_id: device.owner_id };
    }
    if (task.version !== update.expected_version) {
      throw new GatewayStoreError(
        "task_version_conflict",
        "task version is stale",
      );
    }
    if (!canDeviceTransition(task.status, update.status)) {
      throw new GatewayStoreError(
        "invalid_task_transition",
        "task transition is invalid",
      );
    }
    transitionRemoteTask(task, update.status, update.updated_at, update);
    return { task: publicRemoteTask(task), owner_id: device.owner_id };
  }

  #requireTaskDevice(deviceId: string, publicKey: string): RegisteredDevice {
    const device = this.#devices.get(deviceId);
    if (device === undefined) {
      throw new GatewayStoreError(
        "device_not_registered",
        "device is not registered",
      );
    }
    if (device.revoked_at !== undefined) {
      throw new GatewayStoreError("device_revoked", "device is revoked");
    }
    if (device.public_key !== publicKey) {
      throw new GatewayStoreError(
        "device_key_mismatch",
        "device key does not match",
      );
    }
    return device;
  }

  #expireRemoteTasks(): void {
    const now = this.#now();
    for (const task of this.#remoteTasks.values()) {
      if (
        isActiveTaskStatus(task.status) &&
        Date.parse(task.expires_at) <= now.getTime()
      ) {
        transitionRemoteTask(task, "expired", now.toISOString());
      }
    }
  }
}

export function bindingCodeDigest(code: string): string {
  return createHash("sha256").update(code, "utf8").digest("hex");
}

function sessionKey(event: SyncEvent): string {
  return `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
}

export function sessionStatusChange(
  event: SyncEvent,
  previousActivityStatus: ActivityStatus | undefined,
): SessionStatusChange {
  const change: SessionStatusChange = {
    device_id: event.device_id,
    agent_type: event.agent_type,
    agent_session_id: event.agent_session_id,
    project_display_name: event.payload.project.display_name,
    presence: event.payload.state.presence,
    activity_status: event.payload.state.activity_status,
    observed_at: event.observed_at,
  };
  if (event.payload.state.waiting_reason !== undefined) {
    change.waiting_reason = event.payload.state.waiting_reason;
  }
  if (previousActivityStatus !== undefined) {
    change.previous_activity_status = previousActivityStatus;
  }
  return change;
}

function shouldReplaceSnapshot(
  current: StoredSession,
  incoming: SyncEvent,
): boolean {
  const currentTime = Date.parse(current.observed_at);
  const incomingTime = Date.parse(incoming.observed_at);
  if (incomingTime !== currentTime) {
    return incomingTime > currentTime;
  }
  const confidenceRank: Record<SyncEvent["confidence"], number> = {
    unknown: 0,
    inferred: 1,
    observed: 2,
    authoritative: 3,
  };
  if (
    confidenceRank[incoming.confidence] !== confidenceRank[current.confidence]
  ) {
    return (
      confidenceRank[incoming.confidence] > confidenceRank[current.confidence]
    );
  }
  return incoming.sequence > current.sequence;
}

const ACTIVE_TASK_STATUSES = new Set<RemoteTaskStatus>([
  "queued",
  "delivered",
  "accepted",
  "running",
  "waiting_user",
]);

export function isActiveTaskStatus(status: RemoteTaskStatus): boolean {
  return ACTIVE_TASK_STATUSES.has(status);
}

export function canDeviceTransition(
  from: RemoteTaskStatus,
  to: RemoteTaskStatus,
): boolean {
  const allowed: Partial<
    Record<RemoteTaskStatus, readonly RemoteTaskStatus[]>
  > = {
    delivered: ["accepted", "canceled", "failed"],
    accepted: ["running", "waiting_user", "canceled", "failed"],
    running: ["waiting_user", "succeeded", "failed", "canceled"],
    waiting_user: ["running", "succeeded", "failed", "canceled"],
  };
  return allowed[from]?.includes(to) ?? false;
}

function transitionRemoteTask(
  task: RemoteTask,
  status: RemoteTaskStatus,
  updatedAt: string,
  update?: DeviceTaskUpdate,
): void {
  task.status = status;
  task.version += 1;
  task.updated_at = updatedAt;
  if (status === "delivered") task.delivered_at = updatedAt;
  if (status === "running" && task.started_at === undefined) {
    task.started_at = updatedAt;
  }
  if (["succeeded", "failed", "canceled", "expired"].includes(status)) {
    task.completed_at = updatedAt;
  }
  if (update?.agent_session_id !== undefined) {
    task.agent_session_id = update.agent_session_id;
  }
  if (update?.result_summary !== undefined) {
    task.result_summary = update.result_summary;
  }
  if (update?.error_code !== undefined) task.error_code = update.error_code;
}

function publicRemoteTask(
  task: RemoteTask & { owner_id?: string },
): RemoteTask {
  const { owner_id: _ownerId, ...visible } = task;
  return structuredClone(visible);
}

function sameTaskCreation(
  task: RemoteTask,
  input: CreateRemoteTaskRecord,
): boolean {
  return (
    task.target_device_id === input.target_device_id &&
    task.agent_type === input.agent_type &&
    task.prompt === input.prompt &&
    task.project_fingerprint === input.project_fingerprint &&
    task.execution_policy === input.execution_policy &&
    Date.parse(task.expires_at) - Date.parse(task.created_at) ===
      Date.parse(input.expires_at) - Date.parse(input.created_at)
  );
}

function sameTaskUpdate(task: RemoteTask, update: DeviceTaskUpdate): boolean {
  return (
    task.status === update.status &&
    task.updated_at === update.updated_at &&
    task.agent_session_id === update.agent_session_id &&
    task.result_summary === update.result_summary &&
    task.error_code === update.error_code
  );
}
