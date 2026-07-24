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
- [变更日志](docs/CHANGELOG.md)
- [设计取舍](docs/MISC.md)
- [踩坑集](docs/PITFALLS.md)

## 开发

- [Runner 开发说明](crates/runner/README.md)

当前 Runner 已支持 Claude/Codex 一次性和持续采集、状态标准化、隐私脱敏、
SQLite Outbox、Adapter 退避与健康诊断。云端同步和 Web/PWA 尚未开始。
