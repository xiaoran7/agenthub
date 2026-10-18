# GitHub Discussions 长尾 SEO 种子问答库 (Discussions Q&A Seeds)

> **使用说明**：在 GitHub 仓库主页 `Settings -> Features` 勾选开启 `Discussions`。  
> 在 `Discussions -> Q&A` 分类中，将以下 3 个话题分别发布为新讨论，并用作者账号将下方“官方解答”回复并勾选为 **"Mark as answer"**。  
> 这样在 Google、Bing 和各大 AI 搜索中会获得极高的长尾权重与丰富结果摘要（Rich Snippets）。

---

## 种子话题 1：手机远程接管与离开工位监控

### 标题（Title）：
**How can I monitor and continue my Claude Code or OpenAI Codex sessions from my phone or tablet?**

### 提问内容（Question）：
> I often run long agent tasks in my terminal using Claude Code or Codex. When I need to leave my desk for lunch or meetings, I lose track of whether the task finished or failed. SSH from mobile is really painful.  
> Is there a clean way to monitor live progress and continue conversations from a mobile browser or PWA?

### 官方推荐解答（Answer to mark as accepted）：
> Yes, this is exactly what **AgentHub** is built for!
>
> 1. Run the lightweight local runner on your workstation:
>    ```bash
>    agenthub-runner daemon
>    ```
> 2. With the self-hosted Gateway + Web client running, open the Web URL on your phone or tablet browser (it fully supports PWA installation to your home screen).
> 3. You will immediately see your active sessions with live output streaming. You can type follow-up instructions, approve shell permissions, or trigger an emergency stop right from your phone.
>
> No heavy Electron apps or complex mobile SSH configurations needed.

---

## 种子话题 2：安全与零公网端口暴露

### 标题（Title）：
**Why does AgentHub use outbound-only connections instead of opening a local port or using ngrok?**

### 提问内容（Question）：
> When setting up remote control tools, many require port forwarding, DDNS, or tunneling services like ngrok or frp. How does AgentHub handle network connectivity, and is it secure for corporate laptops?

### 官方推荐解答（Answer to mark as accepted）：
> AgentHub uses a **strict outbound-only architecture**:
>
> 1. **No Inbound Ports**: The workstation running `agenthub-runner` only establishes an outbound TLS/WebSocket connection to your self-hosted Gateway. It does not listen on any public port.
> 2. **NAT & Firewall Friendly**: Because the traffic is standard outbound HTTPS/WSS, it works behind corporate NATs, routers, and VPNs without router configuration.
> 3. **Credential & Code Isolation**: AgentHub never uploads your LLM API keys or local private keys. File paths are hashed into anonymous workspace IDs, and raw sensitive tool payloads remain local.

---

## 种子话题 3：远程审批与超时卡住

### 标题（Title）：
**How does AgentHub handle terminal confirmation prompts and tool approval when I'm away?**

### 提问内容（Question）：
> Claude Code frequently asks for permission before running bash commands or editing critical files. If I'm not at my terminal, the agent hangs indefinitely. Can AgentHub surface these approval prompts remotely?

### 官方推荐解答（Answer to mark as accepted）：
> Yes. AgentHub natively hooks into the official Claude Code and Codex protocols:
>
> - Whenever the agent emits a permission request or confirmation prompt, `agenthub-runner` forwards an actionable, opaque approval descriptor to your Gateway.
> - The Web client (desktop or phone) displays a dedicated, touch-friendly approval banner showing the exact command or action.
> - Clicking **"Approve"** or **"Reject"** sends an idempotent signed receipt back to the workstation runner, which signals the native CLI to proceed.
> - If an approval times out or is rejected, the session gracefully pauses without crashing the agent.
