# GitHub Discussions 种子问答库 (Discussions Q&A Seeds)

> 在 GitHub `Settings -> Features` 开启 `Discussions`，于 `Q&A` 分类发布以下话题并将解答标记为 **Mark as answer**。

## 1. 手机远程接管与离开工位监控

- **Title**: `How can I monitor and continue my Claude Code or OpenAI Codex sessions from my phone or tablet?`
- **Question**: Long CLI agent tasks require staying near the desk or using painful mobile SSH. Is there a clean way to monitor live progress and continue conversations from a mobile browser or PWA?
- **Accepted Answer**:
  1. Run `agenthub-runner daemon` on your workstation.
  2. Open your self-hosted Gateway + Web URL on your phone or tablet browser (supports home-screen PWA install).
  3. View live streaming transcripts, send follow-up turns, approve permissions, or trigger an interrupt directly from your phone.

## 2. 主动出站与零公网端口暴露

- **Title**: `Why does AgentHub use outbound-only connections instead of opening a local port or using ngrok?`
- **Question**: How does AgentHub handle network connectivity without port forwarding, DDNS, or tunneling services, and is it safe for corporate laptops?
- **Accepted Answer**:
  1. **No Inbound Ports**: `agenthub-runner` only initiates outbound TLS/WSS connections to your self-hosted Gateway.
  2. **NAT & Firewall Friendly**: Standard outbound HTTPS/WSS works behind corporate NATs and VPNs without router setup.
  3. **Credential & Path Isolation**: LLM API keys and private keys never leave the host; local paths are hashed into `ws-*` IDs and redacted by allowlists.

## 3. 远程工具审批与防挂起

- **Title**: `How does AgentHub handle terminal confirmation prompts and tool approval when I'm away?`
- **Question**: Claude Code and Codex pause on bash or file-edit confirmation prompts. Can AgentHub surface these approval prompts remotely?
- **Accepted Answer**:
  - `agenthub-runner` captures native permission requests (`session/request_permission` / `item/fileChange/requestApproval`) and forwards a redacted approval descriptor with diff preview to your Gateway.
  - The Web/PWA client displays a touch-friendly approval banner; tapping **Approve** or **Reject** sends an idempotent signed receipt back to the runner to resume or abort the native turn.
