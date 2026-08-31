import assert from "node:assert/strict";
import test from "node:test";
import { EventEmitter } from "node:events";

import {
  V2SessionManager,
  type RunnerSessionConnection,
} from "../src/v2-session-manager.js";
import { DualLayerEventStore } from "../src/v2-event-store.js";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import {
  createFrameEnvelope,
  type HandshakePayload,
  type LaunchProfile,
  type StreamDeltaEvent,
} from "@agent-pulse/contracts";

class MockWebSocket extends EventEmitter {
  readyState = 1;
  sentMessages: string[] = [];

  send(data: string): void {
    this.sentMessages.push(data);
  }

  close(): void {
    this.readyState = 3;
    this.emit("close");
  }
}

test("v2 SessionManager registers runner and handles handshake", () => {
  const manager = new V2SessionManager();
  const mockWs = new MockWebSocket() as any;

  const conn = manager.registerRunner("run_01", mockWs);
  assert.equal(conn.runner_id, "run_01");

  const handshakeFrame = createFrameEnvelope("handshake", 1, 0, {
    runner_id: "run_01",
    client_version: "2.0.0",
    min_supported_protocol: 1,
    max_supported_protocol: 2,
    last_seen_server_seq: 0,
    capabilities: {
      providers: ["claude_code", "codex"],
    },
  } as HandshakePayload);

  const response = manager.handleIncomingFrame(conn, handshakeFrame);
  assert.ok(response);
  assert.equal(response.type, "handshake_ack");
  assert.equal(response.seq, 1);
  assert.equal(response.ack_seq, 1);
  assert.equal(conn.handshake_completed, true);
});

test("v2 SessionManager processes stream deltas and aggregates turns in DualLayerEventStore", () => {
  const manager = new V2SessionManager();
  const mockWs = new MockWebSocket() as any;
  const conn = manager.registerRunner("run_01", mockWs);

  const launchProfile: LaunchProfile = {
    workspace_path: "d:/ClaudeSpace/Project/solum",
    workspace_display_name: "Solum",
    target_runner_id: "run_01",
    provider: "claude_code",
    model: {
      model_id: "claude-3-7-sonnet-20250219",
    },
    permissions: {
      approval_mode: "ask_all_edits",
    },
  };

  // 1. Session Launched Event
  manager.handleIncomingFrame(
    conn,
    createFrameEnvelope(
      "event",
      2,
      1,
      {
        event_type: "session_launched",
        hub_session_id: "hub_sess_100",
        provider_session_id: "claude_sess_001",
        launch_profile: launchProfile,
      },
      { hub_session_id: "hub_sess_100" },
    ),
  );

  const session = manager.eventStore.getSession("hub_sess_100");
  assert.ok(session);
  assert.equal(session.provider, "claude_code");
  assert.equal(session.workspace_path, "d:/ClaudeSpace/Project/solum");

  // 2. Turn Started
  manager.handleIncomingFrame(
    conn,
    createFrameEnvelope(
      "event",
      3,
      1,
      {
        event_type: "turn_started",
        turn_id: "turn_1",
        user_prompt: "Build WebSocket Bus",
      },
      { hub_session_id: "hub_sess_100" },
    ),
  );

  // 3. Stream Deltas (Thinking + Text)
  manager.handleIncomingFrame(
    conn,
    createFrameEnvelope(
      "event",
      4,
      1,
      {
        event_type: "stream_delta",
        turn_id: "turn_1",
        block_index: 0,
        delta_type: "thinking",
        delta_content: "Analyzing architecture...",
      } as StreamDeltaEvent,
      { hub_session_id: "hub_sess_100" },
    ),
  );

  manager.handleIncomingFrame(
    conn,
    createFrameEnvelope(
      "event",
      5,
      1,
      {
        event_type: "stream_delta",
        turn_id: "turn_1",
        block_index: 1,
        delta_type: "text",
        delta_content: "Here is the plan.",
      } as StreamDeltaEvent,
      { hub_session_id: "hub_sess_100" },
    ),
  );

  // 4. Turn Completed
  manager.handleIncomingFrame(
    conn,
    createFrameEnvelope(
      "event",
      6,
      1,
      {
        event_type: "turn_completed",
        turn_id: "turn_1",
        token_usage: {
          input_tokens: 50,
          output_tokens: 120,
          thinking_tokens: 30,
        },
      },
      { hub_session_id: "hub_sess_100" },
    ),
  );

  const updatedSession = manager.eventStore.getSession("hub_sess_100");
  assert.ok(updatedSession);
  assert.equal(updatedSession.turns.length, 1);
  assert.equal(
    updatedSession.turns[0]?.thinking_text,
    "Analyzing architecture...",
  );
  assert.equal(updatedSession.turns[0]?.assistant_text, "Here is the plan.");
  assert.equal(updatedSession.turns[0]?.token_usage?.output_tokens, 120);
});

test("v2 SessionManager outbox sliding window and gap replay", () => {
  const manager = new V2SessionManager();
  const mockWs = new MockWebSocket() as any;
  const conn = manager.registerRunner("run_01", mockWs);

  // Send two commands to runner
  manager.sendCommand("run_01", "hub_sess_100", "cmd_01", {
    action: "send_prompt",
    text: "Task 1",
  });
  manager.sendCommand("run_01", "hub_sess_100", "cmd_02", {
    action: "send_prompt",
    text: "Task 2",
  });

  assert.equal(conn.outbox_buffer.length, 2);
  assert.equal(conn.last_sent_seq, 2);

  // Simulate reconnect with last_seen_server_seq = 1 (runner missed seq 2)
  const reconnectHandshake = createFrameEnvelope("handshake", 10, 1, {
    runner_id: "run_01",
    client_version: "2.0.0",
    min_supported_protocol: 1,
    max_supported_protocol: 2,
    last_seen_server_seq: 1,
    capabilities: { providers: ["claude_code"] },
  } as HandshakePayload);

  mockWs.sentMessages = [];
  const ack = manager.handleIncomingFrame(conn, reconnectHandshake);
  assert.ok(ack);

  // The runner should have received handshake_ack AND the replayed frame with is_replay: true
  assert.ok(mockWs.sentMessages.length >= 2);
  const replayed = JSON.parse(
    mockWs.sentMessages[mockWs.sentMessages.length - 1],
  );
  assert.equal(replayed.seq, 2);
  assert.equal(replayed.is_replay, true);
});

test("v2 REST endpoints for capabilities and session lifecycle", async () => {
  const v2Manager = new V2SessionManager();
  const mockWs = new MockWebSocket() as any;
  v2Manager.registerRunner("run_01", mockWs);

  const app = createGatewayApp({
    authenticator: new DevelopmentIdentityAuthenticator(),
    v2Manager,
  });

  // 1. POST /v2/capabilities
  const capRes = await app.inject({
    method: "POST",
    url: "/v2/capabilities",
    headers: { "x-user-id": "dev_user" },
    payload: {
      runner_id: "run_01",
      hostname: "test-box",
      os: "windows",
      architecture: "x86_64",
      providers: [
        {
          provider_id: "claude_code",
          display_name: "Claude Code",
          cli_version: "0.2.29",
          available: true,
          supported_models: [
            {
              id: "claude-3-7-sonnet-20250219",
              display_name: "Claude 3.7 Sonnet",
            },
          ],
          supported_permission_modes: ["ask_all_edits"],
          supported_slash_commands: ["/compact"],
          supports_stream_cancellation: true,
        },
      ],
      reported_at: Date.now(),
    },
  });
  assert.equal(capRes.statusCode, 200);

  // 2. GET /v2/capabilities
  const getCapRes = await app.inject({
    method: "GET",
    url: "/v2/capabilities",
    headers: { "x-user-id": "dev_user" },
  });
  assert.equal(getCapRes.statusCode, 200);
  const caps = JSON.parse(getCapRes.payload);
  assert.equal(caps.capabilities.length, 1);
  assert.equal(caps.capabilities[0].runner_id, "run_01");

  // 3. POST /v2/sessions/launch
  const launchRes = await app.inject({
    method: "POST",
    url: "/v2/sessions/launch",
    headers: { "x-user-id": "dev_user" },
    payload: {
      launch_profile: {
        workspace_path: "d:/ClaudeSpace/Project/solum",
        workspace_display_name: "Solum",
        target_runner_id: "run_01",
        provider: "claude_code",
        model: { model_id: "claude-3-7-sonnet-20250219" },
        permissions: { approval_mode: "ask_all_edits" },
      },
    },
  });
  assert.equal(launchRes.statusCode, 200);
  const launchJson = JSON.parse(launchRes.payload);
  assert.ok(launchJson.hub_session_id);

  // 4. Ingest session_launched event to populate EventStore
  v2Manager.handleIncomingFrame(
    v2Manager.getRunner("run_01")!,
    createFrameEnvelope(
      "event",
      1,
      1,
      {
        event_type: "session_launched",
        hub_session_id: launchJson.hub_session_id,
        provider_session_id: "claude_p_01",
        launch_profile: {
          workspace_path: "d:/ClaudeSpace/Project/solum",
          workspace_display_name: "Solum",
          target_runner_id: "run_01",
          provider: "claude_code",
          model: { model_id: "claude-3-7-sonnet-20250219" },
          permissions: { approval_mode: "ask_all_edits" },
        },
      },
      { hub_session_id: launchJson.hub_session_id },
    ),
  );

  // 5. GET /v2/sessions
  const listRes = await app.inject({
    method: "GET",
    url: "/v2/sessions",
    headers: { "x-user-id": "dev_user" },
  });
  assert.equal(listRes.statusCode, 200);
  const listJson = JSON.parse(listRes.payload);
  assert.equal(listJson.sessions.length, 1);
  assert.equal(listJson.sessions[0].hub_session_id, launchJson.hub_session_id);

  // 6. POST /v2/sessions/:id/prompt
  const promptRes = await app.inject({
    method: "POST",
    url: `/v2/sessions/${launchJson.hub_session_id}/prompt`,
    headers: { "x-user-id": "dev_user" },
    payload: { text: "Run diagnostic" },
  });
  assert.equal(promptRes.statusCode, 200);

  // 7. POST /v2/sessions/:id/approve
  const approveRes = await app.inject({
    method: "POST",
    url: `/v2/sessions/${launchJson.hub_session_id}/approve`,
    headers: { "x-user-id": "dev_user" },
    payload: { approval_id: "appr_01", decision: "approved" },
  });
  assert.equal(approveRes.statusCode, 200);

  // 8. POST /v2/sessions/:id/interrupt
  const interruptRes = await app.inject({
    method: "POST",
    url: `/v2/sessions/${launchJson.hub_session_id}/interrupt`,
    headers: { "x-user-id": "dev_user" },
    payload: { reason: "test_cancel" },
  });
  assert.equal(interruptRes.statusCode, 200);
});
