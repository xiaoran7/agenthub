# 社区收录与 Awesome 列表 PR 指南 (Community Submissions)

| 目标渠道                     | 仓库 / 入口                                                                    | 分类 / PR 标题                                                                              |
| :--------------------------- | :----------------------------------------------------------------------------- | :------------------------------------------------------------------------------------------ |
| **awesome-claude-code**      | `https://github.com/anthropics/anthropic-tools`                                | `Developer Tools` · `Add AgentHub: Remote Web console for Claude Code`                      |
| **awesome-ai-agents**        | `https://github.com/e2b-dev/awesome-ai-agents`                                 | `Agent Infrastructure & Observability` · `Add AgentHub: Remote session control plane`       |
| **awesome-selfhosted**       | `https://github.com/awesome-selfhosted/awesome-selfhosted`                     | `Software Development - AI` · `Add AgentHub to Development Environments`                    |
| **awesome-rust**             | `https://github.com/rust-unofficial/awesome-rust`                              | `Development tools` · `Add AgentHub: Remote AI agent control daemon in Rust`                |
| **HelloGitHub 月刊**         | `https://hellogithub.com/periodical`                                           | 标签：`Rust`、`TypeScript`、`React`、`AI Agent`、`DevTools`                                  |

## 标准 Markdown 条目

```markdown
- [AgentHub](https://github.com/xiaoran7/agenthub) - Self-hosted remote continuation and Web/PWA control plane for official AI coding agents (Claude Code, OpenAI Codex, ACP). Single-binary Rust runner with outbound-only TLS. `MIT` `Rust/Node.js`
```

## 中文简介（HelloGitHub）

> 专为 Claude Code 与 OpenAI Codex 打造的官方会话统一远程继续入口与 Web 控制台。本机 `agenthub-runner` 采用纯 Rust 编写，单文件内嵌轻量 Web 控制台，并通过主动出站 TLS/WSS 连接自部署 Gateway（零公网入站端口），支持手机或桌面浏览器查看流式输出、审批权限与一键中断。
