# AgentHub Remote 协议

更新：2026-09-08。

本文只定义 `agenthubd ↔ Gateway ↔ Web/Android` 的远程可靠性合同。Desktop 到 daemon 使用本地 IPC，但复用同一命令、事件和 Session 状态语义。旧 v1 元数据同步独立兼容。

## 1. 身份与连接

daemon 使用已绑定设备的 Ed25519 身份主动建立 TLS 长连接。Gateway 从认证上下文解析 owner，不接受 payload 自报归属。握手协商：

- protocol version 与 feature flags；
- connection epoch / Host generation；
- 双方最后连续持久接收水位；
- 可回放范围和是否需要快照。

新连接通过 generation fencing 替换旧连接；旧 socket 的关闭回调不能注销新连接。设备撤销或 Remote 停止后，Gateway 立即拒绝旧身份。

## 2. Envelope 与水位

数据帧保留 `version`、`frame_id`、`type`、`hub_session_id`、`cmd_id`、`seq`、`timestamp` 和 `payload`。事件、命令和命令收据使用按设备与方向持久递增的 `seq`；ACK、握手和心跳不占数据序列。

`ack_seq=N` 表示 `<=N` 的帧已连续、持久接收：

- 收到 gap 时暂停推进水位并请求补齐；
- 重复 `seq` 不重复执行或聚合；
- 相同 `seq` 不同 payload 是协议冲突；
- ACK 不能超过已发送水位；
- 发送前落盘，接收事务提交后才 ACK；
- 窗口满时背压或磁盘排队，不丢未确认帧。

超出保留范围返回 `snapshot_required`。快照恢复当前状态，事件库提供历史；两者都不能伪造已丢内容。

## 3. 命令

统一动作：

- `create_session`
- `resume_session`
- `attach_session`（仅 Provider 能力确认后开放）
- `send_prompt`
- `resolve_approval`
- `interrupt_turn`
- `set_remote_enabled`

`discover` 与 `import_history` 是只读操作，不能暗中启动 Runtime。

每个写请求携带稳定 `client_cmd_id`，幂等作用域为 owner + daemon。同 ID、同目标、同负载返回原收据；同 ID 不同负载返回 conflict。账本跨 daemon 与 Gateway 重启保存。

命令状态：

```text
queued -> dispatched -> accepted -> completed
                  \-> rejected / failed / indeterminate
```

HTTP 202 或已写入 socket 只表示 queued/dispatched。Provider 协议确认后才是 accepted；任务完成由原生事件确认。超时后不自动重发可能有副作用的动作，先用 Runtime 状态和账本 reconcile。

审批必须绑定 Session、Host generation、turn ID 和原始 request ID。跨代际、过期或冲突决定被拒绝。中断同理，发出成功不等于执行态已变为 idle。

## 4. 事件

统一事件至少包括：

- `session_bound`、`session_control_changed`
- `execution_state_changed`
- `turn_started`、`turn_completed`
- `message_delta`、`message_completed`
- `tool_started`、`tool_completed`
- `approval_requested`、`approval_resolved`
- `provider_error`

每条事件有稳定 ID、Provider 关联 ID、Host generation 和来源。Provider raw payload 与统一事件并存；未知事件保留但不猜状态。delta 按 turn/item/block 定位，最终 item 用于收敛，重复帧不得重复拼接正文。

正文、工具输入/输出和原始事件只进入授权内容存储，不进入诊断日志。旧发现链路的元数据授权不自动扩大为全文授权。

## 5. 快照、订阅与多端

客户端先获取 `snapshot + cursor`，再订阅 cursor 之后的事件，避免读取与订阅之间丢帧。重复投递允许，业务结果必须幂等。

多个客户端共享同一 Session：

- 客户端数量不改变执行态和 Host 所有权；
- 零订阅者不停止 Runtime；
- 慢客户端使用有界缓冲，落后时重取快照，不能阻塞 daemon 的 Provider 读循环；
- daemon 离线时历史可读，写操作必须显示不可用或真实排队状态；
- Desktop 本地 IPC 与远程命令经过同一 Session Registry 和命令账本。

## 6. Reconcile

连接恢复和定期巡检交换：

- Provider Session ID 与 Hub Session ID；
- Host generation 与 ControlState；
- 当前 turn、待审批集合；
- 最后连续事件/命令水位；
- Runtime 进程与能力探测结果。

以 Runtime/Host 可验证证据收敛。找不到进程时标记 `lost/unknown`，不能依据 Gateway 的旧历史重造 `running`。如果命令可能已执行但收据缺失，标记 `indeterminate` 并要求用户或 Adapter 对齐。

## 7. 验证

必须覆盖乱序、重复、非法 ACK、连接替换、窗口满、daemon/Gateway 重启、同 ID 异负载、审批跨代际、双客户端竞态、设备撤销和跨账户拒绝。真实 WebSocket 加模拟事件只证明传输，不证明官方 Agent Remote。
