/**
 * AgentHub Sync Protocol v2.0 - Frame Envelope & Multiplexing Contracts
 *
 * Reference: docs/SYNC_PROTOCOL.md
 */

import { ContractError } from "./index.js";

export const V2_PROTOCOL_VERSION = 2 as const;
export const MAX_REPLAY_WINDOW = 2_000 as const;

export const frameTypes = [
  "handshake",
  "handshake_ack",
  "heartbeat",
  "heartbeat_ack",
  "event",
  "cmd",
  "cmd_ack",
  "ack",
  "reconcile_check",
  "reconcile_sync",
  "presence",
] as const;

export type FrameType = (typeof frameTypes)[number];

export interface FrameEnvelope<T = unknown> {
  /** 协议大版本号，固定为 2 */
  ver: 2;
  /** 帧唯一 ID (UUID v4 / ULID / frm_前缀) */
  frame_id: string;
  /** 帧类型 */
  type: FrameType;
  /** 发送方递增的全局序列号 (1, 2, 3...) */
  seq: number;
  /** 发送方当前已确认接收到的对方最大连续序列号 */
  ack_seq: number;
  /** 多路复用会话包装 ID (Runner 级帧此字段为 null 或 undefined) */
  hub_session_id?: string | null | undefined;
  /** 客户端发起的幂等命令追踪 ID (仅在 cmd / cmd_ack / 关联 event 中出现) */
  cmd_id?: string | null | undefined;
  /** 是否为断线重放帧 */
  is_replay?: boolean | undefined;
  /** 发送时间戳 (Unix 毫秒) */
  timestamp: number;
  /** 具体业务负载 */
  payload: T;
}

export interface HandshakePayload {
  runner_id: string;
  client_version: string;
  min_supported_protocol: number;
  max_supported_protocol: number;
  last_seen_server_seq: number;
  capabilities: {
    providers: string[];
    supports_stream_compression?: boolean | undefined;
    max_replay_window?: number | undefined;
  };
}

export interface HandshakeAckPayload {
  status: "accepted" | "rejected";
  negotiated_protocol: number;
  server_version: string;
  last_seen_runner_seq: number;
  replay_from_seq?: number | undefined;
  active_hub_sessions: string[];
  error_message?: string | undefined;
}

export interface HeartbeatPayload {
  runner_id: string;
  active_sessions_count: number;
  system_metrics?:
    | {
        cpu_percent?: number | undefined;
        memory_mb?: number | undefined;
      }
    | undefined;
}

export interface HeartbeatAckPayload {
  server_time: number;
  status: "ok";
}

export interface CumulativeAckPayload {
  /** 接收方确认的最新连续序列号 */
  confirmed_seq: number;
}

export interface PresencePayload {
  client_id: string;
  user_id: string;
  client_type: "web" | "android" | "desktop";
  active_viewing_hub_session_id?: string | null | undefined;
  is_focused: boolean;
}

export function createFrameEnvelope<T>(
  type: FrameType,
  seq: number,
  ack_seq: number,
  payload: T,
  options?: {
    frame_id?: string | undefined;
    hub_session_id?: string | null | undefined;
    cmd_id?: string | null | undefined;
    is_replay?: boolean | undefined;
    timestamp?: number | undefined;
  },
): FrameEnvelope<T> {
  if (seq < 1) {
    throw new ContractError(
      "invalid_field",
      "seq must be a positive integer >= 1",
    );
  }
  if (ack_seq < 0) {
    throw new ContractError(
      "invalid_field",
      "ack_seq must be a non-negative integer >= 0",
    );
  }

  return {
    ver: 2,
    frame_id:
      options?.frame_id ??
      `frm_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 9)}`,
    type,
    seq,
    ack_seq,
    hub_session_id: options?.hub_session_id ?? null,
    cmd_id: options?.cmd_id ?? null,
    is_replay: options?.is_replay ?? false,
    timestamp: options?.timestamp ?? Date.now(),
    payload,
  };
}

export function validateFrameEnvelope(raw: unknown): FrameEnvelope {
  if (typeof raw !== "object" || raw === null) {
    throw new ContractError(
      "invalid_format",
      "frame must be a non-null object",
    );
  }

  const obj = raw as Record<string, unknown>;

  if (obj.ver !== 2) {
    throw new ContractError(
      "invalid_version",
      `unsupported protocol version: ${String(obj.ver)}`,
    );
  }

  if (
    typeof obj.frame_id !== "string" ||
    obj.frame_id.length === 0 ||
    obj.frame_id.length > 128
  ) {
    throw new ContractError(
      "invalid_field",
      "frame_id must be a non-empty string <= 128 chars",
    );
  }

  if (
    typeof obj.type !== "string" ||
    !frameTypes.includes(obj.type as FrameType)
  ) {
    throw new ContractError(
      "invalid_field",
      `invalid frame type: ${String(obj.type)}`,
    );
  }

  if (
    typeof obj.seq !== "number" ||
    !Number.isSafeInteger(obj.seq) ||
    obj.seq < 1
  ) {
    throw new ContractError(
      "invalid_field",
      "seq must be a positive safe integer >= 1",
    );
  }

  if (
    typeof obj.ack_seq !== "number" ||
    !Number.isSafeInteger(obj.ack_seq) ||
    obj.ack_seq < 0
  ) {
    throw new ContractError(
      "invalid_field",
      "ack_seq must be a non-negative safe integer >= 0",
    );
  }

  if (
    typeof obj.timestamp !== "number" ||
    !Number.isFinite(obj.timestamp) ||
    obj.timestamp <= 0
  ) {
    throw new ContractError(
      "invalid_field",
      "timestamp must be a valid positive Unix timestamp",
    );
  }

  if (
    obj.hub_session_id !== undefined &&
    obj.hub_session_id !== null &&
    typeof obj.hub_session_id !== "string"
  ) {
    throw new ContractError(
      "invalid_field",
      "hub_session_id must be a string or null",
    );
  }

  if (
    obj.cmd_id !== undefined &&
    obj.cmd_id !== null &&
    typeof obj.cmd_id !== "string"
  ) {
    throw new ContractError("invalid_field", "cmd_id must be a string or null");
  }

  if (obj.is_replay !== undefined && typeof obj.is_replay !== "boolean") {
    throw new ContractError("invalid_field", "is_replay must be a boolean");
  }

  return raw as FrameEnvelope;
}
