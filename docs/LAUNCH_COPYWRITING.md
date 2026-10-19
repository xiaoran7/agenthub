# 全平台宣发与冷启动文案物料包 (Launch Copywriting Kit)

## 1. V2EX（分享创造 `/go/create`）

- **标题**：`写了个让 Claude Code 和 Codex 可以手机远程接管、权限审批的轻量 Web 控制面 AgentHub`
- **正文**：在终端跑 Claude Code 或 Codex 长任务时，离开工位经常遇到任务卡在 Shell/Diff 权限审批无法继续，手机 SSH 体验又极差。为此开源了 **AgentHub**（https://github.com/xiaoran7/agenthub）：
  1. **纯 Rust 单二进制**：`agenthub-runner` 内置本机轻量 Web 控制台（`127.0.0.1:4200`），无 Electron/Tauri 负担；
  2. **主动出站零公网端口**：开发机仅通过 TLS/WSS 主动连接自部署 Gateway，无需公网入站端口或内网穿透；
  3. **手机 PWA 远程续聊与审批**：实时查看流式输出、展开脱敏 Diff、一键点击权限确认或紧急中断；
  4. **官方协议原生对齐**：完整支持 Claude Code、Codex 与 ACP 的 `create`、`resume`、`discover`。

## 2. Reddit (`r/ClaudeAI`, `r/LocalLLaMA`, `r/selfhosted`)

- **Title**: `I built AgentHub: A lightweight, self-hosted Web & Mobile control plane for Claude Code & OpenAI Codex (Outbound-only TLS, zero open ports)`
- **Body**: Being chained to a workstation terminal during 30-minute CLI agent workflows—especially when blocked on permission prompts—pushed me to build **AgentHub** (https://github.com/xiaoran7/agenthub):
  - **Zero Public Port Exposure**: Outbound-only TLS/WSS from `agenthub-runner` to your self-hosted Gateway.
  - **Single-Binary Rust Runner**: Pure Rust 2024 with embedded Web console (`agenthub-runner web --port 4200`).
  - **Mobile Responsive PWA**: Live streaming transcripts, redacted diff approvals, multi-select questions, and turn interrupts.
  - **100% Open Source (MIT)**: Validated with 258 Rust tests and full TypeScript contract suites.

## 3. Hacker News (`Show HN`)

- **Title**: `Show HN: AgentHub – Remote Web and mobile control plane for Claude Code and Codex`
- **Body**: AgentHub (https://github.com/xiaoran7/agenthub) is a remote continuation control plane for official CLI coding agents rather than another agent loop:
  - **Local Runner (`agenthub-runner`)**: Single Rust binary managing native session ownership, single-point transcript parsing, and an embedded local web UI.
  - **Gateway (`@agent-pulse/api`)**: Self-hosted Fastify hub with Ed25519 device verification, SQLite/PG persistence, and idempotent command ledgers.
  - **Outbound-Only Security**: Workstations initiate outbound TLS/WSS connections; local file paths and raw tool payloads are redacted on-host.

## 4. X (Twitter)

> Tired of having your #ClaudeCode or #Codex sessions blocked on permission prompts when away from your desk?
>
> **AgentHub** is a self-hosted remote Web & mobile PWA control plane:
> - Pure Rust single-binary runner (`agenthub-runner`)
> - Outbound-only TLS (zero open inbound ports)
> - Live streaming, diff approvals & instant interrupts
>
> https://github.com/xiaoran7/agenthub #AIAgent #OpenSource #RustLang
