# Security Policy

## Supported Versions

| Version | Supported |
| :------ | :-------- |
| 2.8.x   | Yes       |
| < 2.8.0 | No        |

## Security Model

1. **Outbound-Only Connections**: `agenthub-runner` initiates outbound TLS/WSS connections to the Gateway; zero public inbound ports are opened on the workstation.
2. **Credential Isolation**: AgentHub never proxies or stores LLM provider API keys; authentication remains inside official CLI runtimes.
3. **Path & Payload Redaction**: Workstation paths are hashed into `ws-*` IDs, diffs are capped at 64 KiB with `[local_path_redacted]`, and raw tool payloads stay local.
4. **Single-Writer Fencing**: Each native session enforces single-writer ownership and `host_generation` fencing.

## Reporting a Vulnerability

Do **NOT** open a public issue. Report vulnerabilities privately via [GitHub Private Vulnerability Reporting](https://github.com/xiaoran7/agenthub/security/advisories/new) with reproduction steps and affected components (Runner, Gateway, Web, or Android). Receipt is acknowledged within 48 hours.
