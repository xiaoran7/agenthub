# 项目规则 (AGENTS.md)

## 工作约定

- **协作与留痕**：开工先查 `git status --short`，保留其他会话改动；代码、配置或文档变更同步更新 `docs/CHANGELOG.md`。
- **分步授权**：本地验证、`git commit`、`git push` 与部署按用户明确授权分步执行；公开署名统一用 `Tanner`。
- **单一事实源**：`README.md` 放导航，本文件放约束，`docs/` 放协议与排障，`docs/CHANGELOG.md` 放历史，`MEMORY.md` 放偏好；删改同步修复引用。
- **格式与纯净度**：统一 UTF-8 无 BOM + 纯 LF（`\n`）与半角双引号 `""`，禁直角引号或泄露内网 IP、私有主机名与本地临时路径。
- **安全红线**：禁提交凭据、私钥、Cookie、个人资料或运行数据库（`*.db`、`*-identity.json`）。
- **产物与收尾**：产物归档至 `Deliverables/AgentHub/`，发版递增全仓版本号（含 Android `versionCode`/`versionName`）并删旧包；收尾清理本次启动的临时进程。

## 项目边界

- **核心拓扑**：官方 Agent 会话统一远程继续入口。`agenthubd`（`crates/runner`）为本机唯一控制面，含 `:4200` 控制台、按需 `ui` WebView2 子进程与原生托盘；`services/api` 负责鉴权、命令账本与存储；`apps/web`（Vite 8 静态包 + nginx 同源反代）与 `apps/android`（Kotlin + Compose 原生端）共用 Remote v3 契约。
- **官方事实与单写者**：不自建推理循环，不代管 Provider 凭据。对接 Codex、Claude Code、Antigravity（`agy`）与 ACP；区分 `create`/`discover`/`resume`/`attach`，单写者 fencing，转录解析限 `crates/runner/src/conversation/` 单点实现。
- **契约与行数门禁**：进入 Gateway 的 DTO、收据、脱敏 Diff（`<=64 KiB`）与 `ws-*` 须与 `packages/contracts` 白名单双向对齐；Rust 源码须过 `python3 scripts/check-line-limits.py`（单文件 `<=800` 行）。
- **验证门禁**：Rust 跑 `cargo test --workspace`、`clippy -D warnings`、`cargo fmt --check`；Node 跑 `npm test`、`npm run typecheck`、`npm run format:check`、`npm run lint:lines`。
- **深入导航**：查阅 [PRD](docs/PRD.md)、[系统架构](docs/ARCHITECTURE.md)、[Remote 协议](docs/SYNC_PROTOCOL.md)、[部署说明](docs/DEPLOYMENT.md)、[避坑清单](docs/PITFALLS.md) 与 [MEMORY.md](MEMORY.md)。
