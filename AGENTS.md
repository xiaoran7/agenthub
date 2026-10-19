# 项目规则 (AGENTS.md)

## 工作约定

- **协作与留痕**：开工前先查 `git status --short`，保留其他会话未提交改动；代码、配置或文档变更必须同步更新 `docs/CHANGELOG.md`。
- **操作授权**：本地验证、commit、push 与部署分别按用户授权执行，严禁越权连跑。
- **单一事实源**：`README.md` 放导航；本文件放工程约束；`docs/` 放契约与排障；`docs/CHANGELOG.md` 放历史；`MEMORY.md` 放偏好。同一事实只维护一处，删改同步修复引用。
- **格式与编码**：Markdown 与文本统一用 UTF-8 无 BOM 与纯 LF（`\n`），禁直角引号或泄露私有目录与内网 IP。
- **安全红线**：禁提交凭据、私钥、Cookie、个人资料或运行数据库（`*.db`、`*-identity.json`）。
- **产物与收尾**：临时文件放系统临时目录，安装包归档至 `Deliverables/AgentHub/`；发版必自增版本并删旧包，收尾主动停止本次启动的临时进程。

## 项目边界

- **核心拓扑**：官方 Agent 会话统一远程继续入口（Remote-only）。`agenthubd`（`crates/runner`）为本机唯一控制面并内嵌控制台（`127.0.0.1:4200`）；`services/api`（`@agent-pulse/api`）负责中继与 SQLite/PG 持久化；`apps/web` 负责 PC/手机 PWA 控制（`apps/android` 冻结）。
- **官方事实与单写者**：不自建推理循环，不代管 Provider 凭据。严格区分 `create`、`discover`、`resume`、`attach`；同一原生 Session 默认单写者，无法验证安全 attach 时保持只读或显式 `resume`；transcript 解析限 `crates/runner/src/conversation.rs` 单点有界实现。
- **跨包契约与脱敏**：`Presence`、`ActivityStatus`、`ControlState` 独立建模；进入 managed Remote 的 DTO、收据、脱敏 Diff 与 `ws-*` 必须与 `packages/contracts` 及 `crates/runner` 双向对齐并过 allowlist，物理路径与原始工具参数不出本机。
- **验证门禁**：Rust 跑 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`；TS/Node 跑 `npm.cmd test`、`npm.cmd run typecheck`、`npm.cmd run format:check`。
- **文档导航**：按任务查阅 [PRD](docs/PRD.md)、[系统架构](docs/ARCHITECTURE.md)、[Remote 协议](docs/SYNC_PROTOCOL.md)、[部署指南](docs/DEPLOYMENT.md) 与 [避坑检查表](docs/PITFALLS.md)；长期偏好见 [MEMORY.md](MEMORY.md)。
