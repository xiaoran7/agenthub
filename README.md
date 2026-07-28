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
Web 面板（`apps/web`）已提供设备列表、会话分类视图与状态时间线的只读展示，
并经 `GET /v1/stream`（SSE）实时刷新，轮询保留为兜底。会话进入等待输入或等待
权限时可通过 Web Push 推送通知（需在 Gateway 配置 VAPID 密钥）。
生产认证、跨实例广播与心跳尚未接入；Service Worker 只服务推送，不做离线缓存。
M4 安全与稳定性阶段已开始，字段泄漏、断网保留与恢复重放、Agent 新版字段兼容、
无 Hook 降级和 12 设备/3000 Session 隔离已建立自动化基线；真实 PostgreSQL
压力、真实 Agent 升级和真实网络故障仍待环境验收，详见
[M4 验收记录](docs/M4_VALIDATION.md)。

### Web 面板

先启动 Gateway（见 [Gateway 说明](services/api/README.md)），再启动面板：

```powershell
npm.cmd run dev --workspace @agent-platform/web
```

开发服务器监听 `http://localhost:3000`，并把 `/v1` 请求代理到
`http://127.0.0.1:3001`（可用 `GATEWAY_ORIGIN` 环境变量覆盖）。面板右上角
「设置」可修改开发期用户标识（默认 `local-user`，需与 Gateway 的
`AGENT_PLATFORM_OWNER_ID` 一致）或改为直连网关地址；直连要求 HTTPS 或回环
HTTP，且需要网关侧允许跨源（尚未实现，开发期请使用同源代理）。

「设置」里的「推送通知」开关需要 Gateway 配置了 VAPID 密钥（见 Gateway 说明），
且页面运行在 https 或 localhost 上；开启后会话进入等待输入/等待权限时推送通知，
点击通知会聚焦已打开的面板并定位到该会话。

Rust 与 TypeScript 全量检查：

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```
