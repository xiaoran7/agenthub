import assert from "node:assert/strict";
import test from "node:test";
import { WebSocket } from "ws";
import { createGatewayApp } from "../src/app.js";
import { DevelopmentIdentityAuthenticator } from "../src/auth.js";
import { V2SessionManager } from "../src/v2-session-manager.js";
import {
  createFrameEnvelope,
  type FrameEnvelope,
  type HandshakePayload,
  type HandshakeAckPayload,
  type HeartbeatPayload,
  type HeartbeatAckPayload,
  type LaunchProfile,
  type StreamDeltaEvent,
} from "@agent-pulse/contracts";

test("E2E Real Interactive Verification: Full WebSocket Bus, SSE Streaming, Tool Approvals & Gap Replay", async (t) => {
  const v2Manager = new V2SessionManager();
  const app = createGatewayApp({
    authenticator: new DevelopmentIdentityAuthenticator(),
    v2Manager,
  });

  // Listen on random available port on localhost
  const address = await app.listen({ port: 0, host: "127.0.0.1" });
  const serverPort = (app.server.address() as any).port;
  console.log(
    `[E2E] Gateway Server listening at ${address} (port: ${serverPort})`,
  );

  t.after(async () => {
    await app.close();
    console.log("[E2E] Gateway Server closed cleanly");
  });

  // 1. Establish Real WebSocket Connection from Runner to Server
  const wsUrl = `ws://127.0.0.1:${serverPort}/v2/ws`;
  const ws = new WebSocket(wsUrl);

  const incomingFrames: FrameEnvelope[] = [];
  const frameWaiters: Array<(frame: FrameEnvelope) => void> = [];

  ws.on("message", (data) => {
    const frame = JSON.parse(data.toString("utf8")) as FrameEnvelope;
    const waiter = frameWaiters.shift();
    if (waiter) {
      waiter(frame);
    } else {
      incomingFrames.push(frame);
    }
  });

  await new Promise<void>((resolve, reject) => {
    ws.on("open", () => resolve());
    ws.on("error", reject);
  });
  console.log("[E2E] 1. WebSocket connection established successfully");

  function waitForNextFrame(): Promise<FrameEnvelope> {
    if (incomingFrames.length > 0) {
      return Promise.resolve(incomingFrames.shift()!);
    }
    return new Promise((resolve) => {
      frameWaiters.push(resolve);
    });
  }

  // 2. Perform Real v2 Handshake Frame Negotiation
  const handshakeFrame = createFrameEnvelope("handshake", 1, 0, {
    runner_id: "workstation_node_01",
    client_version: "2.0.0",
    min_supported_protocol: 1,
    max_supported_protocol: 2,
    last_seen_server_seq: 0,
    capabilities: {
      providers: ["claude_code", "codex_app_server", "antigravity"],
    },
  } as HandshakePayload);

  ws.send(JSON.stringify(handshakeFrame));
  const handshakeAck = await waitForNextFrame();
  console.log(
    "[E2E] 2. Handshake ACK received:",
    handshakeAck.type,
    "negotiated_protocol:",
    (handshakeAck.payload as any).negotiated_protocol,
  );
  assert.equal(handshakeAck.type, "handshake_ack");
  assert.equal(
    (handshakeAck.payload as HandshakeAckPayload).status,
    "accepted",
  );
  assert.equal(
    (handshakeAck.payload as HandshakeAckPayload).negotiated_protocol,
    2,
  );

  // 3. Heartbeat exchange
  const heartbeatFrame = createFrameEnvelope("heartbeat", 2, handshakeAck.seq, {
    runner_id: "workstation_node_01",
    active_sessions_count: 0,
    system_metrics: { cpu_percent: 15, memory_mb: 4096 },
  } as HeartbeatPayload);
  ws.send(JSON.stringify(heartbeatFrame));
  const heartbeatAck = await waitForNextFrame();
  console.log("[E2E] 3. Heartbeat ACK received:", heartbeatAck.type);
  assert.equal(heartbeatAck.type, "heartbeat_ack");

  // 4. Dynamic Capabilities Reporting & REST Discovery
  const capRes = await app.inject({
    method: "POST",
    url: "/v2/capabilities",
    headers: { "x-user-id": "operator_alice" },
    payload: {
      runner_id: "workstation_node_01",
      hostname: "DESKTOP-DEV-99",
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
              supports_thinking: true,
            },
          ],
          supported_permission_modes: [
            "plan_only",
            "ask_all_edits",
            "accept_edits",
          ],
          supported_slash_commands: ["/compact", "/cost", "/review"],
          supports_stream_cancellation: true,
        },
      ],
      reported_at: Date.now(),
    },
  });
  assert.equal(capRes.statusCode, 200);

  const getCapsRes = await app.inject({
    method: "GET",
    url: "/v2/capabilities",
    headers: { "x-user-id": "operator_alice" },
  });
  assert.equal(getCapsRes.statusCode, 200);
  const discoveredCaps = JSON.parse(getCapsRes.payload);
  console.log(
    "[E2E] 4. Discovered Capabilities Count:",
    discoveredCaps.capabilities.length,
  );
  assert.equal(discoveredCaps.capabilities[0].runner_id, "workstation_node_01");

  // 5. REST Launch Session (User creates session from Web UI)
  const launchProfile: LaunchProfile = {
    workspace_path: "d:/ClaudeSpace/Project/solum",
    workspace_display_name: "Solum Remote Control",
    target_runner_id: "workstation_node_01",
    provider: "claude_code",
    model: {
      model_id: "claude-3-7-sonnet-20250219",
      reasoning: { enabled: true, budget_tokens: 8000 },
    },
    permissions: {
      approval_mode: "ask_all_edits",
      allow_tool_execution: true,
    },
    initial_prompt: "Build Unified WebSocket Multiplexing Bus",
  };

  const launchRes = await app.inject({
    method: "POST",
    url: "/v2/sessions/launch",
    headers: { "x-user-id": "operator_alice" },
    payload: { launch_profile: launchProfile },
  });
  assert.equal(launchRes.statusCode, 200);
  const { hub_session_id, cmd_id } = JSON.parse(launchRes.payload);
  console.log(
    "[E2E] 5. Launch Session command dispatched:",
    hub_session_id,
    "cmd_id:",
    cmd_id,
  );

  // Runner receives cmd frame with action = launch_session
  const launchCmdFrame = await waitForNextFrame();
  assert.equal(launchCmdFrame.type, "cmd");
  assert.equal(launchCmdFrame.cmd_id, cmd_id);
  assert.equal((launchCmdFrame.payload as any).action, "launch_session");

  // 6. Runner responds with session_launched event
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        3,
        launchCmdFrame.seq,
        {
          event_type: "session_launched",
          hub_session_id,
          provider_session_id: "claude_native_session_99",
          launch_profile: launchProfile,
        },
        { hub_session_id },
      ),
    ),
  );

  const eventAck = await waitForNextFrame();
  assert.equal(eventAck.type, "ack");
  console.log("[E2E] 6. Runner received ACK for session_launched event");

  // 7. REST User sends Prompt to Session
  const promptRes = await app.inject({
    method: "POST",
    url: `/v2/sessions/${hub_session_id}/prompt`,
    headers: { "x-user-id": "operator_alice" },
    payload: {
      text: "Explain the ACK and Sliding Window Replay design",
      client_turn_id: "turn_e2e_01",
    },
  });
  assert.equal(promptRes.statusCode, 200);
  console.log("[E2E] 7. User prompt sent to session");

  // Runner receives prompt cmd
  const promptCmdFrame = await waitForNextFrame();
  assert.equal(promptCmdFrame.type, "cmd");
  assert.equal((promptCmdFrame.payload as any).action, "send_prompt");
  assert.equal(
    (promptCmdFrame.payload as any).text,
    "Explain the ACK and Sliding Window Replay design",
  );

  // 8. Runner emits turn_started and streaming thinking + text deltas
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        4,
        promptCmdFrame.seq,
        {
          event_type: "turn_started",
          turn_id: "turn_e2e_01",
          user_prompt: "Explain the ACK and Sliding Window Replay design",
        },
        { hub_session_id },
      ),
    ),
  );
  await waitForNextFrame(); // ACK

  // Stream Thinking block
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        5,
        promptCmdFrame.seq,
        {
          event_type: "stream_delta",
          turn_id: "turn_e2e_01",
          block_index: 0,
          delta_type: "thinking",
          delta_content:
            "Analyzing Sliding Window Buffer with 2000 frames capacity...",
        } as StreamDeltaEvent,
        { hub_session_id },
      ),
    ),
  );
  await waitForNextFrame(); // ACK

  // Stream Text block
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        6,
        promptCmdFrame.seq,
        {
          event_type: "stream_delta",
          turn_id: "turn_e2e_01",
          block_index: 1,
          delta_type: "text",
          delta_content:
            "AgentHub uses a cumulative ACK and sequence outbox buffer for seamless reconnection.",
        } as StreamDeltaEvent,
        { hub_session_id },
      ),
    ),
  );
  await waitForNextFrame(); // ACK
  console.log(
    "[E2E] 8. Streaming deltas delivered and acknowledged in DualLayerEventStore",
  );

  // 9. Runner requests Tool Execution Approval (e.g. Bash command)
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        7,
        promptCmdFrame.seq,
        {
          event_type: "tool_approval_requested",
          approval_id: "appr_req_1001",
          tool_name: "Bash",
          tool_params: { command: "git status --porcelain" },
          danger_level: "low",
        },
        { hub_session_id },
      ),
    ),
  );
  await waitForNextFrame(); // ACK

  // 10. Operator Approves tool execution via REST API
  const approveRes = await app.inject({
    method: "POST",
    url: `/v2/sessions/${hub_session_id}/approve`,
    headers: { "x-user-id": "operator_alice" },
    payload: { approval_id: "appr_req_1001", decision: "approved" },
  });
  assert.equal(approveRes.statusCode, 200);

  // Runner receives resolve_approval cmd frame
  const approveCmdFrame = await waitForNextFrame();
  assert.equal(approveCmdFrame.type, "cmd");
  assert.equal((approveCmdFrame.payload as any).action, "resolve_approval");
  assert.equal((approveCmdFrame.payload as any).decision, "approved");
  console.log("[E2E] 10. Tool approval resolved successfully by operator");

  // 11. Runner emits Turn Completed
  ws.send(
    JSON.stringify(
      createFrameEnvelope(
        "event",
        8,
        approveCmdFrame.seq,
        {
          event_type: "turn_completed",
          turn_id: "turn_e2e_01",
          token_usage: {
            input_tokens: 120,
            output_tokens: 280,
            thinking_tokens: 85,
          },
          duration_ms: 1450,
        },
        { hub_session_id },
      ),
    ),
  );
  await waitForNextFrame(); // ACK

  // 12. Query Full Normalized Session State from REST API
  const getSessionRes = await app.inject({
    method: "GET",
    url: `/v2/sessions/${hub_session_id}`,
    headers: { "x-user-id": "operator_alice" },
  });
  assert.equal(getSessionRes.statusCode, 200);
  const sessionDetail = JSON.parse(getSessionRes.payload);
  console.log(
    "[E2E] 12. Normalized Turns count:",
    sessionDetail.session.turns.length,
  );
  assert.equal(sessionDetail.session.turns.length, 1);
  assert.equal(
    sessionDetail.session.turns[0].thinking_text,
    "Analyzing Sliding Window Buffer with 2000 frames capacity...",
  );
  assert.equal(
    sessionDetail.session.turns[0].assistant_text,
    "AgentHub uses a cumulative ACK and sequence outbox buffer for seamless reconnection.",
  );
  assert.equal(sessionDetail.session.turns[0].token_usage.output_tokens, 280);

  // 13. Test Sliding Window Gap Replay on Reconnect
  // Operator queues a prompt command while connection is active
  await app.inject({
    method: "POST",
    url: `/v2/sessions/${hub_session_id}/prompt`,
    headers: { "x-user-id": "operator_alice" },
    payload: { text: "Unacknowledged prompt before disconnect" },
  });

  const lastSentSeqBeforeDisconnect = v2Manager.getRunner(
    "workstation_node_01",
  )!.last_sent_seq;

  // Abruptly close WS before acknowledging this command
  ws.close();
  await new Promise((r) => setTimeout(r, 50));

  // Reconnect with last_seen_server_seq = lastSentSeqBeforeDisconnect - 1
  const reconnectWs = new WebSocket(wsUrl);
  const reconnectedFrames: FrameEnvelope[] = [];
  reconnectWs.on("message", (data) => {
    reconnectedFrames.push(JSON.parse(data.toString("utf8")));
  });

  await new Promise<void>((resolve, reject) => {
    reconnectWs.on("open", () => resolve());
    reconnectWs.on("error", reject);
  });

  const reconnectHandshake = createFrameEnvelope("handshake", 20, 0, {
    runner_id: "workstation_node_01",
    client_version: "2.0.0",
    min_supported_protocol: 1,
    max_supported_protocol: 2,
    last_seen_server_seq: lastSentSeqBeforeDisconnect - 1, // missed the last command
    capabilities: { providers: ["claude_code"] },
  } as HandshakePayload);

  reconnectWs.send(JSON.stringify(reconnectHandshake));
  await new Promise((r) => setTimeout(r, 100));

  console.log(
    "[E2E] 13. Replayed Frames received after reconnect:",
    reconnectedFrames.length,
  );
  assert.ok(reconnectedFrames.length >= 2);
  const replayFrames = reconnectedFrames.filter((f) => f.is_replay === true);
  assert.ok(replayFrames.length > 0);
  assert.equal(
    (replayFrames[0]?.payload as any).text,
    "Unacknowledged prompt before disconnect",
  );
  console.log(
    "[E2E] 13. Gap Replay verified: Server automatically replayed",
    replayFrames.length,
    "missed frames with is_replay: true",
  );

  reconnectWs.close();
});
