## What's Changed in v2.4.96

### Highlights & New Features

- **Collapsible Approval Diff Preview**: Extracts structured diffs from Claude Code `Edit`/`Write`, Codex, and ACP `item/fileChange/requestApproval`; redacts local paths to `[local_path_redacted]` with a 64 KiB cap and renders `+add -del` visual blocks.
- **Generic ACP stdio Adapter (`crates/runner/src/collectors/acp_app_server.rs`)**: JSON-RPC 2.0 stdio client for ACP-compliant agents (`claude-agent-acp`, `codex-acp`), `ChildJob` process tree isolation, non-blocking `session/prompt` turn tracking, `session/update` normalization, and `MultiProviderTransport` routing (`acp_` prefix / `agentType == "acp"`).
- **Contract & CI Alignment**: Full test coverage for ACP normalization, diff redaction, and multi-provider routing.

### Precompiled Deliverables

- `AgentHub-Runner-v2.4.96-windows-x64.zip` · `AgentHub-v2.4.96-SHA256.txt`
