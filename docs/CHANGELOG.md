# Changelog

## [2.4.18] - 2026-09-19

- **全端发版 v2.4.18（Android versionCode = 31 / versionName = 2.4.18）**：
  - **对标 ChatGPT Remote 架构：彻底重构 Antigravity 与 Grok 的真实全双工远程接管流**：
    - 根除前端静态伪配置假象，将 Google Antigravity 与 xAI Grok Build 接入后端统一 Remote 协议总线 `MultiProviderTransport`；
    - 全面打通底层 `thread/read`、`thread/resume`、`turn/start`、`turn/interrupt` 生命周期调用，实现与官方客户端一致的后台免抢占接管；
    - 支持会话本地自动扫描（`collect_antigravity_sessions_for_remote` / `collect_grok_sessions_for_remote`）、真实流式增量提取（`item/agentMessage/delta`）与打字机回传；
    - 桌面端解除白名单限制，无缝接驳真实会话收发与新建驱动；插件商城升级为真实 Remote 运行时生命周期开关。
  - **全平台打包交付与生产部署**：Windows Desktop NSIS/MSI、Android APK 归档至 `Deliverables/AgentHub/`，按铁律清理旧版产物，生产容器部署升级。

## [2.4.17] - 2026-09-18

- **全端发版 v2.4.17（Android versionCode = 30 / versionName = 2.4.17）**：
  - **设置面板新增“插件商城” (Plugin Marketplace)**：
    - 将持续孵化与开发的实验性 Agent 驱动（Google Antigravity 与 xAI Grok Build）收敛至设置中心“插件商城”统一管理，默认不在侧边栏直接裸露，保持主视图极简克制。
    - 插件商城支持高保真生态卡片：展示插件名称、分类（实验室插件/第三方集成）、版本标签、模型提示、深度描述与一键“获取/启用/停用”状态切换。
    - 本地持久化与即时联动：用户启用或停用插件后，侧边栏受控 Agent 胶囊选择器与新建会话模态框动态同步更新，配置持久化于本地 LocalStorage。
  - **全平台打包交付与生产部署**：Windows Desktop NSIS/MSI、Android APK 归档至 `Deliverables/AgentHub/`，按铁律清理旧版产物，生产容器部署升级。

## [2.4.16] - 2026-09-18

- **全端发版 v2.4.16（Android versionCode = 29 / versionName = 2.4.16）**：
  - **根治 ZCode 历史会话恢复报错“本机会话不可用”缺陷**：
    - 根因定位：ZCode app-server 在无头模式由 daemon 启动时，其内存工作区模型目录为空；当继续使用自定义模型（如 `gemini-3.8-flash-high`）的历史会话时，ZCode `session/send` 因模型未注册拒绝执行（RPC -32031 `ZCODE_RUNTIME_MODEL_UNAVAILABLE`）。
    - 自动同步注册表：解析并读取 `~/.zcode/v2/config.json` 中的 provider/model 配置，在 ZCodeAppServer 初始化与会话恢复（`session/resume` 及 `turn/start`）时自动调用 `workspace/updateProviderRegistry`，注入符合 ZCode Zod Schema 的模型提供商注册表。
  - **Desktop 桌面端错误暴露与精确诊断**：
    - 增加 `DesktopError::Detailed(String)`，彻底消除在 `launch_codex_session_with_client` 和 `send_codex_message_with_client` 中对后端错误码的暴力吞咽（原直接报“本机会话已不可用，请刷新后重试”）。
    - 针对 `invalid_model`、`external_session_not_resumable`、`workspace_not_authorized`、`resume_workspace_mismatch` 等错误码提供精确、友好的中文诊断提示。
  - **全平台打包交付与生产部署**：Windows Desktop NSIS/MSI、Android APK 归档至 `Deliverables/AgentHub/`，按铁律清理旧版产物，生产容器部署升级。

## [2.4.15] - 2026-09-17

- **全端发版 v2.4.15（Android versionCode = 28 / versionName = 2.4.15）**：
  - **Desktop 三套主题高对比度与语义化重构**：全面采用语义配色变量（37 tokens × 3 主题），彻底根除侧边栏徽章、表格 td/th、代码块头部在各主题下的对比度污染与浅色残留；移除 Markdown 双重 `pre` 边框与嵌套 DOM。
  - **外观偏好与服务端解耦持久化**：新增 `appearance.ts` 独立管理本地外观偏好，修复主题选择后回落 Dark 的状态不同步问题；加载过程明确区分加载中、失败重试与空态，消除白屏。
  - **会话转录读取与合并算法升级**：多候选分片收集并按真实时间戳倒序合并，保证完整行边界截取；消息优先级预算机制确保首条用户意图与最新回复完整留存，解决有头没尾/有尾没头；过滤 Claude Code 内部 Skill/命令提示泄漏。
  - **桌面本地守护稳定性加固**：复用只读守护客户端，将原本“无法连接 AgentHub 服务器”误报修正为精确的本地 IPC 错误；白名单错误码诊断与有界环形内存，工作区路径标准化。
  - **全平台打包交付与生产部署**：Windows Desktop NSIS/MSI、Android APK 归档至 `Deliverables/AgentHub/`，按铁律清理旧版产物，生产容器无缝升级。

## [2.4.1] - 2026-09-15

- **ZCode Agent 全双工远程控制与实时打字机流式输出上线**：
  - **ZCodeAppServer 原生协议宿主驱动**（`crates/runner/src/collectors.rs`）：通过长生命周期进程直驱 `zcode.cjs app-server`，完成与 ZCode CLI 间的标准 JSON-RPC 交互；实现自动化双向握手应答（`session/requestRuntimePreferences` 与 `interaction/requestOfficialMcpAuthHeaders`），闭环管理 `session/resume`、`session/subscribe`（`desktop-continuous` 流式订阅）、`session/send` 与 `session/stop`。
  - **多 Provider 路由传输层 `MultiProviderTransport`**（`crates/runner/src/remote_host.rs`）：基于会话 ID 前缀精准分流，`sess_*` 路由至 ZCode，`thr-*` 继续交由 Codex 处理；打通 `thread/read`、`thread/resume`、`turn/start`、`turn/interrupt` 及工作区路径解析。
  - **事件流与实时打字机转译归一化**：将 ZCode 的 `model.streaming`（`text_delta`）实时转译为 Gateway 与 Web/移动端可消费的 `item/agentMessage/delta` 事件流，实现与 ChatGPT / Codex 一致的丝滑打字机体验；同步转译 `tool.updated` 工具执行状态与回合终态。
  - **全量会话发现与工作区映射**：自动扫描本机会话数据库并在接管流程中放宽工作区授权判定，确保跨目录历史任务即点即用。
- **全端版本自增至 v2.4.1**（Android `versionCode = 14`），完成 Windows Desktop NSIS/MSI 与 Android Debug APK 构建归档；交付物保存在 `Deliverables/AgentHub/`，并按铁律清理旧版本产物。

## [2.3.6] - 2026-09-14

- **双端 UI 深度重构与全面收敛 Remote-only 控制台**：
  - **Web 端**：根路径 `/` 彻底废除旧版监控大屏（`LegacyPanelPage`，包含图表、团队与计费设置），默认直接渲染现代深色 Remote Console；接入顶部/底部 Runner 实时在线状态指示灯、账户条与一键注销；保留电脑绑定弹窗入口（`DeviceConnectDialog`），移除未打通的 SaaS 团队与计费死链接。
  - **Desktop 桌面端**：彻底移除仿 Claude Desktop 本地工作台首页（`claude-prompt-dock`、大输入框、选择工作目录发起任务），纠正“与原生 CLI 竞争”的心智误导；新增全新 `SessionEmptyState` 引导组件，突出“工作区 -> 会话接管 -> 状态判定 -> 继续/审批/中断”，明确守护进程 `agenthubd` 的持续接管定位；新增常驻顶部 `DesktopHeader` 状态栏（常驻暴露本机 `agenthubd` 状态指示灯、已授权工作区数、设置与新建会话按钮）；侧边栏清理：彻底移除点击不可用的假 Provider（Antigravity、Grok Build），严格收敛展示真实受控 Provider（Codex Managed），消除伪兼容与死路。
- **全端版本自增至 v2.3.6**（Android `versionCode = 9`），完成 Windows Desktop NSIS/MSI 与 Android Debug APK 构建归档；交付物保存在 `Deliverables/AgentHub/`，并按铁律清理旧版本产物。
- **验证与门禁**：
  - Web 端 Next.js 生产构建通过（`npm --workspace=@agent-pulse/web run build`，7/7 页面全绿）。
  - Desktop 桌面端 Vite 生产构建通过（`npm --workspace=@agent-pulse/desktop run build`，1917 模块无 TS 错误）。
  - Desktop 单元测试全绿（`vitest run`）。
  - Rust workspace 测试与 Clippy 全绿通过。

## 2026-09-13 — 双端 UI 深度重构：收敛 Remote-only 控制台与清除废弃工作台

针对多次演进导致 UI 割裂、工作台与远程接管冲突的问题，全面重构 Web 端与 Desktop 桌面端：

- **Web 端收拢为统一 Remote Console**：
  - 根路径 `/` 彻底废除旧版监控大屏（`LegacyPanelPage`，包含图表、团队与计费设置），默认直接渲染现代深色 Remote Console。
  - 接入顶部/底部 Runner 实时在线状态指示灯、账户条与一键注销；保留电脑绑定弹窗入口（`DeviceConnectDialog`），移除未打通的 SaaS 团队与计费死链接。
  - 统一深色高质感样式（`remote.css` 扩展加载中全屏、状态指示灯、卡片阴影与响应式排版）。
- **Desktop 桌面端纠偏与模块化**：
  - 彻底移除仿 Claude Desktop 本地工作台首页（`claude-prompt-dock`、大输入框、选择工作目录发起任务），纠正“与原生 CLI 竞争”的心智误导。
  - 新增全新 `SessionEmptyState` 引导组件，突出“工作区 -> 会话接管 -> 状态判定 -> 继续/审批/中断”，明确守护进程 `agenthubd` 的持续接管定位。
  - 新增常驻顶部 `DesktopHeader` 状态栏：常驻暴露本机 `agenthubd` 状态指示灯（loading/starting/stopped/local/connecting/connected）、已授权工作区数、设置与新建会话按钮。
  - 侧边栏 Provider 真实化：彻底移除点击不可用的假 Provider（Antigravity、Grok Build），严格收敛展示真实受控 Provider（Codex Managed），消除伪兼容与死路。
- **验证与门禁**：
  - Web 端 Next.js 生产构建通过（`npm --workspace=@agent-pulse/web run build`，7/7 页面全绿）。
  - Desktop 桌面端 Vite 生产构建通过（`npm --workspace=@agent-pulse/desktop run build`，1917 模块无 TS 错误）。
  - Desktop 单元测试全绿（`vitest run`，4 passed，16 passed）。

## 2026-09-12 — Remote-only 定位收口：会话读取收敛为 daemon 单点

产品定位收口为"官方 Agent 会话的统一远程继续入口（Remote-only）"：不做通用本地工作台，读取会话历史只服务于"判断并执行继续"（对齐 ChatGPT Remote 的形态：读取是视图，不是数据集）。

- **transcript 解析收敛单点**：新增 `crates/runner/src/conversation.rs`，Desktop 原先在 Tauri 层直读并解析四家 Provider transcript 文件的约 500 行逻辑（`load_session_conversation`、`find_agent_transcript`、`normalize_local_conversation_items` 及全部文本解码/规范化辅助函数与测试）整体迁入 runner；Desktop 的 `load_session_conversation` 命令改为经 daemon 本地 IPC 代理，前端不再触碰 Provider 文件，消除 runner collectors 与 Tauri 双处解析的格式耦合。
- **新增本地 IPC 方法**：`DaemonRequest::GetSessionConversation { agent_type, session_id }` / `DaemonResponse::SessionConversation(Vec<Value>)`；该请求不依赖 Host channel 就绪，daemon 启动期即可用。响应增加有界预算（最多 300 条且约 512 KiB 内容），确保不超过 IPC 单帧 1 MiB 上限（`write_frame` 对超限帧会直接 `ipc_frame_too_large`）。
- **只读浏览降级为恢复候选**：Desktop 与会话面板对非托管能力 Provider（Claude Code、Antigravity、Grok Build）不再加载或渲染全文 transcript，仅展示发现元数据（标题、模型、时间），并提示"尚未接入远程控制"；transcript 预取（prefetch）同样只针对 Codex。文案同步更新（前端空态/只读提示与 Tauri 写入口守卫错误消息）。
- **文档对齐**：README、PRD、AGENTS.md、docs/README、ARCHITECTURE、Desktop/Runner README 的定位表述从"local-first 多 Agent 工作台 + 原生 Remote"收口为 Remote-only；PRD 明确"不做通用本地工作台、跨会话搜索、历史分析等与远程继续无关的读取功能"；PRD 删除 Desktop "终端与本地设置"工作台承诺（从未实现嵌入式终端）。
- **清理工作台时代残留**：删除 Tauri 层 `local_memory_summary` 及快照中的 `local.memory` 字段（统计 Claude 记忆文件/Codex 记忆库/Grok 会话的字节数，属于清点式读取，与远程继续无关；Web/Android/Gateway 无消费），设置弹窗"本地存储与索引状态"卡同步收敛为"发现状态"（仅保留已发现会话数）。保留 `find_codex_session_cwd`（恢复流程的工作区预取，元数据级、服务继续语义）。
- 附件读取路径复用 runner 导出的 `decode_local_text`（UTF-8/UTF-16 BOM 嗅探解码），不重复实现。

自动化验证：`cargo test --workspace` 全绿（183 passed，3 个需真实 Codex 的用例按默认配置 ignored，含新增 IPC 帧预算/解析/校验测试）；`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check` 通过；`npm.cmd run typecheck` 通过。本批不递增版本、不打包、不部署；Web/Android 无行为变化（远程回放走 Gateway 事件投影，不读 Provider 文件）。

## [2.3.5] - 2026-09-12

- Web/Android Remote 创建状态与协议边界对齐：保留客户端 `hub_session_id` 作为 correlation key，但将 HTTP 202/queued 明确显示为等待绑定；只有真实 binding 或明确 receipt 终态才收敛为对应状态。
- Web/Android 统一将 `ws-*` workspace 作为 opaque ID 展示，不再把协议字段当作本机路径 basename；文档明确记录前台轮询、无原生推送和无离线命令队列的当前边界。
- **全端版本自增至 v2.3.5**（Android `versionCode = 8`），完成 Windows Desktop NSIS/MSI 与 Android Debug APK 构建归档；交付物保存在 `Deliverables/AgentHub/`，并清理旧同平台产物。
- 交付物 SHA256：Android `b019964680d13a6ee8b00e63e4e1ff9165a3cc6b148d33756cfdddf5fbeeb26e`；Windows NSIS `fb6cb38387a2bccfabecee1a8e892ca247dcd53c8a5a38a787a4fbfa631f0ef1`；Windows MSI `456ff275b21fae3efc96ce348cfb8d08c355cc45f7afe6675edba042a08f370a`。
- 生产 `platform.tannerlab.xyz` 已切换至提交 `606189f`：API/Web 容器内版本均为 `2.3.5`；API 本机 `/health` 返回 200，Web 本机与公网首页返回 200。公网 `/health` 仍为 404，因公网入口只暴露 Web，API health 仅绑定服务器本机。
- 生产保留唯一紧邻回滚目录 `/opt/1panel/apps/agent-pulse.backup-pre-v2.3.5-20260912T085122Z` 及上一版本镜像回滚标签；删除更早的 v2.3.4 目录和临时归档。

## [2.3.4] - 2026-09-10

- 文档与客户端语义对齐：Web/Android 当前明确支持 v3 Remote 的创建、恢复、续聊、事件/收据、审批和中断；创建请求仍遵循 queued → accepted → session-bound，前台轮询不等同于原生推送或离线命令队列。Web/Android 将 `ws-*` workspace ID 作为 opaque 标识展示，避免误作本机路径。
- **全端版本自增至 v2.3.4**（Android `versionCode = 7`），完成 Windows Desktop 安装包（NSIS / MSI）与 Android Debug APK 构建归档，按铁律清理旧版本产物。
- 交付物保存在 `Deliverables/AgentHub/`：
  - `AgentHub-v2.3.4-android-debug.apk` (SHA256 `7c55c0c7488716e6dd1bcd1c706a196673ffd994850afb288760505de0e8492d`)
  - `AgentHub-v2.3.4-windows-x64-setup.exe` (SHA256 `f969b3cd47f28cef6f54b10dc2d3d0ba2cd39712e6b1aad4772254aa173fca3b`)
  - `AgentHub-v2.3.4-x64.msi` (SHA256 `0096995a0dc36c6b6e5f01dd7f1d96402d9ba80c083b095510b3dabf14d6baf9`)
  - `AgentHub-v2.3.4-SHA256.txt`
- 将 `NativeSessionHost` 提升为进程外 daemon：`agent-pulse-runner daemon`、兼容 `remote-run` 与 Desktop 隐藏 `--agenthubd` 模式进入同一主流程，共用一个 `DurableHost`、Session Registry、命令账本和事件日志。
- 新增当前用户 IPC v2：Windows named pipe 使用当前用户 ACL 并拒绝远程客户端，Unix domain socket 权限为 `0600`；采用 1 MiB 上限的长度前缀 JSON frame、协议版本、`request_id` 与 `snapshot + cursor` 重放，live pending approvals 是 v2 必需快照字段。
- 本地事件日志与 Gateway outbox 分离，Gateway ACK 不删除 Desktop replay；Desktop-origin 命令收据只写本地日志，Remote 侧改用合法 ready/Host event 同步状态，避免 `unknown_command_receipt` 永久阻塞序列。Remote 关闭期间继续持久化 bounded local log、Session Registry 和命令账本但不积累未来可上传内容，重开仅追加当前脱敏 ready 快照；cursor 过期显式返回 `snapshot_required`。
- Desktop 移除内嵌 Host 所有权、逐回合 CLI 写路径和独立 Remote Console；Codex 创建、恢复、续聊、审批、中断和归档统一走 daemon，Remote 开关只控制 Gateway 暴露。启动中状态由自动启动、Session 和设置入口统一等待且不重复拉起；Remote 启用、停用、logout 与 profile 提交由同一锁串行化，Dialog 在首次成功轮询时恢复 profile 并清除瞬时错误；退出 Desktop、退出登录和关闭 Remote 不停止本地 Runtime。
- Gateway 协议、水位或畸形 frame 只 fence 当前 transport epoch，不终止共享 daemon；Remote inbound sequence、完整 command payload 与 intent 原子绑定，receipt 和 received watermark 原子完成，崩溃窗口持久收敛为 `indeterminate`。新 Host generation 替换旧 socket 且旧 close 回调不能注销当前连接；本代际 `ready` 作为 Session 全量集合，遗漏绑定降级为 detached/unknown，旧代际事件不覆盖当前投影。命令完成 Gateway 持久提交后即返回 202，即使即时 socket backpressure 阻止发送，同一 `client_cmd_id` 重试仍返回原序号。
- Pending approval 使用不含 Provider request ID 的 opaque handle 并进入 live daemon snapshot；本地日志保留 Provider 原始事实，Remote outbox 在同一事务中只写安全描述符投影，Gateway 拒绝含 Provider 顶层 request ID/raw 的审批事件。所有 Remote event raw 和 receipt 进一步收敛为按种类 allowlist：消息仅保留 item/text、turn 仅保留 ID、工具仅保留 item/type 完成状态，完整 `provider_response`、命令、工具输入输出、敏感字段和嵌入式物理路径只留本机；Remote binding 与 `connect` 只接受 daemon 签发的 `ws-*` workspace ID。Web/Android 以 `ready.pending_approvals` 作为 runner 权威全量集合并支持空快照清除，accepted receipt/opaque resolved event 只按 handle 收敛，工具结果统一显示完成状态。Codex 最终 `item/completed` agent message 覆盖暂态 delta，`turn.output` 只作缺失 final item 的兼容回退。
- Claude Code、Antigravity 与 Grok Build 保留发现和历史读取，managed 写入口明确禁用；daemon 重启后的 Codex binding 先标记 detached，再通过 `thread/read` 验证并显式 `thread/resume`，不抢占活跃外部 writer。
- Windows Codex npm shim 仅按官方 `@openai/codex` 已知包布局解析原生 `codex.exe`，不执行 shell；journal 首次绑定 Gateway 后禁止切换到另一服务，避免历史 outbox 跨域发送。
- 自动化验证：`cargo test --workspace` 全绿（180 passed，3 个需要真实 Codex 的用例按默认配置 ignored）；`cargo clippy --workspace --all-targets -- -D warnings`、Rust/Prettier 格式、全工作区 TypeScript 类型检查和 140 项 Node 测试全绿（其中 134 passed，另有 6 个需 PostgreSQL/真实 Codex 的环境型 skipped）；Android `testDebugUnitTest` 与 `lintDebug`、Desktop 与 Web 生产构建通过，Gateway API 无独立 build script 且以 `tsc --noEmit` 通过。此前已完成真实 Desktop hidden daemon 启停与 IPC 测试，以及真实三回合 daemon E2E（同一原生 Session 两轮、daemon 重启、detached 恢复及第三轮 resume，19.24 秒）；本轮未重复执行这 3 个需 `AGENTHUB_TEST_CODEX_BINARY` 的 ignored 用例。`starting` 状态此前已在 1280、360 与 320 px 视口完成真实浏览器验收，状态文案可见、业务控件禁用且无重叠、裁切或页面溢出。
- 本批不递增版本，不打包、不部署，也不生成安装产物；独立命名二进制与用户级 OS 服务仍属后续范围。

### 文档治理（2026-09-09）

- 精简 README 与项目规则，统一产品定位、仓库边界、最短开发/验证命令和权威文档导航；当前实现与目标差距只在 `docs/README.md` 维护。
- 更新文档索引，明确当前实现与目标架构的差距；将 Runner 与 Gateway API 的 299/250 行混合手册收敛为角色、最短运行/验证、安全边界和权威导航入口。
- 本次仅调整 Markdown，未重跑语言构建、桌面端、Android 或生产验证。

### Codex NativeSessionHost 单一本机会话核心第六轮深度对齐与架构收敛（2026-09-09）

- **解决 P1 统一 Host 数据库作用域，根除数据库与 Session Registry 分裂**：
  - 彻底重构 Desktop 与 Runner 的持久化映射：NativeSessionHost 数据库与配置文件统一收敛为按本机设备唯一作用域（`host_database_path` / `agenthub_native_host_v1 + device_id`），不再随 Gateway URL 分裂出两套不同的 SQLite 库；
  - 修正 `RemoteJournal::bind_identity`：离线打开本机会话库时保持既有注册表与命令账本，动态 `EnableRemote` 或带有 Gateway URL 启动时自动安全同步 server 绑定，退出登录或重启应用后无论以离线还是在线模式打开，均指向同一 SQLite 文件且绝不发生 `IdentityMismatch`，实现会话注册表的真正唯一性；
  - 新增集成单测 `offline_host_enables_remote_and_preserves_registry_across_restart`，全链路断言离线建会话、动态联网、重启回放与多设备防篡改隔离。
- **解决 P1 诚实标定后台 Daemon 架构边界，避免过度声称**：
  - 客观明确当前 Desktop 与 NativeSessionHost 处于跨线程本地 Channel（`mpsc` + `LocalHostClient`）的稳定过渡期，Desktop 退出仍负责管理 Host 生命周期；
  - 独立后台守护进程 `agenthubd`（Named Pipe / Unix Domain Socket IPC）保留为后续独立里程碑，严禁虚假声称已经实现后台独立 daemon。
- **解决 P2 turn ID 校验杜绝 fail-open 风险**：
  - 在 `launch_codex_session_with_client` 与 `send_codex_message_with_client` 中，要求 `SendPrompt` 回执中必须包含非空 `turn.id`，缺失立即作为协议错误返回 `DesktopError::AgentFailed`；
  - 事件接收循环中，严格要求 `event_turn_id == Some(expected_turn_id)`，缺失 turn ID 或不匹配一律忽略，彻底杜绝 fail-open。
- **解决 P2 路径脱敏升级为远程 DTO 白名单与物理路径特征强过滤**：
  - 告别脆弱的字段名黑名单枚举：引入 `is_absolute_filesystem_path` 物理路径结构特征识别（涵盖 Windows 盘符、UNC 路径及 Unix 绝对路径）；
  - 递归遍历任何流向远端 Outbox 的 JSON 载荷（包括 `event.raw` 中的工具参数、命令行参数、文件变动列表），一旦包含物理绝对路径特征一律清洗转换为安全的不可逆 `workspace_id`，杜绝任何工具事件物理路径外泄；新增专用单测验证工具审批事件与深层数组路径清洗。
- **解决 P2 修正 E2E 代理环境变量，杜绝硬编码本机端口**：
  - 移除 `remote-native.test.ts` 中硬编码的 `127.0.0.1:7897` 端口，严格仅继承调用者环境（`process.env`）或可选的专用测试变量 `AGENTHUB_TEST_PROXY`，避免其他机器运行测试时被强制导向不存在的本地代理。
- **全套质量门禁实测通过**：
  - `cargo fmt --all --check` 严格通过；
  - `cargo clippy --workspace --all-targets -- -D warnings` 零告警通过；
  - `cargo test --workspace` 实际为 **135 passed, 1 ignored**；
  - 真实 Codex 启停单测 `desktop_owner_can_stop_native_host_without_a_process_signal` 顺利通过；
  - 全套 Node/TS 测试（118 passed，6 skipped）与 `npm run typecheck`、`npm run format:check` 100% 全绿；
  - 真实两轮 Codex Remote + Gateway 重启与磁盘回放 E2E 测试 `remote-native.test.ts` 约 14.2 秒顺利通过。

### Codex NativeSessionHost 单一本机会话核心第五轮深度收敛与链路闭环（2026-09-09）

- **解决 P1 已有 Host 开启 Remote 真正启用 Gateway network_loop**：
  - 在 `LocalHostCommand` 与 `LocalHostClient` 中新增 `EnableRemote` 控制命令；
  - 当 Desktop 工作台已离线启动本地 Host 后，用户点击开启 Remote 时，`start_remote_host` 直接向运行中的 Host 发送 `enable_remote`，将底层 SQLite WAL Journal 绑定至 Gateway，并动态 spawn 启动 Gateway `network_loop`，彻底解决 connected 始终为 false 的假复用问题。
- **解决 P1 动态授权错误与配置持久化失败严格拦截**：
  - 重构 `start_remote_host`：严格限制工作区数量上限为 32 个，若新增工作区超过 32 个或 `client.authorize_workspace` 抛出错误立即阻断并返回拒绝；
  - `fs::write(&profile_path, ...)` 结果严格向上传播错误，杜绝静默吞掉异常。
- **解决 P1 递归路径脱敏全字段覆盖（覆盖 cwd、workdir、raw 等）**：
  - 扩展 `privacy.rs` 中的 `sanitize_workspace_paths_in_json`：除了 `workspace_path`，全面覆盖 `cwd`、`workdir`、`working_dir`、`working_directory`、`project_path`、`root_path` 等字段；
  - 无论是原生 `provider_response.thread.cwd` 还是事件流中 `raw.params.cwd`，均被递归脱敏替换为安全的 `workspace_id`，物理路径绝不出本机；补充覆盖完整对象结构与嵌套数组的针对性单测。
- **解决 P1 Remote 协议禁止直接提交绝对物理路径与 E2E 真实验证**：
  - 重构 `remote_host.rs` 中 `Connect` 命令处理逻辑：彻底移除直接对物理路径执行 `canonicalize()` 的后门，严格仅接受已授权的 `workspace_id`（`ws-...`）并通过反查映射获取真实物理路径；直接提交物理绝对路径一律返回 `workspace_not_authorized`；
  - 增加单测 `raw_filesystem_path_is_rejected_as_unauthorized_workspace` 显式验证物理绝对路径被严格拦截；
  - 在 `services/api/test/remote-native.test.ts` 中，真实通过 `ready` 包提取 `workspace_id`（`ws-...`）提交 `connect` 命令，完成端到端隐私链路验证。
- **解决 P1 Desktop 回合事件流增加 turn ID 校验**：
  - 在 `launch_codex_session_with_client` 与 `send_codex_message_with_client` 中记录命令返回的 `expected_turn_id`；
  - 严格比对 `turn/completed` 与 `item/agentMessage/delta` 事件中的 `turn_id`，若不匹配当前回合则安全丢弃延迟到达的旧回合残余事件，杜绝错误截断或结束当前新回合。
- **解决 P1 取消收据 accepted 语义对齐与消除双重中断竞争**：
  - 修正 `cancel_session_turn`：中断指令被 Host 接受时返回 `json!({ "accepted": true, "receipt": receipt })`，不再虚假声称 `cancelled: true`；
  - 移除工作线程内部重复构造并再次发送 `RemoteAction::Interrupt` 的冗余调用，避免同一会话发生双重中断竞争；工作线程仅监听 Provider 回传的真实 `status == "interrupted"` 终态信号后返回 `DesktopError::AgentCancelled`。
- **全套质量门禁 100% 通过**：
  - `cargo fmt --all --check` 严格格式化通过；
  - `cargo clippy --workspace --all-targets -- -D warnings` 零告警通过；
  - `cargo test --workspace` 实际为 134 passed，1 ignored；
  - 真实 Codex 启停单测 `desktop_owner_can_stop_native_host_without_a_process_signal` 100% passed；
  - 全套 Node/TS 测试（118 passed，6 skipped）与 `npm run typecheck`、`npm run format:check` 100% 全绿；
  - 真实两轮 Codex Remote + Gateway 重启与磁盘回放 E2E 测试 `remote-native.test.ts` 约 14.8 秒顺利通过。

### Codex NativeSessionHost 单一本机会话核心第四轮深度对齐与闭环整改（2026-09-08）

- **解决 turn/completed 严格判定 status 状态**：
  - `launch_codex_session_with_client` 与 `send_codex_message_with_client` 增加对 `params.turn.status` 的显式校验；
  - 遇到 `status == "interrupted"` 时映射为 `DesktopError::AgentCancelled`，遇到非 `completed`（如 `failed`、`unknown`）时拒绝返回成功与 `completed_at`，严格返回 `DesktopError::AgentFailed`。
- **解决不可逆 workspace_id 映射与 Remote 寻址闭环**：
  - 引入稳定不可逆的 `workspace_id_for_path` 双向安全映射机制（`ws-{safe_display}-{hash16}`）；
  - `RemoteAction::Connect` 既支持本机物理路径，也支持解析 `workspace_id` 反向安全寻址已授权工作区，使 Remote 客户端从 `ready` 包获取工作区标识后原样发起会话可逆寻址成功，杜绝 `workspace_unavailable` / `workspace_not_authorized`。
- **解决路径泄漏闭环（Receipt & Host Events）**：
  - 在 `remote_network.rs` 的 Outbox 数据发送守卫中引入 `sanitize_workspace_paths_in_json` 递归脱敏；
  - 对进入 Outbox 管道的所有数据包（包括 `ready`、`command receipt`、`Host events`）中的 `workspace_path` 进行全面脱敏过滤，彻底杜绝任何原生物理路径泄漏到 Gateway 与远端持久化账本。
- **解决 Remote 启动与 Host 动态授权共存**：
  - 重构 `start_remote_host`：发现健康运行中的 Host 时，直接动态调用 `client.authorize_workspace` 增量授权并追加 profile，不再阻断用户或强制要求“先停止运行中的 Host”，实现本地工作台与 Remote 共存复用同一 NativeSessionHost。
- **解决 ready 等待超时与线程 Join 资源竞争治理**：
  - 将启动等待超时由 5 秒提升至 20 秒，严格对齐底层交互式启动协议超时；
  - 出现超时或退出失败时显式调用 `worker.join()` 回收旧线程，避免 SQLite 锁与通道竞态。
- **解决 Presence 与执行状态完全解耦**：
  - 在 `merge_dashboard_sessions` 中，`presence` 严格由 `control_connected` 布尔值决定（`control_connected == true` 时为 `LIVE`，否则为 `PERSISTED`），彻底消除执行状态（如 completed/error）对连接在席状态的混淆干扰。
- **修复真实 Codex Remote + Gateway 离线回放测试并 100% 验证通过**：
  - 针对原生 Codex 集成测试 `services/api/test/remote-native.test.ts` 进行端到端诊断排查：排查出本地代理环境变量与默认模型权限冲突（`gpt-5.6-sol`/`gpt-5.3-codex` 未授权 400 报错），配置修正并在代理环境下完成 2 轮真实 Codex Remote 会话交互、Gateway 重启与磁盘回放全流程验证，测试用例 100% 绿色通过。
- **全套质量门禁全绿**：
  - `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 零报错；
  - `cargo test --workspace` 131 passed；
  - 全套 Node/TS 测试（`@agent-pulse/contracts` 12 passed、`@agent-pulse/api` 52 passed 6 skipped、`@agent-pulse/desktop` 11 passed、`@agent-pulse/web` 43 passed）与 `npm run typecheck`、`npm run format:check` 100% 全绿。

### Codex NativeSessionHost 单一本机会话核心第三轮深度对齐与状态/路径安全收敛（2026-09-08）

- **解决 P1 回合超时或断流不得伪造成功**：
  - 重构 `launch_codex_session_with_client` 与 `send_codex_message_with_client` 事件接收循环：引入明确的 `turn_completed` 终态信号标记；
  - 严禁在未收到 `turn/completed` 或流断开/超时时落入兜底字符串（如“任务已提交 / 回合已完成”）并写入 `completed_at`，杜绝伪造 Provider 成功状态，一律返回真实 `DesktopError::AgentFailed`。
- **解决 P1 ready 屏障严格闭环与超时拦截**：
  - 修正 `remote_desktop.rs`：启动 Host 等待屏障超时（5秒）或进程未就绪时，严格拒绝返回客户端并标记停止与失败，杜绝未 ready 即放行；
  - `get_host_client` 显式增加 `handle.control.ready.load()` 检查，未就绪或已关闭的 Host 绝对返回 `None`；单测中补充断言未 ready 时返回 `None` 的负向验证。
- **解决 P1 显式建模离线本地运行态，消除虚构登录账户**：
  - 离线快照显式输出 `offline: true`，账户信息真实展示为“离线本地模式”与 `local_only`，前端账号菜单与设置对话框识别离线模式展示“本地工作区 · 未连接 Gateway”，按钮适配为“切换连接 / 登录”，不再呈现虚构的已登录账户与退出登录假象。
- **解决 P1 物理路径安全合规，防止完整本机路径上报远端**：
  - 在 Runner 发送 `ready` 事件时，对 `workspaces` 授权目录列表、`sessions` 绑定的工作区目录以及 `discovery` 发现候选的 `workspace_path` 全面执行脱敏（使用 `project_display_name` 提取项目名称与安全相对标识），彻底杜绝本地操作系统物理绝对路径写入 Remote Outbox 与 Gateway 持久化账本。
- **解决 P1 cancel_session_turn 拒绝盲目成功与假取消**：
  - 当针对 Codex 会话取消时，如果未找到运行中的 Host 或目标 SessionBinding，严格返回错误信息，不再默认声称 `{"cancelled": true}`。
- **解决 P2 Session Registry 准确区分 Presence 与执行态**：
  - 彻底清理无条件 `presence: ACTIVE`，对齐官方规范枚举（`LIVE` / `PERSISTED`）：仅当 `control_connected == true` 且处于活跃执行状态（`Running` / `AwaitingPermission` / `AwaitingInput` / `Idle`）时标记为 `LIVE`；已断开控制连接或终止/异常/已完成状态则准确标定为 `PERSISTED`。
- **解决 P2 Codex 模型目录由 Host 权威主导**：
  - 重构 `list_agent_models_inner`：Codex 提供商优先直接从运行中的 Native Host 获取官方动态模型列表，不优先混入历史 transcript 的陈旧模型名称，避免覆盖原生模型能力与真实标签。
- **自动化测试与门禁核验**：
  - 修复 `cargo fmt` 代码格式问题，`cargo fmt --all --check` 与 `npm run format:check` 严格全绿；
  - `cargo clippy --workspace --all-targets -- -D warnings` 零告警通过；
  - `cargo test --workspace` 131 passed，显式运行本机 Codex 的 ignored 集成测试 `desktop_owner_can_stop_native_host_without_a_process_signal` 顺利通过；
  - 全套 Node/TS 测试（118 项通过，6 skipped）与 `npm run typecheck` 全绿通过。

### 产品与文档收敛：local-first 工作台 + Remote（2026-09-08）

- 明确 AgentHub 是 local-first 多 Agent 工作台，并原生提供 Remote；允许从三端在已授权工作区创建官方 Session，不再把产品限制为“发现、接管、暴露已有会话”。
- 明确 Desktop、Web、Android 共用本机 `agenthubd` 的同一 Session 核心：Desktop 走本地 IPC，Web/Android 经 Gateway；Gateway 不运行 Agent，也不成为 Session 真相源。
- 记录当前实现差距与顺序：先统一 Codex Host 和 Desktop 本地 IPC，再收敛 Remote 入口、接入 Claude 长时 Host，最后处理多实例与生产容量。
- 借鉴 ZCode Remote 的“移动端只做控制面、执行环境留在桌面所属机器、授权可撤销”原则；不复制其自研 Agent 和单手机页面限制。
- 删除重复的 `CAPABILITY_AND_LAUNCH_SPEC.md`、`TECHNICAL_FEASIBILITY.md`、`MISC.md`，将有效结论并入 PRD、架构、协议与精简后的 PITFALLS。
- 清理 Runner、Gateway 与共享契约 README 中已删除的远程任务接口和能力声明。

### 核心定位架构重构与冗余功能清理（2026-09-08）

- **坚守核心定位与三不原则**：严格围绕“多种本地 AI Agent 的统一远程控制入口”，仅保留“发现、接管、暴露”核心链路，清理偏离定位的自造任务系统、死代码与伪功能。
- **清理自建任务代执行系统**：
  - Rust Runner 端彻底移除 `tasks.rs`（476行）、命令行 `Command::Tasks`、`run_agent_task` 及 Gateway 的 `claim_task` / `update_task` 接口；
  - Gateway API 端移除 `/v1/tasks`、`/v1/device-tasks/*` 调度路由、活动 Store 与 `remote-tasks.test.ts`；已发布迁移保持不可变，残留权益/审计字段后续单独清理；
  - Web 端移除 `TasksView.tsx` 及任务导航和过滤项。
- **清除已废弃的 v2 内存原型与协议死代码**：
  - 移除 Gateway 中仅供 mock 注入的 `/v2/*` 路由与 `v2-session-manager.ts`、`v2-event-store.ts`；
  - 移除 `packages/contracts` 下已废弃的 `v2-*.ts` 合同及对应测试；
  - 移除 Runner 端孤立死代码 `v2_protocol.rs`。
- **清理自造 Agent 启动逻辑与陈列假设置**：
  - Web 端移除 `LaunchSessionDialog.tsx`、未引用组件 `ToolApprovalModal.tsx`、`SlashCommandMenu.tsx` 及失效 v2 API 方法；
  - 桌面端设置弹窗精简为核心四项（常规刷新、Agent 路径、远程连接与 Host 守护、账户管理），清理无后端的陈列假开关（电脑操控桥、浏览器探针、个性化记忆等）。

### 架构与产品定位纠偏与文档定界（2026-09-08）

- **确立顶层定位与唯一标准**：
  - **核心定位**：`AgentHub = 多种本地 AI Agent 的统一远程控制入口`。
  - **核心功能三步走**：① 发现本机正在运行的 Codex / Claude 等官方 Agent；② 接管现有 Session；③ 安全暴露给手机端（Android）和 Web 端继续使用。
  - **三不原则**：不替代官方 Agent，不自己再造 Agent，不改变用户本地使用习惯。
  - **唯一裁决标准**：“这个功能是不是在帮助用户‘远程继续使用本机已经运行的 Agent’？不是就不做。”
- **全套规范与协议文档对齐**：
  - 更新 `README.md`、`AGENTS.md`、`docs/PRD.md`、`docs/ARCHITECTURE.md`、`docs/SYNC_PROTOCOL.md`、`docs/README.md`，统一顶层拓扑与设计铁律。

### v2.3.3 恢复默认 UI 与 Remote 协同无缝桥接发版（2026-09-08）

- **恢复原有默认 UI 体验**：桌面端默认恢复为精美 Claude Desktop 风格的 Dashboard（侧边栏、Agent 下拉、工作区树、置顶/展开、模型选择、提示词坞、实时索引轮询与会话详情），消除覆盖默认界面的偏离。
- **桥接 Remote 控制链路**：
  - 桌面端顶栏新增 `Remote` 快捷入口（Radio 图标），支持一键在 Dashboard 与 Remote 控制台之间平滑切换；
  - 桌面端系统设置模态框「远程连接」分区内集成「本机 Remote Host 守护」管理入口，无缝唤起 NativeHostDialog；
  - Web 面板默认恢复多端管控总览，左侧导航轨保留显式「远程接管」直达入口；
  - 统一更新返回入口链接，保障控制台与各视图间流畅流转。
- **版本递增与产物打包**：全端统一递增至 `v2.3.3`（Android versionCode=6），完成 Tauri NSIS 安装包打包与 Deliverables 归档，遵照铁律自动清理旧版 `v2.3.2` 安装包。

## 2026-09-07 — 文档精简

- 通用工程与整洁约束统一引用工作区 AGENT.md；项目入口只保留特有边界，合并重复工具说明和逐会话记录。
- 仅文档变更；检查引用、编码和 diff，未打包、未执行设备或生产验证。

> 所有功能变化都记录在 `Unreleased` 下，格式遵循
> [Keep a Changelog](https://keepachangelog.com/)。

### v2.3.2 打包与安装实测（2026-09-08）

- 同步 Node/Tauri/Cargo 版本为 2.3.2，Android versionCode=5；交付 Windows x64 NSIS 与 Android Debug APK。初包暴露登录页布局与旧只读文案，修复后递增版本重新打包。
- Windows 实际安装/覆盖安装与登录页渲染通过；桌面登录后的完整 Host 操作仍待手动认证，不能记为端到端通过。
- Android 模拟器通过实际 APK 创建 Codex 会话、两轮续聊、覆盖安装保留连接、原生审批、中断及 Host 重启后恢复同一原生 ID；真实 release Runner 的 Gateway 重启重放/命令去重测试通过。
- npm 测试、typecheck、format:check 与 Android 单元/lint/assembleDebug 通过；原生嵌入式停止测试通过。详细边界见 M4_VALIDATION.md、ANDROID_VALIDATION.md；未部署生产。

### 桌面本机 Remote 入口（2026-09-08）

- 桌面增加目录选择、原生 Codex 路径、启动/停止与事实连接状态；内嵌 Host 复用 Runner 协议，通过独立取消信号管理生命周期，退出登录停止本机连接。
- 内容库与目录设置按设备/Gateway 隔离；失败退出清除在线状态。浏览器 Tauri fixture 验证目录选择、按钮禁用、启动/停止和无脚本错误；原生嵌入式停止测试与完整桌面安装验收分开记录。

### Native Remote 重构（2026-09-07）

- 替换产品、架构与协议文档；移除模拟 Router/硬编码能力表，默认关闭旧 v2 原型路由。
- 新增长驻 Codex Host、真实会话 ID、原生审批/中断、持久命令账本和设备签名 WSS；Gateway 使用账户隔离的单实例 SQLite 内容库与事件重放。
- Web 默认改为远程会话；桌面复用该 UI 并通过 Tauri 安全登录态访问；Android 新增原生 Remote 页面，旧监控管理保留。
- 已验证原生两轮续聊、Gateway 重启重放、网页创建/刷新读取与移动布局；桌面打包、Android 设备、Claude 长时控制及生产验收不由这些测试替代。阶段证据统一见 docs/README.md。
- 按现有格式检查整理了客户端既有格式差异，无额外功能变化。

### Changed

- 2026-09-07：按用户确认的原生 Remote 接续目标替换 PRD、架构、同步、能力与可行性规范；明确 discover/resume/attach、原生事实来源、内容同步及凭据边界；修正旧记录中未经验证的完成声明，建立 R0–R4 迁移状态。

## [2.2.0] - 2026-09-02

### Added

- **模型列表从本机 Agent 拉取（去硬编码）**：新增 `list_agent_models` 命令，聚合各 CLI 在本机会话中实测上报的模型（按使用频次排序，标注"本机 Agent 实测"），内置目录仅作为全新安装的兜底；首页 Dock、新建会话与会话面板三处下拉全部接入，未知 id 自动美化显示。
- **会话内模型 / 权限 / 上下文 / 附件（Codex 式 composer 工具行）**：输入框底部新增工具行——模型下拉、权限模式下拉（Claude: 默认/接受编辑/计划/完全访问；Codex: 只读/工作区写入/完全访问，后端映射为 `--permission-mode` / `--sandbox`）、上下文占用百分比、附件按钮；附件经 `pick_files` 选择后由后端读取文本内联进指令（单文件 128K 字符、总量 400K、8MB/二进制校验），失败时自动恢复附件便于重试。
- **会话正文预加载缓存**：新增 `conversationCache`（LRU 60 会话），启动后对置顶 + 最近 10 个会话在 idle 时顺序预热；点击会话先直出缓存，再后台核对 transcript，内容一致不重渲染，不一致才刷新。
- **真实新建会话**：`launch_new_session` 重写为 `run_agent_session_start_streaming`——在所选目录以 `claude -p` / `codex exec` 真正起会话，从流式输出解析真实 session id（此前为假 id 且 cwd 被忽略、Claude/Codex 新会话实际必然失败）；支持携带模型与权限。
- **原生文件夹/文件选择**：新增 `pick_folder` / `pick_files` 命令（rfd），工作目录从手输文本框改为一键系统文件夹选择器（Dock 与新建会话均生效），并记忆最近 5 个工作目录。

### Changed

- **实时通道（SSE）断联治理**：重连退避按故障分类——健康长连接正常结束 1s 重连、服务端 429（流槽位占满）退避 60s、其余指数退避上限 30s 并加抖动；响应头超时放宽到 30s；前端增加 20s 宽限期，单次闪断不再立即把横幅和"轮询连接"打出来。
- **断联横幅浮层化**：系统通知改为 Stage 顶部悬浮 pill（毛玻璃），不再插入文档流——问候语与输入舱不再被顶下/跌落。
- **会话面板滚动修复（#7 根因）**：`.session-control-panel-scrim` 此前完全没有定位与高度，`height:100%` 全部塌陷导致对话区无法滚动；现钉满 Stage 并建立 header/tabs/pane/composer 严格 flex 契约，对话区为唯一滚动容器（`overscroll-behavior: contain`），时间线/运行上下文同样可滚；滚动定位改用 `pane.scrollTop` 替代会连带祖先的 `scrollIntoView`（同时消除首页"到底还能下拉"的滚动链）。
- **工具调用识别与收纳**：历史 transcript 中的 tool_use 不再误标"运行中"（持久记录默认"完成"，`is_error` 才"失败"，Codex rollout 的未完成标记视为已完成）；tool_use 与后续 tool_result 按 id 合并为单条（参数 + 〔结果〕分区），标签升级为 `名称: 关键参数` 摘要（Bash 命令/文件路径等）；行高压缩、默认折叠、失败红色描边，结果区按 Markdown 渲染。
- **Markdown 全面排版**：`.markdown-body` 补齐标题/列表/任务列表/表格/引用/分隔线/行内代码/图片样式，代码块保留语言标签与复制按钮。
- **侧边栏会话名对话化**：会话名优先级改为"对话摘要覆盖 > runtime 标题 > 项目名"，不再显示 `Agent · 目录`；打开会话后自动用首条用户消息生成 64 字内摘要并本地记忆；Codex 会话标题后端兜底从 rollout 首条用户消息提取。
- **新建任务入口重做**：移除侧边栏大按钮，改为顶栏 accent "+" 图标（悬停变品牌橙），紧邻搜索/刷新。
- **现代 composer 组件**：首页 Dock 与会话输入统一为无边框 textarea + 柔和卡片（聚焦品牌色光环）+ 圆形发送钮（ArrowUp），移除首页"交给 XX"标题条与会话页快捷指令条（老古董）；细窄深色滚动条全局统一。
- **新建会话弹窗升级**：模型/权限下拉、文件夹选择按钮 + 最近目录快捷下拉，目录必选校验。

### Fixed

- 会话面板标题与项目名重叠、状态 pill（工作中/等待确认/失败）无样式的问题。
- demo 模式（`?demo`）现注入富样例 transcript，会话视图/工具折叠/Markdown 可脱离 CLI 视觉验收。

## [2.1.0] - 2026-09-01

### Added

- **动态模型选择与工作空间消除硬编码**：首页底部 Prompt Dock、新建会话模态框与设置中均支持各 Agent（Claude Code、Codex、Antigravity、Grok Build）模型动态切换与记忆持久化，移除硬编码路径。
- **现代化极简下拉组件与会话右键菜单**：
  - 新增暗色磨砂 `SelectDropdown` 组件，全局替换原生白块 select 下拉；
  - 侧边栏会话项新增右键上下文菜单（置顶/取消置顶、复制会话 ID、复制标题、复制项目路径、在新任务中打开），配备一键复制 Toast 反馈。
- **结构化折叠与沉浸式消息渲染**：
  - 会话内容彻底移除 Agent 消息卡片框与粉色/紫色边框限制，自然排版；
  - 用户消息采用深灰圆角胶囊组件（无彩色边框）；
  - 自动甄别并折叠思考过程（`<think>` / reasoning）、工具调用参数与输出，代码块新增语言标签与一键复制代码按钮。
- **多维度设置中心（ChatGPT 桌面端风格）**：
  - 依据参考图构建全功能设置面板：权限卡片（默认权限/自动审核/完全访问）、常规（编辑器目标/终端 Shell/语言/刷新周期）、导入与 Agent 探活、个人资料、外观、快捷键速查、集成（MCP/浏览器/电脑操控）与编码环境。

### Changed

- **通知栏顶置防遮挡**：将实时通道断开与系统通知移至 Stage 顶部，优化布局流，杜绝与 Masthead 或 Prompt Dock 发生重叠。
- **精简侧边栏与清理底部冗余栏**：
  - 侧边栏移除“已归档”标签页，专注展示活跃工作区会话；
  - 彻底清理聊天页底部的“Prompt 不上传”及归档会话按钮，将生成停止按钮集成到输入框。
- **消除全局橙色焦点环**：移除全局 `#c86646` 等橙色外边框，换用极简暗色微光反馈。
- **加宽对话视野**：扩宽对话流宽度上限至 1040px，大幅提升宽屏舒适度。

### Release

- 打包 Windows 桌面端 `AgentHub v2.0.9`，包含多 Provider 会话语义解码、Markdown/工具调用渲染、真实会话标题、Codex 风格工作台与左下角设置菜单。

### Changed

- 桌面端本机会话正文改为 Provider 结构化规范化：Claude Code 控制标签不再作为正文裸露，Codex、Antigravity 与 Grok Build 的消息、Markdown、命令、工具调用和状态按语义渲染；正文继续只在当前 WebView 本地展示。
- 会话区改为 Codex 式紧凑文档流，支持 CommonMark/GFM 列表、表格、引用和代码块；工具活动折叠为单行，首页移除统计看板，侧栏采用 272px 工作目录导航和真实会话标题。
- 删除侧栏裸露的“本机设置”按钮和右侧大型账号卡；设置入口收纳到左下角账号栏弹出菜单。
- 桌面端侧边栏收窄并统一为 Claude Desktop 风格的深色单色轨道，新增全局 Agent 选择器；会话按当前 Agent 过滤，补齐“收起”操作。
- 设置面板改为居中遮罩窗口，避免被主内容遮盖；会话打开时在本机按会话 ID 读取 Claude transcript 的 user/assistant 正文，正文不进入同步与云端事件。
- 桌面端进一步改为所选 Agent 下按工作目录分组，增加会话置顶，并将首页、会话顶部与三分类设置页统一为紧凑深色工作台。
- 本机首次扫描、后台持续扫描与健康汇总补齐 Antigravity；历史正文读取扩展到 Claude Code、Codex、Antigravity 与 Grok Build，并兼容 UTF-8 BOM、UTF-16 LE/BE（含无 BOM 启发式识别），避免中文乱码。
- 实时 SSE 心跳缩短到 10 秒，桌面读超时放宽到 45 秒，降低代理空闲超时或网络抖动造成的误断线，保留轮询降级。

## [2.0.8] - 2026-09-01

### Changed

- 发布 AgentHub 2.0.8 Windows x64 安装包，提供 MSI 与 NSIS Setup 两种格式。

## [2.0.7] - 2026-09-01

### Changed

- 发布 AgentHub 2.0.7 Windows x64 安装包，提供 MSI 与 NSIS Setup 两种格式。

## [2.0.6] - 2026-09-01

### Changed

- 发布 AgentHub 2.0.6 Windows x64 安装包，提供 MSI 与 NSIS Setup 两种格式。
- **桌面端工作台 UI 全面重构**：
  - 保留 Claude Desktop 的双栏、低噪音和内容优先体验，改用 AgentHub 专属的暖纸色主舞台、深色本机导航与 Provider 信号色系统；
  - 新增“Agent 星轨”，用真实的本机会话数、运行中数量、采集接入状态和实时/轮询连接状态呈现多 Agent 编队；
  - 侧栏改为真实可用的“进行中 / 已归档”范围切换，“新建会话”现在直接打开启动器，移除无实际行为的模式切换与权限开关；
  - 重构首页会话队列、快速任务输入、会话详情编辑区与新建会话面板，统一中文操作语义、焦点态、响应式布局和减少动态效果支持；
  - 将登录后工作台样式独立到 `workspace.css`，隔离认证/基础组件样式与工作台设计系统。

### Fixed

- 修复 `LaunchSessionModal` 忽略 `open` 属性、启动器在工作台加载后始终常驻的问题。
- 修复侧栏账户区嵌套按钮产生无效交互结构的问题，并补充桌面端 favicon 消除开发环境 404。
- 修复 Rust 1.97 下发布构建被 `manual_pattern_char_comparison` Clippy 门禁阻断的问题。

## [2.0.5] - 2026-09-01

### Added

- **Claude Desktop 级全功能 Agent 交互工作台壳与 Provider 会话重组**：
  - 侧边栏全面仿照 Claude Desktop 极简炭黑视觉语言（`Cowork` / `Code` 模式切换、`+ New`、`Customize`、快捷搜索与前后导航）；
  - **核心重构**：废弃原项目目录分组，严格按照 Provider 供应商（`Claude Code`、`OpenAI Codex`、`Google Antigravity`、`Grok Build`）对 Session 进行分组与折叠展示，提供独立 `+` 快速发起按钮与待关注 `⚠️` / 运行中 `•` 状态指示；
  - 底部个人信息条更新为 `Administrator · Gateway` 状态卡片；
  - 主舞台（Welcome 态）重构为包含欢迎问候、警示横幅（Alert Banner）、`Sessions` 待关注/最近会话卡片列表；
  - 底部嵌入 Claude Desktop 悬浮 Prompt 快速启动卡片（集成 `💻 Local`、`📁 ClaudeSpace`、`📋` 注入、8-bit 复古像素怪兽吉祥物、`Bypass permissions` 切换、模型选择器与快捷发起）；
  - 会话面板全屏沉浸模式与分屏抽屉自由切换，无缝嵌入主舞台交互；
  - 集成快捷 Slash 指令栏（`/help`, `/clear`, `/compact`, `/cost`, `/review`, `/test`）与 Enter 快捷键发送；
  - 优化 Thinking 思考折叠块、工具调用卡片与富文本消息气泡呈现。

## [2.0.4] - 2026-09-01

### Fixed

- **OAuth 丢失 URL & 客户端阻塞问题彻底解决**：
  - 登录界面监听并在 OAuth 发起后展示重试控制面板；
  - 新增「重新打开授权网页」和「取消并重新登录」按钮，彻底解决误关网页后客户端卡死阻塞问题。
- **智能会话命名杜绝“未命名会话”**：
  - 采集器自动提取第一条 User Prompt / Request 摘要作为标题；
  - 前端增加 `sessionDisplayTitle` 智能多级回退机制（显式标题 $\rightarrow$ 指令摘要 $\rightarrow$ Agent · 项目目录 $\rightarrow$ 会话短ID），100% 杜绝“未命名会话”。
- **反重力（Google Antigravity）扫描阻塞彻底解除**：
  - 重构 Antigravity 采集器，由全量遍历磁盘改为基于修改时间快速排序 + 前 64 行高效采样，消除磁盘 IO 阻塞。

## [2.0.3] - 2026-09-01

### Fixed

- **消除 Windows 浏览器调起时的黑框控制台闪烁**：
  - 彻底移除 `rundll32` / `powershell` 等子进程包装，重构为直接调用 Windows 原生 `ShellExecuteW` API（`shell32.dll`）；
  - 实现零子进程、零黑框控制台闪烁、毫秒级静默直调系统默认浏览器。

## [2.0.2] - 2026-09-01

### Fixed

- **Windows 默认浏览器调用 URL 参数截断缺陷修复**：
  - 修复 Windows 环境下使用 `cmd /c start` 拉起默认浏览器时，`&` 字符被 cmd 命令行解释为命令分隔符导致 URL 查询参数（如 `&client_id=agent-pulse` 等）被全部截断抛弃的致命 Bug；
  - 升级为 `rundll32 url.dll,FileProtocolHandler` 与 PowerShell `Start-Process` 双重直调机制，保证完整 URL 及其 PKCE/Client/Scope 查询参数 100% 完整传递至系统默认浏览器。

## [2.0.1] - 2026-09-01

### Fixed

- **Google / VS Code 级原生 Loopback PKCE 静默秒登支持**：
  - 彻底取消繁琐的设备配对码流（Device Code Flow），桌面端 Rust 原生绑定本地临时回环端口（`http://127.0.0.1:<port>/callback`，RFC 8252 §7.3）；
  - 修复服务端 `TgOAuthClient` 构造时环境变量空字符串导致 `client_id=""` 的穿透缺陷，增加 `.trim() ||` 严格防空回退保障；
  - 统一认证中心（`auth.tannerlab.cn`）更新 `agent-pulse` 客户端的 Loopback 重定向白名单；
  - 客户端网络引擎与 Runner 统一更新标准浏览器 User-Agent，杜绝云端 WAF 拦截；
  - 桌面端拉起系统默认浏览器后，用户完成授权即刻自动回环秒进工作台，网页端自动提示成功并关闭。
- **全链路测试与质量基线验证**：
  - 210 项单元测试、集成测试与真实交互 E2E 全量通过（Rust 98 项 + API 60 项 + Desktop 8 项 + Web 42 项 + Contracts 2 项）。

## [2.0.0] - 2026-09-01

### Changed (2.0.0-spec 架构重定界)

- **AgentHub v2.0 统一多 Agent 远程控制平面全栈核心实现与落地**：
  - **共享契约协议扩展 (`packages/contracts`)**：
    - 完整实现 Frame Envelope (`ver: 2`, `seq`, `ack_seq`, `hub_session_id`, `cmd_id`, `is_replay`) 帧封装与校验；
    - 制定并实现 `LaunchProfile`、`SessionPermissions`（`plan_only` / `ask_all_edits` / `accept_edits` / `bypass_permissions`）、`CapabilityReport` 强类型契约；
    - 制定控制命令集（`launch_session`, `send_prompt`, `resolve_approval`, `interrupt_turn`, `import_native_session`）与事件集（`stream_delta`, `session_state_changed`, `tool_approval_requested`, `turn_started`, `turn_completed`）Payload 校验；
    - 实现 Slash 指令三层智能解析分类器（`parseSlashCommand`），精准分类 HubCommand、NativeCommand 与 PromptFallback。
  - **云端多路复用网关与双层事件存储 (`services/api`)**：
    - 实现 `V2SessionManager`：管理 Runner 长连接生命周期、握手与版本协商、滑动窗口 Outbox 缓冲与断网重放（Gap Replay）；
    - 实现 `DualLayerEventStore`：持久化 Turn 规范化事件与 Provider 原始事件，集成 50ms Token Delta 实时分发与落库聚合；
    - 扩展 v2 REST 与 SSE 端点：`GET/POST /v2/capabilities`、`GET /v2/sessions`、`POST /v2/sessions/launch`、`POST /v2/sessions/:id/prompt`、`POST /v2/sessions/:id/approve`、`POST /v2/sessions/:id/interrupt`、`GET /v2/sessions/:id/stream`。
  - **Runner 本地能力探测与命令分发 (`crates/runner`)**：
    - 实现 `capabilities.rs`：动态探测本机 Claude Code、OpenAI Codex、Antigravity 模型列表、思考预算与 Slash 支持；
    - 实现 `command_router.rs`：分发执行控制命令并触发会话生命周期事件；
    - 实现 `v2_protocol.rs`：Rust 端完整的 Envelope 序列化与 Slash 指令解析。
  - **Web 控制台交互控制与审批组件 (`apps/web`)**：
    - 新增 `LaunchSessionDialog`：基于动态能力发现表单发起会话；
    - 新增 `ToolApprovalModal`：拦截并高保真呈现高危工具调用与 Diff 预览，支持一键放行与拒绝反馈；
    - 新增 `SlashCommandMenu`：智能 Slash 指令补全下拉面板；
    - 扩展 `apps/web/src/lib/api.ts` v2 交互控制 API 客户端方法。

### Added

- **Antigravity 智能体适配与观测支持**：Runner 新增对 Antigravity（Google AGY / Gemini CLI）的会话采集、Transcript JSONL 解析与实时状态映射，支持 `USER_INPUT`、`PLANNER_RESPONSE`、工具调用与规划状态标准化识别；集成本地工作区路径脱敏与快照合并，完全遵循不读取/不上传对话正文或代码的安全边界。
- **TannerLab / Tg-oauth 统一身份认证深度接入与持久化支持**：
  - Gateway API 基于 PKCE S256 规范深度集成 TannerLab 统一身份认证（`tg-oauth`），提供授权地址生成、回调验证、用户信息解析与多会话绑定；
  - 数据库新增 `0015_oauth_accounts.sql` 迁移，用户表扩展 `oauth_provider` 与 `oauth_sub` 列及联合唯一索引，`PostgresGatewayStore` 完整实现 OAuth 账号查询与原子 upsert 绑定；
  - 注册生产 IdP 客户端 `agent-pulse` 并支持全链路 Device Code Flow（RFC 8628）；
  - 桌面端集成 `open_external_url` 原生指令，自动拉起系统默认浏览器并提供设备码/链接一键复制与无感轮询授权；
  - Web 控制台 `SettingsPanel` 账户安全面板实时识别并展示 TannerLab SSO 关联状态。
- **项目全平台正式更名为 AgentHub**：
  - 项目名称由 AgentPulse 全面升级更名为 **AgentHub（智能体集成中心）**；
  - 同步更新 Web 元数据、Tauri 桌面端应用名称/窗口标题/权限配置、Android 原生端配置、Runner CLI 帮助名、部署脚本及所有用户端文案与文档。

### Documentation

- 收敛文档入口，明确里程碑证据、当前源码和生产部署是三个不同状态；Grok Build 已进 Gitea 但不能据此声明生产已更新。

### Added

- 桌面端在 Claude Code、Codex 之后新增 Grok Build 采集：扫描 `GROK_HOME`（默认
  `~/.grok`）下的 `summary.json` 与 `active_sessions.json`，用 PID 存活和
  `updates.jsonl` 写入时效推断 LIVE/WORKING/IDLE，不读取对话正文、`last_turn_summary`
  或 `auth.json`。同步契约增加 `grok_build` 与 `grok_sessions`/`grok_hook`/
  `grok_active_sessions` 来源。桌面来源卡、筛选、详情、运行上下文、记忆统计、
  CLI 路径和本机继续会话均已适配；Grok 回合使用 `--prompt-file` 与
  `streaming-json`，只转发文本增量。网页端/移动端界面暂不改筛选与任务入口，
  协议层可接收该来源以便桌面先上线。本地提交 `ea7aba0` 已推到 Gitea `main`；
  生产仍停在 `4ff3082`，等待明确确认后再重建 `agent-pulse-api` / `agent-pulse-web`。

- 桌面会话索引新增设备下拉筛选：账号连接多台设备时可按当前设备或远端设备查看 Session，
  筛选与 Agent、归档和搜索条件组合，设备不存在会话时展示明确空状态，不改变本地扫描和云端
  同步边界。

- 桌面会话详情新增当前筛选结果内的相邻会话巡检：详情头部提供上一条/下一条按钮，左右方向键
  可切换会话，输入框、正在执行回合和关闭操作不会被快捷键抢占；边界按钮明确禁用，不改变
  当前筛选、归档视图或会话排序。

- 桌面会话详情新增只读状态时间线：通过 Tauri Host 携带 OS Keyring Cookie 请求当前账号的
  `/v1/sessions/events`，按设备、供应商和 Session ID 隔离查询，展示序列、观察时间、接收时间、
  项目名与状态变化。Host 会过滤原始状态详情等非展示字段；本地未同步会话明确显示暂无云端记录，
  不把空结果误报为采集失败。

- 桌面会话索引新增供应商显式名称与项目目录分组，详情面板展示模型、推理强度、Claude 权限
  模式、Codex 审批/沙箱策略、上下文窗口、当前用量和累计 Token。Claude 仅扫描有界 JSONL
  头尾的 `custom-title`/`ai-title`/`mode`/助手元数据；Codex 只读状态库白名单列（包括审批与
  沙箱策略）以及限定在
  `CODEX_HOME` 内的 rollout 2 MiB 尾部 `turn_context`/`token_count`。运行上下文缓存 15 秒，
  只附加当前设备桌面快照，不进入 Outbox/Gateway，不读取首条 Prompt 推断标题。搜索现支持
  会话名、分类目录和模型，并新增采集器隐私回归测试与本地/远端合并隔离测试。
- 桌面 App 新增 Gateway SSE 实时变化通道：Rust Host 使用 OS Keyring Cookie 主动连接
  `/v1/stream`，WebView 只接收连接状态和刷新信号；断流按 2→15 秒退避重连，2–60 秒单飞
  轮询继续兜底。会话回合改用 Tauri Channel 增量呈现：Claude `stream-json` 转发文本 delta，
  Codex JSONL 转发助手消息与无参数活动标签；thinking、命令正文、工具参数和 stderr 均被
  过滤。新增单会话并发门控、“停止回合”进程树回收、动画帧批量渲染、SSE/事件解析与取消
  回归测试，并完成 1280×800 和 960×640 浏览器验收。
- 桌面 App 从只读观测台扩展为本机会话控制台：会话抽屉展示完整元数据，并可通过 stdin
  继续 Claude Code/Codex 会话；回复仅保留在当前前端内存，不进入 Outbox 或 Gateway。
  新增活跃/归档视图、Codex 原生归档/恢复、Claude 本机视图归档，以及可配置的 2–60 秒
  单飞刷新周期和 CLI 路径。Agent 子进程使用专用阻塞线程、进程树超时回收和 2 MiB 输出
  上限；会话 ID、Prompt、归档字段和配置均执行边界校验。桌面宽屏与 960×640 窄窗口已完成
  浏览器交互验收。
- 重构 Windows 桌面 App 为本地优先的 Session 观测台：启动时先扫描最多 1000 条 Claude
  与 Codex 历史元数据，再将本地 Outbox 快照与云端结果合并展示；新增真实来源健康、待上传
  数量、Claude/Codex memory 索引元数据、会话搜索/来源筛选和账号详情面板。仪表盘三条云端
  请求改为并行，前端轮询改为 8 秒单飞并在完成后再调度，Tauri 阻塞命令移出 UI 主执行路径，
  避免网络慢或多次刷新导致请求堆积。Codex 桌面扫描上限由 100 提升为 1000；空态和来源卡
  不再把“同步进程存在”误报成“已检测到 Agent”。
- 补齐 Tauri bundle 的跨平台图标清单，使 Windows MSI/NSIS 发布不再因未解析到 `.ico`
  而在 Release EXE 已编译后失败。

- 新增 Windows Tauri 桌面 App：复用现有邮箱注册/登录接口，将 AgentPulse HttpOnly
  会话保存在 OS Keyring；登录后自动创建/加载 Ed25519 设备身份、完成账号设备登记并
  启动内嵌 Runner 与同步 Worker，不再要求用户打开 CMD 或输入配对码。界面提供来源、
  会话、设备、暂停/恢复同步、刷新和退出；默认只允许 HTTPS，回环 HTTP 仅用于开发。
- Runner 新增 Claude Code 本地 Session JSONL 元数据采集，以及 Codex app-server 无法
  启动时的只读 `state_*.sqlite` Thread 元数据兜底。本机实测可发现 Claude 与 Codex
  历史会话；采集器有界读取并只保留白名单字段，不读取或上传消息、工具载荷、rollout
  正文和供应商登录凭据。

- Added signed remote-task execution receipts. Runners can report bounded start/end timestamps, a SHA-256 digest of a restricted receipt, and an optional exit code. The signed fields are persisted in both stores and never include CLI output, paths, prompts, or credentials.

- 新增可选 Stripe 订阅闭环：设置面板显示支付能力，服务端提供当前订阅状态、Pro/Team
  Checkout 和 Customer Portal；Checkout 只使用当前 HttpOnly 会话和服务端 Price ID，
  不接受浏览器提交 Customer/Subscription ID。Stripe webhook 使用原始请求体和官方 SDK
  验签，只映射配置的 Price、账户 metadata 和订阅状态，再复用现有幂等权益 Store。
  未配置 Stripe 时支付 API 显式返回不可用，不影响登录、监控和通用 HMAC 账单入口。

- 新增可选 Resend 邮件适配器：验证邮箱、密码重置和 Team 邀请使用服务端模板发送，HTML
  输出转义显示名和链接；Resend 与原通用 HTTP 邮件适配器互斥，API Key 初始化后从进程
  环境删除。未配置邮件供应商时原有不可用门控保持不变。

- 扩展不可变审计轨迹覆盖设备绑定码创建、设备登记/撤销，以及远程任务创建、设备领取、
  取消和状态回报。工作区事件区分网页登录 actor 与设备签名动作，元数据只保留设备/任务
  公开 ID、Agent 类型和状态，不记录 Prompt、路径或 CLI 输出。PostgreSQL 0013 迁移保存
  绑定码签发操作者，并将审计目标 ID 扩展为受限公开文本。

- PostgreSQL 部署新增跨 API 实例变化广播：每个实例使用独立连接订阅
  `agent_pulse_changes_v1`，本地发布后通过 `LISTEN/NOTIFY` 唤醒其他实例的 owner SSE；
  随机 origin 防止自身回声重复推送，负载只包含设备 ID、计数与变化类型。Broker 连接失败
  时 API 继续使用当前实例内存广播和浏览器轮询兜底。

- 新增独立设备心跳协议：Runner 默认每 15 秒生成 Ed25519 签名心跳并主动发送，Gateway
  校验登记、公钥、撤销状态和 5 分钟时钟窗口；内存与 PostgreSQL Store 按设备保存
  `last_heartbeat_at` 并拒绝旧时间戳重放。心跳只刷新设备活跃时间，不写入 Session 内容。

- 新增账户与工作区审计轨迹：账户创建、登录会话、邮箱验证、密码重置/修改、订阅变化、
  工作区切换，以及 Team 邀请、接受、撤销、角色调整和成员移除均由 Store 在业务变更的
  同一事务内写入不可变事件。`GET /v1/audit` 使用登录会话解析账户或当前工作区范围，
  提供最多 100 条的 UUID 游标分页；已移除成员不能继续读取原团队记录。审计元数据只接受
  角色、套餐、状态、公开 UUID 和撤销数量等固定键，不记录邮箱、密码、Token、IP、请求体、
  Prompt 或 CLI 内容。PostgreSQL 0011 迁移新增双范围索引，设置面板增加账户/当前工作区
  分段时间线，并完成桌面及 390x844 浏览器验收。

- 新增 Team 共享工作区：Web 会话区分登录人和当前数据租户，每个账户保留个人工作区，
  用户可在个人与已加入团队之间切换。Team 所有者可邀请已注册用户为管理员或成员，
  管理员只能管理普通成员；邀请 Token 只存摘要、72 小时过期、通过 URL fragment 传递，
  已加入成员与待处理邀请共同占用席位。成员被移除后，其相关会话自动回到个人工作区。
  设置面板新增席位、邀请、角色、成员和工作区管理；PostgreSQL 0010 迁移新增工作区成员、
  邀请表和会话当前工作区字段，内存与环境门控 PostgreSQL 测试覆盖核心权限和隔离契约。
  Web 同源 Gateway 代理同时转发 `PATCH`，确保设置面板的成员角色更新能到达 API；内存与
  PostgreSQL Store 都在服务端拒绝普通成员管理其他成员。

- 新增完整密码生命周期：登录页可请求不泄露账户存在性的重置邮件；一次性重置 Token
  仅存摘要、1 小时过期并通过 URL fragment 传递，成功后撤销全部 Web 会话。设置面板
  支持校验当前密码后改密，保留当前会话并撤销其他会话，同时作废旧重置链接。
  PostgreSQL 增加 0009 密码重置表，API 与环境门控的 PostgreSQL 测试覆盖签发、消费、
  会话轮换和旧密码失效。

- 新增账户生命周期与订阅事件控制面：邮箱验证 Token 一次性且只存摘要，链接 fragment
  避免进入代理日志；启用邮件验证后，设备绑定与远程任务要求已验证邮箱。用户可在设置
  面板查看、单个撤销或批量撤销 Web 会话。登录失败使用 HMAC scope 做共享 Store 限流。
  厂商无关的账单入口使用时间戳 HMAC、严格字段白名单、事件 UUID 幂等和乱序保护更新
  Free/Pro/Team 权益；取消或欠费在已付周期结束前保留权益。PostgreSQL 迁移新增验证、
  限流、Session UUID 与订阅事件审计表。

- 新增 Pro/Team 远程任务中转闭环：共享契约定义严格任务字段、版本化状态机与 Ed25519
  设备领取/回报；内存与 PostgreSQL Store 按用户、设备、幂等键和并发权益隔离，支持
  取消、过期恢复与审计事件。Runner 以出站轮询领取任务，只用本机项目指纹 allow-list
  解析路径，在终端确认后经 stdin 调用 Claude Code/Codex；运行中断不会自动重复执行。
  Web 新增一级任务中心、Free 限制态、创建/取消和实时状态列表，桌面及移动端共用。
  Runner 的 HTTP 闭环回归覆盖成功、用户拒绝和运行中断恢复，并拒绝 Gateway 返回的
  非本机目标任务，防止错误响应触发跨设备执行。

- 新增多用户账户与订阅权益基础：邮箱注册/登录使用 scrypt 密码摘要，Web 会话使用
  HttpOnly/SameSite Cookie 且数据库只保存随机令牌摘要；所有设备、Session、认证快照、
  SSE 与 Push 查询继续从服务端身份解析用户租户，不接受浏览器伪造 owner。新账户默认
  获得 Free 权益（1 台设备、7 天历史、无远程任务），Pro/Team 权益集中映射为设备数、
  并发任务、历史和成员上限；设备绑定码签发已在服务端执行设备额度检查。
- Web 新增账户入口和当前套餐身份区；Claude Code/Codex 从普通下拉筛选提升为常驻一级
  分段分类。生产同源代理只转发 HttpOnly 会话 Cookie，并保持拦截浏览器 Authorization
  和 `x-user-id` 冒充。旧单 owner Bearer 部署仍作为显式兼容模式保留。

- Runner 增加显式授权的 Claude Code/Codex 认证状态与配额采集：通过官方 CLI/app-server
  查询登录状态、刷新生命周期与配额窗口，使用设备派生密钥将原始凭据仅缓存于本地，
  并以独立的 Ed25519 签名快照同步到 Gateway。Gateway 和 Web 只接收安全字段；后台
  状态循环不启动交互式登录，`auth login/logout/token --reveal` 保留为本地显式操作。
- Web 面板修复来源筛选后检查器残留与筛选状态循环，移动端底栏补齐诊断/帮助入口，
  活动页和总览页的静态标签改为真实可操作筛选，并增加 Agent 认证与配额健康卡片。

- 原生 Android 增加与 Web 同语义的 Jetpack Compose 动效系统：底栏页面按方向切换，
  Session、设备与时间线使用有界 stagger 入场，详情按 Session/状态重新进入，概览
  数字平滑切换，筛选和底栏选中态提供缩放/透明度反馈，C 形品牌标记与粉色信号保持
  低速持续动画。全部使用 Compose 原生 `AnimatedContent`、`AnimatedVisibility`、
  `animate*AsState` 与 `InfiniteTransition`，服从 Android 系统动画时长缩放；模拟器
  已验证 10x 可观察过渡、0x 直接显示最终状态，原有相邻会话和系统 Back 不变。
- Web/桌面端接入 GSAP 3 与 `@gsap/react` 动效层：首屏标题、连接区、概览卡、筛选面和
  列表标题按时间线入场；会话筛选和状态变化使用 transform/opacity stagger，详情、
  时间线与设置/筛选面板按布局方向进入；概览数字平滑过渡，品牌 C 标记和实时连接点
  提供克制的持续反馈，桌面概览卡使用 `quickTo` 指针视差。所有动画限定在组件作用域
  并自动清理，`prefers-reduced-motion` 下停用，完成后移除瞬时内联样式。
- 新增独立原生 Android App（Kotlin + Jetpack Compose，非 WebView/PWA 封装）：
  直接消费现有只读设备、Session 与时间线 API，提供全部/进行中/待处理/最近完成/
  历史五类筛选、等待优先首页、设备列表、详情时间线、相邻会话和系统返回。生产
  Gateway 强制 HTTPS，本地 HTTP 仅允许回环调试；访问令牌经 Android Keystore
  AES-256-GCM 加密且配置禁止备份。Android 36.1 模拟器使用 Runner 签名同步的
  Claude 等待授权与 Codex 完成 Fixture 完成安装、数据、详情和重启恢复验收。
- Web、桌面宽屏与 Android 启用统一“Paper / Ink / Pink Signal”视觉系统：暖白
  纸张背景、高对比标题、24–32px 大圆角细描边卡片、黑色胶囊选中态、粉色提醒点和
  同一 C 形品牌图标；PWA 的 SVG 与 192/512 PNG 图标同步重制，业务分类和状态色
  保持原语义。
- Web/PWA 完成移动端 UI 第五轮：920px 以下的全屏 Session 详情升级为真正的模态
  钻取，打开后锁定背景滚动、焦点进入返回按钮、Tab/Shift+Tab 圈定、Escape 关闭，
  并把焦点恢复到最后查看的 Session 卡；桌面 sticky 侧栏保持非模态。手机详情增加
  safe-area 固定头部和当前筛选结果内的上一条/下一条，切换后回到详情顶部且边界按钮
  禁用时焦点不会逃到页面。真实 Chrome 在 390×844、844×390 与 1280×800 验证
  模态/非模态边界、焦点循环/恢复、相邻切换和 0 横向溢出。
- Web/PWA 完成移动端 UI 第四轮：固定底栏为“待处理”增加有界数量徽标，并在设备
  范围生效时显示筛选状态；浏览器离线事件会立即关闭 SSE、保留当前内存快照并显示
  明确提示，恢复联网后自动重连和刷新。移动壳新增 320px 超窄屏压缩及
  920×480 以内手机横屏判定，横屏保持四列概览与固定底栏。真实 Chrome 使用临时
  Runner 签名同步的等待/完成会话，在 320×568、844×390 验证 0 横向溢出、徽标、
  筛选、断网→恢复；manifest 与 installability error 仍为 0，未执行系统安装。
- Web/PWA 完成移动端 UI 第三轮：小于 520px 仍显示实时/轮询/错误连接状态，启用
  `viewport-fit=cover` 并让主面板与全屏详情同时避让顶部/底部安全区；底部筛选和
  设置面板新增焦点圈定、Escape 关闭、关闭后焦点恢复、页面滚动锁定和
  reduced-motion 兼容，关键关闭/筛选/设备触控目标不小于 44px。真实 Chrome 在
  390×844 下验证无横向溢出、焦点正反循环与详情滚动边界；DevTools 返回完整
  manifest、0 条 PWA installability error，页面实际收到安装能力并显示“安装应用”。
- Web/PWA 完成移动端 UI 第二轮：760px 以下把完整五类会话视图与设备范围收进
  “筛选与设备”底部抽屉，固定导航聚焦全部、活动、待处理、设备和设置；设备卡按
  现有 Session 元数据汇总已观测 Agent、会话、活动与等待数，不把它冒充安装/登录
  清单。设置新增能力驱动的 PWA 安装入口，manifest 补齐应用范围、方向和 192/512
  图标；安全上下文自动注册现有 Service Worker，但不请求通知权限、不增加 fetch
  缓存。390×844 窄屏以真实签名同步的 38 条会话完成组合筛选和无横向溢出验收。
- Web/PWA 增加移动端专属壳层：760px 以下显示带安全区的五项底部快捷导航，
  设置在手机上切换为 94dvh 底部面板，Session 详情继续以
  更高层级全屏钻取。桌面设置同步升级为连接、设备、等待提醒三个分区，并增加绑定码
  复制、设备在线状态、固定操作栏、Escape/遮罩关闭和页面滚动锁定。
- 面板补齐首屏骨架、同步错误重试与筛选空状态：配置和第一次查询完成前不再短暂显示
  全零统计；网关失败保留上次快照并提供显式重连；空分类可一键恢复全部会话。
- Web 监控面板升级为面向异常处置的运营视图：顶部汇总在线设备、活动会话、等待
  处理和最近完成，设备范围与会话视图合并为统一控制面；列表强化 Agent、项目、
  等待原因、设备和状态层级，详情面板补齐独立标题语义。桌面端采用列表与详情
  并列布局，窄屏保持双列概览并以全屏详情承接钻取，不改变既有只读数据契约。
- Web Push 增加浏览器展示回执：Service Worker 仅在解密成功且
  `showNotification()` 完成后，向已打开的同源面板发送不含设备、Session、项目
  或 payload 的时间戳；设置面板显示最近回执且只保存在页面内存，用于区分推送
  服务接受与浏览器展示 API 成功。
- Gateway 为异步 Web Push 增加隐私安全的投递结果日志：只输出
  `delivered / gone / failed` 聚合计数，不记录订阅 endpoint、owner、Session ID
  或 payload，使真实浏览器联调可以区分推送服务拒绝与系统通知展示失败。
- Runner 新增私有 Gateway CA 支持：`run`、`device bind` 和 `sync push` 可通过
  `--gateway-ca-cert` / `AGENT_PLATFORM_GATEWAY_CA_CERT` 读取根证书 PEM，严格
  校验证书链、主机名和有效期，不提供跳过验证开关。新增真实 rustls 握手回归：
  未知签发者返回 `gateway_tls_failed` 且不清 Outbox，配置正确根证书后完成 ACK；
  无效或不可读证书返回稳定错误且不泄露路径/内容。后台同步失败报告现在持续携带
  磁盘 `pending_events`，便于观察长时间离线积压。
- Gateway 新增持久设备生命周期：受 Bearer 保护的 `POST /v1/device-bindings` 签发
  默认 10 分钟有效的一次性绑定码，Store 只保存 SHA-256 摘要；PostgreSQL 以事务
  行锁保证并发单次领取，同设备重试幂等，过期未领取码在后续签发时清理。新增
  `DELETE /v1/devices/:deviceId` owner 隔离软撤销，保留历史 Session、设备显示
  「已撤销」，并拒绝重新登记和后续同步。Web 设置面板可生成、复制绑定码并管理
  设备撤销，内存与真实 PostgreSQL 均覆盖过期、竞态、幂等和历史保留。
- Gateway 新增个人部署 Bearer 访问令牌认证，保护设备/Session 查询、SSE 与 Push
  订阅接口；无效或缺失凭据统一返回 `authentication_required`。Web `/v1` 同源
  Route Handler 从服务端环境注入令牌，丢弃浏览器 Authorization/Cookie，令牌不
  进入 URL、localStorage 或前端配置。`x-user-id` 仅在显式开发模式且回环监听时
  可用，并新增令牌配置验证、身份冒充拒绝和代理覆盖测试。
- Runner `run` 新增可选后台自动同步：配置 `AGENT_PLATFORM_GATEWAY_URL` 与设备身份
  后，以独立线程和 SQLite 连接启动排空旧 Outbox，采集新事件通过容量为 1 的合并
  信号立即唤醒；网络失败按独立 2→60 秒指数退避，期间 Adapter 继续采集。每轮沿用
  `sync push` 的批次上限、HTTPS 策略和 ACK 删除约束，并新增启动重放、采集唤醒及
  真实回环端点失败→恢复后自动清零测试；仅有其他设备旧事件时报告稳定
  `sync_pending_device_mismatch` 并退避。
- M4 现场验收补齐本机 PostgreSQL 18.4：持久化重连与 12 设备/3000 Session
  随机 Schema 并发工作负载全部通过、0 跳过；Runner 对 Desktop 内置
  Codex CLI 0.144.2 完成一次真实只读采集，接收 35 个白名单 Session 且
  Adapter 健康记录为空。外部不可达 HTTPS 端点超时后磁盘 Outbox 保留事件，
  改回本地 Gateway 后合法 ACK 清零。普通 Chrome 完成通知授权、Service Worker
  控制和 Push 订阅登记，但系统级通知呈现仍未观察到，因此保留为未验证边界。
- M4 第二轮把断网用例升级为磁盘 SQLite Outbox：上传失败后关闭并重开数据库，
  事件与序列水位保持，合法 ACK 后再次重开仍为空；同一套 12 设备/3000 Session
  工作负载抽成共享 Fixture，并接入环境门控的 PostgreSQL 随机 Schema 测试，
  以 4 条连接并发验证设备行锁、双 owner 隔离和整批重放不重复。
- M4 安全与稳定性自动化基线：Runner 增加跨层字段泄漏、新版 Agent Fixture、
  无 Hook 轮询和断网恢复重放验收；Gateway 增加 12 个独立签名设备、3000 个
  Session 的完整注册/验签/写入/查询工作负载，验证双 owner 隔离、按设备筛选、
  ACK 重放不重复及读响应字段边界。新增 `docs/M4_VALIDATION.md` 区分自动化证据与
  真实 PostgreSQL、真实 Agent 升级、真实网络故障等待验证项。
- Gateway 新增 owner 维度实时变化通道 `GET /v1/stream`（Server-Sent Events）：
  批次入库且确实产生新事件时推送 `event: sessions`，数据仅
  `{"device_id","accepted"}`，会话内容仍只能从既有读接口获得；20 秒注释心跳、
  单 owner 并发 8 条流上限（超出 429 `too_many_streams`）、重放批次不推送、
  进程关闭时统一收流。Store 的 `ingestBatch` 随之返回
  `{accepted, owner_id, changes}`，`changes` 是本批次前后活动状态确实发生变化的
  会话（内存与 PostgreSQL 两侧同一契约，PostgreSQL 在锁定设备行的事务内前后各读
  一次快照状态）。
- Gateway 新增 Web Push：`GET /v1/push/key`、`POST`/`DELETE
/v1/push/subscriptions`，会话跃迁进入 `WAITING_INPUT`/`WAITING_PERMISSION`
  时向该 owner 的订阅推送通知，推送服务返回 404/410 的订阅立即清理。加密
  （RFC 8188 aes128gcm + RFC 8291）与 VAPID 授权（RFC 8292）基于 `node:crypto`
  自实现，用 RFC 8291 官方示例向量逐字节钉死，不引入新依赖。未配置完整 VAPID
  三元组时功能整体关闭（`push_disabled`）而不是半开；新增
  `npm run vapid-keys` 生成密钥对。订阅 endpoint 必须是无凭据的 HTTPS URL，
  p256dh/auth 按点格式与长度校验；订阅表随 0003 迁移落库，删除按 owner 隔离。
- 面板接入 Web Push：新增 `apps/web/public/sw.js`（只处理 `push` 与
  `notificationclick`，**不注册 fetch 处理器、不做离线缓存**）、设置面板"推送
  通知"开关（不支持/网关未配置/权限被拒三种状态各自禁用并说明原因）、订阅与
  注销走 `/v1/push/subscriptions`。点击通知聚焦已打开的面板并通过 postMessage
  传递会话引用后自动选中该会话，标识不进 URL；没有已打开窗口时只打开面板首页。
  订阅登记失败会撤销刚建立的浏览器订阅，避免出现网关不知道的"死订阅"。
- 面板接入实时通道：变化到达即刷新列表与详情时间线，轮询降级为兜底（连上流
  30 秒、断流 5 秒），顶部状态显示「实时 / 轮询」。断流按 1→15 秒指数退避重连，
  流不可用时行为回落到原来的 5 秒轮询。
- `apps/web` 新增 `app/v1/stream/route.ts` 流式代理：Next 的 `/v1/:path*`
  rewrite 会缓冲响应体，SSE 经它转发永远到不了浏览器，改由 Route Handler 直接
  透传上游 body，同源代理与「网关不开 CORS」的结构保持不变。

- M3 响应式 Web 面板（`apps/web`，Next.js 16 + React 19，纳入 npm workspace
  与根质量门）：设备列表（在线状态、最近上报时间、点击筛选）、会话五类视图
  （全部/活动/等待处理/最近完成/历史，视图是可重叠的过滤器而非分区）、会话
  详情与状态时间线、5 秒轮询（页面不可见时暂停周期轮询但首次拉取总是执行）、
  移动端全屏详情、深浅色主题与 PWA manifest。开发期经 Next rewrites 把 `/v1`
  代理到网关（默认 `http://127.0.0.1:3001`，可用 `GATEWAY_ORIGIN` 覆盖），
  浏览器只访问同源地址，网关无需开 CORS。网关地址与 `x-user-id` 存
  localStorage，不进 URL；直连地址校验沿用 Runner 出站策略（HTTPS 或回环
  HTTP，禁凭据/查询/片段）。
- Gateway 新增 `GET /v1/sessions/events`（Session 状态时间线）：按
  `device_id + agent_type + agent_session_id` 返回该会话的历史观察事件，
  sequence 降序，`limit` 默认 100、上限 500；查询按 owner 隔离，未知参数与
  非法 limit 拒绝。内存与 PostgreSQL 双 store 实现同一契约，PostgreSQL 增加
  0002 迁移建立 `(device_id, agent_type, agent_session_id, sequence DESC)`
  表达式索引。内存 store 的事件表随之保存 `received_at`，与 PostgreSQL 返回
  形状一致。

- 设备注册增加 Ed25519 私钥持有证明：Runner 对
  `{binding_code, device_id, public_key}` 紧凑 JSON 签名，Gateway 验签后才接受
  注册，公开可见的公钥不能再被第三方抢注绑定到其他账号。
- `sync push` 在没有可发送批次但 Outbox 仍有事件时输出警告，提示这些事件是在
  没有 `--identity` 的情况下采集、归属另一个设备 ID。
- 跨语言签名 Fixture 扩展出 `raw_status_detail` 多键、`waiting_reason` 与
  `started_at` 字段，锁定 detail 键排序在 Node 与 Rust 之间的字节一致性。
- 新增 `.gitattributes` 强制 `*.sql` 等文本文件使用 LF，避免迁移校验和随检出
  平台漂移。

### Changed

- 移除 Claude Session 采集对非官方/当前不存在的 `claude agents --json` 命令依赖；
  历史文件只映射为 `PERSISTED/UNKNOWN`，实时工作与等待状态继续由 Hook 提供。
- Claude/Codex 遇到未知 Hook 名时仍记录来源，但活动状态固定降为 `UNKNOWN`、
  置信度降为 `inferred`；已识别 Hook 继续使用 `observed`。这样 Agent 新版本新增
  Hook 不会被平台误报成确定状态。
- PostgreSQL 集成测试补上 `ingestBatch` 新返回值与推送订阅的断言：该测试在本机
  始终 SKIP，旧断言 `assert.equal(await ingestBatch(...), 1)` 在真实数据库上第
  一次运行就会失败。
- 采集守护进程遇到本地 SQLite 暂时性错误（例如 hook 进程并发写导致的
  SQLITE_BUSY）不再退出，改为按失败退避重试并输出稳定错误码；SQLite 连接同时
  设置 5 秒 busy_timeout。
- Outbox 入队前先与本地会话快照合并，同步事件流携带与本地一致的"保留最近
  活动状态"语义（Codex `notLoaded` 不再把云端会话状态抹成 UNKNOWN）；事件仍
  保留本次观察自身的 `event_id` 与 `observed_at`。
- Claude `agents --json` 中缺少会话 ID 的单个条目改为跳过，不再让整批采集
  失败（与 Codex thread 列表行为一致）。
- Claude `blocked` 且 `waitingFor` 无法识别时映射为 `UNKNOWN`（Inferred），
  不再以权威置信度伪造 `WAITING_INPUT`。
- Codex 未识别的 thread 状态与 app-server 方法映射为 Inferred 置信度，与
  `notLoaded` 同等对待；thread 列表缺 `cwd` 时回退到已记住的项目路径，且不再
  用空值覆盖记忆。
- 状态合并在保留上一活动状态时同时保留其 `waiting_reason`；
  `last_activity_at` 合并后不会倒退。
- 移除 `SessionObservation` 中从未填充的会话级 `display_name` 遗留字段，
  代码与"任务标题不进入云端"的决策完全一致。
- TypeScript 编译目标升至 ES2024（需要 `String.prototype.isWellFormed`，
  仓库已要求 Node >= 22）。

### Fixed

- Windows 桌面端 HTTPS 客户端读取当前用户的系统代理配置；从资源管理器启动、没有
  `HTTP_PROXY` 等终端环境变量时，也能沿用系统代理连接生产 Gateway。
- Windows Release 桌面程序改用 GUI subsystem，安装后启动只显示 Tauri 前端窗口，
  不再同时弹出控制台黑框；Debug 构建仍保留终端，方便本地诊断。
- PostgreSQL 集成测试的 `listSessions` 过滤断言从 `WORKING` 修正为
  fixture 实际的 `WAITING_PERMISSION`：该测试因环境门控在本机始终 SKIP，
  错误断言从未暴露，真实数据库上第一次运行就会失败。
- Claude 采集成功路径先关闭 Windows Job Object 再等待读取线程：后代进程持有
  stdout 写端时不再造成守护进程永久挂死（超时路径此前已正确处理）。
- Unix 侧采集子进程放入独立进程组，超时与清理时对整组发送 SIGKILL，对齐
  Windows Job Object 的进程树终止语义（本机为 Windows，Unix 路径为交叉编译
  验证）。
- Codex app-server 响应匹配不再吞掉与请求 id 撞号的服务端请求：带 `method`
  的消息一律进入被动通知队列，审批/输入信号不会因 id 冲突丢失。
- Codex 协议行读取改为边读边限长，无换行的异常输出不再在 16MiB 上限检查前
  无界占用内存。
- 亚秒轮询间隔不再被取整为零导致忙循环（调度使用完整 Duration 精度）。
- 网关侧内存 store 的事件重复检测改用深度相等（与 PostgreSQL 路径一致），
  不再依赖 JSON 键序。
- 注册失败（如 device_conflict）时释放一次性绑定码，不再烧毁后阻止换设备
  重试。
- PostgreSQL 迁移校验和在计算前归一化 CRLF，Windows 检出与 Linux 部署不再
  因行尾差异触发启动失败。

- 初始化 AgentPulse 独立 Git 仓库及 Solum 风格的 Agent 工作规范、变更日志、
  踩坑集和杂项决策记录。
- 建立 Rust Runner：Claude/Codex Session 状态标准化、Hook 事件适配、工作目录
  HMAC 脱敏、SQLite Outbox、Session 快照和 Fixture 隐私契约测试。
- 增加 `collect-once` 命令，可调用 Claude `agents --json` 或通过 Codex
  app-server `thread/list` 采集一次 Session 元数据。
- 增加 `run` 常驻采集命令：Claude/Codex Adapter 可独立或同时运行，根据活动状态
  使用 2/10 秒自适应轮询，失败时指数退避到最多 60 秒，并支持 `Ctrl-C` 协作式退出。
- 增加 SQLite Adapter 健康状态和 `outbox health` 诊断命令；失败只记录固定安全
  错误码，不保存可执行路径或原始系统错误。
- 为一次性与持续采集增加可配置的强制超时；Windows 使用 Job Object 在超时或退出时
  终止完整采集进程树。
- Codex app-server 改为后台读取协议消息，被动合并状态、Turn、审批和用户输入通知，
  同时保留 `thread/list` 与 `thread/loaded/list` 轮询兜底。
- 增加 Ed25519 本地设备身份：设备 ID 从公钥派生，身份可创建、加载、校验和展示，
  并可通过全局 `--identity` 同时提供设备 ID 与项目指纹密钥。
- 增加版本化同步批次协议及 `sync prepare`、`sync verify`、`sync ack` 诊断命令；
  批次包含固定白名单事件、单调序列、设备公钥和签名。
- 建立 npm workspace、共享 TypeScript 协议包和 Fastify Gateway API；支持开发期
  一次性设备绑定、签名批次验收、内存幂等写入、设备列表和 Session 过滤查询。
- 增加 Node 生成、Rust 验证的协议版本 1 跨语言签名 Fixture，并将 Rust、Node、
  TypeScript 与 Prettier 检查纳入仓库质量门。
- 增加 Runner `device bind` 和 `sync push`：可直接登记设备公钥、上传有界签名
  批次、校验 Gateway 确认并清理已确认 Outbox 事件。
- 增加可配置全局超时、稳定网络错误分类和 Gateway URL 安全校验；支持 HTTPS 远程
  地址，并为本地开发保留回环 HTTP。
- 增加 PostgreSQL Gateway Store、版本化 SQL 迁移和 `migrate` 命令；设备登记、
  事件幂等写入、序列水位、Session 快照与查询可跨服务重启持久化。
- 增加环境门控的 PostgreSQL 集成测试，在随机测试 Schema 中验证迁移幂等、批次
  重放、用户隔离和重连后的持久化查询。

### Fixed

- Outbox 序列号在已确认事件删除后仍保持单调递增，并兼容已有 Outbox 事件的
  序列水位回填。
- Codex `active + waitingOnApproval` 不再误报为“正在工作”，统一映射为
  `WAITING_PERMISSION`。
- Outbox 诊断命令不再错误要求项目指纹密钥。
- 持续轮询检测到 Session 快照没有变化时不再重复生成 Outbox 事件，避免空闲状态下
  本地队列持续增长。
- Claude CLI 或 Codex app-server 卡住时不再无限阻塞采集线程；协议读取线程和管道会
  在子进程树终止后可靠回收。

### Security

- 更新传递依赖 `fast-uri` 至 3.1.5/4.1.2、`nanoid` 至 3.3.18，修复 URL host
  confusion 与零长度自定义生成器死循环 advisory；生产镜像构建前重新执行
  `npm audit --audit-level=high`，结果为零漏洞。
- 根 package.json 增加 npm `overrides`，把 Next.js 依赖链中的 postcss 顶至
  8.5.23、sharp 顶至 0.35.3，消除三个高危 advisory（postcss sourceMappingURL
  路径穿越与 XSS、sharp 捆绑 libvips CVE）；受影响范围覆盖 Next 全部现行
  版本，无法靠升级 Next 本体解决，`npm audit` 现为零漏洞。
- Claude `agents --json` 路径的 `kind`、`waiting_for`、`cli_status` 值改为
  经过共享消毒器（64 字符 + 固定字符集）再入 detail，`raw_status` 钳制到协议
  上限 128 字符；此前这些 CLI 控制的字符串会原样进入同步字段，未来 CLI 若在
  `waitingFor` 内嵌任务文本将原样上云。Codex 方法名与 source 同样走共享
  消毒器。
- 会话工作目录恰为当前用户主目录时显示名固定为 `home`，不再把账号用户名当作
  项目名上传。
- Rust 同步结构启用 `deny_unknown_fields`：`sync verify` 不再对注入了未签名
  附加字段的批次文件报告有效，签名语义从"已知字段投影"收紧为"结构完全匹配"。
- 协议 detail 键排序明确为 Unicode 码点升序并限制键为 snake_case ASCII：
  Node 侧此前使用 locale 相关的 `localeCompare`，混合大小写或非 ASCII 键会
  与 Rust BTreeMap 字节序算出不同签名字节，导致合法批次被拒收、Outbox 卡死
  （当前所有键均为小写 ASCII，属潜伏态；已同步收紧空 detail 对象为非法）。
- 合约层 UUID 只接受小写：PostgreSQL 将 uuid 归一化为小写，大写变体会让丢失
  ack 后的合法重放批次误判 `sequence_conflict` 而永久卡死。
- 合约层拒绝孤立代理项（lone surrogate）字符串：此前能穿过白名单，在内存
  store 正常入库、在 PostgreSQL jsonb 报 500，两个 store 行为分叉且数据在
  Rust 侧无法解析。
- Gateway 验签对非法公钥的 `createPublicKey` 异常做防御性捕获，统一返回
  401 invalid_signature 而不是 500。
- Runner HTTP 客户端禁用重定向跟随：3xx 不再可能把请求重发到 HTTPS/回环
  策略未校验过的地址。
- 设备身份文件改为同目录临时文件 + 不覆盖式落位的原子写入，写入中途崩溃不再
  留下无法自愈的半写身份文件；Unix 侧补充目录 fsync。
- 标准化层只输出固定元数据白名单；测试夹具中的 Prompt、命令、预览、完整路径和
  敏感标记不得进入事件或 Outbox。
- Codex 初始化显式退出消息、推理、计划、Diff、命令输出等内容型通知，被动事件合并
  只允许标准化后的 Session 元数据进入 Outbox。
- 设备身份文件使用“仅新建”语义，禁止静默覆盖；Unix 创建权限为 `0600`。同步批次
  不包含私钥，篡改事件或批次元数据会导致签名校验失败。
- 同步确认必须匹配协议版本、设备和批次，且不得越过实际发送的序列范围，避免误删
  尚未发送的 Outbox 事件。
- Gateway 对每一层同步对象执行字段白名单，拒绝未知字段、未知 Schema、控制字符、
  非法时间、重复事件 ID、序列间隙、篡改签名和未注册设备。
- Claude Agent 名称和 Codex Thread 标题不再进入标准化事件；同步线协议移除
  Session `display_name`，防止默认上传可能来源于用户任务文本的标题。
- Runner 拒绝向非回环明文 HTTP 地址同步，也拒绝包含凭据、查询或片段的 Gateway
  URL；网络错误与服务端错误正文不会写入本地状态或用于清理 Outbox。
- PostgreSQL 批次接收以设备行锁和单事务保护，任何事件冲突、序列间隙或写入失败均
  整批回滚；迁移记录 SHA-256 并拒绝同版本 SQL 漂移。
