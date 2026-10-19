# Security Policy

## Supported Versions

| Version | Supported |
| :------ | :-------- |
| 2.5.x   | Yes       |
| < 2.5.0 | No        |

## Security Model

1. **Outbound-Only Connections**: `agenthub-runner` initiates outbound TLS/WSS connections to the Gateway; no public inbound ports are opened on the workstation.
2. **Credential Isolation**: AgentHub never proxies or stores LLM provider API keys; authentication stays inside the official CLI runtimes.
3. **Path & Payload Redaction**: Workstation filesystem paths are mapped to opaque `ws-*` IDs, and raw tool outputs stay local.
4. **Single-Writer Fencing**: Each native session enforces single-writer ownership and generation fencing.

## Reporting a Vulnerability

Please do **NOT** open a public issue. Email **tangwg089@gmail.com** with reproduction steps and affected component details (Runner, Gateway, or Web). We will acknowledge receipt within 48 hours.
