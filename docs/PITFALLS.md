# AgentHub 实现检查表 (PITFALLS)

仅保留易重复触发的协议、安全、反代与平台硬性检查项。

## 1. 证据与 Session 边界
- Fixture 只证明解析分支，模拟 Provider 只证明传输；声明某 Runtime Remote 可用必须由真实 CLI 完成创建/恢复、多轮、审批与中断。
- 严格区分 `discover`（只读候选）与 `create`/`resume`/`attach`；历史转录仅在 `crates/runner/src/conversation/` 单点有界解析，单原生会话严格单写者，禁止每轮重新 `spawn` 破坏常驻宿主语义。
- Codex JSON-RPC 必须区分响应、被动通知与带 `method` 的服务端审批请求；多版本 CLI 并存时仅对特定会话未命中错误做窄匹配候选回退，超时/非零退出严禁盲目重放。

## 2. 命令幂等、水位与日志体积
- HTTP `202`、写入 socket、Runtime accepted 与 turn completed 四态独立；`client_cmd_id` 同时绑定目标与负载，超时副作用命令标记 `indeterminate`。
- 审批强绑定 `session + turn + host_generation + approval_id`（缺失 `decision` 按 fail-closed 拒绝）；重连必须做 generation fencing。
- `ready` 帧入 `remote_events` 前必须经 `storableFrame()` 剥离 `models` 与 `discovery`（仅保留 `pending_approvals` 等恢复锚点），避免 400KB+ 目录快照把 `remote.db` 撑爆至数 GB；Runner 侧发现指纹对纯 token/时间戳跳变执行冷却节流，无 `hub_session_id` 的杂音事件不得入库。

## 3. Nginx 同源反代与 SSE 背压
- **Nginx `proxy_set_header` 继承陷阱**：子 `location` 一旦写任意一条 `proxy_set_header` 就会丢弃 `server` 层全部同名指令（导致 Cookie 白名单与 Bearer 注入静默失效、SSE 401）；请求头必须统一收口在 `server` 层。
- **`envsubst` 变量边界**：`GATEWAY_ACCESS_TOKEN`、`WEB_ROOT` 等模板变量必须显式定义（可为空串），且 `ENVSUBST_FILTER` 必须收窄至 `^(GATEWAY_|WEB_)`，防止冲掉 `$host`/`$http_cookie`。
- **SSE 写背压与精确路由**：`/v3/remote/stream` 遇到 `!raw.write(...)` 必须异步等待 `drain` 而非直接断开；邮件验证页 `/verify-email`、`/reset-password` 须配精确 location 避免目录 301 丢失 `#token` hash。

## 4. 跨平台进程与内存治理
- Windows 停止子进程或按需 `agenthub-runner ui` WebView2 宿主必须绑定 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 作业对象；托盘监听 `TaskbarCreated`（带 UIPI `MSGFLT_ALLOW`）防止 Explorer 重启丢图标。
- Android x86_64 模拟器 `berberis` 不支持 Linux `statx()`，模拟器验收须构建 `x86_64` 或 universal 包；真机分发用 `arm64-v8a`。
