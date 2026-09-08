# AgentHub Gateway API

Device Gateway、原生 Remote 会话中转与查询 API。

## 原生 Remote v3

使用 Node.js 24+，设置 `AGENTHUB_REMOTE_DATABASE` 为独立 SQLite 内容数据库路径后启用。账户、设备继续使用既有存储；Remote 的命令、正文和游标持久化在该文件中。当前为单实例实现，目录须持久挂载并限制访问，不能在多个 Gateway 实例共享此文件。

- `/v3/remote/ws`：已绑定设备使用 Ed25519 挑战签名连接，带 Host generation 和重放水位。
- `GET /v3/remote/sessions`、`/sessions/:runner/:hub`：账户隔离的状态、原生绑定和命令回执。
- `POST /v3/remote/runners/:runner/commands`：持久接受请求；HTTP 202 只表示排队，实际结果看 Runner 回执。
- `GET /v3/remote/events`、`/stream`：持久事件分页与 SSE，支持游标重放。

默认不再注册旧 v2 内存原型接口。现有 v1 账户/设备/只读采集保持独立；未设置内容数据库时 v3 不启用。原生集成测试 `test/remote-native.test.ts` 需设置 `AGENTHUB_TEST_RUNNER_BINARY` 和 `AGENTHUB_TEST_CODEX_BINARY`，会在临时工作目录运行两轮真实请求并重启 Gateway。

## 当前接口

| 方法     | 路径                                   | 用途                                           |
| -------- | -------------------------------------- | ---------------------------------------------- |
| `GET`    | `/health`                              | 进程健康检查                                   |
| `POST`   | `/v1/auth/register`                    | 注册账户并建立 HttpOnly Web 会话               |
| `POST`   | `/v1/auth/login`                       | 登录并轮换 Web 会话                            |
| `POST`   | `/v1/auth/logout`                      | 销毁当前 Web 会话                              |
| `GET`    | `/v1/auth/me`                          | 返回账户、订阅与服务端计算的权益               |
| `POST`   | `/v1/auth/email-verifications`         | 向当前账户重发邮箱验证                         |
| `POST`   | `/v1/auth/email-verifications/confirm` | 一次性确认邮箱验证 Token                       |
| `POST`   | `/v1/auth/password-resets`             | 请求不泄露账户存在性的密码重置邮件             |
| `POST`   | `/v1/auth/password-resets/confirm`     | 使用一次性 Token 设置新密码并撤销全部会话      |
| `POST`   | `/v1/auth/password`                    | 校验当前密码后改密并撤销其他会话               |
| `GET`    | `/v1/auth/sessions`                    | 列出当前账户的网页登录会话                     |
| `DELETE` | `/v1/auth/sessions/:sessionId`         | 撤销一个自己的网页登录会话                     |
| `DELETE` | `/v1/auth/sessions`                    | 撤销除当前会话外的其他网页登录会话             |
| `GET`    | `/v1/audit`                            | 分页查询账户或当前工作区的审计事件             |
| `GET`    | `/v1/team`                             | 查询当前工作区的成员、邀请和席位               |
| `POST`   | `/v1/team/invitations`                 | 邀请已注册用户加入 Team 工作区                 |
| `POST`   | `/v1/team/invitations/accept`          | 使用一次性 Token 接受 Team 邀请                |
| `DELETE` | `/v1/team/invitations/:invitationId`   | 按角色权限撤销待处理邀请                       |
| `PATCH`  | `/v1/team/members/:userId`             | 更新工作区成员角色                             |
| `DELETE` | `/v1/team/members/:userId`             | 从工作区移除成员                               |
| `POST`   | `/v1/team/workspace`                   | 切换当前 Web 会话的数据工作区                  |
| `GET`    | `/v1/billing`                          | 返回支付供应商可用性和当前订阅管理能力         |
| `POST`   | `/v1/billing/checkout`                 | 为当前账户创建 Pro/Team Checkout 跳转          |
| `POST`   | `/v1/billing/portal`                   | 创建当前账户的支付客户门户跳转                 |
| `POST`   | `/v1/billing/events`                   | 接收内部账单适配器的签名订阅事件               |
| `POST`   | `/v1/billing/stripe/events`            | 验签并处理 Stripe 订阅 webhook                 |
| `POST`   | `/v1/device-bindings`                  | 生成有时限的一次性设备绑定码                   |
| `POST`   | `/v1/devices/register`                 | 使用一次性绑定码登记设备公钥                   |
| `POST`   | `/v1/devices/heartbeat`                | 验证设备签名并更新在线活跃时间                 |
| `DELETE` | `/v1/devices/:deviceId`                | 撤销当前用户的设备，保留既有会话历史           |
| `POST`   | `/v1/sync/batches`                     | 验证并幂等接收签名事件批次                     |
| `POST`   | `/v1/device-auth`                      | 验证并幂等接收 Runner 的认证/配额安全快照      |
| `GET`    | `/v1/devices`                          | 查询当前开发用户的设备                         |
| `GET`    | `/v1/sessions`                         | 按设备、Presence、Activity Status 查询 Session |
| `GET`    | `/v1/device-auth`                      | 查询当前用户的认证/配额安全快照                |
| `POST`   | `/v1/tasks`                            | 按订阅权益向自己的设备创建远程任务             |
| `GET`    | `/v1/tasks`                            | 查询当前用户的任务与执行状态                   |
| `POST`   | `/v1/tasks/:taskId/cancel`             | 取消当前用户的未终态任务                       |
| `POST`   | `/v1/device-tasks/claim`               | 设备签名领取一个待处理任务                     |
| `POST`   | `/v1/device-tasks/:taskId/status`      | 设备签名回报版本化任务状态                     |
| `GET`    | `/v1/sessions/events`                  | 查询单个 Session 的状态事件时间线              |
| `GET`    | `/v1/stream`                           | 订阅本 owner 的会话变化信号（SSE）             |
| `GET`    | `/v1/push/key`                         | 取 VAPID 应用服务器公钥                        |
| `POST`   | `/v1/push/subscriptions`               | 登记浏览器 Web Push 订阅                       |
| `DELETE` | `/v1/push/subscriptions`               | 注销订阅（按 endpoint，owner 隔离）            |

`/v1/sessions/events` 需要 `device_id`、`agent_type`、`agent_session_id` 三个
必填查询参数定位会话，按 `sequence` 降序返回历史观察事件；`limit` 默认 100、
上限 500，查询按 owner 隔离。

`/v1/stream` 是 Server-Sent Events：连接后先发 `retry` 与 `event: ready`，此后
每当该 owner 的批次真的写入新事件就发一帧 `event: sessions`，数据只有
`{"device_id", "accepted"}`——客户端据此回读查询接口，流本身不携带会话内容。
每 20 秒一条注释心跳；单 owner 并发上限 8 条流，超出返回 429
`too_many_streams`。重放批次不产生推送。配置 PostgreSQL 时，API 启动独立的
`LISTEN/NOTIFY` Broker，把同一 owner 的变化信号广播到其他实例；通知只含公开设备 ID、
计数和变化类型，不携带 Session 数据。监听连接失败时保留当前实例的内存广播与轮询兜底。

`POST /v1/device-auth` 接收独立于 Session Outbox 的版本化 Ed25519 签名批次。Gateway
会校验设备注册、公钥、撤销状态、批次顺序和每个 provider 的白名单字段，并按
`(device_id, agent_type)` 幂等更新最新快照。`GET /v1/device-auth` 只返回状态、来源、
脱敏账号、过期时间和配额窗口；原始 access/refresh token、cookie、凭据文件内容和环境变量
值不会被该 API 接收或返回。Runner 只有在用户显式开启凭据访问后才会上报，后台同步不会
代替登录或自动完成交互式重新认证。

`POST /v1/devices/heartbeat` 接收设备 Ed25519 签名的当前时间戳。Gateway 校验设备已登记、
公钥匹配且未撤销，并拒绝超过 5 分钟时钟偏差的请求。相同或更旧的 `sent_at` 返回
`accepted: false`，不会刷新 `last_seen_at`；Runner 默认每 15 秒发送一次，最长间隔为
30 秒。PostgreSQL 使用 `last_heartbeat_at` 持久化重放水位。

Web Push 默认关闭。生成一对 VAPID 密钥并配置齐三个变量后才启用，缺一个则
`/v1/push/key` 返回 404、订阅接口拒绝登记：

```powershell
npm.cmd run vapid-keys --workspace @agent-pulse/api

$env:AGENT_PLATFORM_VAPID_PUBLIC_KEY = '...'
$env:AGENT_PLATFORM_VAPID_PRIVATE_KEY = '...'
$env:AGENT_PLATFORM_VAPID_SUBJECT = 'mailto:you@example.com'
```

私钥是服务端机密，不要提交进仓库。只有会话跃迁进入 `WAITING_INPUT` /
`WAITING_PERMISSION` 才发通知，负载仅含面板已展示的元数据。

新部署默认使用邮箱注册/登录与 HttpOnly、SameSite=Lax 会话 Cookie；生产 HTTPS 下 Cookie
同时启用 Secure 并使用 `__Host-` 前缀。密码通过带随机盐的 scrypt 保存，随机会话 Token
只在浏览器 Cookie 中出现，数据库只保存 SHA-256 摘要。设置面板可列出带公开 UUID 的
会话并逐个或批量撤销；不返回 Cookie Token 或摘要。会话分别记录登录人和当前工作区：
账户、密码与会话管理使用登录人，设备、Session、任务、SSE 与 Push 使用工作区 owner，
不接受前端传入 owner。Free 设备上限在绑定码签发前由服务端强制检查。

`GET /v1/audit` 的 `scope` 可为 `account`（默认）或 `workspace`，`limit` 为 1–100，
`before` 使用上一页最后一条事件的公开 UUID。账户范围只返回登录人的事件；工作区范围使用
当前 Web 会话选择的工作区，并在查询时验证成员关系。设备绑定/登记/撤销和远程任务创建、
领取、取消、状态回报也写入工作区时间线；设备动作不携带网页登录操作者，任务状态只保留
Agent 类型、前一状态、当前状态和公开 ID。事件只包含动作、公开 UUID、角色、套餐、状态
和数量等白名单元数据，不返回邮箱、IP、请求体、Token、Prompt 或 CLI 内容。

生产必须设置 `AGENT_PULSE_LOGIN_RATE_LIMIT_SECRET`（32 字节以上随机值的 base64url
编码）；登录失败按规范化邮箱和 Fastify 解析的客户端地址做 HMAC scope，15 分钟内第
6 次尝试起返回 429，数据库不保存原始邮箱/IP。可用以下命令生成密钥：

```powershell
node -e "console.log(require('node:crypto').randomBytes(32).toString('base64url'))"
```

邮箱验证、密码重置与 Team 邀请可通过通用 HTTPS 邮件适配器发送，也可直接使用 Resend。
通用适配器要求 `AGENT_PULSE_PUBLIC_BASE_URL`、`AGENT_PULSE_EMAIL_ENDPOINT`、
`AGENT_PULSE_EMAIL_TOKEN` 成组配置；Resend 要求 `AGENT_PULSE_PUBLIC_BASE_URL`、
`AGENT_PULSE_RESEND_API_KEY`、`AGENT_PULSE_RESEND_FROM` 成组配置，且两种适配器不能同时启用。
启用后未验证
账户不能签发设备绑定码或创建远程任务。验证 Token 只在邮件链接 fragment 中出现，
不会发送给 Web/Nginx，页面读取后立即清理地址栏，数据库只保存摘要。密码重置请求
统一返回接受，避免泄露邮箱是否注册；重置成功撤销所有会话，登录后改密只保留当前
会话。Team 邀请要求对方已注册，Token 只保存摘要、72 小时过期，未过期邀请与已加入
成员共同占用席位，接受时再次校验工作区所有者的当前 Team 权益。邮件适配器接收
`{template,to,variables}` JSON，模板名分别为 `agent-pulse-email-verification`、
`agent-pulse-password-reset` 和 `agent-pulse-team-invitation`。

未配置支付供应商时，`GET /v1/billing` 返回 `available: false`，Checkout/Portal 返回
`billing_unavailable`，不会影响账户和监控功能。配置 Stripe 的五个变量后，
`POST /v1/billing/checkout` 只接受 `pro` 或 `team`，服务端从当前 HttpOnly 会话解析用户，
把用户 UUID、方案和邮箱写入 Checkout metadata，再只返回 Stripe HTTPS 跳转 URL；已有
订阅必须进入 Portal，避免重复创建订阅。`POST /v1/billing/portal` 只使用服务端保存的
Stripe Customer ID，浏览器不能提交 Customer/Subscription ID。

Stripe webhook 必须指向 `/v1/billing/stripe/events`，使用原始请求体和
`AGENT_PULSE_STRIPE_WEBHOOK_SECRET` 验签；只接受配置的 Pro/Team Price ID，并从订阅
metadata 解析账户。事件随后进入同一个订阅 Store，沿用事件 UUID 幂等和乱序保护。
通用供应商也可以先在独立适配器内验签规范化，再向 `/v1/billing/events` 发送内部事件。
请求需带 Unix 秒 `x-agent-pulse-timestamp` 和
`x-agent-pulse-signature: sha256=<hex>`；签名输入是 `<timestamp>.<canonical-json>`，
密钥来自 `AGENT_PULSE_BILLING_WEBHOOK_SECRET`。Store 按事件 UUID 幂等并按
`occurred_at + event_id` 抗乱序；前端不能直接修改权益。`past_due/canceled` 在已付周期
结束前保留原方案权益，周期结束后自动按 Free 计算。

Stripe 配置示例：`AGENT_PULSE_PUBLIC_BASE_URL`、
`AGENT_PULSE_STRIPE_SECRET_KEY`、`AGENT_PULSE_STRIPE_WEBHOOK_SECRET`、
`AGENT_PULSE_STRIPE_PRICE_PRO`、`AGENT_PULSE_STRIPE_PRICE_TEAM` 必须成组设置，
并使用正式 HTTPS 公网地址。Stripe 密钥只在 API 进程初始化时读取，随后从进程环境删除。

远程任务只对 Pro/Team 开放，并由服务端强制执行设备归属和 3/10 个并发任务上限。
当前只接受 `local_confirmation`，任务不包含服务器路径或 shell 命令。设备领取与状态
回报使用现有 Ed25519 身份签名，合法迁移与版本号在 Store 中统一检查；Web 通过同一
owner SSE 收到变化后回读任务列表。

旧个人部署仍可显式使用 Bearer 访问令牌兼容模式。在
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
$env:AGENT_PLATFORM_DATABASE_URL = 'postgresql://agent_pulse:password@127.0.0.1:5432/agent_pulse'

npm.cmd start --workspace @agent-pulse/api
```

另一个终端启动 Web；浏览器注册或登录后通过同源 Cookie 访问：

```powershell
$env:GATEWAY_ORIGIN = 'http://127.0.0.1:3001'
npm.cmd run dev --workspace @agent-pulse/web
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

npm.cmd run migrate --workspace @agent-pulse/api
npm.cmd start --workspace @agent-pulse/api
```

`migrate` 可用于部署阶段单独执行迁移；服务启动仍会幂等检查迁移版本和 SHA-256，
历史迁移内容与数据库记录不一致时拒绝启动。当前持久化账户、邮箱验证/密码重置摘要、Web 会话、
工作区成员与邀请、登录限流 scope、订阅事件审计、`devices`、绑定码摘要、远程任务及审计
事件、不可变事件流和 Session 快照；批次写入会锁定设备行，在一个事务内完成事件幂等
检查、连续序列验证、快照合并和设备水位更新。

在 Web 设置面板点击「生成绑定码」，或带 Bearer 令牌调用
`POST /v1/device-bindings`，取得默认 10 分钟有效且仅返回一次的绑定码。数据库只
保存 SHA-256 摘要；同一设备可凭同一码幂等重试，其他设备不能复用。另开终端后，
可直接用 Runner 完成真实 HTTP 同步闭环：

```powershell
$env:AGENT_PLATFORM_GATEWAY_URL = 'http://127.0.0.1:3001'
$env:AGENT_PLATFORM_BINDING_CODE = '<从设置面板取得的一次性绑定码>'

cargo run -p agent-pulse-runner -- `
  device bind --identity runner-identity.json

cargo run -p agent-pulse-runner -- `
  sync push --identity runner-identity.json --db runner.db
```

Runner 对非回环地址强制要求 HTTPS。未配置 PostgreSQL 时 Gateway 是进程内开发
实现，重启后需要重新绑定，不能作为持久化或生产部署验证。撤销设备是软撤销：既有
Session 历史继续可读，设备显示为「已撤销」且后续同步统一被拒绝。

## 质量检查

```powershell
npm.cmd run typecheck --workspace @agent-pulse/api
npm.cmd test --workspace @agent-pulse/api
```

PostgreSQL 集成测试默认显式跳过。对专用测试数据库设置以下变量后会为每次测试创建
随机 Schema，并在结束时删除该 Schema：

```powershell
$env:AGENT_PLATFORM_TEST_DATABASE_URL = 'postgresql://agent_platform:password@127.0.0.1:5432/agent_platform_test'
npm.cmd test --workspace @agent-pulse/api
```

设置后会同时运行基础持久化契约和 M4 的 12 设备/3000 Session 并发工作负载；
后者验证双 owner 隔离、按设备查询和整批重放，不设置机器相关的耗时阈值，也不作为
容量基准。

## 尚未实现

- 联合身份提供商和实际邮件供应商适配
- 设备密钥轮换、恢复绑定和完整绑定审计日志
- 全局/设备 API 分布式速率限制
- 支付供应商收银台/webhook 适配、订阅自助变更、任务执行收据与可信执行策略
