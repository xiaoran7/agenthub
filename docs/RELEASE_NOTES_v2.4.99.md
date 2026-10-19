## What's Changed in v2.4.99

### Highlights & New Features

- **Federated Multi-Agent Library (`crates/runner/src/library.rs`)**: Scans Skills (`~/.claude/skills`, `.claude/skills`, Codex skills with YAML frontmatter & `has_scripts`), MCP Servers (`~/.claude.json`, `~/.codex/config.toml`, `.mcp.json`, `~/.agenthub/mcp.json` via zero-dependency TOML parser `parse_mcp_from_codex_toml`), and Hooks (`PreToolUse`, `PostToolUse`, `SessionStart`, `PermissionRequest`) with cross-agent deduplication.
- **Capabilities View & MCP Registration**: 4 KPI cards (Skills, MCP Servers, Hooks, Agents), dimension/keyword filtering, interactive MCP registration modal (`POST /api/capabilities/mcp`), and hot rescan (`POST /api/capabilities/rescan`).
- **Context Capacity, Multi-Select Questions & Incremental Markdown**: Normalized `{used, window}` context ring across Claude/Codex/ACP, `multi_select` question checkbox submission (`string[]`), and tail-cached incremental Markdown parsing.

### Precompiled Deliverables

- `AgentHub-Runner-v2.4.99-windows-x64.zip` · `AgentHub-v2.4.99-android-debug.apk` · `AgentHub-v2.4.99-SHA256.txt`
