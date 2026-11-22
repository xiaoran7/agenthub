# 开源宣发物料精简包

## 1. 中文社区（V2EX / 科技周刊）
- **标题**：`开源了 AgentHub：让 Claude Code / Codex / Antigravity 支持手机与浏览器远程续聊、Diff 审批的轻量控制面`
- **要点**：
  1. **纯 Rust 单二进制 Runner**：稳态约 `13.5 MB` 内存，内置 `:4200` 控制台、原生 Win32 托盘与冷热分层用量账本；
  2. **主动出站零公网端口**：开发机仅出站 WSS 连自部署 Gateway，API Key 与物理路径不出本机；
  3. **极速静态 Web PWA + 原生 Android**：Vite 8 + Nginx 同源反代（`~8.4 MiB` 内存），随时随地看流式输出、批 Diff 权限或一键急停。
  - 仓库：`https://github.com/xiaoran7/agenthub`（MIT）

## 2. English Communities (`Show HN` / `Reddit` / `X`)
- **Title**: `Show HN: AgentHub – Self-hosted remote Web & mobile control plane for Claude Code, Codex, and Antigravity`
- **Highlights**:
  - **Single-Binary Rust Runner (`agenthub-runner`)**: Native session ownership, embedded `:4200` console, and SQLite cold/hot usage ledger without Electron bloat.
  - **Outbound-Only Security**: Zero open workstation ports; Ed25519 device auth, `ws-*` path redaction, and `client_cmd_id` idempotency.
  - **Lightweight Web PWA & Kotlin Compose Android**: Static Vite + Nginx front-door (`~8.4 MiB` RAM) for live streaming, diff approvals, and interrupts.
  - Repo: `https://github.com/xiaoran7/agenthub`
