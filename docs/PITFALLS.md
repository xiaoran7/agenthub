# AgentHub 实现检查表 (PITFALLS)

更新：2026-10-19。仅保留会重复踩或影响架构与验收结论的硬性检查项。

## 1. 证据与完成声明

- Fixture 只证明解析或协议分支；真实 WebSocket + 模拟 Provider 只证明传输；Provider Remote 必须用真实 Runtime 完成多轮、审批、中断和恢复。
- "构建成功"不等于"运行可用"；模拟器不等于真机；回环 Gateway 不等于生产 HTTPS。

## 2. Session 与 Provider

- `discover`、`read_history`、`resume`、`attach` 严格区分；发现 PID、JSONL 或状态库不能推导为可写控制，未收到真实 `provider_session_id` 前不得建立成功绑定。
- 历史转录只在 `crates/runner/src/conversation.rs` 单点解析生成有界快照，禁止在客户端或 Gateway 直接解析 Provider 文件。
- 同一原生 Session 默认单写者，禁止每回合 `Command::spawn` 新 CLI 破坏常驻 Host 语义。
- Codex app-server 的请求响应、被动通知与带 `method` 的 server request 必须分类处理，避免 JSON-RPC ID 撞号吞掉审批请求。
- 原生最终消息（`item/completed`）与流式增量重叠时原位覆盖，不按数组尾部盲目追加。
- Agent CLI 多版本并存时，旧 binary 无法解析新 transcript（会误报 `No conversation found with session ID`）；发现必须按探测版本择优，仅对该特定错误窄匹配回退候选二进制，超时/取消/通用非零退出绝不重放。
- Grok Build 仅读 `summary.json` / `active_sessions.json` 白名单，不读 `auth.json`。

## 3. 命令、审批与重连

- HTTP 202、写入 socket、Provider accepted、turn completed 是四个独立状态；超时且可能产生副作用的命令禁止自动重试，无法确认时标 `indeterminate`。
- `client_cmd_id` 同时绑定目标与 payload，同 ID 异负载必须返回冲突。
- 审批绑定 `session + turn + host_generation + opaque approval_id`，Host 重启后拒绝旧审批；`interrupt` 请求成功不代表状态已 `idle`，需等待原生事件确认。
- ACK 只能推进到连续持久水位，窗口越界返回 `snapshot_required`；重连必须做 generation fencing，防止旧 socket 关闭回调误删新连接。

## 4. 数据与安全

- 发现通道仅传元数据，managed Remote 正文同步需明确授权；Provider Token、Cookie、环境变量与本机物理路径严禁进入 Gateway、浏览器或日志。
- 远端只提交 `ws-*` 工作区 ID；设备 owner 只能从认证上下文解析，不信请求体自报。
- 设备心跳使用独立时间水位；审计写入与业务状态同事务提交；Stripe webhook 保留原始请求体；私有 CA 通过显式根证书信任接入，严禁关闭 TLS 校验。

## 5. Windows、构建与前端

- WindowsApps shim 可能不可直接执行，启动前做真实握手；停止 Host/turn 必须通过 Windows Job Object / Unix 进程组终止完整进程树，且管道读取需带超时防死锁。
- SQLite 并发写需设置 `busy_timeout` 与单实例锁；PostgreSQL 迁移文件保持 LF 行尾字节稳定，严禁修改已发布迁移。
- PowerShell 脚本显式检查 `$LASTEXITCODE`；Node 统一用 `npm.cmd`；严禁并行运行争用 `.next` 的 `next build` 与 `next typegen`。
- Next.js `/v1/stream` SSE 必须走独立流式 Route Handler 并关闭 Nginx/OpenResty `proxy_buffering`；快照与订阅必须共享同一 cursor。
- 严禁在 `body` 使用 `background-attachment: fixed` 或在滚动列表大面积堆叠 `backdrop-filter: blur(...)`；动画服从 `prefers-reduced-motion` 且轮询不低于 500ms。
