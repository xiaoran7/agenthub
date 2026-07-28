# Agent-Platform Gateway API

Device Gateway 与只读查询 API 的开发期实现。

## 当前接口

| 方法     | 路径                     | 用途                                           |
| -------- | ------------------------ | ---------------------------------------------- |
| `GET`    | `/health`                | 进程健康检查                                   |
| `POST`   | `/v1/device-bindings`    | 生成有时限的一次性设备绑定码                   |
| `POST`   | `/v1/devices/register`   | 使用一次性绑定码登记设备公钥                   |
| `DELETE` | `/v1/devices/:deviceId`  | 撤销当前用户的设备，保留既有会话历史           |
| `POST`   | `/v1/sync/batches`       | 验证并幂等接收签名事件批次                     |
| `GET`    | `/v1/devices`            | 查询当前开发用户的设备                         |
| `GET`    | `/v1/sessions`           | 按设备、Presence、Activity Status 查询 Session |
| `GET`    | `/v1/sessions/events`    | 查询单个 Session 的状态事件时间线              |
| `GET`    | `/v1/stream`             | 订阅本 owner 的会话变化信号（SSE）             |
| `GET`    | `/v1/push/key`           | 取 VAPID 应用服务器公钥                        |
| `POST`   | `/v1/push/subscriptions` | 登记浏览器 Web Push 订阅                       |
| `DELETE` | `/v1/push/subscriptions` | 注销订阅（按 endpoint，owner 隔离）            |

`/v1/sessions/events` 需要 `device_id`、`agent_type`、`agent_session_id` 三个
必填查询参数定位会话，按 `sequence` 降序返回历史观察事件；`limit` 默认 100、
上限 500，查询按 owner 隔离。

`/v1/stream` 是 Server-Sent Events：连接后先发 `retry` 与 `event: ready`，此后
每当该 owner 的批次真的写入新事件就发一帧 `event: sessions`，数据只有
`{"device_id", "accepted"}`——客户端据此回读查询接口，流本身不携带会话内容。
每 20 秒一条注释心跳；单 owner 并发上限 8 条流，超出返回 429
`too_many_streams`。重放批次不产生推送。

Web Push 默认关闭。生成一对 VAPID 密钥并配置齐三个变量后才启用，缺一个则
`/v1/push/key` 返回 404、订阅接口拒绝登记：

```powershell
npm.cmd run vapid-keys --workspace @agent-platform/api

$env:AGENT_PLATFORM_VAPID_PUBLIC_KEY = '...'
$env:AGENT_PLATFORM_VAPID_PRIVATE_KEY = '...'
$env:AGENT_PLATFORM_VAPID_SUBJECT = 'mailto:you@example.com'
```

私钥是服务端机密，不要提交进仓库。只有会话跃迁进入 `WAITING_INPUT` /
`WAITING_PERMISSION` 才发通知，负载仅含面板已展示的元数据。

绑定码签发、设备撤销、设备与 Session 查询、SSE 和 Push 订阅接口使用 Bearer
访问令牌认证。个人部署在
Gateway 配置 `AGENT_PLATFORM_ACCESS_TOKEN`，在 Web 服务端配置相同的
`GATEWAY_ACCESS_TOKEN`；浏览器只访问同源代理，令牌不会进入 localStorage、URL
或浏览器请求头。Gateway 构造认证器后只保留令牌的 SHA-256 摘要，并以固定长度
比较验证请求，同时删除进程中的原始令牌环境项。令牌要求 32–512 UTF-8 字节且
不能包含空白字符。

`x-user-id` 只在显式设置 `AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true` 时启用，
且服务会拒绝将该模式绑定到非回环 `HOST`。它只用于本地开发和协议测试。请求体
上限为 1 MiB。未配置 PostgreSQL 时存储是进程内实现，服务重启后数据和绑定码状态
都会丢失。

## 本地运行

```powershell
$env:AGENT_PLATFORM_ACCESS_TOKEN = node -e "console.log(require('node:crypto').randomBytes(32).toString('base64url'))"
$env:AGENT_PLATFORM_OWNER_ID = 'local-user'

npm.cmd start --workspace @agent-platform/api
```

另一个终端启动 Web 时，把同一令牌作为服务端变量传入：

```powershell
$env:GATEWAY_ACCESS_TOKEN = '<与 Gateway 相同的令牌>'
$env:GATEWAY_ORIGIN = 'http://127.0.0.1:3001'
npm.cmd run dev --workspace @agent-platform/web
```

默认只监听 `127.0.0.1:3001`。可通过 `HOST` 和 `PORT` 调整；对外部署前必须接入
TLS 终止、速率限制和持久化存储。令牌只应注入进程环境或密钥管理器，不要提交、
写入 URL 或传给浏览器。仅做本地开发时可不配置令牌，改为：

```powershell
$env:AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH = 'true'
```

默认 Store 保存在进程内。设置 PostgreSQL 连接串后，Gateway 会在启动时串行执行
待处理迁移，并使用事务 Store：

```powershell
$env:AGENT_PLATFORM_DATABASE_URL = 'postgresql://agent_platform:password@127.0.0.1:5432/agent_platform'

npm.cmd run migrate --workspace @agent-platform/api
npm.cmd start --workspace @agent-platform/api
```

`migrate` 可用于部署阶段单独执行迁移；服务启动仍会幂等检查迁移版本和 SHA-256，
历史迁移内容与数据库记录不一致时拒绝启动。当前持久化 `devices`、绑定码摘要、
不可变事件流和 Session 快照；批次写入会锁定设备行，在一个事务内完成事件幂等
检查、连续序列验证、快照合并和设备水位更新。

在 Web 设置面板点击「生成绑定码」，或带 Bearer 令牌调用
`POST /v1/device-bindings`，取得默认 10 分钟有效且仅返回一次的绑定码。数据库只
保存 SHA-256 摘要；同一设备可凭同一码幂等重试，其他设备不能复用。另开终端后，
可直接用 Runner 完成真实 HTTP 同步闭环：

```powershell
$env:AGENT_PLATFORM_GATEWAY_URL = 'http://127.0.0.1:3001'
$env:AGENT_PLATFORM_BINDING_CODE = '<从设置面板取得的一次性绑定码>'

cargo run -p agent-platform-runner -- `
  device bind --identity runner-identity.json

cargo run -p agent-platform-runner -- `
  sync push --identity runner-identity.json --db runner.db
```

Runner 对非回环地址强制要求 HTTPS。未配置 PostgreSQL 时 Gateway 是进程内开发
实现，重启后需要重新绑定，不能作为持久化或生产部署验证。撤销设备是软撤销：既有
Session 历史继续可读，设备显示为「已撤销」且后续同步统一被拒绝。

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

设置后会同时运行基础持久化契约和 M4 的 12 设备/3000 Session 并发工作负载；
后者验证双 owner 隔离、按设备查询和整批重放，不设置机器相关的耗时阈值，也不作为
容量基准。

## 尚未实现

- 多用户登录、令牌签发/轮换/撤销和联合身份提供商
- 设备密钥轮换、恢复绑定和完整绑定审计日志
- 心跳（`/v1/stream` 只推变化，不代表设备活性）
- 跨实例的变化广播（当前 broker 在进程内，多实例部署会漏推）
- 多实例并发、速率限制与审计日志
