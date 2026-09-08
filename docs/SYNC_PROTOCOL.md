# AgentHub 远程会话协议

更新：2026-09-07。本文定义目标控制协议；当前 v1 元数据同步独立兼容。旧 v2 示例不能证明本规范已实现。

## 传输与身份

Runner 主动建立单条 WSS 多路复用连接。升级前/握手时校验已绑定设备身份、签名、时效与撤销状态；owner 由 Gateway 解析，不接受客户端自报归属。Web/桌面/Android 经账户鉴权使用同一命令与订阅接口。

握手协商 protocol version、feature flags、connection epoch、双方最后连续持久接收水位及重放范围。无共同版本则拒绝；新连接不能以未经校验的 runner_id 覆盖别人的连接。替换连接使用 generation fencing，旧连接 close 回调不能注销新连接。

## Envelope 与序列

保留 ver/frame_id/type/hub_session_id/cmd_id/timestamp/payload 概念。数据帧 event/cmd/cmd_ack 使用按设备与方向持久递增的 seq；ACK/握手/心跳为控制帧，不占可重放数据序列，避免 ACK 风暴和虚假缺口。下一代协议变更必须使用显式版本或协商 feature，不能默默改变旧 v2 seq 语义。

ack_seq=N 表示 <=N 的数据已连续持久接收，而非“见过的最大序号”。重复 seq 不重复执行/聚合；收到 gap 暂停推进水位并请求补齐。ACK 不得超过已发送水位。seq 相同但 payload 不同视为协议冲突。

出站记录在发送前落盘，事务提交接收记录后才 ACK。窗口满采用背压/磁盘排队，不丢弃未确认数据。超出保留范围明确返回 snapshot_required；快照恢复状态，持久日志提供历史，不伪称恢复了已丢正文。

## 命令与结果

每个控制请求携带 client_cmd_id（线上 Envelope 的 cmd_id）。其作用域是 owner + runner，同 ID、同目标和同负载返回同一收据；同 ID 不同负载返回 conflict。持久账本不得因进程重启丢失去重信息。

动作：launch_session、resume_session、attach_session、send_prompt、resolve_approval、interrupt_turn。discover/import_history 是只读发现/历史同步动作，绝不暗中启动进程。现有 import_native_session 在迁移期只能表达历史关联，不能代表 attach。

命令状态：queued → accepted → completed 或 rejected/failed/indeterminate。accepted 表示原生协议接受，并不代表任务完成。HTTP 202 或 WS 已发送也不代表 accepted。超时之后禁止自动重复产生副作用；先通过原生状态与账本对齐。

审批必须关联 hub_session_id、Host generation、turn_id 和原始 request_id。过期、跨会话、重复冲突决策拒绝；成功写入回复后仍等待原生后续事件更新执行态。interrupt 同理，不能立即把本地状态改成 idle。

## 事件与内容

统一事件包含 session_bound、session_control_changed、execution_state_changed、turn_started、message_delta、message_completed、tool_started、tool_completed、approval_requested、approval_resolved、turn_completed、provider_error。每条有稳定 event_id、原生关联 ID 与 Provider 消息来源。

Provider raw payload 与统一事件并存，未知事件保存且不猜测状态。Stream delta 使用 turn/item/block 身份定位，不能只按数组尾部追加。重复帧不重复追加文字；原生最终 item 用于收敛结果。工具输入和输出进入授权内容存储，不进入日志。

## Reconcile 与多端

重连和定期巡检交换原生 ID、Host generation、控制状态、当前 turn、待审批集合与最后事件水位。以 Agent/Host 可验证证据收敛；找不到进程标记 lost/unknown，不能根据服务器历史重造 running。

多端先读取 snapshot + cursor，再订阅 cursor 之后的事件。重复投递可接受，业务结果必须幂等。零订阅者不停止 Runner；慢客户端采用有界缓冲并要求重取快照，不能拖住 Agent 读循环。

## 验证

必须测试乱序、重复、非法 ACK、重连、窗口满、进程重启、同 ID 异负载、审批跨代际、双客户端竞态与跨账户拒绝。真实 WebSocket 加模拟事件只算传输测试，不是原生 Agent Remote 验收。