## What's Changed in v2.4.96

### 🚀 Highlights & New Features

* **🔍 Collapsible Approval Diff Preview on Approval Cards**:
  - Structured code diff comparison automatically extracted from Claude Code `Edit`/`Write`, Codex, and generic ACP file changes (`item/fileChange/requestApproval`).
  - Strict privacy and payload bounds: local filesystem paths are automatically redacted to `[local_path_redacted]` with a 64 KiB frame cap to protect IPC/WebSocket buffers.
  - Responsive visual diffing: collapsible viewer with `+add -del` stat badges, dual-chunk visual blocks (subtle red deletion, subtle green addition), independent copy buttons, and mobile-friendly wrapping without viewport overflow.
* **🔌 Generic ACP (Agent Client Protocol) stdio Adapter**:
  - Implemented `AcpAppServer` in `crates/runner/src/collectors/acp_app_server.rs`, providing standard JSON-RPC 2.0 stdio client connectivity to any ACP-compliant agent server (`@agentclientprotocol/claude-agent-acp`, `@agentclientprotocol/codex-acp`, or custom community agents).
  - Process tree isolation via `ChildJob` (Windows Job Object `KILL_ON_JOB_CLOSE` and Unix process group) ensuring zero orphaned background processes.
  - Solved architectural impedance match between long-running `session/prompt` RPCs and immediate non-blocking `turn/start` requirements via asynchronous turn tracking.
  - Normalized ACP `session/update` notifications to AgentHub stream events and translated `session/request_permission` directly into approval cards with live diff preview.
  - Multi-provider dynamic routing in `MultiProviderTransport` for `acp_` prefixed sessions and `agentType == "acp"`.
* **🧪 100% Green CI & Contract Alignment**:
  - Added full test coverage for ACP protocol normalization, diff redaction, and multi-provider transport routing (all 225 Rust tests, 71 TypeScript tests, and Web build passing).

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.96-windows-x64.zip` (single executable + quickstart connect guide)
* **Integrity**: `AgentHub-v2.4.96-SHA256.txt`

### 🔗 Attribution
* Upstream layout primitives & tokens from [magpie](https://github.com/yetone/magpie) (MIT).
* Mobile companion UI & accessory row philosophy from [stablyai/orca](https://github.com/stablyai/orca) (Apache 2.0 / MIT).
* Protocol alignment with [Agent Client Protocol](https://agentclientprotocol.com) (ACP).
