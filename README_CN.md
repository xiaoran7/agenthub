# AgentHub

> **官方 Agent 会话的统一远程继续入口（Remote-only）**  
> 专为 **Claude Code** 与 **OpenAI Codex** 设计的跨设备会话控制面与 Web 终端。

<p align="center">
  <a href="README.md">English</a> •
  <a href="README_CN.md"><b>简体中文</b></a> •
  <a href="docs/README.md">文档索引</a> •
  <a href="docs/ARCHITECTURE.md">系统架构</a> •
  <a href="docs/CHANGELOG.md">更新日志</a>
</p>

<p align="center">
  <a href="https://github.com/xiaoran7/agenthub/releases"><img src="https://img.shields.io/badge/version-2.5.2-0284c7.svg?style=flat-square" alt="Version" /></a>
  <a href="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml"><img src="https://img.shields.io/badge/CI-passing-10b981.svg?style=flat-square" alt="CI Status" /></a>
  <a href="https://github.com/xiaoran7/agenthub"><img src="https://img.shields.io/badge/tests-258%20passed-10b981.svg?style=flat-square" alt="Tests" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-64748b.svg?style=flat-square" alt="License" /></a>
  <a href="llms.txt"><img src="https://img.shields.io/badge/llms.txt-standardized-8b5cf6.svg?style=flat-square" alt="llms.txt" /></a>
  <img src="https://img.shields.io/badge/Rust-2024-f97316.svg?style=flat-square" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Node-%3E%3D22-10b981.svg?style=flat-square" alt="Node" />
</p>

---

## 核心定位

AgentHub 不自建重复的 Agent 推理循环，而是通过官方协议托管 **Claude Code**、**OpenAI Codex** 及 **ACP** 兼容智能体。本机轻量守护进程 `agenthub-runner`（`agenthubd`）以主动出站 TLS/WebSocket 连接自部署 Gateway，让你离开工位后在手机或任意电脑浏览器上实时查看输出流、预览 Diff 审批权限、继续多轮对话或一键急停。

- ⚡ **单文件 Rust Runner**：内嵌零外部依赖的本机 Web 控制台（`http://127.0.0.1:4200`），涵盖会话、智能体、逐请求用量账本、工作区及联邦能力生态（Skill / MCP / Hook）设置页。
- 🌐 **远程 Web 控制端（PWA）**：自部署 Gateway + 响应式 Web 界面，兼顾桌面浏览器与手机触控操作。
- 🛡️ **主动出站零端口暴露**：开发机仅向 Gateway 发起主动 TLS/WSS 连接，无需开放公网入站端口；不上传 Provider 凭据，物理路径脱敏为 `ws-*` 工作区 ID。
- 🎯 **官方协议原生对齐**：严格区分 `create`、`resume` 与只读 `discover`，保障单写者所有权与命令幂等。

---

## 架构拓扑

```text
本机开发环境
  Codex / Claude Code / ACP 官方 Runtime
          ▲  (官方原生协议)
          ▼
    agenthub-runner (单文件 Rust 守护进程)
      ├─ 内嵌本机控制台  ──► http://127.0.0.1:4200
      └─ 主动 TLS 出站   ──► 自部署 Gateway (鉴权 / 命令账本 / 事件回放)
                                   │
                                   ▼ (HTTPS / WSS / SSE)
                            远程 Web 控制端 (PC 浏览器 / 手机 PWA)
```

---

## 极速上手

### 1. 运行本机 Runner 与内嵌控制台

```bash
# 启动内置本机 Web 控制台（默认监听 http://127.0.0.1:4200）
agenthub-runner web

# 或以常驻守护模式启动
agenthub-runner daemon
```

### 2. 源码启动全栈套件（Node >= 22, Rust >= 1.80）

```powershell
npm.cmd ci
npm.cmd start --workspace @agent-pulse/api    # 启动 Gateway (http://127.0.0.1:3001)
npm.cmd run dev --workspace @agent-pulse/web  # 启动远程 Web 端 (http://127.0.0.1:3000)
cargo run -p agenthub-runner -- daemon        # 启动本机 Rust Runner
```

### 3. 全套测试与规范检查

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

---

## 开源引用与上游溯源

本项目对引用的外部开源资产严格留痕并保持模块边界清晰：

| 上游项目                                          | 开源协议                               | 在本项目中负责的模块                                                                                                                                                                                                                                                                                              |
| :------------------------------------------------ | :------------------------------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [magpie](https://github.com/yetone/magpie)        | MIT License, Copyright (c) 2026 yetone | 本机 Runner 内嵌 Web 控制台的设计系统与布局原语（`crates/runner/resources/dashboard.{html,css,js}`）、原版厂商图标集（`crates/runner/resources/icons/` 与 `console_icons.rs`）、逐请求用量账本视图（`usage_ledger.rs`）及跨智能体能力资料库架构（`collectors/capabilities.rs` 的 Skill / MCP Server / Hook 纳管） |
| [stablyai/orca](https://github.com/stablyai/orca) | Apache 2.0 / MIT                       | 移动端 Companion 控制舱（Mission Control）布局、终端虚拟按键配件行与深色令牌体系（`apps/android`）                                                                                                                                                                                                                |

> Runner 内嵌控制台的视觉令牌、布局原语及 `crates/runner/resources/icons/` 下的厂商图标均移植自 `yetone/magpie`，文件头完整保留上游版权与 MIT 许可证声明；相关模块排查问题时优先核对上游仓库最新记录。

---

## 核心文档索引

- **[AGENTS.md](AGENTS.md)**：工程约束、安全红线与提交纪律
- **[docs/README.md](docs/README.md)**：文档索引、当前实现矩阵与历史验收摘要
- **[docs/PRD.md](docs/PRD.md)**：产品边界、Session 语义与验收标准
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**：系统架构、组件职责与所有权模型
- **[docs/SYNC_PROTOCOL.md](docs/SYNC_PROTOCOL.md)**：本地 IPC v2、Remote WSS 协议、水位 ACK 与幂等状态机
- **[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)**：Systemd、Docker、1Panel 与 HTTPS 反代部署指南
- **[docs/PITFALLS.md](docs/PITFALLS.md)**：已知踩坑与实现检查表
- **[docs/CHANGELOG.md](docs/CHANGELOG.md)**：版本演进记录

## 许可证

基于 [MIT 许可证](LICENSE) 开源。
