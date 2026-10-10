# AgentHub

AgentHub 是**官方 Agent 会话的统一远程继续入口（Remote-only）**。

它不实现自己的 Agent 推理循环，而是调用 Codex、Claude Code 等官方 Runtime。会话由本机 `agenthubd`（Runner）经官方协议托管；在任意设备通过自部署的 **Gateway + Web** 看到同一 Session 的实时过程，继续对话、处理审批或急停。读取历史只有一个目的：判断并执行“继续”。

> **路线调整（2026-10）**：当前聚焦 **Gateway + Web** 自部署主线。Web 端同时承担桌面浏览器与移动设备（响应式 PWA）的远程全功能控制；Desktop（Tauri）与原生 Android 暂时冻结，延后至下一阶段。

## 产品结构

```text
本机 agenthubd (Runner) ──官方协议──► Codex / Claude Code Runtime
      │
      └──主动出站 (WebSocket/TLS)──► Gateway ◄── Web (PC / 手机浏览器)
                                      ┆
                                   [挂起/延后: Desktop / Android]
```

- **agenthubd (Runner)**：本机唯一会话控制面。管理官方 Runtime、Session 所有权、事件流、命令幂等，并主动向 Gateway 建立出站连接；可由 `agent-pulse-runner daemon` 启动。
- **Gateway**：自部署控制中枢。负责设备与用户身份、命令账本、状态路由、事件持久化与重放，不运行 Agent，也不是 Session 真相源。
- **Web**：核心远程控制端（自部署单页应用，支持桌面与移动端响应式）。用于跨设备查看实时执行、续聊、处理权限审批与紧急中断。
- **Desktop / Android（暂时冻结）**：已暂停主动迭代，代码保留在工程中，核心精力全部收敛至 Gateway + Web 闭环。

官方协议不支持第三方 attach 正在运行的 CLI 进程，因此“远程继续”要求 Session 由 `agenthubd` 托管：**create / resume** 是托管的正式能力；**discover** 只产生恢复候选的只读元数据，不等于取得控制权，三种语义不能互相冒充。

## 当前能力

当前实现、目标架构和未完成差距只在 [文档索引](docs/README.md#当前实现与目标差距) 维护；本入口不复制阶段状态。验收记录只能证明其明确列出的环境，不能由 UI、模拟测试或旧版本数字推定。

## 开发与运行 (Gateway + Web 主线)

```powershell
# 依赖安装
npm.cmd ci

# 启动 Gateway (默认监听 http://127.0.0.1:3001)
npm.cmd run dev --workspace @agent-pulse/server

# 启动 Web 前端 (默认连接 Gateway)
npm.cmd run dev --workspace @agent-pulse/web

# 启动本机 Runner daemon
cargo run -p agent-pulse-runner -- daemon
```

完整验证命令：

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

仅文档变更不运行语言构建。

## 文档

| 内容                     | 入口                                                                                                                                       |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| 工程约束                 | [AGENTS.md](AGENTS.md)                                                                                                                     |
| 产品方向                 | [PRD](docs/PRD.md)                                                                                                                         |
| 目标架构与 Provider 接入 | [ARCHITECTURE](docs/ARCHITECTURE.md)                                                                                                       |
| Remote 可靠性合同        | [SYNC_PROTOCOL](docs/SYNC_PROTOCOL.md)                                                                                                     |
| 组件开发                 | [Runner](crates/runner/README.md)、[Gateway](services/api/README.md)、[Desktop](apps/desktop/README.md)、[Android](apps/android/README.md) |
| 运行维护                 | [DEPLOYMENT](docs/DEPLOYMENT.md)、[PITFALLS](docs/PITFALLS.md)                                                                             |
| 验收与历史               | [M4](docs/M4_VALIDATION.md)、[Android](docs/ANDROID_VALIDATION.md)、[CHANGELOG](docs/CHANGELOG.md)                                         |
