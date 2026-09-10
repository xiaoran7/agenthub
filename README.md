# AgentHub

AgentHub 是一个 **local-first 的多 Agent 工作台，并原生提供 Remote 能力**。

它不实现自己的 Agent 推理循环，而是调用 Codex、Claude Code 等官方 Runtime。用户可以在 AgentHub Desktop 中创建、恢复和继续官方 Session；离开电脑后，再从 Web 或 Android 访问同一个 Session。

## 产品结构

```text
Desktop ──本地 IPC──► agenthubd ──官方协议──► Codex / Claude Code Runtime
                         │
                         └──主动出站──► Gateway ◄── Web / Android
```

- **Desktop**：主要工作台。本地管理工作区、创建或恢复 Session、查看工具过程、处理审批；断网时仍可工作。
- **agenthubd**：本机唯一会话控制面。管理官方 Runtime、Session 所有权、事件流、命令幂等和远程连接；当前由 Desktop 以隐藏进程模式自动启动，也可通过 `agent-pulse-runner daemon` 启动。尚未提供 OS 服务安装包。
- **Web**：无需安装的远程完整客户端，用于跨设备查看、续聊、审批和中断。
- **Android**：移动控制端，强调进度、通知、审批和紧急干预。
- **Gateway**：只做身份、路由、持久化和回放，不运行 Agent，也不是 Session 真相源。

Desktop、Web、Android 是同一产品的三种客户端，不是三套会话系统。创建 Session 是正式能力；“发现历史”“恢复 Session”“连接已运行 Session”则是三种不同语义，不能互相冒充。

## 当前能力

当前实现、目标架构和未完成差距只在 [文档索引](docs/README.md#当前实现与目标差距) 维护；本入口不复制阶段状态。验收记录只能证明其明确列出的环境，不能由 UI、模拟测试或旧版本数字推定。

## 开发

```powershell
npm.cmd ci
npm.cmd run tauri --workspace @agent-pulse/desktop -- dev
npm.cmd run dev --workspace @agent-pulse/web
```

Web 默认访问 `http://127.0.0.1:3001` 的 Gateway；桌面可用 `AGENT_PULSE_SERVER_URL` 覆盖。Android 需要 JDK 17+、SDK 36，可用 `adb reverse tcp:3001 tcp:3001` 连接本机 Gateway。

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
