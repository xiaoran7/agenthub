/**
 * AgentHub Dual-Layer Event Store & Stream Aggregator (v2.0)
 *
 * Reference: docs/ARCHITECTURE.md §5 & docs/SYNC_PROTOCOL.md §6
 */

import {
  type EventPayload,
  type StreamDeltaEvent,
  type TurnStartedEvent,
  type TurnCompletedEvent,
  type ToolApprovalRequestedEvent,
  type SessionStateChangedEvent,
  type SessionLaunchedEvent,
} from "@agent-pulse/contracts";

export interface NormalizedTurn {
  turn_id: string;
  user_prompt: string;
  started_at: number;
  completed_at?: number | undefined;
  thinking_text: string;
  assistant_text: string;
  tool_calls: Array<{
    approval_id?: string | undefined;
    tool_name: string;
    tool_params: Record<string, unknown>;
    decision?: "approved" | "rejected" | undefined;
    execution_started_at?: number | undefined;
    execution_finished_at?: number | undefined;
    exit_code?: number | undefined;
    duration_ms?: number | undefined;
    error?: string | undefined;
  }>;
  token_usage?:
    | {
        input_tokens?: number | undefined;
        output_tokens?: number | undefined;
        thinking_tokens?: number | undefined;
      }
    | undefined;
}

export interface HubSessionRecord {
  hub_session_id: string;
  provider_session_id: string;
  runner_id: string;
  workspace_path: string;
  workspace_display_name?: string | undefined;
  provider: string;
  model_id: string;
  execution_state: string;
  created_at: number;
  last_active_at: number;
  turns: NormalizedTurn[];
  raw_events_count: number;
}

export class DualLayerEventStore {
  private sessions = new Map<string, HubSessionRecord>();
  private rawEvents = new Map<
    string,
    Array<{ timestamp: number; payload: unknown }>
  >();
  private deltaSubscribers = new Map<
    string,
    Set<(delta: StreamDeltaEvent) => void>
  >();

  createSession(
    hub_session_id: string,
    runner_id: string,
    launch_event: SessionLaunchedEvent,
  ): HubSessionRecord {
    const profile = launch_event.launch_profile;
    const session: HubSessionRecord = {
      hub_session_id,
      provider_session_id: launch_event.provider_session_id,
      runner_id,
      workspace_path: profile.workspace_path,
      workspace_display_name: profile.workspace_display_name,
      provider: profile.provider,
      model_id: profile.model.model_id,
      execution_state: "running",
      created_at: Date.now(),
      last_active_at: Date.now(),
      turns: [],
      raw_events_count: 0,
    };
    this.sessions.set(hub_session_id, session);
    return session;
  }

  getSession(hub_session_id: string): HubSessionRecord | undefined {
    return this.sessions.get(hub_session_id);
  }

  listSessions(filter?: {
    runner_id?: string | undefined;
    workspace_path?: string | undefined;
  }): HubSessionRecord[] {
    let result = Array.from(this.sessions.values());
    if (filter?.runner_id) {
      result = result.filter((s) => s.runner_id === filter.runner_id);
    }
    if (filter?.workspace_path) {
      result = result.filter((s) => s.workspace_path === filter.workspace_path);
    }
    return result.sort((a, b) => b.last_active_at - a.last_active_at);
  }

  updateSessionState(
    hub_session_id: string,
    state_event: SessionStateChangedEvent,
  ): void {
    const session = this.sessions.get(hub_session_id);
    if (!session) return;
    session.execution_state = state_event.current_state;
    session.last_active_at = Date.now();
  }

  recordTurnStarted(
    hub_session_id: string,
    turn_event: TurnStartedEvent,
  ): NormalizedTurn {
    const session = this.sessions.get(hub_session_id);
    const turn: NormalizedTurn = {
      turn_id: turn_event.turn_id,
      user_prompt: turn_event.user_prompt,
      started_at: Date.now(),
      thinking_text: "",
      assistant_text: "",
      tool_calls: [],
    };
    if (session) {
      session.turns.push(turn);
      session.last_active_at = Date.now();
    }
    return turn;
  }

  recordStreamDelta(hub_session_id: string, delta: StreamDeltaEvent): void {
    const session = this.sessions.get(hub_session_id);
    if (session) {
      session.last_active_at = Date.now();
      const currentTurn = session.turns.find(
        (t) => t.turn_id === delta.turn_id,
      );
      if (currentTurn) {
        if (delta.delta_type === "thinking") {
          currentTurn.thinking_text += delta.delta_content;
        } else if (delta.delta_type === "text") {
          currentTurn.assistant_text += delta.delta_content;
        }
      }
    }

    // Broadcast stream delta in real-time to active UI listeners (Web / Mobile / Desktop)
    const subs = this.deltaSubscribers.get(hub_session_id);
    if (subs) {
      for (const listener of subs) {
        try {
          listener(delta);
        } catch {
          // ignore subscriber error
        }
      }
    }
  }

  recordToolApproval(
    hub_session_id: string,
    appr_event: ToolApprovalRequestedEvent,
  ): void {
    const session = this.sessions.get(hub_session_id);
    if (!session) return;
    const currentTurn = session.turns[session.turns.length - 1];
    if (currentTurn) {
      currentTurn.tool_calls.push({
        approval_id: appr_event.approval_id,
        tool_name: appr_event.tool_name,
        tool_params: appr_event.tool_params,
      });
    }
  }

  recordTurnCompleted(
    hub_session_id: string,
    turn_event: TurnCompletedEvent,
  ): void {
    const session = this.sessions.get(hub_session_id);
    if (!session) return;
    const currentTurn = session.turns.find(
      (t) => t.turn_id === turn_event.turn_id,
    );
    if (currentTurn) {
      currentTurn.completed_at = Date.now();
      currentTurn.token_usage = turn_event.token_usage;
    }
    session.last_active_at = Date.now();
  }

  saveRawEvent(hub_session_id: string, payload: unknown): void {
    let list = this.rawEvents.get(hub_session_id);
    if (!list) {
      list = [];
      this.rawEvents.set(hub_session_id, list);
    }
    list.push({ timestamp: Date.now(), payload });
    const session = this.sessions.get(hub_session_id);
    if (session) {
      session.raw_events_count = list.length;
    }
  }

  getRawEvents(
    hub_session_id: string,
  ): Array<{ timestamp: number; payload: unknown }> {
    return this.rawEvents.get(hub_session_id) ?? [];
  }

  subscribeDeltas(
    hub_session_id: string,
    listener: (delta: StreamDeltaEvent) => void,
  ): () => void {
    let subs = this.deltaSubscribers.get(hub_session_id);
    if (!subs) {
      subs = new Set();
      this.deltaSubscribers.set(hub_session_id, subs);
    }
    subs.add(listener);
    return () => {
      subs?.delete(listener);
      if (subs?.size === 0) {
        this.deltaSubscribers.delete(hub_session_id);
      }
    };
  }
}
