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
  <a href="https://github.com/xiaoran7/agenthub/releases"><img src="https://img.shields.io/badge/version-2.4.98-0284c7.svg?style=flat-square" alt="Version" /></a>
  <a href="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml"><img src="https://github.com/xiaoran7/agenthub/actions/workflows/ci.yml/badge.svg?style=flat-square" alt="CI Status" /></a>
  <a href="https://github.com/xiaoran7/agenthub"><img src="https://img.shields.io/badge/tests-233%20passed-10b981.svg?style=flat-square" alt="Tests" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-64748b.svg?style=flat-square" alt="License" /></a>
  <a href="llms.txt"><img src="https://img.shields.io/badge/llms.txt-standardized-8b5cf6.svg?style=flat-square" alt="llms.txt" /></a>
  <img src="https://img.shields.io/badge/Rust-2024-f97316.svg?style=flat-square" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Node-%3E%3D22-10b981.svg?style=flat-square" alt="Node" />
  <a href="https://github.com/xiaoran7/agenthub/stargazers"><img src="https://img.shields.io/github/stars/xiaoran7/agenthub?style=flat-square&color=eab308" alt="GitHub Stars" /></a>
</p>

---

## 为什么需要 AgentHub？

当你在工作机上运行 Claude Code 或 OpenAI Codex 时，你是否遇到过这些尴尬场景：

- ☕ **人被拴在工位电脑前**：启动了一个长任务，起身去开会、吃饭或上洗手间，无法实时获知执行进度；
- 🛑 **权限审批卡住任务**：Agent 跑着跑着触发了终端权限确认或 Shell 审批，你不在电脑前，任务只能无限挂起；
- 📱 **缺少优雅的移动端体验**：用手机 SSH 连回电脑极其别扭，字体微小、极易误触断连；
- 🔒 **公网暴露的高危风险**：为了远程访问不得不给开发机打洞开放高危入站端口。

**AgentHub 彻底解决这些问题：**
它不自建重复的 Agent 推理循环，而是原生调用官方 Runtime。通过本机的轻量守护进程 `agenthubd`（Runner）以主动出站 WebSocket 连接自部署的 Gateway，无论你在手机、平板还是外网笔记本的浏览器上，都能像坐在工位前一样，实时查看输出流、进行授权审批、继续多轮对话或一键急停。

---

## 核心特性

- ⚡ **单文件秒起（内置 CDS Web 控制台）**：本机 Runner（`agenthub-runner`）是纯 Rust 编写的自包含二进制，内置轻量极简 Web 界面，内存占用极低，无任何 Electron 或重型桌面运行库依赖。
- 🌐 **双端闭环与跨设备漫游**：
  - **本机控制台**：浏览器直接打开 `http://127.0.0.1:4200`，本地极速管理会话、模型、用量与工作区；
  - **远程 Web 控制端（PWA）**：自部署的 Gateway + Web 支持响应式布局，手机浏览器秒变触控友好的运维中枢。
- 🛡️ **绝对安全的主动出站设计**：开发机仅通过 TLS/WSS 主动连接你自己的 Gateway，**无需开放任何公网入站端口，无需配置内网穿透路由器**，杜绝黑客扫描。
- 🎯 **官方协议原生对齐**：完整支持 Claude Code 与 OpenAI Codex 的官方 Session 创建（`create`）、恢复（`resume`）与探测（`discover`），精确还原工具调用、Diff 与权限确认。
- 📊 **统一账本与用量观测**：多 Provider 聚合统计，精准区分免费模型与已定价模型，会话历史与消费账单透明可查。

---

## 架构拓扑

```text
┌──────────────────────────────────────────────┐
│                  本机开发环境                 │
│                                              │
│   Codex / Claude Code CLI 官方 Runtime       │
│                     ▲                        │
│               (官方原生协议)                  │
│                     ▼                        │
│             agenthub-runner                  │
│   (本机唯一控制面，单二进制，零重型依赖)         │
│          │                      │            │
│   (内置本地 Web 控制台)   (主动 TLS 出站)     │
│          ▼                      ▼            │
│  http://127.0.0.1:4200          │            │
└─────────────────────────────────┼────────────┘
                                  │
                                  ▼
                    ┌───────────────────────────┐
                    │     自部署 Gateway        │
                    │ (状态路由 / 鉴权 / 事件重放)│
                    └─────────────┬─────────────┘
                                  │
                                  ▼ (WebSocket / HTTPS)
                    ┌───────────────────────────┐
                    │      远程 Web 控制端       │
                    │  (PC 浏览器 / 手机响应式 PWA) │
                    └───────────────────────────┘
```

---

## 极速上手

### 1. 本机直接运行预编译 Runner（最简方式）

下载预编译的单文件程序直接启动：

```bash
# 启动 Runner 内置的轻量 Web 控制台（默认监听 http://127.0.0.1:4200）
agenthub-runner web

# 或者以常驻后台守护模式启动
agenthub-runner daemon
```

打开浏览器访问 `http://127.0.0.1:4200`，即可立即管理本机会话。

### 2. 源码启动主线套件（Gateway + Web + Runner）

#### 环境准备
- Node.js >= 22
- Rust >= 1.80 (2024 edition)

#### 启动服务
```powershell
# 1. 安装前端与网关依赖
npm.cmd ci

# 2. 启动自部署 Gateway（默认端口 http://127.0.0.1:3001）
npm.cmd run dev --workspace @agent-pulse/server

# 3. 启动远程 Web 前端控制台（默认连接 Gateway）
npm.cmd run dev --workspace @agent-pulse/web

# 4. 启动本机 Rust Runner
cargo run -p agenthub-runner -- daemon
```

#### 完整测试与代码规范检查
```powershell
# Rust 全套检查
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# 前端与网关全套检查
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

---

## 开源引用与上游溯源

遵循严格的开源工程纪律，本项目对于引用的外部优质开源资产均做明确留痕：

| 上游项目 | 开源协议 | 在本项目中负责的模块 |
| :--- | :--- | :--- |
| [magpie](https://github.com/yetone/magpie) | MIT License, Copyright (c) 2026 yetone | 本机 Runner 内嵌 Web 控制台（`crates/runner/resources/dashboard.html` / `dashboard.css` / `dashboard.js`）的设计系统与布局原语 |
| [stablyai/orca](https://github.com/stablyai/orca) | Apache 2.0 / MIT | 移动端 Companion 的控制舱（Mission Control）设计哲学、终端虚拟按键配件行与高密度机队布局（`apps/android`） |

> **说明**：Runner 控制台的视觉与布局规范由 magpie 移植而来。双主题令牌（浅色/深色）、顶栏分段器、网格系统与表单控件均沿用上游规范，上游许可证声明完整保留在文件头部。若出现布局或样式问题，优先核对并拉取上游最新修复。

---

## 核心文档索引

| 专题 | 链接 | 说明 |
| :--- | :--- | :--- |
| **项目规则** | [AGENTS.md](AGENTS.md) | 工程约束、安全边界与提交铁律 |
| **产品设计** | [PRD](docs/PRD.md) | 业务边界、痛点拆解与路线图 |
| **系统架构** | [ARCHITECTURE](docs/ARCHITECTURE.md) | 领域模型、状态机与 Provider 适配 |
| **可靠性协议** | [SYNC_PROTOCOL](docs/SYNC_PROTOCOL.md) | 幂等操作、分布式命令对齐与事件信封 |
| **部署与运维** | [DEPLOYMENT](docs/DEPLOYMENT.md) | Docker 容器编排、反向代理与 TLS 配置 |
| **避坑指南** | [PITFALLS](docs/PITFALLS.md) | 踩坑记录、边界案例与协议防错 |
| **版本演进** | [CHANGELOG](docs/CHANGELOG.md) | 版本发版记录与技术变更留痕 |

---

## 许可证 (License)

本项目基于 [MIT 许可证](LICENSE) 开源。欢迎提交 Issue 与 Pull Request！
