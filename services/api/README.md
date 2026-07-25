# Agent-Platform Gateway API

Device Gateway 与只读查询 API 的开发期实现。

## 当前接口

| 方法   | 路径                   | 用途                                           |
| ------ | ---------------------- | ---------------------------------------------- |
| `GET`  | `/health`              | 进程健康检查                                   |
| `POST` | `/v1/devices/register` | 使用一次性绑定码登记设备公钥                   |
| `POST` | `/v1/sync/batches`     | 验证并幂等接收签名事件批次                     |
| `GET`  | `/v1/devices`          | 查询当前开发用户的设备                         |
| `GET`  | `/v1/sessions`         | 按设备、Presence、Activity Status 查询 Session |

读接口暂时使用 `x-user-id` 头模拟已认证用户，只允许本地开发和协议测试，不能作为
生产认证。请求体上限为 1 MiB。存储是进程内实现，服务重启后数据和绑定码状态都会
丢失。

## 本地运行

```powershell
$env:AGENT_PLATFORM_BINDING_CODE = 'local-code'
$env:AGENT_PLATFORM_OWNER_ID = 'local-user'

npm.cmd start --workspace @agent-platform/api
```

默认只监听 `127.0.0.1:3001`。可通过 `HOST` 和 `PORT` 调整；对外部署前必须接入 TLS
终止、正式认证、速率限制和持久化存储。

另开一个终端后，可直接用 Runner 完成真实 HTTP 同步闭环：

```powershell
$env:AGENT_PLATFORM_GATEWAY_URL = 'http://127.0.0.1:3001'
$env:AGENT_PLATFORM_BINDING_CODE = 'local-code'

cargo run -p agent-platform-runner -- `
  device bind --identity runner-identity.json

cargo run -p agent-platform-runner -- `
  sync push --identity runner-identity.json --db runner.db
```

Runner 对非回环地址强制要求 HTTPS。本地 Gateway 是进程内开发实现，重启后需要重新
绑定，不能作为持久化或生产部署验证。

## 质量检查

```powershell
npm.cmd run typecheck --workspace @agent-platform/api
npm.cmd test --workspace @agent-platform/api
```

## 尚未实现

- PostgreSQL 事务存储和迁移
- 用户登录与正式访问令牌
- 绑定码生成、过期、撤销和设备密钥轮换
- WebSocket 心跳与实时推送
- 多实例并发、速率限制与审计日志
