# Security Policy

## Supported Versions

Only the latest release branch of AgentHub receives active security updates.

| Version | Supported          |
| ------- | ------------------ |
| 2.4.x   | :white_check_mark: |
| < 2.4.0 | :x:                |

---

## Security Model & Design Principles

AgentHub is designed from the ground up for strict workstation and session isolation:

1. **Outbound-Only Connections**: The local runner (`agenthub-runner`) initiates outbound TLS/WSS connections to the self-hosted Gateway. It does not open inbound listening ports on public networks.
2. **Credential Privacy**: AgentHub does NOT proxy, record, or store LLM provider API keys or session credentials. Authentication to official agents is managed natively by their respective CLI runtimes.
3. **Workspace Path Obfuscation**: Workstation filesystem paths are converted to irreversible opaque identifiers (`workspace_id`) before entering sync streams.
4. **Single-Writer Safety**: Each native session is bound to an authoritative owner with atomic lease fencing to prevent duplicate or conflicting tool executions.

---

## Reporting a Vulnerability

If you discover a security vulnerability within AgentHub, please do **NOT** open a public issue.

Instead, please send an email to:
📧 **tangwg089@gmail.com**

Please include:
- A description of the vulnerability and its potential impact.
- Step-by-step instructions or proof-of-concept to reproduce the issue.
- Details of the affected component (Runner, Gateway, or Web).

You will receive an acknowledgment within 48 hours, followed by updates as we assess and patch the issue.
