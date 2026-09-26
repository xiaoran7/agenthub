# AgentHub 系统架构

更新：2026-09-12。

## 1. 架构结论

AgentHub 只有一个 Session 核心：运行在开发机上的 `agenthubd`。Desktop 是本地宿主客户端，Web 与 Android 是远程客户端；Gateway 只是出站通道上的控制中继和持久化层。读取会话历史只服务于“判断并执行继续”：transcript 解析是 daemon 的单点职责，对暂不能远程控制的 Provider 只展示有界元数据。

```text
┌──────────────────────────── 开发机 ────────────────────────────┐
│                                                                │
│ Desktop ── local IPC ──► agenthubd ── Provider Adapter ──► Runtime
│                              │                         Codex app-server
│                              │                         Claude Code …
│                              │
│                              ├─ Session Registry / Ownership
│                              ├─ Command Ledger / Event Log
│                              ├─ Workspace Authorization
│                              └─ Discovery (read-only)
└──────────────────────────────┬─────────────────────────────────┘
                               │ outbound TLS, optional
                               ▼
                         ┌── Gateway ──┐
                         │ auth/route  │
                         │ store/replay│
                         └────┬───┬────┘
                              │   │
                             Web Android
```

本地操作不绕 Gateway。远端控制不创建第二个 Runtime。网络、客户端和执行状态相互独立。

## 2. 组件职责

### agenthubd

`agenthubd` 是当前已经落地的架构角色，实现主体仍在 `agent-pulse-runner`。Desktop 通过自身可执行文件的隐藏 `--agenthubd` 模式承载同一实现，尚未提供独立命名的二进制或 OS 服务安装包：

- 由 Desktop 自动连接或启动；退出 Desktop、退出登录和关闭 Remote 均不停止本地 Runtime；
- 也可通过 `agent-pulse-runner daemon` 显式运行，`remote-run` 进入同一 daemon 主流程；
- 管理设备身份、授权工作区和 Provider 可执行文件；
- 启动/恢复官方 Runtime，长期持有控制通道；
- 保存 Session Registry、写者所有权、Host generation、命令账本和可恢复事件日志；
- 同时接受 Desktop 本地 IPC 和 Gateway 远程命令；
- 断网继续执行；Remote 已开启时按持久水位补发，Remote 关闭期间只保留本地事件、Registry 和命令账本，不积累未来可上传正文；
- 只读发现外部历史，但不把发现结果升级成控制权。

同一设备只能有一个 daemon 实例持有数据库和 Provider Host。Desktop 退出只断开本地订阅，退出登录和关闭 Remote 只撤销 Gateway 暴露；本地 Runtime 继续运行。停止本地 daemon 的独立产品控制和 OS 服务生命周期仍待后续实现。

### Desktop

Desktop 是 `agenthubd` 的富客户端和当前启动器：

- 连接当前用户的 daemon IPC；仅在 endpoint 不可用时启动自身隐藏 `--agenthubd` 进程，不因业务或协议错误重复拉起；
- 通过版本化本地 IPC 获取快照、订阅事件和提交命令；
- 提供本地目录选择、Runtime 登录入口、工作区设置和完整过程 UI；
- Gateway 不可用时保持创建、恢复、续聊、审批和中断；
- 不直接 `Command::spawn` 每个回合，也不维护另一份 Session 状态机；
- 不直接解析 Provider transcript 文件：会话预览经 daemon 的 `GetSessionConversation` IPC 获取；明确恢复为 managed Remote 后，缺少原生历史回放的 Provider 由同一 daemon 解析器生成有界历史快照。transcript 解析只在 `crates/runner/src/conversation.rs` 单点实现；发现态 Provider 只有有界元数据，无全文浏览。

本地 IPC 已使用 v2 的版本化、长度前缀 JSON frame：Windows 为当前用户 ACL 且拒绝远程客户端的 named pipe，Unix 为当前 UID 命名且权限 `0600` 的 domain socket，单帧上限 1 MiB。请求使用 `request_id` 关联；订阅以包含 live pending approvals 的 `snapshot + cursor` 起步，断流后按最后 cursor 重放，窗口过期显式返回 `snapshot_required`。daemon 启动中是可等待状态，Desktop 的自动启动、Session 和 Remote 设置由同一锁协调，最多拉起一个本进程可见的实例；Remote enable、disable、logout 与 profile 提交也由该锁串行化，运行态切换失败不预写持久状态。

### Provider Adapter 与 Runtime

Adapter 将统一命令翻译为官方协议，同时保留原始事件：

- `create`、`resume`、`send_prompt`、`resolve_approval`、`interrupt` 分别映射到 Provider 的真实动作；
- 每项能力由运行时探测报告，不用硬编码模型或假能力表；
- 未知通知持久保存但不猜测状态；
- 进程树、stderr、超时和协议版本错误由 Adapter 隔离。

Codex 首选常驻 app-server。官方 App Server 提供初始化、thread start/resume/read/fork、turn、流式通知和审批 server request，适合富客户端维护长期会话。当前代码已经实现 create/resume/turn/approval/interrupt 的首个 Host。

Claude Code 已有历史发现与流式续聊实现；长时输入、事件、审批、认证和 Session ID 仍需按实际 CLI 版本分别验收，源码分支不证明所有 managed Remote 语义已通过。Antigravity 与 Grok Build 在等价控制协议建立证据前保持发现/只读。

### Gateway

Gateway 负责：

- 账户、工作区、设备身份和撤销；
- 将远程命令路由到 Session 所属的在线 daemon；
- 持久保存事件、命令收据和双方水位，向客户端提供快照与回放；
- SSE/WebSocket/推送等客户端分发；
- 多租户隔离、限流和审计元数据。

Gateway 不解析 Provider 凭据、不运行代码、不自行推进执行状态，也不在 daemon 离线时假装命令成功。当前 v3 内容库是单实例 SQLite；需要横向扩展时再迁移到 PostgreSQL 和跨实例连接路由。

### Web 与 Android

二者只通过 Gateway 使用同一 Remote API：

- Web 提供接近 Desktop 的远程完整视图；
- Android 优先呈现状态、通知、审批、急停和短输入；
- 都可以在 daemon 已授权的工作区内创建官方 Session；
- 都不能提交任意本机路径或建立新的 SSH/WSL/Docker 环境；
- daemon 离线时仍可读历史，但写操作明确禁用或排队策略明确显示。

## 3. Session 与所有权

稳定映射：

```text
(owner_id, daemon_id, provider, provider_session_id) -> hub_session_id
```

SessionBinding 至少保存：

- `origin`: managed / discovered；
- `control_state`: detached / connecting / connected / lost；
- Provider Session ID、工作区 ID、Host generation；
- 当前 turn、待审批集合和最后原生事件水位；
- 经探测确认的 capability report。

执行态 `unknown / idle / running / awaiting_input / awaiting_permission / error / completed / terminated` 只由原生证据改变。daemon 离线改变的是 Presence 和 ControlState，不把 running 改成 completed。

启动流程先落盘命令意图，Runtime 返回真实 ID 后再绑定。恢复必须确认原生 ID；attach 必须先验证共享 writer 或所有权转移。新的 Host generation 不接收旧审批和不确定命令。

## 4. 数据与可靠性

daemon SQLite 保存 Registry、命令账本、出站日志、接收水位和最小恢复信息。Gateway 仅保存 owner-scoped Session、安全事件 DTO、Turn 聚合、受限命令收据和传输水位；Provider 原始事件、完整收据、工具命令/参数/输出只留在 daemon 本地。

- Provider raw 与 Hub Envelope 在 daemon 本地分层保存；Remote outbox 按事件类型只投影 turn ID、消息 item/text、工具 item/type 和 opaque approval descriptor，完整 `provider_response` 与工具输入/输出不进入 Gateway；Gateway 对 receipt、event raw、binding 和 `ws-*` 工作区 ID 再做严格 allowlist 校验；`ready.pending_approvals` 作为 runner 远端权威全量集合驱动 Web/Android 增量恢复与清空；
- 事件发送前落盘，接收事务提交后 ACK；
- 多端先读 `snapshot + cursor`，再订阅 cursor 之后的事件；
- delta 按 turn/item/block 身份聚合，最终 `item/completed` agent message 全量收敛暂态内容，`turn.output` 仅在缺少 final item 时兜底；
- Remote inbound sequence 与完整 command payload、durable intent 原子绑定；命令可能已产生 Provider 副作用但无 receipt 时持久返回 `indeterminate`，不自动重放；Desktop 本地命令的 receipt 只写本地日志，Remote 状态通过 `ready` 与 Host 事件同步，避免生成 Gateway 不认识的收据；
- Gateway 协议故障只 fence 当前 transport epoch，不改变 daemon/Provider 生命周期；新认证 generation 替换旧 socket，旧回调不能注销当前连接；当前 generation 的 `ready` 是 Session 全量集合，遗漏绑定降级为 detached/unknown，旧代际事件保留审计历史但不能改写当前状态；
- 命令在 Gateway 持久提交后即返回 202；即时 socket backpressure 只推迟投递，同一 `client_cmd_id` 重试返回原序号；
- 超出保留窗口返回 `snapshot_required`，不伪称补回已丢事件；
- 旧 v1 Outbox 只承载元数据，不扩展为正文通道。

## 5. 安全边界

- daemon 主动出站，生产远程链路必须 TLS；
- 本地 IPC 仅当前 OS 用户可访问；
- 工作区使用 daemon 生成的稳定 ID，远端只能选择授权列表，不能提交任意路径或 shell；
- Provider Token 留在官方 Runtime；浏览器、Android 与 Gateway 不持有；
- managed Remote 内容同步前明确告知用户，历史全文导入需要显式选择；
- 设备撤销、停止 Remote 或轮换授权后，旧连接和链接立即失效；
- 诊断日志只记错误码、ID、水位和状态，不记录正文、工具参数、命令输出或环境变量。

## 6. 从 ZCode Remote 借鉴什么

[ZCode Remote Control](https://zcode.z.ai/cn/docs/remote-control) 公开说明了几项值得采用的产品结构：手机只是控制界面；代码与命令仍在 Desktop 已连接的本地、SSH、WSL 或 Docker 环境执行；手机可在已登记工作区中创建任务，但不能浏览任意目录或新建执行环境；远程链接必须显式停止或轮换。

AgentHub 吸收这些原则，但不复制其产品限制：

- Remote 归属于本机 daemon 和已授权工作区，而不是云端 Runtime；
- 移动端是控制面，执行目标由 Desktop 预先建立；
- 允许远端在既有工作区创建官方 Session；
- 授权入口短期可扫码，长期应绑定账户设备并可撤销；
- 支持 Desktop、Web、Android 多端一致性，而不是限制单手机页面；
- 保持多 Provider 适配，不采用 ZCode 的自研 Agent 核心。

公开资料没有披露 ZCode 的内部传输协议，因此不据此推断其 WebSocket、持久化或进程模型；AgentHub 的可靠性仍以自己的协议和测试为准。

## 7. 官方协议依据

- [OpenAI Codex App Server](https://learn.chatgpt.com/docs/app-server)：富客户端的长期会话、历史、流式事件和审批协议依据。
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code/cli-usage)：CLI 的 print、流式 JSON、resume 与权限参数依据；不自动证明第三方可 attach 任意运行中会话。

## 8. 后续路线

1. 将当前隐藏进程模式升级为可安装、可升级、可诊断的用户级 OS 服务，并提供独立本地停止控制。
2. 让 Web/Android 完整消费与 Desktop 相同的能力报告、审批和离线状态语义。
3. 收敛旧 v1 元数据链路与残留账户/任务字段，再接 Claude 长时 Host。
4. 验证 Provider 支持的安全 attach/所有权转移；无可靠协议时继续只读或显式 resume。
5. 完成 Gateway 多实例、真机、安装升级和生产容量验证后再扩大承诺。

当前 P0 已完成 Codex Host 进程外置、Desktop 本地 IPC、单一 Registry/命令账本/事件日志，以及同一 daemon 的可选 Gateway Remote。阶段状态与证据见 [文档索引](README.md)，本设计不等同于安装包或生产完成报告。
