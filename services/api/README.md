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
| `GET`  | `/v1/sessions/events`  | 查询单个 Session 的状态事件时间线              |
| `GET`  | `/v1/stream`           | 订阅本 owner 的会话变化信号（SSE）             |

`/v1/sessions/events` 需要 `device_id`、`agent_type`、`agent_session_id` 三个
必填查询参数定位会话，按 `sequence` 降序返回历史观察事件；`limit` 默认 100、
上限 500，查询按 owner 隔离。

`/v1/stream` 是 Server-Sent Events：连接后先发 `retry` 与 `event: ready`，此后
每当该 owner 的批次真的写入新事件就发一帧 `event: sessions`，数据只有
`{"device_id", "accepted"}`——客户端据此回读查询接口，流本身不携带会话内容。
每 20 秒一条注释心跳；单 owner 并发上限 8 条流，超出返回 429
`too_many_streams`。重放批次不产生推送。

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

默认 Store 保存在进程内。设置 PostgreSQL 连接串后，Gateway 会在启动时串行执行
待处理迁移，并使用事务 Store：

```powershell
$env:AGENT_PLATFORM_DATABASE_URL = 'postgresql://agent_platform:password@127.0.0.1:5432/agent_platform'

npm.cmd run migrate --workspace @agent-platform/api
npm.cmd start --workspace @agent-platform/api
```

`migrate` 可用于部署阶段单独执行迁移；服务启动仍会幂等检查迁移版本和 SHA-256，
历史迁移内容与数据库记录不一致时拒绝启动。当前持久化 `devices`、不可变事件流和
Session 快照；批次写入会锁定设备行，在一个事务内完成事件幂等检查、连续序列验证、
快照合并和设备水位更新。

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

PostgreSQL 集成测试默认显式跳过。对专用测试数据库设置以下变量后会为每次测试创建
随机 Schema，并在结束时删除该 Schema：

```powershell
$env:AGENT_PLATFORM_TEST_DATABASE_URL = 'postgresql://agent_platform:password@127.0.0.1:5432/agent_platform_test'
npm.cmd test --workspace @agent-platform/api
```

## 尚未实现

- 用户登录与正式访问令牌
- 绑定码持久化、生成、过期、撤销和设备密钥轮换
- 心跳（`/v1/stream` 只推变化，不代表设备活性）
- 跨实例的变化广播（当前 broker 在进程内，多实例部署会漏推）
- 多实例并发、速率限制与审计日志
