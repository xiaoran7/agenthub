# AgentHub

AgentHub 是**官方 Agent 会话的统一远程继续入口（Remote-only）**。

它不实现自己的 Agent 推理循环，而是调用 Codex、Claude Code 等官方 Runtime。会话由本机 `agenthubd` 经官方协议托管；离开电脑后，从 Web 或 Android 看到同一 Session 的实时过程，继续对话、处理审批或急停。读取历史只有一个目的：判断并执行“继续”。

## 产品结构

```text
Desktop ──本地 IPC──► agenthubd ──官方协议──► Codex / Claude Code Runtime
                         │
                         └──主动出站──► Gateway ◄── Web / Android
```

- **Desktop**：本机宿主客户端。选择授权工作区、创建或恢复官方 Session、查看实时过程、处理审批；同时控制 Gateway 暴露开关。它不是通用本地工作台，也不做会话归档浏览。
- **agenthubd**：本机唯一会话控制面。管理官方 Runtime、Session 所有权、事件流、命令幂等和远程连接；当前由 Desktop 以隐藏进程模式自动启动，也可通过 `agent-pulse-runner daemon` 启动。尚未提供 OS 服务安装包。
- **Web**：无需安装的远程完整客户端，用于跨设备查看、续聊、审批和中断。
- **Android**：移动控制端，强调进度、通知、审批和紧急干预。
- **Gateway**：只做身份、路由、持久化和回放，不运行 Agent，也不是 Session 真相源。

Desktop、Web、Android 是同一产品的三种客户端，不是三套会话系统。官方协议不支持第三方 attach 正在运行的 CLI 进程，因此“远程继续”要求 Session 由 `agenthubd` 托管：**create / resume** 是托管的正式能力；**discover** 只产生恢复候选的只读元数据，不等于取得控制权，三种语义不能互相冒充。

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
