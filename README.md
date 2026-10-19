# AgentHub

> **The Unified Remote Continuation & Control Plane for Official AI Agents**  
> Seamless Web & mobile management for **Claude Code** and **OpenAI Codex** sessions anywhere, anytime.

<p align="center">
  <a href="README.md"><b>English</b></a> •
  <a href="README_CN.md">简体中文</a> •
  <a href="docs/README.md">Documentation</a> •
  <a href="docs/ARCHITECTURE.md">Architecture</a> •
  <a href="docs/CHANGELOG.md">Changelog</a>
</p>

<p align="center">
  <a href="https://github.com/xiaoran7/agenthub/releases"><img src="https://img.shields.io/badge/version-2.5.2-0284c7.svg?style=flat-square" alt="Version" /></a>
  <a href="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml"><img src="https://img.shields.io/badge/CI-passing-10b981.svg?style=flat-square" alt="CI Status" /></a>
  <a href="https://github.com/xiaoran7/agenthub"><img src="https://img.shields.io/badge/tests-258%20passed-10b981.svg?style=flat-square" alt="Tests" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-64748b.svg?style=flat-square" alt="License" /></a>
  <a href="llms.txt"><img src="https://img.shields.io/badge/llms.txt-standardized-8b5cf6.svg?style=flat-square" alt="llms.txt" /></a>
  <img src="https://img.shields.io/badge/Rust-2024-f97316.svg?style=flat-square" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Node-%3E%3D22-10b981.svg?style=flat-square" alt="Node" />
</p>

---

## Overview

AgentHub hooks directly into official CLI agent runtimes (**Claude Code**, **OpenAI Codex**, and **ACP**-compatible agents) without reinventing another inference loop. Through a single-binary Rust daemon (`agenthub-runner`) establishing outbound-only TLS/WebSocket connections to a self-hosted Gateway, you can inspect live output streams, approve tool permissions with diff previews, continue multi-turn sessions, or trigger emergency stops from any browser or mobile PWA.

- ⚡ **Single-Binary Rust Runner**: Zero-dependency embedded Web Console at `http://127.0.0.1:4200` (sessions, agents, per-request usage ledger, workspaces, and federated Skill/MCP/Hook settings).
- 🌐 **Remote Web & Mobile PWA**: Self-hosted Gateway + responsive Next.js Web client for multi-device session continuation and one-tap permission approvals.
- 🛡️ **Outbound-Only TLS Security**: Workstation connects outwards to Gateway over WSS—zero public inbound ports required, zero provider credentials uploaded, and local paths redacted into `ws-*` workspace IDs.
- 🎯 **Native Official Protocol Fidelity**: Exact `create`, `resume`, and read-only `discover` semantics with single-writer ownership and durable command idempotency.

---

## Architecture

```text
Local Workstation
  Codex / Claude Code / ACP Runtime
          ▲  (Official Protocol)
          ▼
    agenthub-runner (Rust daemon)
      ├─ Embedded Web Console  ──► http://127.0.0.1:4200
      └─ Outbound TLS / WSS    ──► Self-Hosted Gateway (Auth, Ledger, Replay)
                                          │
                                          ▼ (HTTPS / WSS / SSE)
                                   Remote Web Client (PC / Mobile PWA)
```

---

## Quick Start

### 1. Local Runner & Embedded Console

```bash
# Launch embedded local Web Console at http://127.0.0.1:4200
agenthub-runner web

# Or run as a background session daemon
agenthub-runner daemon
```

### 2. Full Stack from Source (Node >= 22, Rust >= 1.80)

```powershell
npm.cmd ci
npm.cmd start --workspace @agent-pulse/api    # Gateway (http://127.0.0.1:3001)
npm.cmd run dev --workspace @agent-pulse/web  # Remote Web UI (http://127.0.0.1:3000)
cargo run -p agenthub-runner -- daemon        # Local Rust Runner
```

### 3. Verification Gates

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

---

## Upstream Attribution & Open Source Traceability

| Upstream Project                                  | License                                | Module / Role in AgentHub                                                                                                                                                                                                                                                                                                                                           |
| :------------------------------------------------ | :------------------------------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [magpie](https://github.com/yetone/magpie)        | MIT License, Copyright (c) 2026 yetone | Embedded Web Console design system & layout primitives (`crates/runner/resources/dashboard.{html,css,js}`), vendor icon set (`crates/runner/resources/icons/` served by `console_icons.rs`), per-request usage ledger view (`usage_ledger.rs`), and federated Multi-Agent Library architecture (Skill, MCP Server, Hook management in `collectors/capabilities.rs`) |
| [stablyai/orca](https://github.com/stablyai/orca) | Apache 2.0 / MIT                       | Mobile Companion UI philosophy, terminal accessory row, and fleet mission-control layout (`apps/android`)                                                                                                                                                                                                                                                           |

> Foundation CSS tokens, vendor marks under `crates/runner/resources/icons/`, and model-family prefix selection are ported from `yetone/magpie` with upstream copyright and MIT license headers preserved intact.

---

## Documentation Index

- **[AGENTS.md](AGENTS.md)**: Project rules, security redlines & commit discipline
- **[docs/README.md](docs/README.md)**: Documentation index, implementation status & validation summary
- **[docs/PRD.md](docs/PRD.md)**: Product scope, session semantics & acceptance criteria
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**: System topology, component roles & ownership boundaries
- **[docs/SYNC_PROTOCOL.md](docs/SYNC_PROTOCOL.md)**: Local IPC v2, Remote WSS protocol, watermarks & idempotency
- **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)**: Systemd, Docker, 1Panel & HTTPS reverse-proxy setup
- **[docs/PITFALLS.md](docs/PITFALLS.md)**: Known edge cases & verification checklist
- **[docs/CHANGELOG.md](docs/CHANGELOG.md)**: Release history

## License

Licensed under the [MIT License](LICENSE).
