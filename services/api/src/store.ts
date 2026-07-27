import { isDeepStrictEqual } from "node:util";

import type {
  ActivityStatus,
  AgentType,
  Presence,
  SyncBatch,
  SyncEvent,
} from "@agent-platform/contracts";

export interface RegisteredDevice {
  device_id: string;
  public_key: string;
  owner_id: string;
  created_at: string;
  last_seen_at: string;
}

export interface DeviceView {
  device_id: string;
  created_at: string;
  last_seen_at: string;
  online: boolean;
}

export interface StoredSession extends SyncEvent {
  received_at: string;
}

export interface SessionFilters {
  device_id?: string;
  activity_status?: ActivityStatus;
  presence?: Presence;
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

export interface GatewayStore {
  registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult>;
  ingestBatch(batch: SyncBatch): Promise<number>;
  listDevices(ownerId: string): Promise<DeviceView[]>;
  listSessions(
    ownerId: string,
    filters: SessionFilters,
  ): Promise<StoredSession[]>;
  listSessionEvents(
    ownerId: string,
    filters: SessionEventFilters,
  ): Promise<StoredSession[]>;
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
  readonly #eventsById = new Map<string, StoredSession>();
  readonly #eventIdsBySequence = new Map<string, string>();
  readonly #lastSequenceByDevice = new Map<string, number>();
  readonly #sessions = new Map<string, StoredSession>();
  readonly #now: () => Date;

  constructor(now: () => Date = () => new Date()) {
    this.#now = now;
  }

  async registerDevice(
    ownerId: string,
    deviceId: string,
    publicKey: string,
  ): Promise<DeviceRegistrationResult> {
    const existing = this.#devices.get(deviceId);
    if (existing !== undefined) {
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

  async ingestBatch(batch: SyncBatch): Promise<number> {
    const device = this.#devices.get(batch.device_id);
    if (device === undefined) {
      throw new GatewayStoreError(
        "device_not_registered",
        "device is not registered",
      );
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

    const receivedAt = this.#now().toISOString();
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
      const current = this.#sessions.get(key);
      if (current === undefined || shouldReplaceSnapshot(current, event)) {
        this.#sessions.set(key, { ...event, received_at: receivedAt });
      }
    }
    device.last_seen_at = receivedAt;
    return newEvents.length;
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
        online: Date.parse(device.last_seen_at) >= offlineBefore,
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
}

function sessionKey(event: SyncEvent): string {
  return `${event.device_id}:${event.agent_type}:${event.agent_session_id}`;
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
