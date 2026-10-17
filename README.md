# AgentHub

AgentHub 是**官方 Agent 会话的统一远程继续入口（Remote-only）**。

它不实现自己的 Agent 推理循环，而是调用 Codex、Claude Code 等官方 Runtime。会话由本机 `agenthubd`（Runner）经官方协议托管；在任意设备通过自部署的 **Gateway + Web** 看到同一 Session 的实时过程，继续对话、处理审批或急停。读取历史只有一个目的：判断并执行“继续”。

> **架构演进（2026-10）**：彻底移除重型 Desktop 桌面客户端（Tauri），收敛为双端闭环：
> 1. **本机 Runner（内置轻量自包含 Web 控制台）**：单二进制文件秒级启动，自带 CDS 设计系统内嵌 Web 界面（`agenthub-runner web`），无需安装任何桌面运行库；
> 2. **远程 Web 控制台（`apps/web`）**：承担桌面浏览器与移动设备（响应式 PWA）的跨设备远程全功能控制。

## 产品结构

```text
本机 agenthubd (Runner) ──官方协议──► Codex / Claude Code Runtime
      │                     └────► 内置轻量 Web 控制台 (http://127.0.0.1:4200)
      │
      └──主动出站 (WebSocket/TLS)──► Gateway ◄── 远程 Web 控制端 (PC / 手机浏览器)
```

- **agenthubd (Runner)**：本机唯一会话控制面。管理官方 Runtime、Session 所有权、事件流、命令幂等，并主动向 Gateway 建立出站连接；内置 CDS 轻量 Web 控制台，可由 `agenthub-runner web` 或 `agenthub-runner daemon` 启动。
- **Gateway**：自部署控制中枢。负责设备与用户身份、命令账本、状态路由、事件持久化与重放，不运行 Agent，也不是 Session 真相源。
- **Web**：核心远程控制端（自部署单页应用，支持桌面与移动端响应式）。用于跨设备查看实时执行、续聊、处理权限审批与紧急中断。

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
cargo run -p agenthub-runner -- daemon
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

## 开源引用与上游溯源

| 上游项目                               | 许可证                              | 在本项目中负责的模块                                                                                        |
| -------------------------------------- | ----------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| [magpie](https://github.com/yetone/magpie) | MIT License, Copyright (c) 2026 yetone | 本机 Runner 内嵌 Web 控制台（`crates/runner/resources/dashboard.html` 与 `dashboard.css`）的设计系统与布局原语 |

Runner 控制台的视觉与布局不再自行发明。`dashboard.css`（由页面以 `/dashboard.css?v=<版本>` 引入）的 Foundation 层逐条移植自 magpie 的 `internal/gui/assets/app.css`：双主题设计令牌（background、card、text、muted、line、accent 与 `--ease` 缓动）、顶栏、带分组标签的侧栏及其顶栏 `.seg` 分段形态、可自滚动的 `.view`、`.list` / `.row` / `.row.stat` 行模型、`.stats` / `.kpi` 指标卡、`.chips` / `.segs` 分段控件、`.text` / `.icon` 按钮、`.modal` / `.dialog` 与 `.editor` 表单网格。AgentHub 只在其上追加业务组合（会话、智能体、用量、模型、工作区、网关六个视图）与数据绑定。

上游文件头注释与 MIT 版权声明原文保留在 `dashboard.css` 起始处。该模块的主权与维护边界属于上游仓库：出现视觉或布局缺陷时，先核对 magpie 上游的 Release / Commit / Issue / PR，确认上游未修复后再考虑本地补丁。

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
