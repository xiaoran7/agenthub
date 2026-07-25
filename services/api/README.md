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
