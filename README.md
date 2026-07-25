# Agent-Platform

一个跨 Claude Code、OpenAI Codex 等本地 Coding Agent 的统一 Session
监控与远程接入平台。

用户在电脑上安装轻量级本地客户端后，可以通过网页端或移动端统一查看：

- 多台电脑的在线状态
- 每台电脑已安装并登录的 Coding Agent
- 多个项目和 Session 的实时状态
- 等待用户处理、最近完成及历史 Session

## 当前阶段

第一阶段聚焦安全、准确、低延迟的只读 Session 监控，不提供远程控制、
模型服务或完整代码与对话上传能力。

## 文档

- [产品需求文档](docs/PRD.md)
- [技术可行性报告](docs/TECHNICAL_FEASIBILITY.md)
- [MVP 系统架构](docs/ARCHITECTURE.md)
- [设备同步协议](docs/SYNC_PROTOCOL.md)
- [变更日志](docs/CHANGELOG.md)
- [设计取舍](docs/MISC.md)
- [踩坑集](docs/PITFALLS.md)

## 开发

- [Runner 开发说明](crates/runner/README.md)
- [Gateway API 开发说明](services/api/README.md)
- [共享同步契约](packages/contracts/README.md)

当前 Runner 已支持 Claude/Codex 一次性和持续采集、状态标准化、隐私脱敏、
SQLite Outbox、Adapter 退避与健康诊断，以及本地设备身份和签名同步批次。
云端已具备开发期设备绑定、签名批次验收、内存幂等写入与只读 Session 查询。
Runner 已可通过 HTTPS（本地开发允许回环 HTTP）完成设备绑定、批次上传和确认清理。
Gateway 已提供 PostgreSQL 迁移和事务 Store；未配置数据库时仍使用开发期内存存储。
生产认证、WebSocket/心跳和 Web/PWA 尚未接入。

Rust 与 TypeScript 全量检查：

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```
