# AgentHub Remote 协议

更新：2026-09-10。

本文只定义 `agenthubd ↔ Gateway ↔ Web/Android` 的远程可靠性合同。Desktop 到 daemon 使用本地 IPC，但复用同一命令、事件和 Session 状态语义。旧 v1 元数据同步独立兼容。

## 1. 本地 IPC

Desktop 与 daemon 使用协议 v2 的 4-byte little-endian 长度前缀 JSON frame，单帧最大 1 MiB。每个请求/响应包含 `protocol_version` 和 `request_id`；版本、请求 ID、响应类型、游标倒退或畸形 frame 不匹配时立即失败。v2 将仍可操作的 pending approval descriptor 纳入必需快照字段，旧客户端必须显式升级，不能把缺失字段默认为空。

- Windows endpoint 是当前用户 ACL 的 named pipe，并启用 `PIPE_REJECT_REMOTE_CLIENTS`；
- Unix endpoint 含当前 UID，domain socket 权限为 `0600`；
- Desktop 先获取 `snapshot + cursor`，再以该 cursor 建立订阅；首包 replay 和后续事件使用同一订阅 `request_id`；
- 断流后从最后合法 cursor 有界重连，receipt/host-lost 只推进 cursor，Host event 才进入 UI channel；
- cursor 超出保留窗口返回 `snapshot_required`，Desktop 当前回合显式失败，不跳过缺口；
- UI channel 有界，溢出或 5 秒未收到每秒心跳/事件时断开本地 waiter，但不自动 interrupt Provider turn；
- daemon 已绑定 IPC 但 Provider 尚未 ready 时返回 `daemon_starting`；Desktop 等待同一实例，不重复启动或把它显示为 stopped。

本地命令复用远程 `RemoteCommand`、`client_cmd_id` 和 `host_generation`，最终进入同一个 durable ledger。IPC 响应丢失时 Desktop 只以同一完整命令和同一 `client_cmd_id` 有界对齐；超过期限返回结果未知，不生成新业务命令。本地命令的 receipt 只进入本地事件日志，绝不伪装成 Gateway 已排队命令的收据；Remote 开启时另以合法的脱敏 `ready` 快照和后续 Host 事件同步当前 Session 状态。

## 2. 身份与连接

daemon 使用已绑定设备的 Ed25519 身份主动建立 TLS 长连接。Gateway 从认证上下文解析 owner，不接受 payload 自报归属。握手协商：

- protocol version 与 feature flags；
- connection epoch / Host generation；
- 双方最后连续持久接收水位；
- 可回放范围和是否需要快照。

新连接通过 generation fencing 替换旧连接；旧 socket 的关闭回调不能注销新连接。单个 Gateway epoch 的握手、水位、frame 或 `error` 故障只隔离该 transport，不能停止 daemon 或 Provider Runtime；后续连接使用新 epoch 重新对齐。设备撤销或 Remote 停止后，Gateway 立即拒绝旧身份。

## 3. Envelope 与水位

数据帧保留 `version`、`frame_id`、`type`、`hub_session_id`、`cmd_id`、`seq`、`timestamp` 和 `payload`。事件、命令和命令收据使用按设备与方向持久递增的 `seq`；ACK、握手和心跳不占数据序列。

`ack_seq=N` 表示 `<=N` 的帧已连续、持久接收：

- 收到 gap 时暂停推进水位并请求补齐；
- 重复 `seq` 不重复执行或聚合；
- 相同 `seq` 不同 payload 是协议冲突；daemon 在一个事务中持久绑定 inbound sequence、完整命令 payload 与 durable intent，receipt、Session、outbox 和 received watermark 也原子提交；
- ACK 不能超过已发送水位；
- 发送前落盘，接收事务提交后才 ACK；
- 窗口满时背压或磁盘排队，不丢未确认帧；远程命令一旦完成 Gateway 持久提交，HTTP 始终返回 202，即使当前 socket 因背压不能立即发送，调用方以原 `client_cmd_id` 重试仍得到同一序号。

超出保留范围返回 `snapshot_required`。快照恢复当前状态，事件库提供历史；两者都不能伪造已丢内容。

## 4. 命令

统一动作：

- `create_session`
- `resume_session`
- `attach_session`（仅 Provider 能力确认后开放）
- `send_prompt`
- `resolve_approval`
- `interrupt_turn`
- `set_remote_enabled`

`discover` 与 `import_history` 是只读操作，不能暗中启动 Runtime。

每个写请求携带稳定 `client_cmd_id`，幂等作用域为 owner + daemon。同 ID、同目标、同负载返回原收据；同 ID 不同负载返回 conflict。账本跨 daemon 与 Gateway 重启保存。远程 `connect` 的 `workspace_path` 必须是 daemon 在 `ready.workspaces` 发布的 `ws-{display}-{hash16}` 标识；Gateway Contracts 和 daemon 都拒绝物理路径、未知 binding 字段与未授权 ID。

命令状态：

```text
queued -> dispatched -> accepted -> completed
                  \-> rejected / failed / indeterminate
```

HTTP 202 或已写入 socket 只表示 queued/dispatched。Provider 协议确认后才是 accepted；任务完成由原生事件确认。超时后不自动重发可能有副作用的动作，先用 Runtime 状态和账本 reconcile。

审批必须绑定 Session、Host generation 和 turn ID。跨端只暴露域分离哈希生成的 opaque `approval_id`、turn ID 与允许/拒绝能力，不暴露 Provider request ID、命令、reason、diff 或路径；真实 response handle 和完整审批 raw 只存于当前 daemon/Provider transport。本地事件保留原始 Provider 事实，Remote outbox 在同一 SQLite 事务中写入独立安全投影。`ready.pending_approvals` 是该 runner 的远端权威全量集合，明确的空数组清除旧按钮；`approval_requested` / `approval_resolved` 只携带 opaque 描述符或 ID。Desktop 重连可从仍运行 daemon 的快照恢复按钮，daemon/Provider 重启后不得伪恢复失效审批。跨代际、过期或冲突决定被拒绝。中断同理，发出成功不等于执行态已变为 idle。

## 5. 事件

统一事件至少包括：

- `session_bound`、`session_control_changed`
- `execution_state_changed`
- `turn_started`、`turn_completed`
- `message_delta`、`message_completed`
- `tool_started`、`tool_completed`
- `approval_requested`、`approval_resolved`
- `provider_error`

Provider raw payload 与统一事件在 daemon 本地并存；未知事件保留但不猜状态。跨 Remote 边界使用按事件种类定义的显式 DTO：turn 只保留 ID，消息 delta/final 只保留 item ID 与文本，工具事件只保留 item ID、类型和完成状态。Provider server request 的顶层 JSON-RPC `id`、审批原始载荷、命令、工具输入/输出和未知 raw 字段不得进入 Gateway；Gateway 在持久化前再次按同一 allowlist 校验。Remote receipt 只允许 binding、turn ID、opaque approval ID 与 `response_sent`，完整 `provider_response` 只留本机。delta 按 turn/item/block 定位，最终 item 用于收敛，重复帧不得重复拼接正文。

消息正文与显式投影后的状态事件只进入已开启 Remote 的授权内容存储，不进入诊断日志；工具命令、参数和输出只留在本机。Remote 关闭期间 daemon 仍写有界本地事件日志、Session Registry 与命令账本，但不把新内容写入未来可上传的 outbox；Remote 已开启但网络断开时继续保留 durable outbox。重新开启时先保留此前已授权且未确认的连续序列，再入队一份当前脱敏 ready 快照，不追传关闭期间的 delta。`ready` 是当前 Host generation 的 Session 权威全量集合：本代际未列出的旧绑定保留历史身份但标记 detached/unknown，旧代际事件只能推进审计序列，不能覆盖当前 binding。旧发现链路的元数据授权不自动扩大为正文授权。

## 6. 快照、订阅与多端

客户端先获取 `snapshot + cursor`，再订阅 cursor 之后的事件，避免读取与订阅之间丢帧。重复投递允许，业务结果必须幂等。

多个客户端共享同一 Session：

- 客户端数量不改变执行态和 Host 所有权；
- 零订阅者不停止 Runtime；
- 慢客户端使用有界缓冲，落后时重取快照，不能阻塞 daemon 的 Provider 读循环；
- daemon 离线时历史可读，写操作必须显示不可用或真实排队状态；
- Desktop 本地 IPC 与远程命令经过同一 Session Registry 和命令账本。

## 7. Reconcile

连接恢复和定期巡检交换：

- Provider Session ID 与 Hub Session ID；
- Host generation 与 ControlState；
- 当前 turn、待审批集合；
- 最后连续事件/命令水位；
- Runtime 进程与能力探测结果。

以 Runtime/Host 可验证证据收敛。找不到进程时标记 `lost/unknown`，不能依据 Gateway 的旧历史重造 `running`。如果命令可能已执行但收据缺失，标记 `indeterminate` 并要求用户或 Adapter 对齐。

## 8. 验证

必须覆盖乱序、重复、非法 ACK、连接替换、窗口满、daemon/Gateway 重启、同 ID 异负载、审批跨代际、双客户端竞态、设备撤销和跨账户拒绝。真实 WebSocket 加模拟事件只证明传输，不证明官方 Agent Remote。
