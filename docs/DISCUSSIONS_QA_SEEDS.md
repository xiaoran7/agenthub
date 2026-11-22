# GitHub Discussions 种子问答库

## 1. 离开工位后如何用手机继续 Claude Code / Codex / Antigravity 会话？
- **Q**: `How can I monitor and continue CLI agent sessions from my phone without painful mobile SSH?`
- **A**: Run `agenthub-runner daemon` on your workstation and open your self-hosted AgentHub Web PWA (or Android client). You get live streaming transcripts, touch-friendly diff approvals, multi-select prompt cards, and one-tap turn interrupts.

## 2. 为什么采用主动出站 TLS/WSS 而非本地开端口或内网穿透？
- **Q**: `Why does AgentHub use outbound-only connections instead of opening workstation ports?`
- **A**: `agenthub-runner` initiates standard outbound WSS connections to your Gateway—working seamlessly behind NAT/corporate firewalls with zero inbound ports, while keeping LLM API keys on-host and redacting paths into `ws-*` IDs.

## 3. 远程权限审批与防重复执行如何保证安全？
- **Q**: `How are terminal permission prompts and network retries handled safely?`
- **A**: Native approval requests are projected with `<=64 KiB` path-redacted diffs and bound to `session + turn + host_generation + approval_id`. Every remote action carries an idempotent `client_cmd_id` so retries never double-execute commands.
