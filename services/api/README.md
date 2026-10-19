# AgentHub Gateway API (`@agent-pulse/api`)

Fastify Gateway 负责账户鉴权、设备验签、Remote v3 路由、命令账本与事件回放；不执行 Agent，也不持有 Provider 凭据。

## 启动与存储

```powershell
# 本地启动（默认监听 127.0.0.1:3001）
# 默认使用 node:sqlite 持久化：./data/gateway.db（账户与设备）与 ./data/remote.db（Remote v3 内容库）
npm.cmd ci
npm.cmd start --workspace @agent-pulse/api

# 可选：设置 AGENT_PLATFORM_DATABASE_URL 切换账户与设备存储至 PostgreSQL
npm.cmd run migrate --workspace @agent-pulse/api
npm.cmd start --workspace @agent-pulse/api
```

## 验证

```powershell
npm.cmd run typecheck --workspace @agent-pulse/api
npm.cmd test --workspace @agent-pulse/api
```

## 路由概览

- `/v1/auth/*`：单管理员首次初始化（`GET /v1/auth/status`、`POST /v1/auth/setup`）、登录会话与密码管理；
- `/v1/device-bindings`、`/v1/devices/*`：一次性绑定码（SHA-256 摘要）、Ed25519 身份、心跳与软撤销；
- `/v1/sync/*`、`/v1/sessions/*`、`/v1/stream`、`/v1/push/*`：只读发现同步、SSE 变更流与可选 VAPID Web Push；
- `/v3/remote/*`：managed Remote 会话、幂等命令（HTTP 202 仅代表已入队）、事件游标与设备 WSS。

精确 schema 与部署边界见 [SYNC_PROTOCOL](../../docs/SYNC_PROTOCOL.md) 与 [DEPLOYMENT](../../docs/DEPLOYMENT.md)。
