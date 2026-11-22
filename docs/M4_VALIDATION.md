# Milestone 4 安全与稳定性验收基线

严格区分自动化契约测试、本机回环实测与公网生产验收。

## 1. 自动化安全与可靠性矩阵

| 验收维度 | 自动化验证证据 | 状态 |
| :--- | :--- | :--- |
| **隐私与白名单脱敏** | 含物理路径与敏感字段的 Claude/Codex/ACP 载荷经 Outbox/DTO 过滤，敏感键与绝对路径零泄漏 | 通过 |
| **断网重放与水位幂等** | SQLite Outbox 跨进程重启保留序列，恢复连接获连续 ACK 后归零；`storableFrame` 重放不误报冲突 | 通过 |
| **多设备与 PG 并发隔离** | 12 台 Ed25519 设备并发写入 3000 会话（PostgreSQL 18.4），双 owner 严格隔离、软撤销后返 `403` | 通过 |
| **同源反代与凭据隔离** | `scripts/test-web-proxy.sh`（13 项容器实测）：伪造 Bearer 被覆盖、非白名单 Cookie 被剥离、API 强制 `no-store` | 通过 |
| **TLS 与私有 CA 校验** | rustls 自签根证书握手：未配 CA 严格拒连并保留 Outbox，配 `AGENT_PLATFORM_GATEWAY_CA_CERT` 后通过 | 通过 |

## 2. 真实 Runtime 端到端实测摘要

- **多引擎原生闭环**：Runner 配合真实 Codex、Claude Code、Antigravity（`agy`）与 ACP 跑通创建、恢复、连续多轮、Diff 审批、多选问答、中断及 Gateway 重启后水位回放。
- **移动端与 PWA 视口**：在 `390x844`、`320x568` 视口验证零横向溢出、`44px` 触控热区、模态焦点圈定与 PWA 可安装性检查。
