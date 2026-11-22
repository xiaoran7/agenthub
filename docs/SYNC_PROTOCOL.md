# AgentHub Remote 与本地 IPC 协议

本文定义 `agenthubd <-> 本地客户端`（IPC v2）与 `agenthubd <-> Gateway <-> Web/Android`（Remote v3）的通信契约。

## 1. 本地 IPC v2
- **传输帧**：4 字节小端长度前缀 JSON（单帧上限 `1 MiB`）；Windows 使用当前用户 ACL + `PIPE_REJECT_REMOTE_CLIENTS` 命名管道，Unix 使用 `0600` Domain Socket。
- **游标订阅**：必带 `protocol_version` 与 `request_id`；先读 `snapshot + cursor`（含 `pending_approvals`）再按游标订阅增量，越界返回 `snapshot_required`。本地命令复用同一 `RemoteJournal` 幂等账本，收据不伪造为远端收据。

## 2. 设备身份、代际隔离与结构化发现刷新
- **认证与 Fencing**：daemon 持 Ed25519 私钥建立出站 WSS；Gateway 仅从认证上下文解析 `workspace_owner_id`。新连接替换旧 socket 时做 `connection_epoch` / `host_generation` 隔离，旧 close 回调绝不误删新连接；`AGENTHUB_RUNNER_OFFLINE_GRACE_MS`（生产默认 `6000ms`）吸收瞬时闪断。
- **设备名与发现节流**：`ready.device_name` 限 `1..64` 非空字符（可通过 `agenthub-runner device name` 持久化至 `device.json`）。`evaluate_ready_publish` 对 `discovery.candidates` 做结构与状态指纹比对，纯 token/时间戳跳变引发的全量 `ready` 发布被节流至 `120s` 冷却，无 `hub_session_id` 的本地杂音通知在宿主轮询层直接过滤。

## 3. 控制台身份与瘦列表投影
- **身份解析**：Web/Android 请求优先解析会话 Cookie（`agent_pulse_session` / `__Host-agent_pulse_session`）对应的 `workspace_owner_id`，无账户自部署模式才回退 `x-user-id`；严禁客户端写死开发头。
- **列表与完整目录分离**：`GET /v3/remote/sessions` 仅返回轻量设备头（`host_generation`、`device_name`、`workspaces`、`pending_approvals`、`provider`）；完整 `models` 与 `discovery` 目录由 `GET /v3/remote/runners/:runner/hello` 按需拉取；`remote_events` 入库经 `storableFrame()` 剥离 `ready` 大字段但保留 `pending_approvals` 恢复锚点，单设备保留上限 `20,000` 条。

## 4. 持久水位 ACK 与幂等命令
- **连续水位**：数据帧含单调递增 `seq`（心跳/握手/ACK 不占号）；先落盘后发送，事务提交后回 `ack_seq`，同号同负载幂等忽略、异负载报冲突，越界返 `snapshot_required`。`/v3/remote/stream` 省略 `after` 时从尾部实时订阅（`?after=0` 保留重放），写缓冲满时异步等待 `drain` 不断流。
- **命令状态机**：写命令（`create_session`、`resume_session`、`attach_session`、`send_prompt`、`resolve_approval`、`interrupt_turn`、`set_remote_enabled`）携带 `client_cmd_id`（作用域 `owner + daemon`），流转为 `queued -> dispatched -> accepted -> completed | rejected | failed | indeterminate`。超时副作用命令绝不盲目重放。
- **审批与路径安全**：仅接受 `ws-{display}-{hash16}` 工作区标识；审批绑定 `session + turn + host_generation + approval_id`（缺失 `decision` 默认 fail-closed 拒绝），Diff 上限 `64 KiB` 且按 UTF-8 字符边界安全截断。

- **Codex 历史和控制**：Gateway 仅向在握手声明 `codex_session_control` 的 Runner 派发新版控制指令。`connect/preview` 使用 `thread/read` 校验原生会话和工作区，再读取本机已保存的历史。`connect/take_control` 和明确包含 `acquire_control: true` 的 `send_prompt` 才能恢复原生写入控制。Runner 重启时把历史绑定恢复为断开。`release_control` 或借用会话空闲后调用 `thread/unsubscribe`，会话在收到 `thread/closed` 或确认原生会话已卸载前为 `releasing_control`。执行中的回合、审批及提问会阻止归还。

## 5. 事件投影与增量收敛
- **白名单事件**：`session_bound`、`session_control_changed`、`execution_state_changed`、`turn_started`、`turn_completed`、`message_delta`、`message_completed`、`tool_started`、`tool_completed`、`approval_requested`、`approval_resolved`、`provider_error`。
- **收敛规则**：`message_delta` 按 `turn/item/block` 聚合，`item/completed` 原位覆盖流式增量；`settledConversationItems` 按已落盘用户消息绝对行数阈值退避乐观气泡，防重复文本误消或超时连坐清空。

## 6. Runner 权威插件状态同步 (Plugin Sync v2)
- **权威来源与持久化**：Runner 在 `ready.plugins` 上报本机已发现/已安装插件快照与 `registry_url`；Gateway 在 SQLite `RemoteContentStore` 的 `remote_runner_plugins` 表持久化（PostgreSQL 主库由迁移 `0018_dynamic_plugin_agent_types.sql` 放开 `agent_type` 校验），不保留写死默认列表。
- **双向控制中继与 SSE 推送**：Web/Android 通过 `POST /v3/remote/runners/:runner/plugins/action`（支持 `install`、`uninstall`、`toggle`、`refresh_catalog`、`set_registry`）下发 WebSocket `plugin_command`（远端禁用本地 `source_url` 并强制校验 HTTPS 公共源）；Runner 执行下载与 SHA-256 校验，先推 `downloading` 中间态 `plugin_state` 帧，再推带 `ok` 结果的最终 `plugin_state` 帧，同时 Gateway 向 `/v3/remote/stream` 推送 `event: plugins` 通知。Gateway 对离线目标返回 `503 runner_offline`，超时返回 `504 plugin_action_timeout`。
