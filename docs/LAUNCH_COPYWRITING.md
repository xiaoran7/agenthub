# 全平台宣发与冷启动文案物料包 (Launch Copywriting Kit)

本物料包为 **AgentHub** 量身定制，针对不同技术社区的读者调性进行了差异化撰写，直接复制发布即可。

---

## 一、V2EX 发帖文案（节点：分享创造 /go/create）

**标题**：  
`写了个让 Claude Code 和 Codex 可以手机远程接管、权限审批的轻量 Web 控制面 AgentHub`

**正文**：  
大家好，最近大家应该都在重度使用 Claude Code 或 OpenAI Codex 写代码或跑自动化脚本。

在日常使用中我经常遇到一个很蛋疼的场景：
在工位启动了一个 20 分钟的长任务，起身去开会、吃午饭或者下楼拿个快递，心里老挂念着“跑完没、报错没”；更恼火的是，Agent 跑着跑着经常弹出一个 Shell 执行权限审批，如果人不在电脑前回车确认，整个任务就会无限期卡在那挂起。用手机 SSH 连回电脑体验极差，字体微小且极其容易误触按错键。

为此我做了一个自用的轻量开源项目：**AgentHub**。

### 核心设计与亮点：
1. **纯 Rust 单文件秒级启动**：本地 Runner（`agenthub-runner`）是纯 Rust 编写的自包含程序，内置极简 Web 控制台，极度省内存，彻底移除了 Tauri/Electron 重型桌面环境；
2. **主动出站零公网端口**：开发机仅通过 TLS/WebSocket 主动连接你自部署的 Gateway，**电脑不需要开任何公网入站端口，无需配置内网穿透**，在公司内网或合规电脑上跑很安心；
3. **手机友好 PWA**：手机打开浏览器就能看到流式输出，收到权限确认提示时，手指点一下就能继续执行或一键急停；
4. **官方协议原生对齐**：完整支持 Claude Code 与 Codex 的官方会话创建（create）与恢复（resume），精准还原 diff 和工具调用。

### 快速尝鲜：
下载 Release 单二进制直接运行：
```bash
agenthub-runner web
```
浏览器打开 `http://127.0.0.1:4200` 即可管理本机会话。

- 开源地址：https://github.com/xiaoran7/agenthub
- 欢迎大家体验并拍砖！

---

## 二、Reddit 发帖文案（板块：r/ClaudeAI, r/LocalLLaMA, r/selfhosted）

**Title**:  
`I built AgentHub: A lightweight, self-hosted Web & Mobile control plane for Claude Code & OpenAI Codex (Outbound-only TLS, zero open ports)`

**Post Body**:  
Hey everyone,

Like many developers here, my workflow has completely shifted to CLI coding agents like Claude Code and OpenAI Codex. However, being chained to my terminal during 30-minute agent workflows was frustrating—especially when an agent stops dead in its tracks waiting for a bash permission prompt while I'm away from my desk.

So I built and open-sourced **AgentHub**:  
👉 GitHub: https://github.com/xiaoran7/agenthub

### What makes it different?
- **Zero Public Port Exposure**: The local runner connects outbound to your self-hosted Gateway over TLS/WSS. No router port forwarding, no ngrok tunnels, and no inbound security risks.
- **Single-Binary Rust Runner**: Written in pure Rust 2024 with an embedded Web Console (`agenthub-runner web`). Minimal RAM footprint, zero Electron bloat.
- **Mobile Responsive PWA**: Monitor live stream outputs, approve tool permissions with a tap, and continue conversations from any phone or tablet browser.
- **Native Protocol Fidelity**: Natively maps official Claude Code & Codex session lifecycles (`create`, `resume`, `discover`).

It's 100% open-source under the MIT license with full test coverage (226 passed tests).

I'd love to hear your thoughts, feedback, or feature requests!

---

## 三、Hacker News (Show HN)

**Title**:  
`Show HN: AgentHub – Remote Web and mobile control plane for Claude Code and Codex`

**Post Body**:  
URL: https://github.com/xiaoran7/agenthub

Hi HN,

I built AgentHub because I wanted to monitor long-running Claude Code and Codex tasks and approve permissions without remaining tethered to my workstation terminal.

Instead of writing another agent framework, AgentHub focuses purely on the remote continuation control plane:
- Local Runner (`agenthub-runner`): A single Rust binary that wraps native agent runtimes, manages session ownership, and embeds a clean web UI.
- Gateway: A self-hosted Fastify hub that routes events and guarantees command idempotency across disconnections.
- Outbound-Only Architecture: Workstations make outbound TLS/WSS connections, keeping private networks completely closed.

Code is MIT licensed. Technical specification and architecture diagrams are included in the repository.

---

## 四、X (Twitter) 宣发推文

Tired of having your #ClaudeCode sessions blocked on permission prompts when stepping away from your desk? ☕

Introducing **AgentHub**: A lightweight, self-hosted remote Web console for Claude Code & OpenAI Codex.

🦀 Pure Rust single-binary runner  
📱 Mobile-ready responsive PWA  
🔒 Outbound-only TLS (zero open ports)  
✅ 100% open source (MIT)

🔗 GitHub: https://github.com/xiaoran7/agenthub

#AIAgent #OpenSource #DevTools #RustLang #Anthropic
