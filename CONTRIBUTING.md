# Contributing to AgentHub

## 1. Ground Rules

- **Minimal Surface**: AgentHub is a remote continuation control plane for official CLI agent runtimes (Claude Code, OpenAI Codex, Google Antigravity, ACP). It never implements its own reasoning loop or stores LLM provider credentials.
- **Line Limits & Upstream Attribution**: Source files must stay within `scripts/check-line-limits.py` bounds (`<=800` lines). Ported upstream assets must preserve license headers and attribution in `README.md`.
- **Changelog**: Every code, config, or documentation change must update `docs/CHANGELOG.md`.

## 2. Local Development (Node >= 22, Rust 2024 toolchain)

```bash
npm ci
npm start --workspace @agent-pulse/api     # Gateway API (http://127.0.0.1:3001)
npm run dev --workspace @agent-pulse/web   # Vite Web UI (http://127.0.0.1:5173)
cargo run -p agenthub-runner -- daemon     # Local Rust Runner (:4200 console)
```

## 3. Verification Gates

```bash
python3 scripts/check-line-limits.py
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
npm run format:check && npm run typecheck && npm test
```
