# Contributing to AgentHub

Thank you for your interest in contributing to AgentHub! We welcome contributions, bug fixes, feature proposals, and documentation improvements.

---

## 1. Ground Rules & Engineering Discipline

- **Quality First**: All pull requests must pass the complete test suites (`cargo test --workspace` and `npm test`).
- **Minimal Surface**: AgentHub acts as a remote continuation control plane for official runtimes. It does NOT implement its own agent reasoning loop or store LLM provider credentials.
- **Traceability**: If your change ports or incorporates third-party open-source components, you must document upstream licensing and attribution in `README.md`.

---

## 2. Setting Up Local Development

### Prerequisites
- Node.js >= 22
- Rust >= 1.80 (2024 edition)
- Git

### Installation & Launching Services
```bash
# 1. Clone repository
git clone https://github.com/xiaoran7/agenthub.git
cd agenthub

# 2. Install dependencies
npm ci

# 3. Start Gateway in dev mode (listens on http://127.0.0.1:3001)
npm run dev --workspace @agent-pulse/server

# 4. Start Web UI in dev mode (connects to Gateway)
npm run dev --workspace @agent-pulse/web

# 5. Run the local runner daemon
cargo run -p agenthub-runner -- daemon
```

---

## 3. Running Verification Suites

Before opening a PR, ensure all checks pass cleanly:

```bash
# Rust checks
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# TypeScript / Web checks
npm test
npm run typecheck
npm run format:check
```

---

## 4. Submitting a Pull Request

1. Fork the repository and create your branch from `main`:
   ```bash
   git checkout -b feat/your-feature-name
   ```
2. Commit your changes following [Conventional Commits](https://www.conventionalcommits.org/):
   ```text
   feat(runner): add support for custom provider timeout
   fix(web): resolve mobile viewport shift in session view
   ```
3. Update `docs/CHANGELOG.md` with your change.
4. Push to your fork and submit a Pull Request to `xiaoran7/agenthub` using our PR template.
