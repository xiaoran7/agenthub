# Contributing to AgentHub

## 1. Ground Rules

- **Minimal Surface**: AgentHub is a remote continuation control plane for official runtimes. It does NOT implement its own agent reasoning loop or store LLM provider credentials.
- **Upstream Traceability**: Third-party open-source components must retain license headers and be documented in `README.md` (`Upstream Attribution`).
- **Changelog**: Every behavior, config, or doc change must update `docs/CHANGELOG.md`.

## 2. Local Development (Node >= 22, Rust >= 1.80)

```bash
npm ci
npm start --workspace @agent-pulse/api     # Gateway (http://127.0.0.1:3001)
npm run dev --workspace @agent-pulse/web   # Web UI (http://127.0.0.1:3000)
cargo run -p agenthub-runner -- daemon     # Local Rust Runner
```

## 3. Verification Gates

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test
npm run typecheck
npm run format:check
```
