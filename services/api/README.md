# AgentHub Gateway API

Fastify Gateway 负责账户、设备、Remote 路由、持久化和回放，不运行 Agent，也不是本机会话真相源。Runner 主动出站；Web 与 Android 通过账户隔离的 API/SSE 访问授权数据。

## 本地启动

需要仓库根 `package.json` 支持的 Node.js 版本。默认监听 `127.0.0.1:3001`：

```powershell
npm.cmd ci
npm.cmd start --workspace @agent-pulse/api
```

默认 Store 仅在进程内，重启后丢失。使用 PostgreSQL 时设置 `AGENT_PLATFORM_DATABASE_URL`，迁移会校验版本和历史脚本哈希：

```powershell
npm.cmd run migrate --workspace @agent-pulse/api
npm.cmd start --workspace @agent-pulse/api
```

原生 Remote v3 另需设置 `AGENTHUB_REMOTE_DATABASE` 指向持久 SQLite 文件。它是单实例内容库，目录必须持久化并限制访问，不能由多个 Gateway 实例共享。

## 验证

```powershell
npm.cmd run typecheck --workspace @agent-pulse/api
npm.cmd test --workspace @agent-pulse/api
```

PostgreSQL 集成测试仅在提供隔离的 `AGENT_PLATFORM_TEST_DATABASE_URL` 时运行。真实 Runner/Codex E2E 还需要测试专用二进制变量；普通单测不证明生产数据库、邮件、支付、Push、TLS 或 Remote 恢复可用。

## 接口边界

- `/v1/auth/*`：注册、登录、邮箱验证、密码重置、会话和改密。
- `/v1/team/*`、`/v1/billing/*`：工作区成员与订阅；支付供应商未配置时必须保持不可用。
- `/v1/device-bindings`、`/v1/devices/*`：一次性绑定、Ed25519 身份、心跳与撤销。
- `/v1/sync/*`、`/v1/sessions/*`、`/v1/stream`：只读发现事件、快照和 SSE 变化通知。
- `/v3/remote/*`：managed Remote 会话、持久命令、事件游标和设备 WSS。
- `/v1/push/*`：可选 Web Push，只有 VAPID 配置完整时启用。

精确路由、DTO、状态码和限额以 `src/app.ts`、`src/remote-routes.ts`、contracts 包和测试为准，不在 README 维护逐端点副本。

## 安全与部署

- 生产必须使用 PostgreSQL、外部 TLS 终止、可信代理配置、持久 Remote 目录和最小文件权限；API 端口不直接暴露公网。
- 浏览器使用 HttpOnly 会话或同源代理，不能持有设备私钥、Provider 凭据、Gateway 内部 Token 或支付密钥。
- 开发 `x-user-id` 模式只允许显式启用并绑定回环地址；不得用于公网。
- 一次性绑定码只返回一次且有时限；数据库只保存摘要。设备撤销后拒绝新同步，但保留授权用户可读的历史。
- Remote HTTP 202 只表示命令已排队，不能表述为 Provider 已执行成功。
- 邮件、Stripe、VAPID 和内部适配器变量必须成组配置；密钥只通过环境或 secret manager 注入。

## 文档

- [系统架构](../../docs/ARCHITECTURE.md)与[Remote 协议](../../docs/SYNC_PROTOCOL.md)。
- [单机部署](../../docs/DEPLOYMENT.md)与[已知问题](../../docs/PITFALLS.md)。
- [仓库入口](../../README.md)与[文档索引](../../docs/README.md)。
