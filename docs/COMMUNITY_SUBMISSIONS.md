# 权威社区收录与 Awesome 列表 PR 指南 (Community Submissions)

本文件整理了与 **AgentHub** 高度契合的顶级 Awesome 仓库与社区收录渠道。每一个都已备齐精准分类、PR 标题和准备粘贴的那一行 Markdown 代码。

---

## 1. 提交至 awesome-claude-code / awesome-claude

- **目标仓库**：https://github.com/anthropics/anthropic-tools 或社区热门 `awesome-claude`
- **目标分类**：`Developer Tools` / `Web UIs & Dashboards`
- **PR 标题**：`Add AgentHub: Remote Web console for Claude Code`
- **插入内容（Markdown）**：
  ```markdown
  - [AgentHub](https://github.com/xiaoran7/agenthub) - Unified remote continuation and Web control plane for Claude Code sessions. Pure Rust single-binary runner with mobile PWA support.
  ```

---

## 2. 提交至 awesome-ai-agents

- **目标仓库**：https://github.com/e2b-dev/awesome-ai-agents
- **目标分类**：`Agent Infrastructure & Observability`
- **PR 标题**：`Add AgentHub: Remote session manager and control plane`
- **插入内容（Markdown）**：
  ```markdown
  - [AgentHub](https://github.com/xiaoran7/agenthub) - The unified remote continuation and control plane for official AI agents (Claude Code & OpenAI Codex). Supports outbound-only TLS and mobile approvals.
  ```

---

## 3. 提交至 awesome-selfhosted

- **目标仓库**：https://github.com/awesome-selfhosted/awesome-selfhosted
- **目标分类**：`Software Development - Development Environments / AI`
- **PR 标题**：`Add AgentHub to Development Environments`
- **插入内容（Markdown）**：
  ```markdown
  - [AgentHub](https://github.com/xiaoran7/agenthub) - Self-hosted Web console and session control plane for official AI coding agents (Claude Code, OpenAI Codex). `MIT` `Rust/Node.js`
  ```

---

## 4. 提交至 awesome-rust

- **目标仓库**：https://github.com/rust-unofficial/awesome-rust
- **目标分类**：`Development tools`
- **PR 标题**：`Add AgentHub: Remote AI agent control daemon in Rust`
- **插入内容（Markdown）**：
  ```markdown
  - [AgentHub](https://github.com/xiaoran7/agenthub) — The unified remote continuation and control plane for official AI agents. Fast single-binary daemon with embedded Web console.
  ```

---

## 5. 提交至 HelloGitHub 月刊

- **投稿入口**：https://hellogithub.com/periodical 或在 GitHub 仓库发起 Issue
- **项目名**：AgentHub
- **仓库地址**：https://github.com/xiaoran7/agenthub
- **中文介绍**：
  > 专为 Claude Code 与 OpenAI Codex 打造的官方会话统一远程继续入口与 Web 控制台。  
  > 解决了日常跑长流程 Agent 时必须守在工位电脑前回车审批的痛点。本机 Runner 采用纯 Rust 编写，单文件秒启动且内嵌极简 Web 控制台；主机通过主动出站 WebSocket 连接自部署 Gateway，无需开放任何公网端口。开发者在手机或任意浏览器上就能随时查看实时流式输出、审批权限与一键急停。
- **技术标签**：Rust、TypeScript、React、AI Agent、DevTools
