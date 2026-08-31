/**
 * AgentHub v2 Session Manager & WebSocket Multiplexing Bus
 *
 * Reference: docs/ARCHITECTURE.md & docs/SYNC_PROTOCOL.md
 */

import {
  type FrameEnvelope,
  type HandshakePayload,
  type HandshakeAckPayload,
  type HeartbeatPayload,
  type HeartbeatAckPayload,
  type LaunchProfile,
  type CommandPayload,
  type EventPayload,
  type StreamDeltaEvent,
  type CapabilityReport,
  createFrameEnvelope,
  validateFrameEnvelope,
  V2_PROTOCOL_VERSION,
  MAX_REPLAY_WINDOW,
} from "@agent-pulse/contracts";
import { type WebSocket } from "ws";
import { DualLayerEventStore } from "./v2-event-store.js";

export interface RunnerSessionConnection {
  runner_id: string;
  ws: WebSocket;
  authenticated: boolean;
  handshake_completed: boolean;
  client_version: string;
  last_received_seq: number;
  last_sent_seq: number;
  capabilities?: CapabilityReport | undefined;
  outbox_buffer: FrameEnvelope[]; // Sliding window buffer for ACK + Replay
  last_heartbeat_at: number;
}

export class V2SessionManager {
  private runners = new Map<string, RunnerSessionConnection>();
  private sessionToRunner = new Map<string, string>();
  private capabilitiesMap = new Map<string, CapabilityReport>();
  readonly eventStore: DualLayerEventStore;

  constructor(eventStore?: DualLayerEventStore) {
    this.eventStore = eventStore ?? new DualLayerEventStore();
  }

  registerRunner(runner_id: string, ws: WebSocket): RunnerSessionConnection {
    const existing = this.runners.get(runner_id);
    const initialBuffer = existing ? existing.outbox_buffer : [];

    const conn: RunnerSessionConnection = {
      runner_id,
      ws,
      authenticated: true,
      handshake_completed: false,
      client_version: "2.0.0",
      last_received_seq: existing ? existing.last_received_seq : 0,
      last_sent_seq: existing ? existing.last_sent_seq : 0,
      outbox_buffer: initialBuffer,
      last_heartbeat_at: Date.now(),
    };

    this.runners.set(runner_id, conn);
    return conn;
  }

  unregisterRunner(runner_id: string): void {
    const conn = this.runners.get(runner_id);
    if (conn) {
      try {
        conn.ws.close();
      } catch {
        // ignore close errors
      }
      this.runners.delete(runner_id);
    }
  }

  getRunner(runner_id: string): RunnerSessionConnection | undefined {
    return this.runners.get(runner_id);
  }

  listRunners(): Array<{
    runner_id: string;
    client_version: string;
    last_heartbeat_at: number;
  }> {
    return Array.from(this.runners.values()).map((c) => ({
      runner_id: c.runner_id,
      client_version: c.client_version,
      last_heartbeat_at: c.last_heartbeat_at,
    }));
  }

  saveCapabilities(report: CapabilityReport): void {
    this.capabilitiesMap.set(report.runner_id, report);
    const conn = this.runners.get(report.runner_id);
    if (conn) {
      conn.capabilities = report;
    }
  }

  getCapabilities(runner_id: string): CapabilityReport | undefined {
    return this.capabilitiesMap.get(runner_id);
  }

  listAllCapabilities(): CapabilityReport[] {
    return Array.from(this.capabilitiesMap.values());
  }

  handleIncomingFrame(
    conn: RunnerSessionConnection,
    raw: unknown,
  ): FrameEnvelope | null {
    const frame = validateFrameEnvelope(raw);

    // Update cumulative received seq
    if (frame.seq > conn.last_received_seq) {
      conn.last_received_seq = frame.seq;
    }

    // Process ACK from runner to evict frames from our server outbox buffer
    if (frame.ack_seq > 0) {
      conn.outbox_buffer = conn.outbox_buffer.filter(
        (f) => f.seq > frame.ack_seq,
      );
    }

    switch (frame.type) {
      case "handshake": {
        const payload = frame.payload as HandshakePayload;
        conn.runner_id = payload.runner_id;
        conn.client_version = payload.client_version;
        conn.handshake_completed = true;

        // Check if runner requested replay
        let replayFrom: number | undefined;
        if (payload.last_seen_server_seq < conn.last_sent_seq) {
          replayFrom = payload.last_seen_server_seq + 1;
        }

        const activeHubSessions = this.eventStore
          .listSessions({ runner_id: conn.runner_id })
          .filter(
            (s) =>
              s.execution_state !== "completed" &&
              s.execution_state !== "terminated",
          )
          .map((s) => s.hub_session_id);

        const ackPayload: HandshakeAckPayload = {
          status: "accepted",
          negotiated_protocol: V2_PROTOCOL_VERSION,
          server_version: "2.0.0",
          last_seen_runner_seq: conn.last_received_seq,
          replay_from_seq: replayFrom,
          active_hub_sessions: activeHubSessions,
        };

        const responseFrame = this.sendFrame(conn, "handshake_ack", ackPayload);

        // If runner missed frames, replay past missed frames in order
        if (replayFrom !== undefined) {
          const toReplay = conn.outbox_buffer.filter(
            (f) => f.seq >= (replayFrom as number) && f.seq < responseFrame.seq,
          );
          for (const replayFrame of toReplay) {
            this.sendRawFrame(conn, { ...replayFrame, is_replay: true });
          }
        }

        return responseFrame;
      }

      case "heartbeat": {
        conn.last_heartbeat_at = Date.now();
        const ackPayload: HeartbeatAckPayload = {
          server_time: Date.now(),
          status: "ok",
        };
        return this.sendFrame(conn, "heartbeat_ack", ackPayload);
      }

      case "event": {
        const event = frame.payload as EventPayload;
        const hubSessionId = frame.hub_session_id;

        if (hubSessionId) {
          this.eventStore.saveRawEvent(hubSessionId, frame);

          if (event.event_type === "session_launched") {
            this.sessionToRunner.set(hubSessionId, conn.runner_id);
            this.eventStore.createSession(hubSessionId, conn.runner_id, event);
          } else if (event.event_type === "stream_delta") {
            this.eventStore.recordStreamDelta(
              hubSessionId,
              event as StreamDeltaEvent,
            );
          } else if (event.event_type === "session_state_changed") {
            this.eventStore.updateSessionState(hubSessionId, event);
          } else if (event.event_type === "tool_approval_requested") {
            this.eventStore.recordToolApproval(hubSessionId, event);
          } else if (event.event_type === "turn_started") {
            this.eventStore.recordTurnStarted(hubSessionId, event);
          } else if (event.event_type === "turn_completed") {
            this.eventStore.recordTurnCompleted(hubSessionId, event);
          }
        }

        // Send back standard cumulative ACK
        return this.sendFrame(conn, "ack", {
          confirmed_seq: conn.last_received_seq,
        });
      }

      case "ack": {
        // Already processed above
        return null;
      }

      default:
        return null;
    }
  }

  sendCommand(
    runner_id: string,
    hub_session_id: string | null,
    cmd_id: string,
    command: CommandPayload,
  ): FrameEnvelope<CommandPayload> {
    const conn = this.runners.get(runner_id);
    if (!conn) {
      throw new Error(`Runner ${runner_id} is not connected`);
    }

    if (hub_session_id) {
      this.sessionToRunner.set(hub_session_id, runner_id);
    }

    return this.sendFrame(conn, "cmd", command, {
      hub_session_id,
      cmd_id,
    });
  }

  private sendFrame<T>(
    conn: RunnerSessionConnection,
    type: FrameEnvelope["type"],
    payload: T,
    options?: {
      hub_session_id?: string | null | undefined;
      cmd_id?: string | null | undefined;
    },
  ): FrameEnvelope<T> {
    conn.last_sent_seq += 1;
    const frame = createFrameEnvelope(
      type,
      conn.last_sent_seq,
      conn.last_received_seq,
      payload,
      options,
    );

    // Buffer in Sliding Window Outbox
    conn.outbox_buffer.push(frame as unknown as FrameEnvelope);
    if (conn.outbox_buffer.length > MAX_REPLAY_WINDOW) {
      conn.outbox_buffer.shift();
    }

    this.sendRawFrame(conn, frame);
    return frame;
  }

  private sendRawFrame(conn: RunnerSessionConnection, frame: unknown): void {
    if (conn.ws.readyState === 1 /* OPEN */) {
      conn.ws.send(JSON.stringify(frame));
    }
  }
}
