# AgentHub 原生远程会话架构

更新：2026-09-07。目标由 [PRD](PRD.md) 定义；本文替代旧 v2 架构。迁移中的旧路径不因此自动改变行为。

## 组件边界

- Clients：Web、Tauri、Android 共享远程会话契约与命令语义，展示历史和实时事件。Tauri 保留设备安装/本地选择能力，不再自建另一套会话执行语义。
- Gateway：账户/设备鉴权、所有权路由、会话历史、命令收据、实时订阅与持久化传输。不能自行推定 Agent 已执行。
- Runner：每个已绑定设备一条主动出站控制连接；持有本地工作区授权、Session Registry、可靠事件日志以及 Provider Hosts。网络断开不销毁 Host。
- Provider Host：长期持有官方控制通道，处理创建、恢复、输入、审批、原生中断和事件。不得用每条 Prompt 另起一次进程来模拟持续会话。
- Discovery：只读来源枚举，与 Host 分离。历史导入、resume、attach 是不同动作。

## 会话与状态

稳定身份：(owner_id, runner_id, provider, provider_session_id) 唯一映射到 hub_session_id。工作区实体由 runner_id + 本机规范化路径标识，同名目录不合并；不同设备的同一仓库也不凭名字自动合并。

SessionBinding 保存 origin(managed/discovered)、control_state(detached/connecting/connected/lost)、原生 ID、工作区、原生进程代际及配置。执行态为 unknown/idle/running/awaiting_input/awaiting_permission/error/completed/terminated，仅原生回报或已确认退出能推动变化。客户端数量不改变执行态；Runner 离线只改变连通性和控制可用性。

启动先记录命令意图，取得真实原生 ID 后建立绑定；失败必须返回失败收据。恢复须验证 Provider 和 ID，禁止静默 fork。外部 attach 只有适配器已验证支持时开放。Host 重启产生新代际，旧审批与未知命令不能投递给新进程。

## Provider 适配

Codex 通过常驻 app-server JSON-RPC：initialize/initialized、thread/start、thread/resume、turn/start、turn/interrupt。持续消费通知与 server requests；审批使用原始 JSON-RPC request id 回应。扫描用的通知过滤不能复用于交互 Host。

Claude 采用官方支持的长时流式控制接入；SDK 认证适用性独立验证。仅看到原生 Remote 产品存在，不代表存在第三方可调用的 Remote 隧道。可行性与官方来源见 [技术可行性](TECHNICAL_FEASIBILITY.md)。

适配器的错误、未知事件和不支持动作显式返回；不可构造“成功启动”事件或替 Provider 宣布审批/中断已完成。

## 持久化与重连

Runner SQLite 保存 Registry、命令账本、出站日志及接收水位。Gateway 数据库保存 owner-scoped Session、原始事件、统一事件、聚合 Turn、命令收据及传输水位。

首个可运行远程切片使用 Gateway 独立 SQLite 内容库（Node 24+），事务保存内容、命令与水位；单实例部署，旧账户/设备 Store 可继续使用 PostgreSQL。多实例部署须补 PostgreSQL 内容 Store 与跨实例连接路由，不能把本切片当多实例验收。内容库路径必须显式配置，未配置时不开新控制入口。

原始层保存 Provider 消息及关联元数据；Hub Envelope 不能冒充 Provider 原始事件。流式广播和持久化解耦，但 ACK 之前必须存在可恢复的持久记录。Turn/Block 聚合支持重复事件去重，结束时最终消息校正暂态 delta。

所有客户端读取同一个 Gateway 读模型；先获取带 cursor 的快照，再从 cursor 订阅变化，避免读快照与订阅之间丢事件。落后超出保留范围返回 snapshot_required，历史仍从持久存储读取。

传输细节见 [同步协议](SYNC_PROTOCOL.md)。旧 v1 SQLite Outbox 是元数据协议，不可直接把全文塞进旧白名单。

## 访问边界

复用已绑定设备身份验证 Runner；握手 payload 自报 runner_id 不构成认证。每个会话列表、正文、订阅、命令和能力接口都检查 owner 与设备归属。远端必须 TLS；浏览器不持有 Runner 私钥或 Provider 凭据。

远程可选工作区来自本机授权列表。客户端提交的路径必须经 Runner 规范化并核验，不执行客户端提交的任意 shell。正文与原始事件可能含用户信息，仅保存于授权内容存储，不输出到诊断日志。

## 迁移策略

保留旧发现、元数据同步、账户、设备和本地渲染能力。v2 的模拟 Router、硬编码探测和内存事件存储属于待替换骨架。先落地一个真实 Provider Host 与协议测试，再接鉴权的持久化远程通道，最后切换三端。未完成的入口不得宣称已接入；旧本地 CLI 路径在客户端迁移完成后移除。

具体里程碑与证据统一维护在 [文档索引](README.md)，不把本设计当完成报告。
