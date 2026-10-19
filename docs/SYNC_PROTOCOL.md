# AgentHub Remote 与本地 IPC 协议

更新：2026-10-19。本文定义 `agenthubd ↔ 本地客户端`（IPC v2）与 `agenthubd ↔ Gateway ↔ Web/Android`（Remote v3）的可靠性合同。

## 1. 本地 IPC v2

- **帧格式**：4-byte little-endian 长度前缀 JSON frame，单帧上限 1 MiB；Windows 使用当前用户 ACL 且 `PIPE_REJECT_REMOTE_CLIENTS` 的 Named Pipe，Unix 使用 `0600` Domain Socket；
- **握手与订阅**：请求/响应必带 `protocol_version` 与 `request_id`。客户端先读 `snapshot + cursor`（含 live `pending_approvals`），再按 cursor 订阅增量；cursor 越界显式返回 `snapshot_required`；
- **命令对齐**：本地命令复用 `RemoteCommand`、`client_cmd_id` 与 `host_generation` 写入同一 durable ledger；本地命令收据只写本机日志，不伪造为 Gateway 收据。

## 2. 身份、连接与代际隔离 (Generation Fencing)

- daemon 用设备 Ed25519 私钥发起 TLS/WSS 长连接；Gateway 从认证上下文解析 owner，拒绝 payload 自报归属；
- 握手协商 `protocol_version`、`connection_epoch` / `host_generation`、双方连续接收水位与可回放窗口；
- **Generation Fencing**：新认证连接替换旧 socket，旧 socket 的 close 回调不得注销新连接；单次传输故障只隔离当前 transport epoch，不停止本地 Runtime。当前代际的 `ready` 为 Session 权威全量集合，未列出的旧绑定降级为 `detached/unknown`。

## 3. Envelope 与水位 ACK

数据帧包含 `version`、`frame_id`、`type`、`hub_session_id`、`cmd_id`、`seq`、`timestamp`、`payload`。事件、命令与收据使用按设备和方向单调递增的持久 `seq`（心跳/握手/ACK 不占序号）：

- `ack_seq = N` 表示 `<= N` 的帧已连续持久落盘；先落盘后发送，接收事务提交后才回 ACK；
- 遇到序列间隙暂停推进水位并请求补齐；同 `seq` 同负载幂等忽略，同 `seq` 异负载报协议冲突；
- 超出保留窗口返回 `snapshot_required`，绝不静默跳帧。

## 4. 幂等命令与审批

统一写命令：`create_session`、`resume_session`、`attach_session`、`send_prompt`、`resolve_approval`、`interrupt_turn`、`set_remote_enabled`（`discover` 与 `import_history` 为只读）。

- **幂等键**：每个写请求带 `client_cmd_id`（作用域 `owner + daemon`）。Gateway 持久提交后即返回 HTTP 202；同 ID 重试返回原收据，同 ID 异负载返回 `conflict`；
- **状态流转**：`queued -> dispatched -> accepted -> completed | rejected | failed | indeterminate`。超时且可能已产生副作用的命令不自动重放，先与 Runtime 账本 reconcile，无法确认则持久标为 `indeterminate`；
- **工作区与审批安全**：`connect` 仅接受 `ready.workspaces` 发布的 `ws-{display}-{hash16}` 标识，拒绝物理路径；跨端审批只暴露 opaque `approval_id`、turn ID、脱敏 Diff（≤64 KiB）与允许/拒绝动作，`ready.pending_approvals` 为权威全量集合（空数组清空旧按钮），跨代际或过期审批一律拒绝。

## 5. 事件投影与多端一致性

- **统一事件**：`session_bound`、`session_control_changed`、`execution_state_changed`、`turn_started`、`turn_completed`、`message_delta`、`message_completed`、`tool_started`、`tool_completed`、`approval_requested`、`approval_resolved`、`provider_error`；
- **白名单 DTO**：Remote outbox 只投影 turn ID、消息文本、工具基础状态（`id/type/name/status`）、归一化上下文容量 `{used, window}` 与 opaque 审批描述符；Provider 原始 JSON-RPC 载荷、未脱敏路径与敏感输出只留本机；
- **增量收敛**：`message_delta` 按 `turn/item/block` 聚合，`item/completed` 最终消息原位覆盖暂态增量；`history/snapshot` 只取最新非空快照并丢弃其前陈旧重放；
- **多端并发**：客户端数量不影响 Runtime 生命周期；慢客户端使用有界缓冲，落后时通过 `snapshot_required` 重取快照，绝不阻塞 Provider 读循环。
