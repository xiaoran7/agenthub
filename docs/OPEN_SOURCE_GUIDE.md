# 开源 SEO 与全网曝光实操指南 (Open Source SEO & Promotion Guide)

本指南针对 **AgentHub**（以及同类全干工程师开源项目），整理了一套从“GitHub 站内权重最大化”到“全网精准技术社区引流”的可执行清单。

---

## 一、GitHub 仓库后台设置（直接复制配置）

进入 GitHub 仓库主页，点击右上角齿轮 `Edit repository details`：

### 1. Description（仓库简介）
> **推荐填写**：  
> `Remote Web console & session manager for Claude Code and OpenAI Codex. Continue sessions anywhere via browser or mobile PWA.`

- **为什么这样写**：包含了 `Claude Code`、`OpenAI Codex`、`Web console`、`session manager`、`mobile PWA` 五大高频搜索热词，字符数在 130 字左右，在移动端与搜索结果中刚好完整展示不被省略。

### 2. Website（展示网址）
> 填写你的在线演示地址、文档地址或 release 下载地址，例如：  
> `https://git.tannerlab.xyz/tangwg/agent-pulse` 或专属文档站域名。

### 3. Topics（仓库标签，权重极高，全部添加）
复制以下标签逐一添加到 Topics 中：

```text
claude-code
openai-codex
ai-agent
agent-runner
session-manager
remote-control
web-ui
dashboard
self-hosted
pwa
developer-tools
rust
typescript
react
cli
```

### 4. Include in the home page（勾选选项）
- [x] Releases
- [x] Packages
- [ ] Environments（无则不勾）

### 5. Social Preview（社交媒体分享大卡片）
- 路径：`Settings` -> `General` -> `Social preview` -> `Edit` -> `Upload an image`。
- 尺寸规范：`1280 x 640` 像素（PNG / JPG）。
- 内容建议：浅灰/深色底，左侧是大字标题 `"AgentHub"` + 副标题 `"Remote Web Console for Claude Code & Codex"`，右侧放一张内嵌 Web 控制台的运行缩略图。

---

## 二、国内高权重阵地投稿模版（复制即用）

### 1. HelloGitHub 推荐项目（月刊推荐）
- **投稿入口**：https://hellogithub.com/periodical 或 GitHub Issues
- **投稿内容草稿**：
  > **项目名称**：AgentHub  
  > **项目地址**：https://github.com/<your-username>/agenthub  
  > **一句话简介**：专为 Claude Code 与 OpenAI Codex 打造的轻量远程继续入口与 Web 控制台。  
  > **项目描述**：  
  > 很多人在本地跑 Claude Code 或 Codex 长任务时，离开电脑就断了进度，或者卡在权限审批上。AgentHub 不自建推理循环，而是原生调用官方 Runtime。  
  > 它由一个纯 Rust 编写的极轻量单文件本地 Runner（内置极简 Web 控制台）与自部署 Gateway 组成。开发机主动出站连接 Gateway，无需开放任何公网端口。用户只要在手机或任意电脑的浏览器打开 Web 端，就能随时查看实时执行过程、点击审批权限、继续对话或一键急停。  
  > **技术栈**：Rust、TypeScript、React、Node.js。  
  > **效果截图**：（贴一张 Web 端实时控制台截图）

---

### 2. 阮一峰《科技爱好者周刊》投稿
- **投稿入口**：https://github.com/ruanyf/weekly/issues
- **标题**：`开源项目推荐：AgentHub - 官方 Agent 会话的跨设备远程接管与 Web 控制台`
- **内容草稿**：
  > 阮老师好，推荐一个近期开源的开发者工具项目：**AgentHub**。  
  >
  > - **项目地址**：https://github.com/<your-username>/agenthub  
  > 
  > **解决的痛点**：  
  > 现在大家都在终端用 Claude Code 或 Codex 写代码，但经常遇到“人一离开电脑，Agent 卡在 Shell 审批无法继续”、“想用手机瞄一眼进度却只能折腾繁琐的 SSH”等麻烦。  
  > 
  > **项目特色**：  
  > 1. **单文件秒启动**：本机 Runner（`agenthub-runner`）是纯 Rust 编写的自包含程序，内置极简 Web 控制台，极省内存，告别重型桌面框架。  
  > 2. **主动出站安全设计**：主机只向自己的 Gateway 建立 TLS WebSocket 出站连接，不用公网开端口，杜绝安全隐患。  
  > 3. **手机随时审批**：在手机浏览器（支持 PWA）随时收到权限提示并一键确认，让长流程 Agent 真正离开工位也能持续运转。  
  > 
  > 欢迎大家试用并提建议！

---

### 3. V2EX “分享创造” 节点发帖
- **节点**：`/go/create`
- **标题**：`做了一个让 Claude Code 和 Codex 可以手机远程接管、权限审批的轻量 Web 控制面`
- **正文结构建议**：
  1. 为什么做这个（自己日常在工位跑 Agent，去吃个饭就被终端确认卡住的真实经历）；
  2. 架构是怎么设计的（为什么坚决不用 Tauri/Electron 重型客户端，改用 Rust 单文件内嵌 Web + 自部署 Gateway）；
  3. 核心亮点（零端口暴露、全双工流式、手机 PWA）；
  4. 开源地址与快速上手命令（贴上 `agenthub-runner web` 一键启动演示）；
  5. 附 1~2 张清爽的控制台截图，诚恳求 Star 和 Feedback。

---

## 三、海外阵地与 Awesome 列表 PR（收割全球流量）

### 1. 向权威 Awesome Lists 提 PR
搜索并 Fork 以下仓库，在合适的分类（如 `Developer Tools` 或 `Clients / Web UI`）提 PR：
- `awesome-claude`
- `awesome-ai-agents`
- `awesome-devtools`
- `awesome-selfhosted`

**PR 添加格式范例**：
```markdown
- [AgentHub](https://github.com/<your-username>/agenthub) - Unified remote continuation & Web control plane for Claude Code and OpenAI Codex sessions. Built with Rust and TypeScript.
```

---

### 2. Reddit 社区发帖模版
- **目标板块**：`r/ClaudeAI`, `r/LocalLLaMA`, `r/selfhosted`, `r/webdev`
- **Title**: `I built a lightweight, self-hosted Web & Mobile control plane for Claude Code & OpenAI Codex`
- **Post Body**:
  > Hey everyone,  
  > 
  > Like many of you, I've been heavily using CLI agent tools like Claude Code and OpenAI Codex. However, being tethered to my terminal while an agent is running a 20-minute task was frustrating—especially when it pauses for a bash permission prompt while I'm away from my desk.
  > 
  > So I built **AgentHub**:
  > - **Zero port exposure**: The local daemon connects outwards via TLS/WSS to your self-hosted Gateway. No open inbound ports.
  > - **Single binary local runner**: Written in pure Rust with an embedded web UI (`agenthub-runner web`). Ultra-low RAM usage.
  > - **Mobile & Browser PWA**: Check streaming events, approve tools/permissions, and resume conversations right from your phone.
  > 
  > Completely open source under MIT: https://github.com/<your-username>/agenthub  
  > 
  > Would love to hear your thoughts and feedback!

---

### 3. X (Twitter) 推文示范
> Tired of having your #ClaudeCode sessions blocked on permission prompts when stepping away from your desk? ☕  
> 
> Introducing **AgentHub**: A lightweight, self-hosted remote Web console for Claude Code & Codex.  
> 
> 🦀 Pure Rust single-binary runner  
> 📱 Mobile-ready responsive PWA  
> 🔒 Outbound-only TLS (zero open ports)  
> 
> Repo: https://github.com/<your-username>/agenthub  
> 
> #AIAgent #OpenSource #DevTools #RustLang

---

## 四、自检清单（发布前 5 分钟速查）

- [x] 根目录存在标准的 `README.md`（英文，覆盖全网检索词）
- [x] 根目录存在中文说明 `README_CN.md`
- [x] 根目录存在权威 `LICENSE` 文件（MIT）
- [x] 明确标注了上游开源项目的溯源引用（如 magpie）
- [x] 快速上手命令无需阅读冗长文档，1 分钟能跑起来
- [ ] GitHub 仓库已填满 15+ Topics 标签
- [ ] GitHub 仓库已填写包含精准热词的 Description
- [ ] GitHub 仓库已上传 1280x640 Social Preview 图片
