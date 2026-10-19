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
  <a href="https://github.com/xiaoran7/agenthub/releases"><img src="https://img.shields.io/badge/version-2.5.1-0284c7.svg?style=flat-square" alt="Version" /></a>
  <a href="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml"><img src="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml/badge.svg?style=flat-square" alt="CI Status" /></a>
  <a href="https://github.com/xiaoran7/agenthub"><img src="https://img.shields.io/badge/tests-258%20passed-10b981.svg?style=flat-square" alt="Tests" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-64748b.svg?style=flat-square" alt="License" /></a>
  <a href="llms.txt"><img src="https://img.shields.io/badge/llms.txt-standardized-8b5cf6.svg?style=flat-square" alt="llms.txt" /></a>
  <img src="https://img.shields.io/badge/Rust-2024-f97316.svg?style=flat-square" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Node-%3E%3D22-10b981.svg?style=flat-square" alt="Node" />
  <a href="https://github.com/xiaoran7/agenthub/stargazers"><img src="https://img.shields.io/github/stars/xiaoran7/agenthub?style=flat-square&color=eab308" alt="GitHub Stars" /></a>
</p>

---

## Why AgentHub?

Running CLI-based AI agents like Claude Code or OpenAI Codex transforms developer productivity, but introduces familiar real-world friction:

- ☕ **Tethered to your desk**: You trigger a long-running execution and step away for lunch, a meeting, or a commute, leaving you completely blind to progress.
- 🛑 **Blocked on permission prompts**: The agent requires a shell confirmation or tool permission approval. Without you at the terminal, the entire session halts indefinitely.
- 📱 **Clumsy mobile access**: Connecting via mobile SSH is painful, fragile, and prone to accidental termination and keystroke errors.
- 🔒 **Security vulnerabilities of open ports**: Port-forwarding or exposing raw developer ports to the public internet presents major security risks.

**AgentHub eliminates these pains:**  
AgentHub does not reinvent another agent inference loop. Instead, it hooks directly into official runtimes (Claude Code & Codex). Through a lightweight local daemon (`agenthubd` / `agenthub-runner`) establishing secure outbound-only WebSockets to a self-hosted Gateway, you can inspect live output streams, approve tool permissions, continue multi-turn conversations, or trigger emergency stops from any browser or smartphone PWA.

---

## Key Features

- ⚡ **Single-Binary Instant Launch**: The local Runner (`agenthub-runner`) is written in pure Rust with an embedded, zero-dependency Web Console. Starts in milliseconds with minimal RAM overhead—no Electron or desktop runtime bloat.
- 🌐 **Two-Tier Experience & Seamless Roaming**:
  - **Local Console**: Instant local web management at `http://127.0.0.1:4200` to manage sessions, models, usage, and workspaces.
  - **Remote Web Console (PWA)**：Self-hosted Gateway + Web single-page app with full responsive support across desktop browsers, tablets, and phones.
- 🛡️ **Zero Inbound Port Exposure (Outbound-Only TLS)**: The runner connects outwards to your private Gateway over TLS/WSS. **Zero public inbound ports required, zero NAT holes needed**, keeping your host immune to external port scanners.
- 🎯 **Native Official Protocol Alignment**: Full native support for official Claude Code & OpenAI Codex lifecycle semantics: session creation (`create`), resumption (`resume`), and detection (`discover`).
- 📊 **Unified Usage & Model Observability**: Multi-provider metrics dashboard that distinguishes unpriced/free models from billed models, providing complete transparency into tokens and session costs.

---

## Architecture

```text
┌──────────────────────────────────────────────┐
│             Local Workstation                │
│                                              │
│   Codex / Claude Code CLI Official Runtime   │
│                     ▲                        │
│             (Official Protocol)              │
│                     ▼                        │
│             agenthub-runner                  │
│     (Single binary, pure Rust daemon)        │
│          │                      │            │
│  (Embedded Local Web)    (Outbound TLS/WSS)  │
│          ▼                      ▼            │
│  http://127.0.0.1:4200          │            │
└─────────────────────────────────┼────────────┘
                                  │
                                  ▼
                    ┌───────────────────────────┐
                    │    Self-Hosted Gateway    │
                    │ (Auth, Ledger, Routing)   │
                    └─────────────┬─────────────┘
                                  │
                                  ▼ (WebSocket / HTTPS)
                    ┌───────────────────────────┐
                    │      Remote Web Client    │
                    │  (Desktop Browser / PWA)  │
                    └───────────────────────────┘
```

---

## Quick Start

### 1. Run Precompiled Runner Locally (Fastest)

Download the precompiled single-file binary for your OS and launch:

```bash
# Launch the built-in lightweight Web Console (serves at http://127.0.0.1:4200)
agenthub-runner web

# Or run as a resident background daemon
agenthub-runner daemon
```

Open `http://127.0.0.1:4200` in your browser to manage local sessions immediately.

### 2. Full Stack from Source (Gateway + Web + Runner)

#### Prerequisites
- Node.js >= 22
- Rust >= 1.80 (2024 edition)

#### Launching Services
```powershell
# 1. Install workspace dependencies
npm.cmd ci

# 2. Start self-hosted Gateway (default: http://127.0.0.1:3001)
npm.cmd run dev --workspace @agent-pulse/server

# 3. Start remote Web client (connects to Gateway)
npm.cmd run dev --workspace @agent-pulse/web

# 4. Start local Rust Runner
cargo run -p agenthub-runner -- daemon
```

#### Run Full Test & Lint Suites
```powershell
# Rust checks
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Web & Gateway checks
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

---

## Upstream Attribution & Open Source Traceability

AgentHub adheres strictly to open-source governance and licenses:

| Upstream Project | License | Module / Role in AgentHub |
| :--- | :--- | :--- |
| [magpie](https://github.com/yetone/magpie) | MIT License, Copyright (c) 2026 yetone | Embedded Web Console design system, layout primitives, the vendor icon set (`crates/runner/resources/icons/`, copied verbatim from magpie's `internal/gui/assets/icons/` and served by `crates/runner/src/console_icons.rs`), the per-request usage ledger page (tabs / period / status / heatmap / stacked trend / ranking / request table, after magpie's usage view), and the federated Multi-Agent Library architecture (Skill, MCP Server, Hook ecosystem management in `crates/runner/src/collectors/capabilities.rs` & `crates/runner/resources/`) |
| [stablyai/orca](https://github.com/stablyai/orca) | Apache 2.0 / MIT | Mobile Companion UI philosophy, terminal accessory row, and fleet mission-control layout (`apps/android`) |

> **Note**: Foundation CSS tokens (light/dark themes, segmented top bar, responsive grid, dialogs, and stat rows) are ported from magpie's `internal/gui/assets/app.css`; the vendor marks under `crates/runner/resources/icons/` are byte-for-byte copies from magpie's `internal/gui/assets/icons/`, selected by the same model-family prefix table (`internal/agent/icons.go`). Upstream copyright and MIT license declarations remain intact in the source headers.

---

## Documentation Index

| Topic | Link | Description |
| :--- | :--- | :--- |
| **Project Rules** | [AGENTS.md](AGENTS.md) | Engineering boundaries, security rules & commit discipline |
| **Product Requirements** | [PRD](docs/PRD.md) | Problem analysis, target users & scope |
| **System Architecture** | [ARCHITECTURE](docs/ARCHITECTURE.md) | Domain models, state machines & provider adapters |
| **Reliability Protocol** | [SYNC_PROTOCOL](docs/SYNC_PROTOCOL.md) | Idempotent commands, distributed alignment & event envelopes |
| **Deployment** | [DEPLOYMENT](docs/DEPLOYMENT.md) | Docker orchestration, reverse proxy & TLS setup |
| **Known Pitfalls** | [PITFALLS](docs/PITFALLS.md) | Edge cases, debugging checklist & common mistakes |
| **Changelog** | [CHANGELOG](docs/CHANGELOG.md) | Version history and technical progression |

---

## License

This project is licensed under the [MIT License](LICENSE). Contributions, bug reports, and pull requests are warmly welcomed!
