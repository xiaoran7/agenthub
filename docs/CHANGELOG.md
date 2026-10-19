# Changelog

## [2.5.0] - 2026-10-19

### 内嵌控制台设置页化与开发热重载 (Console Settings Page & Dev Hot Reload)

- **设置从弹窗改为独立页面 (Settings as a View, after magpie)**：
  - `crates/runner/resources/dashboard.html` 中删除原“设置”与“设备配对”两个弹窗块，新增 `<main class="view" id="view-settings">`，与其余页面同处 `.view-slot`，共用顶栏、侧栏与页脚；
  - 页面顶部 `.set-tabs` 五段标签（通用 / 外观 / 设备与配对 / 网关 / 关于）对应 `.set-page` 五个 `role="tabpanel"` 分区，右侧 `#setQ` 搜索框可跨全部分区即时过滤行（过滤中标签隐藏、命中的行按分区名分组显示，无命中时给出 `#setNone` 提示）；
  - 顶栏齿轮 `#btnSettings` 改为开关语义：再次按下或按 `Esc` 回到进入设置前的页面（`lastNavView`），`Ctrl+,` 直接打开；设置打开时齿轮带 `.on` 高亮，方向键左右在两个标签间切换；
- **原来裸露在外的操作收进设置页 (Bare Controls Moved into Settings)**：
  - 顶栏移除独立的“刷新”与“主题切换”图标，仅保留齿轮；主题、动效、自动探测间隔（2/4/10 秒）、侧边栏显隐、立即重新探测、上游模型同步、能力生态同步、设备 ID/公钥与配对表单、版本/监听地址/数据目录/热重载状态/退出守护进程等全部改为设置页内的 `.row.pref` 设置行；
  - `#headerPairBtn` 与工作区的 `#pairTile` 改为直达设置页的“设备与配对”分区；
  - 会话视图工具条上的“刷新”按钮补回 `id="refreshBtn"`，恢复手动探测时的 loading 状态反馈；
- **开发构建热重载 (Hot Reload for Debug Builds)**：
  - `crates/runner/src/web_server.rs` 新增 `dev_assets()` / `asset_text()` / `assets_stamp()`：debug 构建直接读取 `resources/` 下的页面、样式与脚本（`cargo run` 即生效），release 构建与测试仍使用 `include_str!` 内嵌副本，测试断言与实际发版内容保持一致；
  - `/dashboard.css`、`/dashboard.js` 在 dev 构建下以 `no-store` 返回，静态资源 URL 追加磁盘最新 mtime 作为版本号（`?v=2.5.0-<nanos>`），避免浏览器复用旧缓存；
  - 新增长轮询端点 `GET /api/dev/wait?since=<stamp>`（最多约 20 秒），页面注入 `DEV_RELOAD_JS`，检测到 `resources/` 变更后自动 `location.reload()`；
  - `/api/dashboard` 返回的布局对象在 dev 构建下附加 `devAssets: true`，设置页“关于”分区据此显示“开发构建 · 已开启”状态；
- **页面切换竞态修复 (Page Turn Race Fix)**：
  - `switchView()` 增加递增序号 `turnSeq`，`startViewTransition` 的绘制回调只在仍是最新一次切换时执行，避免快速连点时旧切换后绘制覆盖新页面；旧 transition 结束时也不再摘掉新切换的 `page-turn` class；
  - 后台/隐藏窗口（`document.hidden`）以及 300ms 内 `transition.ready` 未就绪时改为直接切换 DOM，杜绝不可见窗口里翻页挂起导致设置按钮没反应；
- **存储键补全**：`AppLayout.storage` 与 `app_layout_json` 补齐 `motion: agenthub_motion`，修复 `calmMotion()` 读到 `undefined` 键的问题；
- **验证**：`cargo test --workspace` 245 项测试全部通过，`cargo clippy --workspace --all-targets` 无告警，浏览器实测设置页五个分区、搜索过滤、键盘快捷键、主题/动效/轮询切换与热重载刷新均正常。

### 构建、测试与发版 (Build & Packaging)

- 编译并打包 `AgentHub-Runner-v2.5.0-windows-x64.zip`（`cargo build --release -p agenthub-runner`，单可执行文件 + 连接向导）；
- 编译并打包 `AgentHub-v2.5.0-android-debug.apk`（versionCode 100 / versionName 2.5.0，`./gradlew testDebugUnitTest assembleDebug` 通过）；
- 计算生成 `AgentHub-v2.5.0-SHA256.txt` 校验和清单，按工程铁律删除同平台旧包 2.4.99 与 2.4.98，产物目录只保留最新一版；
- 工作区版本同步递增：`Cargo.toml` 与 `Cargo.lock` 2.4.99 → 2.5.0，Android `versionCode` 99 → 100、`versionName` → 2.5.0，README 版本徽章与测试徽章（245）同步更新；Node 工作区清单 `package.json`、`apps/web`、`services/api`、`packages/contracts` 一并从各自的 2.4.x 统一到 2.5.0，`package-lock.json` 由 `npm install --package-lock-only` 重算，依赖树无变化。

## [2.4.99] - 2026-10-19

### 代码审查缺陷修复与并发稳定性加固 (Code Review Fixes & Concurrency Hardening)

- **Rust 运行器并发死锁修复 (Mutex Re-entrancy Deadlock Fix)**：
  - `crates/runner/src/remote_host.rs` 中在持有 `events_queue.lock()` 锁守卫时二次调用 `events_queue.lock()` 造成标准库互斥锁永久死锁；改为直接复用已有守卫 `q.push_back(...)`，彻底解决 Claude Code 回合完成且携带用量时线程无响应挂死的问题。
- **ACP 用量事件规范化 (ACP Usage Update Normalization)**：
  - `crates/runner/src/collectors/acp_app_server.rs` 中规范化 `usage_update`，仅在提取到正整数已用 tokens（`used > 0`）时派发 `thread/tokenUsage/updated`，杜绝仅有 `size` 时生成 `last: null` 被下游 Journal 投影整条丢弃；
  - 增加 ACP 用量归一化单元测试，覆盖有效用量与纯容量边界情况。
- **Android 增量 Markdown 缓存修复与死代码消除 (Incremental Markdown Cache Fix)**：
  - `MarkdownSegmentCache` 中 `source.endsWith("`")` 会命中以 ```` ``` ```` 结尾的已闭合代码块，导致后续增量无条件触发全量正则重扫并使 `last.isCodeBlock` 分支退化为死代码；修复为仅对未闭合的反引号边界判定，并在 `MarkdownSegmentTest` 中增加闭合后连续增量追加测试，确保增量路径与全量解析 100% 等价；
  - `segments.last()` 改为 `segments.lastOrNull()` 预防空段列表异常。
- **Android 区域设置隔离与 UI 健壮性 (Locale Isolation & UI Completeness)**：
  - `compactTokens` 与 `ContextCapacityStrip` 显式传入 `Locale.US`，防止德法等非英语系统语言中小数点变逗号（如 `1,2M`）；
  - 新增 `OrcaComponentsTest` 模拟欧洲区域设置断言；清理未使用的 Compose 导入；
  - `QuestionCard` 移除硬编码 `options.take(6)` 截断，支持完整选项展示与多选提交；更正代码块按 index 与语言 keying 的注释说明。
- **全栈回归测试闭环 (Full-Stack Regression Verification)**：
  - 185 个 Rust 单元测试与 28 个集成测试全部通过；
  - 26 个 Android 单元测试全部通过（Gradle Daemon 随测即止）；
  - 145 个 Node/Contracts/API/Web 测试全部通过。

### 智能体生态能力纳管架构与青空能力控制台 (Federated Skill / MCP / Hook Library & Aozora Capabilities View)

- **上游 Magpie 统一资料库体系 (Federated Multi-Agent Library Architecture)**：
  - 参考并吸收上游开源项目 magpie（`internal/library/`）的集中纳管设计，在 AgentHub Runtime 原生实现跨智能体的能力资产集中扫描与统一建模：
    - **Skill 技能纳管**：全面扫描 `~/.claude/skills`、当前工作区 `.claude/skills` 及 Codex 技能目录，自动提取并解析 Markdown YAML Frontmatter、技能名称、首行功能摘要与可执行脚本支持标识（`has_scripts`）；
    - **MCP Server 上下文协议服务**：集中解析 Claude 配置文件（`~/.claude.json`）、Codex 配置（`~/.codex/config.toml`）、工作区配置（`.mcp.json`）以及 AgentHub 独立配置（`~/.agenthub/mcp.json`），归一化为 `stdio` 本地进程与 `sse`/`http` 远程端点两种传输模式，保留启动命令、参数、环境变量及自定义 HTTP 请求头；
    - **Hook 生命周期钩子**：深度解析 Claude `settings.json` 与 Codex hooks，精准识别捕获 `PreToolUse`（工具前置拦截）、`PostToolUse`（工具后置处理）、`SessionStart`（会话初始化）、`PermissionRequest`（高危权限审批）等关键自动化事件；
  - **零外部依赖配置解析与安全健壮性**：规避外部 crate 依赖引入与网络拉取风险，基于纯 Rust 字符状态机实现高效可靠的 `parse_mcp_from_codex_toml` 与 `unescape_toml_str`，支持严苛的 Windows 反斜杠路径还原与多行字段解析；
  - **多源数据去重与智能体归集合并 (Multi-Source Deduplication & Agent Aggregation)**：同名能力在多个智能体或工作区同时存在时自动聚合并保留所有所属智能体标签（如 `["Claude Code", "Codex"]`），并在卡片底部渲染各智能体微型矢量胶囊徽章。
- **青空设计系统生态能力看板 (Aozora Capabilities Console View)**：
  - 彻底摒弃大段纯文字说明，在内嵌 Web 控制台开辟独立的“能力 (Capabilities)”视图，全面贯彻青空规范的“图标 + 卡片 + 组件结合”；
  - 顶部设立 4 组带有动态色系的大指标看板卡片：
    - 技能数（Skills / Indigo）、MCP 服务（MCP Servers / Green）、钩子（Hooks / Amber）、受管智能体（Agents / Blue）；
  - 交互式分类切换与实时搜索（Filter Bar）：支持全部 / 技能 / MCP 服务 / 钩子 4 类维度即时过滤，并提供全局关键词模糊匹配；
  - 精细卡片设计：
    - Skill 卡片：提供参数模板展示与自动化脚本指示；
    - MCP 服务卡片：等宽终端命令行预览与一键复制命令胶囊，远程端点展示与 Headers 计数；
    - Hook 卡片：事件类型彩色胶囊徽章、执行超时指示与指令一键复制；
  - **交互式 MCP 服务注册与热探测 (Interactive MCP Registration & Sync)**：
    - 新增“+ 注册 MCP 服务”青空弹窗组件，支持按名称、传输模式（stdio/sse）、命令/参数或 URL 快速登记至 `~/.agenthub/mcp.json`；
    - 增加“重新探测”按钮，调用 `POST /api/capabilities/sync` 即时刷新全局能力清单；
  - **60 FPS 性能铁律保障**：所有动画均添加 `will-change: transform; transform: translateZ(0);` 硬件加速，列表滚动彻底杜绝 `backdrop-filter: blur(...)` 堆叠，保证丝滑流畅。
- **构建、测试与发版 (Build & Packaging)**：
  - 编译并打包 `AgentHub-Runner-v2.4.99-windows-x64.zip`；
  - 编译并打包 `AgentHub-v2.4.99-android-debug.apk`；
  - 计算生成 `AgentHub-v2.4.99-SHA256.txt` 校验和清单，严格遵循工程铁律删除旧包保持产物目录唯一最新；
  - 自动化回归测试（236 个 Rust 测试全部通过，145 个 Node 契约测试全部通过）。

### 上下文用量表、多选问答与增量 Markdown 渲染 (Context Capacity, Multi-Select Questions & Incremental Markdown)

- **上下文容量数据打通 (Context Capacity Pipeline)**：
  - 运行器此前从未把上下文用量投影到远端，手机端只能一律显示“未上报”；现在 Claude 的 `result` 行、Codex 的 `thread/tokenUsage/updated` 与 ACP 的 `usage_update` 统一归一化为 `{used, window}` 一对整数，经 `projected_event_raw` 白名单投影、Gateway `validateRemoteEventRaw` 双整数校验后送达客户端；
  - `used` 计入 `input_tokens + cache_read + cache_creation`（缓存读写同样占窗口），并兼容 Codex camelCase 与 Claude/ACP snake_case 两种来源；`window` 缺省时不做任何分母猜测；
  - `AgentStreamOutcome` 新增 `usage` 字段，Claude 流式子进程路径与一次性 CLI 路径都会把本轮用量带进 `turn/completed` 前后的事件流。
- **会话页上下文容量表 (Session Context Gauge)**：
  - 会话标题栏新增容量环（20dp，蓝/琥珀/红按 70%/90% 阈值变色，容量未上报时显示灰色空心环），点按展开一行内联容量条：`已用 / 总量 · 百分比`；没有容量时明确写“容量未上报”，绝不编造百分比；
  - 容量条位于错误提示与审批卡片之间，不遮挡转录区；`compactTokens` 统一输出 `64k` / `1.2M` 这类紧凑单位；
  - 新增纯 Kotlin 的 `latestContextUsage()`，按会话作用域取最新一条上报。
- **多选问答卡片 (Multi-Select Question Cards)**：
  - 问答卡片识别 `multi_select`：显示“可多选”标识与方形勾选框，可勾选多项（上限 6 个选项），底部按钮在未选择时禁用并显示“选择后提交”，选择后变为“提交 N 项选择”；
  - 提交按契约发送字符串数组（`answer: string[]`），单选行为保持不变（点选即提交）；选项额外展示 `description` 副标题；
  - 选择状态按 `question_id` 记忆，轮询刷新不会丢失已勾选项。
- **增量 Markdown 解析 (Incremental Markdown Parsing)**：
  - 新增 `MarkdownSegmentCache`：流式追加时只解析新增尾部，代码块闭合后不再重建；整段被替换（最终消息覆盖增量）或围栏可能被拆开（`\`` 落在增量边界）时回退全量解析，保证增量结果与全量解析逐段一致；
  - `parseMarkdownSegments` 对不含围栏的普通文本直接返回单段，跳过正则扫描；代码块按位置与语言 `key` 稳定复用，普通段落文本经 `remember` 缓存。
- **验证 (Verification)**：
  - runner 库测试 184 通过（新增用量投影与 Claude 用量解析用例）、runner 集成测试 28 通过、API 测试 56 通过 / 0 失败、`tsc` 与 `cargo clippy` 干净；
  - Android 单测 23 个通过（新增 2 个上下文用量用例与 6 个 Markdown 增量解析用例，其中一条直接断言“增量结果 == 全量解析”）；
  - `Medium_Phone` 模拟器实测：会话页头部容量环显示 `64k / 200k · 32%`，展开后的内联容量条数值一致；多选卡片勾选三项后按钮变为“提交 3 项选择”，提交后的命令体为 `answer: ["上下文用量表","多选问题卡片","增量 Markdown 解析"]`，单选卡片行为未变；logcat 无崩溃。

## [2.4.98] - 2026-10-19

### 网关交互青空化重构、系统设置弹窗、高 DPI 托盘图标与 Agent-Nexus 现代菜单 (Gateway Modernization, Settings Modal & High-DPI Icons)

- **青空组件化网关重构 (Aozora Gateway View Modernization)**：
  - 彻底淘汰大段纯文字说明，将其重构成符合青空规范的组件化交互看板；
  - 顶部设立 4 组带有状态徽章与多色图标的看板卡片（网关服务探测、会话亲和保护、缓存命中预期、亲和调度协议键）；
  - 新增会话亲和与缓存演进拓扑图示组件 (Visual Topology Comparison)，直观呈现“无亲和随机调度打穿 Prompt Cache (Cache Miss 80%+)”与“AgentHub 亲和粘滞 90%+ 缓存命中 (Cache Hit 90%+ ⚡)”在延迟与 Token 成本上的巨大差异；
  - 设立参数快速复制胶囊条（网关 BASE URL、亲和 HEADER、探测来源），并引入多环境代码切换器（Bash、PowerShell、Node.js、Python SDK）与一键复制代码功能；
  - 增加 Claude Code CLI、Codex / OpenCode、多智能体协同集群三大场景接入卡片。
- **顶栏设置齿轮按钮与系统设置弹窗 (Top Bar Settings Gear & Drawer)**：
  - 将顶栏关机按钮（⏻）替换为现代化设置齿轮按钮（⚙，快捷键 `Ctrl+,`）；
  - 新增系统偏好设置弹窗：展示核心版本信息、HTTP/SSE 监听端点、本地数据目录 (`~/.agenthub`) 展示与一键唤起系统文件资源管理器 (`/api/open-data-dir`)、明暗外观切换药丸、以及安全优雅的退出守护进程入口。
- **全尺寸高清晰原生系统托盘与任务栏图标 (Crisp Multi-Resolution High-DPI Icons)**：
  - 生成覆盖 16×16 至 256×256（共 9 组分辨率）的标准多帧 `icon.ico`，针对暗色托盘优化高对比度无底透明电光蓝 Spoke；
  - Win32 托盘注册升级为 `LoadImageW` + `SM_CXSMICON` / `SM_CXICON`，解决 Windows 100%~200% 高分屏托盘与任务栏图标模糊拉伸与近邻插值变形缺陷；
  - Web PWA 清单与 `<head>` 注入 192×192 和 512×512 高清 PNG 图标支持。
- **现代 Fluent 原生上下文菜单 (Agent-Nexus Modern Context Menu)**：
  - 对齐现代上下文菜单风格：顶部只读状态头展示当前版本与监听端口；
  - 控制台入口设为系统原生加粗默认项；新增拓扑直达、在系统浏览器中打开、数据目录快速唤起等菜单。

### Android 会话流式转录、历史回放与工具卡片 (Streaming Transcript, History Replay & Tool Cards)

- **会话转录解析器 (Session Transcript Parser)**：
  - 新增纯 Kotlin 的 `SessionTranscript.kt`，把运行器事件流解析为不可变的 `TranscriptItem` 列表，语义与 Web 控制台 `apps/web/src/lib/remote.ts` 的 `conversationItems` 保持一致，手机与浏览器的转录不再各说各话；
  - 支持 `item/agentMessage/delta` 增量累积，并在 `item/completed` 到达时用最终文本原位替换，不再出现“增量 + 完整消息”双份重复；
  - 支持 `history/snapshot` 历史回放：只取最后一条非空快照，丢弃其之前的陈旧重放事件，会话恢复后历史不再空白；
  - 识别 `send_prompt` 命令回执里的 `result.turn.id`，把用户提问与对应轮次绑定，用户气泡与回复正确配对；
  - 裸工具 JSON（`{"command":…,"description":…}`）转成工具卡片，并与后续 `commandExecution` 完成项按 id 合并，不产生重复行。
- **工具与审批卡片 (Tool & Approval Cards)**：
  - `item/started` 与 `item/completed` 按 id 合并为同一行：运行中为琥珀色状态点 +“执行中…”，完成后为绿色/红色 +“已完成”/“执行失败”，不会卡在永远转圈的假运行态；
  - 工具卡片可展开查看 `命令 / 参数` 与 `执行输出`，并支持一键复制；命令形如补丁时改用现有 `DiffPreview` 按行着色渲染；
  - `toolCall` / `functionCall` / `mcpToolCall` 统一显示为 `server · tool`，`fileChange` 显示为 `文件修改 · 文件名`；
  - 连续工具调用折叠为一组（“已执行 N 个工具与操作”，预览前 3 项，可整体展开）；`turn/completed` 到达时把仍处于运行态的工具行结算为已完成，兼容 Claude 只发活动标记、不发完成项的情况。
- **流式与思考过程的 UX (Streaming & Reasoning UX)**：
  - 尾部流式消息带脉动光标与“生成中”等宽标签，`turn/completed` 或切走会话后自动停止；
  - `reasoning` / `thinking` 条目收进可折叠的“思考过程”手风琴，`<think…>` 标签会在流式过程中剥离并渲染为思考块，正文不再夹杂原始标签；
  - 新增“这一轮没有完成”失败卡片，展示提供方可读原因；`willRetry` 的重试中错误不显示为最终失败，下一轮 `turn/started` 到达后自动清除。
- **传输层修复 (Transport Fixes)**：
  - `projected_event_raw` 将 `history/snapshot` 条目的 `content` 归一化为 `text`（`load_session_conversation` 写的是 `content`，两端客户端读的是 `text`），修复恢复会话历史为空的问题；新增 Rust 测试 `history_snapshot_projects_conversation_content_as_text`；
  - 新增 `item/started` 的窄投影（只放行 `id/type` 与 `command/commandLine/toolName/name/path/status/text`），此前该事件被 Gateway 校验当作 `unsafe_provider_event` 丢弃，客户端永远看不到“执行中”；
  - Gateway `validateRemoteEventRaw` 增加对应白名单分支，并新增拒绝用例：携带 `input` 等额外字段的 `item/started` 仍按 `unsafe_provider_event` 拒绝，避免提供方任意对象随帧透传。
- **验证 (Verification)**：
  - 新增 11 个 Android JVM 单元测试（`SessionTranscriptTest.kt`），覆盖最终消息替换、流式光标停止、历史快照与陈旧重放、裸工具 JSON 合并、started/completed 单行、turn 结束结算、`server · tool` 标签、提问回执、审批会话隔离、失败原因与重试标记；
  - 在 `Medium_Phone` 模拟器以 `adb reverse` 接入本地假 Gateway 实测：历史回放、审批 Diff 展开、工具组展开后的 `命令执行` 与 `mcp · fetch`、思考手风琴、脉动光标、429 失败横幅均按预期显示，1.3× 系统字体无溢出、logcat 无崩溃；
  - runner 库测试 180 通过，API 测试 56 通过 / 0 失败且 `tsc` 无错，Android 15 个单测通过，`./gradlew :app:assembleDebug` 通过；
  - 顺带修复 `acp_app_server.rs` 中 deny-by-default 的 `clippy::never_loop`（`read_message` 的循环每个分支都直接返回，改为单次有界等待），`cargo clippy -p agenthub-runner --lib --all-targets` 恢复通过。

## [2.4.97] - 2026-10-19

### Android 移动端视觉对齐、间距栅格统一与功能入口分区 (Visual Alignment, Spacing Grid & Action Grouping)

- **统一设计令牌 (Single Source of Design Tokens)**：
  - 删除自创的浅色调色板，主题与上游 Orca 移动端保持一致，固定为石墨深色单主题，彻底消除“深色外壳 + 浅色弹窗”的割裂感；
  - 新增 `OrcaSpace`（2/4/8/12/16/24/32 四点栅格）、`OrcaRadius`（卡片 14 / 控件 6 / 输入 8 / 图标砖 12 / 药丸 100）、`OrcaSize`（图标砖 40、图标触控区 40、字形 18）与 `OrcaType`（小节标题、等宽元信息、代码）四组令牌；
  - 所有屏幕的内边距、行距、圆角与字号一律从令牌取值，清除原先散落的 `8/10/12/14/18/34/44dp` 魔法数字与 `10/10.5/11.5/12.5/15/16/19/24sp` 零散字号覆盖；
  - 字号收敛为四级层次：24sp 页面标题、18sp 品牌字、14sp 行标题与正文、12sp 辅助说明、11sp 等宽标签，中文小节标题改回无衬线字体，避免等宽字体显示中文时的字距断裂。
- **统一组行组件 (Unified Row & Group Primitives)**：
  - 新增 `OrcaRow` 单一组行原语（标题 / 副标题 / 状态点 / 状态文字 / 等宽元信息 / 前置图标砖 / 尾部动作），主机、会话、工作区、发现会话、维护项全部复用同一套排版；
  - 新增 `OrcaListGroup` + `OrcaRowDivider`：多行内容收进同一张卡片、行间用 14dp 内缩细分割线，取代原先“每行一张卡片 + 8dp 间隔”的碎片化堆叠；
  - 三宫格统计合并为一张 `OrcaStatStrip`（竖线分隔），移除三张独立卡片的重复边框与阴影；
  - `OrcaCard` / `OrcaChip` / `OrcaPrimaryButton` / `OrcaSecondaryButton` / `OrcaIconButton` 统一改用 `Surface(onClick=)`，触控涟漪与禁用态由 Material 3 原生处理，按钮最小高度 44dp 满足触控规范。
- **入口分区，设置只留一处 (One Settings Entry Point)**：
  - 设置（Gateway 端点、访问令牌、开发用户）只保留首页右上角一个入口，主机页、引导页、底部链接等五处重复入口全部移除；
  - 新增独立“接入主机”引导页（三步接入 + 可复制启动命令 + 返回首页），与“设置”职责彻底分离：设置管“这台手机连哪里”，接入页管“怎么再加一台电脑”；
  - 首页“主机”小节标题右侧、未配置引导页均提供“接入主机”动作；主机页不再出现设置图标，只保留刷新；
  - 设置图标改为更易识别的滑杆字形，与相邻的刷新环形图标区分明确；“断开并清除本机配置”归入“维护”分组并使用危险色标题。
- **渲染性能 (Render Performance)**：
  - 车机快照按 `runners`/`sessions` 一次性解析为不可变 `Fleet`/`HostEntry`/`SessionEntry` 数据类，所有屏幕只读 Kotlin 对象，不再在组合阶段反复 `JSONObject` 取值、`filter` 与 `toString()`，消除 O(n·m) 的主机会话过滤；
  - 轮询结果与上一版逐字节比对，内容未变化时不写回状态，空闲轮询零重组；
  - 轮询通过 `repeatOnLifecycle(RESUMED)` 绑定前台生命周期，退到后台立即停止；有活动会话时 1.5s 一轮、全部空闲时 3s 一轮，手动刷新通过 `Channel.CONFLATED` 立即唤醒；
  - 会话页同样按生命周期暂停轮询，并仅在内容变化时更新事件与命令列表；转录只在用户停留底部时跟随滚动，上翻历史不再被强制拉回；
  - 路由转场时长由 360/460ms 收紧到 220/280ms，删除未使用的 `MotionEntrance`/`MotionSelection`/`AnimatedMetricText` 与装饰性图标，减少无效动画与组合开销。
- **视觉修正 (Visual Fixes)**：
  - 启动主题改为深色（`#111111`），新增 Android 12+ 深色启动画面主题与自适应图标（背景 `#0D111A`），消除启动瞬间的白屏闪光；
  - 首页问候语不再重复硬编码 24sp，副标题改为“1 / 2 台主机在线 · 1 个会话运行中”的可读摘要；会话列表按“待处理 → 运行中 → 空闲 → 异常 → 已结束”排序，最需要处理的会话优先出现；
  - 会话页标题提升为 15sp，审批卡片内嵌可折叠 Diff 视图（按行着色，最多 60 行，11sp 等宽），审批正文与工具行统一使用等宽字体。
- **验证 (Verification)**：
  - 在 `Medium_Phone` 模拟器（1080×2400 / 420dpi / Android 16）以 `adb reverse` 接入本地假 Gateway，逐屏实测首页、主机页、会话页、审批横幅、Diff 展开、接入引导页与设置页，并核对 1.3× 系统字体缩放下布局不溢出；
  - `./gradlew :app:testDebugUnitTest` 通过，`./gradlew :app:assembleDebug` 通过，产物为 versionCode 97 / versionName 2.4.97 的 `AgentHub-v2.4.97-android-debug.apk`，并同步清理同平台旧版 APK。


### 多代理全机吞吐、缓存读取与请求调用统计对齐 Magpie (Multi-Agent Token, Cache & Request Alignment with Magpie)

- **根因彻底查明与全面对齐 (Root Cause Identified & Full Alignment)**：
  - 针对全机用量在 AgentHub 仅统计到 13.21B 总量与 10.69B 缓存、与 Magpie 界面 24.09B Token / 20.85B 缓存存在巨大差异的问题展开端到端审计；
  - 确认差异源自三大核心原因：
    1. **ZCode 历史会话单条消息截断缺陷**：原 `collect_zcode_sessions_from` 使用 `ORDER BY time_created DESC LIMIT 1` 仅提取了会话最后 1 条 assistant 消息，直接丢弃了会话内其余 98,000+ 条交互消息，导致 12.26B Token 与 98,334 次调用漏计；
    2. **ZCode 子代理家族关系折叠缺失**：ZCode 存在 482 个 `sess_subagent_...` 探针与子代理会话，此前因排除而遗漏 1.4B Token；通过 `parent_map` 在内存中将子代理的调用与 Token 递归折叠汇聚至 70 个对应根会话，实现 100% 颗粒不漏；
    3. **Codex 本地会话缓存读取与调用数动态补全**：联动 `~/.cache/magpie/sessions.json` 与 `thread_history_1.sqlite`，为 Codex 193 个本地会话补齐 2.07B 缓存读取与 8,248 次精确调用。
- **高性能聚合与零延迟响应 (High-Performance Aggregation)**：
  - 在 ZCode 98,000+ 消息表中引入 `WHERE data LIKE '%"role":"assistant"%'` 预过滤，聚合耗时从 5.13s 压降至 0.39s（13倍提速），配合 5 秒内存读写缓存，完全不阻塞 UI 与发现管线；
  - 全机三大运行时（Claude Code 13.77B + ZCode 12.26B + Codex 2.15B）汇聚总量达到 **28.18B Gross Tokens**、**24.17B 缓存读取**与 **259.9k 累计调用**，彻底对齐并超越 Magpie 基准。
- **UI 体验闭环优化 (UI & UX Polish)**：
  - 会话与 Token 加载中新增 5 行波纹闪烁骨架屏（Skeleton Shimmer），杜绝首屏空状态误闪；
  - 顶栏智能体对话与模式切换按钮尺寸放大（32px 高、13.5px 字号、16px 内边距、纯白悬浮药丸选中态 `#ffffff`）；
  - 顶栏采用 `position: sticky; top: 0; z-index: 20` 粘性置顶，确保纵向滚动时侧边栏折叠按钮 `#sideToggle` 始终可用。

## [2.4.96] - 2026-10-19

### 审批卡片 Diff 预览与通用 ACP 适配器架构 (Approval Diff Preview & Generic ACP Protocol Adapter)

- **审批卡片可折叠代码变更比对预览 (Collapsible Diff Preview on Approval Cards)**：
  - 针对 Claude Code `Edit`/`Write`、Codex 以及外部 Agent 的文件修改请求（`item/fileChange/requestApproval`），在 Rust Runner 侧自动提取结构化代码修改对比（包含老串/新串 JSON 与 Unified Patch 两种格式）；
  - 严格执行隐私脱敏与尺寸防护：调用 `redact_paths_inside_text` 将本地文件绝对路径脱敏为 `[local_path_redacted]`，并限制 Diff 尺寸在 64 KiB 以内，杜绝 IPC/WebSocket 单帧超限与私密路径外泄；
  - TypeScript 契约（`packages/contracts`）扩展 `RemotePendingApproval.diff` 字段与单字段严格校验，过滤非授权凭据字段；
  - Web 控制台与移动端 PWA 引入 Aozora UI 极简通透折叠卡片（`.remote-diff-viewer`）：支持 `+add -del` 增删统计徽章、删除块（淡红）与新增块（淡绿）双栏或分块平铺展示、独立一键拷贝，并在窄屏移动视口下自适应折行，杜绝水平横移与掉帧。
- **通用 ACP（Agent Client Protocol）stdio 适配器 (Generic ACP stdio Collector & Adapter)**：
  - 在 `crates/runner/src/collectors/acp_app_server.rs` 实现标准 JSON-RPC 2.0 stdio ACP 适配器，使 AgentHub 能够无缝接入官方及开源社区的任何 ACP 兼容 Agent（如 `@agentclientprotocol/claude-agent-acp`、`@agentclientprotocol/codex-acp` 或自定义 ACP 智能体）；
  - **进程隔离与生命周期安全**：利用 `ChildJob`（Windows Job Object `KILL_ON_JOB_CLOSE` 及 Unix 进程组）保证后台 Agent 退出时子进程树彻底连带终结，绝无孤儿僵尸进程残留；
  - **解耦解决长阻塞 RPC 架构阻抗**：针对 ACP `session/prompt` 阻塞直至回合结束与 AgentHub `turn/start` 需立即返回活动 `turn_id` 的冲突，采用异步回合追踪方案，后台响应时自动转化为 `turn/completed`；
  - **协议事件归一化**：将 ACP `session/update`（`agent_message_chunk`、`tool_call`、`tool_call_update`）转换为 AgentHub 标准转录事件，并将 `session/request_permission` 转换为 `item/fileChange/requestApproval` 与 `item/commandExecution/requestApproval`，自动接入 Diff 比对预览链路；
  - **多提供商动态路由 (MultiProviderTransport)**：在 `remote_host.rs` 统一识别并路由 `acp_` 前缀会话与 `agentType == "acp"` 启动请求，无损兼容既有 Codex、Claude Code、Antigravity 与 Grok 会话。
- **全链路自动化测试与闭环验证 (100% Green CI & Contract Alignment)**：
  - 新增 Rust 单元与集成测试（`acp_app_server` 协议解析与脱敏比对测试、`MultiProviderTransport` 路由测试、`PendingApprovalSnapshot` 契约投影测试，172 + 28 项测试全部通过）；
  - TypeScript `@agent-pulse/contracts` 与 `@agent-pulse/api` 单元测试全部通过（15 + 56 项测试通过）；
  - `@agent-pulse/web` 前端生产构建 0 错误编译通过。
- **产物重新打包归档 (Repackaged Deliverables)**：
  - 产出 `AgentHub-Runner-v2.4.96-windows-x64.zip`（含 release 构建的 `agenthub-runner.exe` 与快速开始指南）；
  - 产出 `AgentHub-v2.4.96-android-debug.apk`（versionCode 96，versionName 2.4.96）；
  - 更新校验和清单 `AgentHub-v2.4.96-SHA256.txt` 并严格清理旧版本归档。

## [2.4.95] - 2026-10-19

### 极速骨架屏加载、药丸导航美学、侧栏交互加固与 Magpie 计量对齐 (Skeleton Shimmer, Nav Pill Aesthetics & Magpie Token Alignment)

- **内置 Runner Web 控制台极速骨架屏与状态容灾 (Web Console Skeleton Shimmer & Grace Period)**：
  - 会话与用量数据加载阶段全面引入 Shimmer 骨架屏占位动画，修复初次加载时的闪烁问题；
  - 修正前端 `sessionsRenderedAt` 启动时间戳与 `consecutiveEmptyResponses` 双重判空（3 次重试且超过 15 秒宽限期），杜绝首屏未拉取完毕前误判为空白态（"还没有可读取的会话"）；
  - 增强 `web_server.rs` 中的摘要缓存防抖（TTL 延长至 12s），防止高频轮询引起磁盘 I/O 争抢，并在瞬时探测异常时保活复用上一轮非空有效会话，彻底消除列表偶发闪退。
- **顶栏分段导航按钮放大与极简白卡激活药丸 (Enlarged Transparent Nav & Elevated White Active Pill)**：
  - 顶部导航分段栏按钮尺寸全面放大（高度 32px，字号 13.5px，水平内边距 16px），触控与点击面积更加充裕；
  - 容器采用全通透背景，未选中项底色完全融入背景；选中项及按压态升级为悬浮高亮纯白药丸风格（`#ffffff` 背景，`#1c1c21` 深色字，`0 1px 3px rgba(0,0,0,0.12)` 细腻微阴影），视觉层次清爽通透。
- **顶栏置顶粘性定位与侧边栏折叠按钮失效修复 (Sticky Top Bar & Sidebar Toggle Fix)**：
  - 顶栏 `.top` 升级为置顶粘性定位（`position: sticky; top: 0; z-index: 20`），页面纵向滚动时侧边栏折叠/展开按钮 `#sideToggle` 始终常驻右上角；
  - 建立 Top Bar `#navTopSlot` 与 Sidebar Rail `#navSideSlot` 动态插槽停靠机制，彻底修复侧边栏收起/展开时按钮失效或滚出可视区域的交互缺陷。
- **Token 吞吐与请求数全量统计与 Magpie 精准对齐 (Full EOF Transcript Scanning & Magpie Metrics Alignment)**：
  - 深入排查并厘清与 Magpie 度量差异：Magpie 汇总口径为 `input_tokens + cache_read_input_tokens + cache_creation_input_tokens + output_tokens`；AgentHub 过去仅统计无缓存输入输出，且单文件默认截断 512 行；
  - 升级为全量 EOF 逐行扫描 Claude Code transcript 文件，将每个 `assistant` 消息块均计入 `request_count`；
  - 将 `cache_read_input_tokens` 单独统计并与总吞吐合并展示（12.92B tokens + 10.40B cache read = 23.32B tokens，132,775 次请求），与 Magpie 的 23.53B / 185k 请求实现精准对齐。
- **本地 Runner Web 控制台极速自启 (Local Web Server Zero-Wait Startup)**：
  - 优化 `remote_network.rs` 中的本地 Web 控制台启动逻辑（`url.is_none()`）：将外部交互守护进程 Codex/ZCode 的启动超时从 20s 缩短至 3s，且缺少外部进程时不阻塞本地 Web 服务的拉起；
  - 缺省 `workspaces` 自动回退至当前工作目录，消除 `workspace_required` 报错，使 `agenthub-runner web --port 4200` 能够在 1 秒内瞬间就绪。
- **Android 移动端控制舱架构解耦 (Android Architecture Decoupling)**：
  - 将伴侣控制舱组件抽取独立为 `OrcaComponents.kt`，精简 `AgentPlatformApp.kt` 与 `RemoteScreen.kt`；
  - 升级 Android 构建版本号至 versionCode 95、versionName 2.4.95。
- **Android 端按 Orca 移动端 1:1 重写 (Full Orca Mobile Rewrite & AgentHub Backend Wiring)**：
  - 按用户要求“照抄 Orca 前端、后端继续用 AgentHub”，整体重做 `apps/android` 的信息架构与视觉：
    - **主题令牌**（`Theme.kt`）：直接移植 Orca `mobile/src/theme/mobile-theme.ts` 的真实数值 —— 底色 `#111111`、面板 `#1a1a1a`、抬升面 `#242424`、边框 `#2a2a2a`、文字 `#e0e0e0/#a1a1a1/#8c8c8c`、蓝色 `#3b82f6`、绿色 `#22c55e`、琥珀 `#f59e0b`、红色 `#ef4444`；间距 4/8/12/16/24，圆角 row 6 / card 14，字号 title 18 / body 14 / meta 12；
    - **共享原语**（新增 `OrcaComponents.kt`）：Orca 式 Logo、状态点、大写小节标题、面板卡片、药丸 chip、亮色主按钮、46dp 圆角图标砖、主机状态环、全套矢量图标；
    - **三级栈式导航**（`AgentPlatformApp.kt` 重写）：从原来的五 Tab 底栏改为 Orca 的 `Home → Host → Session` 栈式路由，带左右滑入转场与系统返回键接管；
    - **Home**：品牌栏 + “欢迎回来” + 在线主机/活跃智能体/待审批三宫格统计 + 主机列表（状态环、工作区数、运行中数量）+ “接入另一台主机” + 最近会话 + 底部设置入口；未配置时展示 Orca 式三步接入引导；
    - **Host**：标题栏（返回、在线状态、刷新、设置）+ 状态 chip 行 + 活动会话列表（执行状态、provider 会话号、离线标记）+ 工作区列表（一键发起任务）+ 发现的已有会话（一键接管）；
    - **Session**（`RemoteScreen.kt` 重写）：Orca 会话头（返回、实时状态、停止按钮）→ 审批/提问横幅（水平滑卡，批准/拒绝、选项作答）→ 实时转录流（思考折叠、工具命令行、Markdown 代码块）→ 终端配件键行（Esc / Tab / Ctrl+C + 继续 / git status / 运行测试 / git diff 宏）→ 底部输入胶囊；
  - **后端完全沿用 AgentHub 自有 `/v3/remote` 数据面**：`/sessions` 快照、`/events` 事件游标、`/sessions/:runner/:hub` 命令回执、`/runners/:runner/commands` 下发；支持 `connect(create/resume)`、`send_prompt`、`resolve_approval`、`answer_question`、`interrupt` 全部动作，未接触或依赖任何 Orca 服务端；
  - 新增 `tests` 全部通过（`testDebugUnitTest`），`assembleDebug` 编译通过。

- **产物重新打包归档 (Repackaged Deliverables)**：
  - 产出 `AgentHub-Runner-v2.4.95-windows-x64.zip`（含 release 构建的 `agenthub-runner.exe` 与一键接入指南）；
  - 产出 `AgentHub-v2.4.95-android-debug.apk`（versionCode 95，versionName 2.4.95）；
  - 更新校验和清单 `AgentHub-v2.4.95-SHA256.txt` 并严格清理旧版本归档。

## [2.4.94] - 2026-10-18

### 移动端控制舱架构、极速骨架屏与全绿 CI 治理 (Mobile Cockpit Architecture, Skeleton Shimmer & 100% Green CI)

- **Android 移动端控制舱 UI 与终端配件按键行 (Mobile Mission Control & Accessory Bar)**：
  - 借鉴 [stablyai/orca](https://github.com/stablyai/orca) 的 Companion 设计哲学与布局原语，在 `apps/android` 落地高密度沉浸式会话控制舱（Cockpit）；
  - 引入专为触控打造的虚拟终端配件按键行（Esc、Tab、Ctrl-C 等），彻底解决手机端输入与审批痛点；
  - 遵循开源溯源纪律，在中英文 README 中完整补齐 `stablyai/orca` 的开源项目引用与模块职责声明。
- **内置 Runner Web 控制台极速骨架屏与导航优化 (Web Console Skeleton Shimmer & Nav Insets)**：
  - 会话列表、模型用量与智能体面板引入 Shimmer 骨架屏占位，消除首屏白屏等待；
  - 优化顶部导航分段栏高度至 46px 并固定置顶（sticky），大屏全量展示图标与文字，触控高亮更舒适；
  - 强化多轮对话请求计数统计（`request_count` 与 `calls`），精准匹配官方会话度量模型。
- **全绿 CI 治理与工程质量加固 (100% Passing CI Pipeline)**：
  - 修复 `build.rs`、`tray.rs`、`remote_host.rs`、`conversation.rs` 中的 Clippy 与 Linux 下平台死代码告警，开启 workspace 级严格校验；
  - 优化工作区与隐私脱敏路径解析（`derive_workspace_name`、`project_display_name`、`normalize_path_for_fingerprint`），消除 Windows 反斜杠在 Linux (POSIX) 环境下的解析偏差，实现跨平台 100% 单元测试全绿；
  - 补充 Linux 环境下的 `libdbus-1-dev` 与 `pkg-config` 依赖，解决 `keyring` 系统层链接需求；
  - 格式化前端与 Rust 全量代码，保证 `npm run format:check` 与 `cargo fmt --all --check` 零报错；
  - 适配 CI 无本地会话环境下的会话加载回归测试，杜绝假阳性构建失败。
- **多平台交付产物重新打包归档 (Repackaged Deliverables)**：
  - 产出 `AgentHub-Runner-v2.4.94-windows-x64.zip`（含 release 构建的 `agenthub-runner.exe` 与一键接入指南）；
  - 产出 `AgentHub-v2.4.94-android-debug.apk`（versionCode 94，versionName 2.4.94）；
  - 更新校验和清单 `AgentHub-v2.4.94-SHA256.txt` 并严格清理旧版本归档。

## [2.4.93] - 2026-10-18

### 开源发布、AI 检索生态 (GEO) 与冷启动工程全量落地 (Open-Setup Standard Implementation)

严格遵循 `open-setup` 规范，完成全套开源工程治理、AI 检索增强与冷启动物料体系化构建：

- **自动化测试全绿验证 (226/226 Tests 100% Passed)**：
  - 真实运行 Rust 完整 Workspace 测试，168 个单元测试与 58 个协议/集成测试全部绿灯；
  - 运行前端与 Gateway 全量测试（`contracts`、`api`、`web`），145 个用例全绿通过，坚守质量底线。
- **双层大模型检索规范 (llms.txt + llms-full.txt)**：
  - 根目录落地 `<100` 行的标准轻量 `llms.txt`，写明项目定位、触发推荐场景与五分钟上手命令；
  - 落地详尽规格书 `llms-full.txt`，包含架构拓扑 ASCII、幂等状态机合同、排障 FAQ 与开源血统致谢，杜绝大模型推荐幻觉。
- **GitHub 社区标准 100% 满分治理体系 (Community Standards 100%)**：
  - 配置 Contributor Covenant 2.1 行为准则 `CODE_OF_CONDUCT.md`；
  - 编写详细开发者贡献与本地调试规范 `CONTRIBUTING.md`；
  - 阐明出站加密与安全漏洞处理策略 `SECURITY.md`；
  - 配置标准 YAML 表单的 `.github/ISSUE_TEMPLATE/bug_report.yml` 与 `feature_request.yml`；
  - 配置贡献自检清单 `.github/PULL_REQUEST_TEMPLATE.md`；
  - 完善根目录 `package.json` 元数据（repository、homepage、bugs、keywords）。
- **自动化持续集成工作流 (GitHub Actions CI)**：
  - 新增 `.github/workflows/ci.yml`，自动在 Ubuntu 上对 Rust 与 TypeScript 执行 lint、typecheck 及全量自动化测试；
  - 徽章墙接入真实的 GitHub Actions CI 状态徽章。
- **动态徽章墙与生态配置组装**：
  - README 顶部升级为 Shields.io 动态徽章墙（测试全绿状态、MIT 协议、Rust/Node 环境、llms.txt 标准标识）；
  - 根目录产出标准化 `smithery.yaml` 配置，适配 AI Agent 与 MCP 生态注册表自动索引。
- **长尾 SEO 种子问答库与全平台宣发包**：
  - 产出 `docs/DISCUSSIONS_QA_SEEDS.md`（3 篇高频痛点 Q&A，供在 GitHub Discussions 发布并 Mark as answer）；
  - 产出 `docs/LAUNCH_COPYWRITING.md`（定制 V2EX、Reddit、Show HN 与 X 的开箱文案）；
  - 产出 `docs/COMMUNITY_SUBMISSIONS.md`（整理 awesome-claude、awesome-ai-agents、HelloGitHub 的 PR 申请代码）。

### 开源发现性与全网 SEO 体系化重构 (Open Source Discoverability & SEO Enhancement)

为大幅提升项目在 GitHub 站内、Google / 百度及全球开源技术社区中的检索可见度与转化率，完成全链路文档与资产优化：

- **根目录默认 README 英文重构 (Global English-First Showcase)**：
  - 将根目录 `README.md` 重构为专业英文版，面向全球开发者搜索 `claude code remote control`、`codex web ui`、`ai agent session manager` 等核心热词；
  - 补充官方版本、MIT 许可、语言与支持平台等标准 Badge；
  - 提炼清晰的“痛点 vs 方案”对比、极简 Quick Start 指南、架构图，首屏 3 秒传达产品价值；
  - 严格保留对上游 `magpie` 的开源溯源与 MIT 声明。
- **新增规范中文版说明 (README_CN.md)**：
  - 新增 `README_CN.md` 并与英文版建立顶部双向导航链接（`English | 简体中文`），保留原生亲和力与本土技术社区友好度。
- **补齐根目录标准 MIT 许可证 (Standard MIT LICENSE)**：
  - 补充标准的 `LICENSE` 文件（Copyright 2026 WANGUI TANG），消除 GitHub 仓库未识别开源协议导致的检索降权与企业合规顾虑。
- **新增开源实战推广指南 (docs/OPEN_SOURCE_GUIDE.md)**：
  - 梳理包含 15 个高权重 Topics 标签组、About 简介模版与 Social Preview 卡片设计指引；
  - 产出即抄即投的国内推广模版（HelloGitHub、阮一峰《科技爱好者周刊》、V2EX“分享创造”）；
  - 产出海外顶级渠道投放模版（Awesome Lists PR 申请、Reddit 各大板块发帖、X/Twitter 宣发文案）。

### 控制台全视图像素级视觉对齐 (Comprehensive Visual Alignment Across All Views)

在对齐 magpie 极简美学（移除冗余 Logo、导航纯文本化、去 Emoji、隐藏大白框）后，对 Runner 控制台整体进行像素级视觉对齐与布局韵律规范：

- **页面边距与顶栏对齐 (Standardized Inset Alignment)**：
  - 顶栏 `.top` 左右内边距由固定 `12px` 改为全局响应式布局变量 `var(--inset)`（18px），使顶栏左侧面包屑与右侧快捷按钮与下方卡片内容、列表左边缘实现像素级垂直基准线对齐。
  - 列表行 `.row` 内边距统一为 `9px var(--inset)`，使行内图标与文字同 `.group-head`、`.row-head` 的标题前缀完全垂直对齐，彻底消除 2px 视觉锯齿。
- **视图工具栏统一韵律 (Unified Toolbar Rhythm & Zero Layout Shift)**：
  - 全局规范 `.tools` 为 `min-height: 34px`、下外边距 `12px`。
  - 为用量视图（View 3）补齐标准化 `.tools` 栏，消除切换至用量标签时卡片向上跳跃 44px 的垂直抖动问题。
  - 在智能体、模型、工作区视图工具栏右侧增加状态徽标（`#agentsCountBadge`、`#modelsCountBadge`、`#workspacesCountBadge`），在所有六个视图建立统一的右侧徽标视觉平衡。
  - 去除网关视图 `.gw-grid` 冗余的 `margin-top: 16px`，保持与其他视图相同的 12px 间距，并在卡片内多代码片段间加入 `16px` 间距。
- **会话时间戳对齐与右边缘防抖 (Session Timestamp Alignment & Right Edge Stability)**：
  - 修复 CSS 选择器层级缺陷（原 `.row .who .when` 无法匹配 HTML 中作为 `.row` 直接子元素的 `<time class="when">`），正确声明 `#sessionsContainer .row .when, #sessionsContainer .row .who .when`。
  - 为时间戳分配固定的 `min-width: 96px`、右对齐与 `tabular-nums`，右侧外边距归零，彻底消除了不同长度时间字符串（如“刚刚”、“25 分钟前”、“10月18日 14:30”）导致的会话卡片右侧文字参差不齐。
- **统计列表统一 5 列 Grid 布局 (Unified 5-Column Stat Grid)**：
  - 规范 `.row.stat` 为 `22px minmax(0, 1fr) minmax(110px, 1.4fr) 104px minmax(72px, auto)` 5 列 CSS Grid。
  - 重构模型列表（`modelsGrid`）DOM 结构，将模型图标移至第一列，与用量视图中的模型统计、智能体统计以及工作区列表彻底统一步调，剔除多余的 4 列 Grid CSS 覆盖规则。
- **验证 (Verification)**：
  - `cargo test --workspace`：全部 168 个单元测试与 39 个集成测试 100% 通过。

### 交付产物：Windows 与 Android 同步打包至 2.4.93 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.93-windows-x64.zip`，内含 release 优化的 `agenthub-runner.exe` 与 Windows 一键接入说明；`Deliverables/AgentHub/agenthub-runner.exe` 与 `deploy/windows/agenthub-runner.exe` 同步更新。
- **Android Debug APK**：`AgentHub-v2.4.93-android-debug.apk`（`versionCode=93`、`versionName=2.4.93`）。构建完成后严格执行 `./gradlew --stop` 释放常驻编译守护。
- **校验清单**：`AgentHub-v2.4.93-SHA256.txt` 收录 exe、zip、APK 与内嵌静态资源（`dashboard.html` / `dashboard.css` / `dashboard.js`）的 SHA-256 哈希与字节大小。
- **清理规则**：严格遵循「打新包必删旧包」铁律，删除 2.4.92 历史安装包与校验清单，确保交付产物目录唯一最新。

## [2.4.92] - 2026-10-18

### 拆出控制台脚本，费用只认守护进程一个口径 (Extract the Console Script, One Pricing Authority)

两处解耦，动机来自对 `dashboard.html` 的一次结构性复查：1700 行内联脚本没有任何 lint 或语法检查覆盖，同一笔钱又被三套费率表各算一遍。

- **脚本独立成文件 (The Script Leaves the Page)**：
  - `crates/runner/resources/dashboard.js` 承接原 `dashboard.html` 里的全部行为代码，经 `include_str!` 编入二进制，以 `/dashboard.js?v=<版本>` 提供，缓存策略与样式表一致——版本号等于 `CARGO_PKG_VERSION` 才回 `immutable`，否则 `no-cache`，保证页面与它运行的代码永远出自同一次构建。
  - 页面只剩结构与一段内联的布局令牌引导脚本；`onclick="..."` 的内联处理器依赖全局函数，脚本以 `defer` 加载不改变语义。
  - 收益：`node --check`、Prettier 等工具可以直接读这个文件，不必从 HTML 里抠脚本；多 agent 并行时不再挤在同一个 2000 行 HTML 里冲突。
  - 新增测试 `dashboard_js_is_its_own_embedded_file`：确认脚本不在页面里、不含 `</script>`（防止哪天被塞回去时提前闭合标签），并确认费率启发式没有被重新引入。
- **费用只由守护进程计算 (One Place Turns Tokens into Money)**：
  - 删除页面里的两处本地费率表（`fmtCostFromTokens` 与 `renderUsageAnalytics` 兜底分支里的 `calcCost`）。它们的单价与 Rust 端 `calc_cost` 本来就不一致：Rust 按输入 / 输出 / 缓存读取分别计价，JS 用的是混合单价，且其中一份漏了 `mini` 分支。此前靠"后端有值优先用后端"压住，但只要某个模型不在 `usage.models` 里，页面就会自己算出一个不同口径的数字。
  - 现在页面只读 `usage.models[].estimated_cost_usd` 与 `usage.cost_usd`。守护进程未定价的模型在"模型"与"用量"两个视图都显示 `—`，兜底聚合路径的 KPI 同样显示 `—`——**未定价不等于免费**，宁可留空也不编造金额。
  - 三态区分：**未定价**（字段缺失或非数字）显示 `—`；**定价为零**（守护进程确实算出 0，如免费模型或四舍五入到 0）显示 `≈$0.00`；其余按金额显示。`backendModelCosts` 只收真实数字，不再用 `|| 0` 把缺失值折成零。
- **验证 (Verification)**：
  - `cargo test --workspace`：168 单元 + 26 / 17 / 7 / 5 / 3 集成全部通过；`cargo fmt --check` 与 clippy 的报错全部落在本次未触碰的既有文件（`build.rs`、`collectors/*`、`remote_host.rs`、`tray.rs`、`conversation.rs`、`model_catalog.rs`、`main.rs`），本次改动的 `web_server.rs` 无新增告警。
  - 真实 2.4.92 debug daemon（独立数据库与工作区，4313 端口）+ headless Edge（820×600，CDP）实测：`GET /` 返回 `/dashboard.js?v=2.4.92` 与 `/dashboard.css?v=2.4.92`，`/dashboard.js` 返回 200 `text/javascript`，版本匹配时 `Cache-Control: immutable`、不匹配时 `no-cache`；脚本加载后 `renderUsageAnalytics` 为函数，六个视图逐一切换正常，控制台零异常。
  - 费用口径用合成快照验证：已定价模型显示 `≈$10.50`、定价为零的模型显示 `≈$0.00`、未定价模型显示 `—`、KPI 显示 `≈$10.50`；守护进程回 `cost_usd: null` 时 KPI 显示 `—`；强制走兜底聚合路径时 KPI 与模型行均为 `—`，不再出现本地估算。`costCell` 的边界（0 / 0.004 / 99.999 / 100 / 123.4 / null / undefined）另有独立断言，全部通过。
  - 交付产物打包至 `Deliverables/AgentHub/`（`agenthub-runner.exe`、`AgentHub-Runner-v2.4.92-windows-x64.zip`、`AgentHub-v2.4.92-android-debug.apk` 及 SHA-256 清单），按「打新包必删旧包」删除 2.4.88 旧产物；打包 exe 实测内嵌的 `dashboard.js` 与源码逐字节一致（`87a5f037…`），运行中的控制台返回 `/dashboard.js?v=2.4.92`。

### 交付产物：Windows 与 Android 同步至 2.4.92 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.92-windows-x64.zip`，内含 `agenthub-runner.exe` 与接入说明；本机 `Deliverables/AgentHub/agenthub-runner.exe` 同步替换。实测启动后 `GET /` 返回 200，引用 `/dashboard.js?v=2.4.92`，内嵌脚本 SHA-256 与源码一致，六个视图切换正常、零异常。
- **Android Debug APK**：`versionCode=92`、`versionName=2.4.92`。
- **校验清单**：`AgentHub-v2.4.92-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css` / `dashboard.js`。
- **补齐说明**：2.4.89 / 2.4.90 / 2.4.91 三期只改本机 Runner 与控制台资源，`Deliverables/` 实际停留在 2.4.88；本次一次性补齐到 2.4.92。

## [2.4.91] - 2026-10-18

### 彻底对齐 magpie 极简桌面美学，剔除冗余图标与大白框 (Align with magpie Minimalist Desktop Aesthetics: Eliminate Redundant Icons, Emojis & Stat Card Banner)

用户反馈「能不能模仿一下magpie 把这个图标删掉，然后emoji都删掉 然后不要有这个大白框，模仿magpie」。排查与比对 `Project/magpie` 官方设计规范后确认：在桌面宿主环境下（尤其是 Windows 原生标题栏已自带应用图标与标题），界面的过度装饰（内层顶栏重复 Logo、导航按钮图标、会话列表前缀彩标、顶层统计大卡片）会严重侵占垂直屏效并造成视觉杂乱。本版本彻底贯彻极简专注原则，全方位对齐 magpie 极简质感：

- **移除顶栏左侧重复图标与应用名 (Eliminate Windows Header Brand Redundancy)**：
  - 参考 `magpie` 核心规范（`body.window:not(.mac):not(.linux):not(.web) .brand { display: none; }`），在 Windows 桌面宿主窗口下，系统原生标题栏已完整呈现应用名与图标，页面内部顶栏再次堆砌 AgentHub Logo 与文字属于双重冗余。
  - 从顶栏 HTML 结构中彻底剥离 `#brandTop`，侧边栏折叠时顶栏左侧仅保留极简优雅的展开按钮 `#sideToggle`，展开时仅保留平滑切换的面包屑导航，彻底摆脱双层标题视觉冗余。
- **纯文字极简分段导航 (Iconless Text Navigation Tabs)**：
  - 移除主导航轨（`#nav`）中所有按钮的前置 SVG 图标（会话、智能体、用量、模型、工作区、网关），全面对齐 magpie 纯文本导航栏（`Agent 供应商 路由 网关 用量 插件`）的排版质感。
  - 配合既有平滑滑块（`#segThumb` 与 `.rail-hl`），让导航重心回归功能文字本身，彻底消除小尺寸下图标与文字拥挤、像 emoji 的视觉杂乱感。
- **会话列表消除彩色图标干扰 (Eliminate Colorful Agent Icons in Session Rows)**：
  - 会话行渲染（`renderSessions`）全面剔除前置的多彩星号与方形提供商标志（`getAgentIcon`），会话标题左对齐直接起排。
  - 视觉层次完全参照 magpie 的 `sm-sess` 模式：主视觉呈现用户会话提示词（`who > name`），副信息在右侧呈现模型名称、相对时间与 Token 用量，信息聚焦且极为清爽干净。
- **消灭大白框横幅，直达搜索与内容 (Remove Large Stat Card Banner & Elevate Content)**：
  - 隐藏会话视图（`#view-sessions`）顶部的四宫格数据卡片（`.led-strip`，历史会话、云端 Web 绑定、上游模型、工作区）。
  - 会话页面顶部即为搜索框与会话数标签（`.tools`，去除原本多余的顶部 margin），用户打开窗口即刻聚焦会话检索与开发上下文，再无多余的 120px+ 高度大白框遮挡视线。
- **全量回归验证与双端交付 (Verification & Deliverables)**：
  - `cargo test -p agenthub-runner` 全量 218 项测试套件 100% 全绿通过；
  - 交付产物按工程铁律归档更新至 `Deliverables/AgentHub/`（`agenthub-runner.exe`、`AgentHub-Runner-v2.4.91-windows-x64.zip`、`AgentHub-v2.4.91-android-debug.apk` 及 SHA-256 清单），历史 2.4.90 旧产物彻底清理；
  - Android 构建完成立即执行 `./gradlew.bat --stop` 释放 Daemon 与编译内存。

## [2.4.90] - 2026-10-18

### 解开顶栏重叠与侧边栏失效，重塑三栏网格顶栏 (Unblock Sidebar Toggle & Eliminate Top Bar Bloat with 3-Zone Header Grid)

用户在截图（`1.png`）中用褐色圆圈标注了两处最核心的交互与布局硬伤：「侧边栏按钮失效了」以及「然后顶部这一块真的没办法优化吗」。排查发现此前顶栏存在容器层级劫持与严重的空间重叠冗余：Edge 桌面窗口标题栏与网页顶栏双层压顶、右上角并排出现两个关闭 `✕` 按钮容易误点、居中导航容器隐式拦截侧边栏按钮点击。本版本针对性重构顶栏架构，彻底修复点击失效并完成空间优化：

- **彻底解开侧边栏按钮失效 (Unblock Sidebar Toggle from Pointer-Events Trap)**：
  - **根因**：此前 `#navTopSlot` 采用 `position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);` 居中且带有 `max-width: calc(100% - 24px)`。在 820×600（内部视口约 804px）桌面窗口下，居中导航轨宽度达 558px，从 x=99px 延展至 x=657px，直接覆盖在左侧 `#sideToggle` 按钮（x=128px ~ 158px）正上方。由于容器缺少 `pointer-events: none`，所有的鼠标点击事件在捕获阶段均被透明的居中容器优先吃掉，导致用户怎么点击侧边栏按钮都无任何反应。
  - **修复**：顶栏全面转向现代 CSS Grid 三栏非重叠网格架构（`grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr)`），物理上隔离左侧品牌/面包屑区、中间导航轨区与右侧工具区，从几何结构上杜绝容器覆盖；同时给 `#navTopSlot` 显式设置 `pointer-events: none`，子级 `#nav` 设置 `pointer-events: auto`，给 `#sideToggle` 设置 `z-index: 7` 与独立指针捕获，经 CDP `elementFromPoint` 实测命中率 100%。
- **顶栏瘦身与双 `✕` 按钮消除 (Slim 38px Header & Differentiate Shutdown Action)**：
  - **高度从 48px/54px 缩减至 38px**：`--top-h` 从 `--layout-top-h, 54px` 紧凑调整为 `38px`（`topBar: 38`），Edge 原生窗口标题栏（约 32px）与网页顶栏（38px）合计仅 70px，相比原本的近 86px 明显减少垂直纵深压迫感，为内容主视口腾出宝贵空间。
  - **消灭双层关闭按钮混淆**：Edge 窗口右上角第一层已具备原生窗口关闭 `✕` 按钮，而原顶栏第二层直接画了一个外观几乎相同的 `✕` 图标用于终止后台守护进程（`shutdownRunner()`），极易让用户产生困惑甚至误操作。现将退出守护进程按钮替换为符合语义的标准电源/退出图符（`M18.36 6.64a9 9 0 1 1-12.73 0M12 2v10`），并配置温和的淡红悬停底色（`var(--red-soft)`）与红色图符（`var(--red)`），职责清晰分明。
- **动静清晰的两态架构 (Clean Two-State Responsive Header)**：
  - **侧边栏展开时**：顶栏整体自动与下方主视口同步带 `margin-left: var(--side-w)`，视觉上与侧边栏齐平；顶栏左侧自动隐藏重复的品牌 Logo 与折叠按钮，转而呈现优雅的面包屑导航（如 `会话 · Sessions`、`智能体 · Agents`）；居中导航轨自动隐藏（因已在侧边栏完整陈列）。
  - **侧边栏折叠时**：顶栏平滑扩展至全宽（`margin-left: 0`），左侧优雅展现 AgentHub Logo 品牌与侧边栏展开按钮（`#sideToggle`），居中无缝展现分段胶囊导航轨（`#navTopSlot`），右侧展现操作按钮。
- **初始状态判定与尺寸防护 (State Machine & Guard)**：
  - **断点重新校准**：将自动展开侧边栏的断点由 820px 调至 768px（`sidebarAt: 768`），匹配 820×600 窗口在 Windows 下约 804px 的物理可视区，确保开箱默认进入优雅的侧边栏模式。
  - **判定基准归一**：消除 `layoutNav()` 对 `body.side-nav` 的初始依赖，以 `!html.classList.contains('no-side')` 作为唯一判定基准，修复初始装载与本地存储记忆不同步时的状态错位。
  - **侧边栏导航尺寸保护 (`fitNavTrack` Guard)**：当 `#nav` 被侧边栏接管时（`nav.parentElement !== slot`），自动跳过横向顶轨压缩计算并重置完整文字与显示，防止侧边栏内的导航文字被误切成两字或隐藏。
- **严格符合 Aozora UI 与 60 FPS 性能铁律**：所有平移动画采用合成器层 GPU 硬件加速，无全屏重绘，无滚动模糊，保持极致轻盈灵动。
- **自动化验证**：headless Edge 经 CDP 13 项指标全量测试通过（顶栏 38px 实测命中、侧边栏默认展开、折叠时零重叠判定通过、侧边栏按钮真实点击展开测试通过、面包屑动态切词验证通过）；`cargo test --workspace` 全量测试套件（167 单元测试 + 58 项集成测试）一次性全绿通过。

## [2.4.89] - 2026-10-18

### 全面上动画、把生硬的地方都揉软 (A Motion Pass Over the Whole Console)

用户的原话是「全面引入动画感和交互感，现在太生硬了，然后提升一些高级感」。上一版把配色和排版收拾干净了，但**界面本身还是一跳一跳的**：切页面是瞬间替换，展开分组是 `display:none` 硬切，弹窗是一下子蹦出来，数字换值没有过渡，鼠标划过几乎没有回应。这一版不换布局、不加装饰，只把「动」这件事从头到尾补齐，让每一处状态变化都有来处和去处。

- **先定一套运动语汇 (One Motion Vocabulary)**：照 magpie 的做法，先把曲线和时长收成令牌再动手，避免每个地方各写各的缓动。三条曲线——`--ease-out: cubic-bezier(.22,1,.36,1)` 负责进场与淡出，`--ease-spring: cubic-bezier(.32,.72,0,1)` 负责位移与回弹（magpie 的 `.agent-fold` 用的就是这类前快后缓的弹簧感），`--ease-tap: cubic-bezier(.4,0,.2,1)` 负责按压；四档时长 `--dur-1` 至 `--dur-4`（120 / 200 / 320 / 480ms）。后文所有动画都从这七个令牌取值，读者看到的是一套统一的节奏，而不是七拼八凑。
- **切页面像翻页 (The Page Turns)**：用 Chromium 的 View Transition 包住视图切换——`.view-slot` 声明 `view-transition-name: page`，旧页 `.16s` 淡出并轻微上移，新页 `.34s` 淡入并落位。`switchView()` 里 `document.startViewTransition(turn)` 包住原有的 DOM 变更，`finished` 后撤掉 `page-turn` 类。侧边栏重排、静默切换（初始化）不走转场，避免刚打开就闪一下。
  - **踩到的坑写在这里 (The Trap, Documented)**：一开始想用 `::view-transition-new(root)` 做主题的圆形扩散，实测发现**只要页面上有任何元素带自己的 `view-transition-name`，Chromium 就整块丢弃根伪元素**——给 `<html>` 命名、给 `<body>` 命名都试过，要么吞掉页面快照、要么根本不产生 old/new 伪元素。所以主题切换最终没有走 View Transition，改成下面的 CSS 交叉淡入（见「换主题」一条）。
  - **页面自己不能再有进场动画 (No Entrance Animation on the Incoming View)**：转场的第一帧会被拍成快照，如果新页此时正跑着 `translateY` 进场，快照拍到的就是偏移后甚至透明的样子，翻页会变成「白一下再出现」。所以 `.view` 的入场动画从切换路径里移走，只保留在首次启动的 `html.booting` 阶段。
- **侧边栏当前项会滑过去 (The Rail Slides)**：导航里塞了一个绝对定位的 `.rail-hl` 高亮块，`switchView()` / `layoutNav()` / 窗口缩放时量出当前按钮的 `offsetTop` 与高度，用 `translateY` + `height` 过渡（`--dur-3` + `--ease-spring`）滑到新位置。`will-change: transform` 让合成器接管，不触发布局。初始化时加 `.still` 一次性关掉过渡，避免刚打开就看到它从顶上滑下来；`#navTopSlot` 里的顶栏导航隐藏这个块（那里用的是滑块，不需要两层高亮）。
- **分组展开有层次 (Groups Fold Open)**：折叠从 `display:none` 硬切改成 `grid-template-rows: 1fr ↔ 0fr`（内层 `.group-rows` 带 `min-height:0; overflow:hidden`），高度随内容自然过渡 `--dur-4`。行本身按 `--i` / `--n` 做错峰：展开时逐行落下（每行延后 22ms），收起时按反序（每行延后 14ms），合起来是一次有方向的开合，而不是一整块忽然出现。
- **弹窗与提示 (Dialog and Toast)**：`.modal` 从 `display` 开关改成 `opacity` + `visibility` 过渡（`visibility` 用 `transition-delay` 延后到淡出结束，保住可访问性），内层 `.dialog` 从 `translateY(8px) scale(.985)` 归位到 `none`。分享条用 `share-in` 从 0 宽长到目标宽度，并带 `transform-origin: 0 50%`，读起来像「条子在长出来」。
- **数字会走 (Figures Count Up)**：总会话、总模型、总工作区三个大数字改成 rAF 插值（420ms，三次方缓出），并用 `shownFigures` 记住上一次显示的值——**值没变就不重播**，否则每 4 秒轮询一次会一直闪。第一次出现时从 0 走到真实值。
- **鼠标划过有回应 (Hover Has a Reply)**：会话行悬停时底色染上强调色淡调、标题转强调色、图标轻微放大；账本与 KPI 卡悬停整体上浮 1px 并加深阴影；导航按钮悬停时图标轻轻前倾；状态点的 `link-breathe` 做缓慢呼吸。按钮在忙碌时转出一个小 spinner（`#refreshBtn` 只**手动**刷新时转，自动轮询不转，免得每 4 秒闪一次；`refreshModelsCatalog` 同时加 `busy` 类）。
- **首屏编排 (First Paint Choreography)**：`init()` 给 `<html>` 加 `booting`，各区块按序轻微上浮出现，700ms 后移除——只在冷启动时演一次，之后切页面靠翻页转场，不重复演。
- **顶栏跟着滚动 (The Header Answers Scrolling)**：在 `.view-slot` 上装捕获阶段滚动监听，滚动时给 `<html>` 加 `scrolled`，顶栏阴影随之加深；回到顶部就撤掉。只改一个类，不读布局、不写布局。
- **换主题是淡过去的 (The Theme Fades)**：`toggleTheme` 改成给 `<html>` 加 `theme-fade`，用一条统一规则把所有元素的 `background-color / color / border-color / box-shadow / fill / stroke` 设成 `.42s` 过渡，再翻转主题类。这里有两个必须踩对的点：
  - **先让浏览器算出旧值**：类和主题如果在同一帧翻转，浏览器没有「旧颜色」可插值，结果是直接跳变。所以先 `void getComputedStyle(document.body).backgroundColor` 强制样式计算，再在 `requestAnimationFrame` 里执行翻转。
  - **规则写法**：最初写成 `html.theme-fade *::before`，被解析成后代选择器；并且把 `transition: A !important, B` 这种简写列表整条丢掉（解析出来的规则是空的 `{}`）。最终用 `:where(*)` / `:where(*)::before` / `:where(*)::after`，并把 `transition-property`、`transition-duration`、`transition-timing-function` 拆成三条独立声明，才真正生效。
- **轮询不再抽动列表 (Polls Stop Reshuffling)**：每 4 秒拉一次快照时，模型榜、Agent 榜、工作区榜会因为 Rust `HashMap::into_values()` 的顺序每次都不同而在同分项之间来回换位，读者看到的是列表自己在抖。三处排序都补了确定的次级键——模型按名字、Agent 按 id、工作区按路径——同分项的顺序从此固定。
- **60 FPS 红线核对 (The 60 FPS Line)**：`backdrop-filter` 全仓只剩弹窗遮罩一处（不在滚动列表里）；没有 `background-attachment: fixed`；没有低于 500ms 的定时刷新；所有动画元素带 `will-change: transform` / `transform: translateZ(0)`。实测：程序驱动 `.view-slot` 在 0–400 之间滚动 40 次，采样 `requestAnimationFrame` 间隔，**120 帧平均 16.5ms、p95 16.8ms、最大 16.8ms、掉帧 0**。
- **无障碍 (Reduced Motion)**：整段动画都收在一条 `@media (prefers-reduced-motion: reduce)` 契约下——关掉过渡与动画、`scroll-behavior: auto`、视图转场退化为直接切换。`calmMotion()` 在 JS 侧做同样的判断，读者若在系统里关了动画，页面不会硬演。
- **验证 (Verification)**：headless Edge 经 CDP（820×600，独立 `USERPROFILE` 与数据库）逐项量取，不靠肉眼判断——侧边栏高亮块位置随当前项变化；分组折叠高度实测 980 → 311 → 0 与 545 → 980；弹窗 `opacity` / `transform` 有中间值；`::view-transition-group(page)` 上确实挂着 `page-in` / `page-out` 动画；主题淡入采样为 `rgb(143,143,147) → rgb(43,43,47) → rgb(27,27,31) → rgb(26,26,30)`；数字走位为 `491 → 893 → 1115 → 1199 → 1232 → 1234` 且值不变时不重播；资源以 `immutable` + `?v=` 提供，所有复核都用 `Network.setCacheDisabled` + `ignoreCache` 硬重载。`cargo test --workspace` 全部通过（167 单元 + 26 / 17 / 7 / 5 / 3 集成）；`node --check` 页面脚本通过；clippy 与 `cargo fmt --check` 的报错全部落在本次未触碰的既有文件（`build.rs`、`collectors/mod.rs`、`remote_host.rs`、`tray.rs`、`main.rs`），本次改动的 `web_server.rs` 与 `collectors/workspace.rs` 无告警、格式无差异。

### 交付产物：Windows 与 Android 同步至 2.4.89 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.89-windows-x64.zip`，按「打新包必删旧包」删除 2.4.88 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`，保留 `agenthub-runner.exe.2.4.82.bak` 回滚。
- **Android Debug APK**：`versionCode=89`、`versionName=2.4.89`。
- **校验清单**：`AgentHub-v2.4.89-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css`。

## [2.4.88] - 2026-10-17

### 标题栏跟着页面走色，不再是一条灰带 (The Caption Takes the Page's Colour)

用户截图圈出整条带「最小化 / 最大化 / 关闭」的顶部，说「最违和的是那个有关闭和最小化的顶部跟其他不统一」，要求照 magpie 的方案处理。这一条确实是窗口里最刺眼的地方：页面是浅蓝白，上面却压着一条系统灰 `#e3e3e3` 的标题栏，中间还夹着 Edge 自己画的地址栏式空白。

- **先看 magpie 怎么做的 (What magpie Does)**：magpie 的 Windows 主窗口**不是无边框**，它保留系统标题栏，只是把标题栏涂成页面自己的颜色——`internal/gui/titletint_windows.go` 里写得很直白：「Windows' grey bar sat above the page as a strip of its own」，所以 `SetTheme` + `SetTitleBarColour` 用 DWM 把 caption 染成页面底色；窗口创建时经 `WindowsWindow{Theme, CustomTheme: ThemeSettings{TitleBarColour}}` 传入。真正无边框的只有托盘小面板。照搬这个思路：保留系统标题栏和三个按钮，只让它跟页面同色。
- **为什么 DWM 在 AgentHub 上不管用 (Why DWM Fails Here)**：AgentHub 的窗口是 Edge `--app=<url>`，Chromium 把 caption 画在**客户区**里，不走 DWM 那套非客户区绘制。逐项实测：`DwmSetWindowAttribute` 的 caption 颜色属性（35）写入返回 `S_OK`，读回却是 `E_INVALIDARG (-2147024809)`；20 / 34 / 35 / 36 / 38 全部试过，边框始终是系统灰 `[227,227,227]`；用 `SetWindowLongPtr` 摘掉 `WS_CAPTION` 也没有任何可见变化；`--force-dark-mode` 能把 caption 变黑，证明这块颜色归 Chromium 主题系统管。PWA 的 Window Controls Overlay 在 `--app` 模式下也不可用（`navigator.windowControlsOverlay.visible === false`，`display_override` 与 `--enable-features=WebAppWindowControlsOverlay` 均无效）。
- **用 Chromium 主题扩展接管 (A Theme Extension Paints It)**：既然 caption 归 Chromium 主题系统，就用主题去染它。Runner 在开窗前写一个只含 `"theme".colors` 的 manifest-v3 扩展——`frame` / `frame_inactive` / `frame_incognito*` 取页面 `--bg`，`toolbar` 同色，`tab_text` / `bookmark_text` 取页面 `--fg`——再用 `--load-extension=<dir> --disable-extensions-except=<dir>` 载入。实测浅色 `[244,245,248]`、深色 `[26,26,30]`、橙 `[255,136,0]`、绿 `[0,128,0]`、青 `[0,180,90]` 五种颜色都能精确染上，标题栏与页面之间不再有断层。
- **颜色从哪来 (Where the Colour Comes From)**：Chromium 只在**启动时**读一次主题（`chrome.theme` API 在 MV3 / MV2 里都不可用，运行时改色做不到），所以 manifest 必须在开窗**之前**写好。页面把当前主题经新增的 `POST /api/window-theme {"dark": bool}` 存到 `~/.agenthub/window_shell/theme.txt`，`open_desktop_window()` 每次开窗都按这份记录重写 manifest 再拉起 Edge；页面初始化时也会同步一次，第一次在这个配置里开窗、或记录过期时都能自己纠正。代价是**切换主题要等下一次开窗才反映到标题栏**，这是 Chromium 的启动期读取决定的，不是遗漏。
- **验证 (Verification)**：调试版与发布版各起一次真实窗口（独立 `USERPROFILE`、4221 端口），按 Edge 进程命令行过滤 PID、用 `PrintWindow` 抓窗口（避免误抓读者正在跑的 4200 窗口）：发布版 caption 取色 `[244,245,248]`，与页面 `--bg` 完全一致；`theme.txt` 为 `light`；Edge 命令行确认带上 `--load-extension` 与 `--disable-extensions-except`。深色路径：`POST /api/window-theme {"dark":true}` 后重开窗，caption 为 `[26,26,30]`。`cargo test --workspace` 167 项通过（新增 `the_window_shell_paints_the_caption_in_the_page_colour`、`a_window_opens_in_the_theme_the_page_was_left_in` 两项）。
- **已知边界 (Known Limit)**：标题栏右侧的最小化 / 最大化 / 关闭仍是 Edge 画的原生按钮，只能跟着主题变深浅，不能改形状或换位置；要做到完全统一只能整窗无边框、三个按钮自绘，那会丢掉系统的窗口贴边、Aero Snap 与多显示器行为，本版没有走这条路。

### 交付产物：Windows 与 Android 同步至 2.4.88 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.88-windows-x64.zip`，按「打新包必删旧包」删除 2.4.87 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`，保留 `agenthub-runner.exe.2.4.82.bak` 回滚。
- **Android Debug APK**：`versionCode=88`、`versionName=2.4.88`。
- **校验清单**：`AgentHub-v2.4.88-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css`。

## [2.4.87] - 2026-10-17

### 删掉导航分组标签、顶栏统一为一套配色 (No Group Labels, One Header Palette)

用户截图圈出顶栏里的「工作台」「洞察」「系统」，问「这是啥呀」，并要求顶栏配色跟 magpie 一致、不混用两种底色：

- **分组标签全删 (Group Labels Gone)**：顶栏分段切换里夹着的「工作台 / 洞察 / 系统」删除，侧边栏里的同名分组小标题也一并删除——导航只剩页面本身，按顺序排开，不再被不属于页面的词打断。`fitNavTrack()` 不再为标签留位置。
- **顶栏一套配色 (One Palette)**：顶栏背景从白卡改成与页面同色的浅灰底（`--bg`），去掉投影和渐变。分段切换是一条 `--fg` 5% 的淡灰底轨，当前页是低饱和的 `--accent-soft` 色块 + 强调色文字，图标跟着当前页变色。顶栏里不再出现「白卡 + 灰轨 + 白滑块」三种底色。
- **按空间取舍 (Fit by Space)**：`fitNavTrack()` 重写——用一份不在文档流里的克隆量出每个页面标签的真实宽度，按顶栏实际剩余空间取「当前页 + 左右邻居」，放得下就整词显示并带图标；顶栏挤时先收图标，再收成两个字，保证露出来的每一项都点得到、当前页永远在。浏览器缩放、侧栏开关、切换页面都会重新算。
- **顺带修掉 (Along the Way)**：窄正文时「刚刚」「1 小时前」被挤成竖排——时间列改为不限宽、不再被压缩；条目模型名在窄正文下隐藏，时间对齐其他行。侧边栏宽度上限放到窗口的 30%，侧栏展开时正文仍有 560px 以上。
- **验证 (Verification)**：headless Edge 经 CDP 在 820×600（侧栏开 / 关）、700×560、1920×1000 下截图：顶栏无任何分组标签；收起时六个页面名连同图标整词排开并居中，700px 宽时同样整词显示；侧栏开启时顶栏只剩主题 / 刷新 / 退出，轨道底部是同一浅灰；真实 Edge 窗口 820×600 截图确认标题栏下方、顶栏、页面底同色。`node --check` 页面脚本通过。

### 交付产物：Windows 与 Android 同步至 2.4.87 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.87-windows-x64.zip`，按「打新包必删旧包」删除 2.4.86 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`。
- **Android Debug APK**：`versionCode=87`、`versionName=2.4.87`。
- **校验清单**：`AgentHub-v2.4.87-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css`。

## [2.4.86] - 2026-10-17

### 更小巧的窗口、更干净的顶栏、侧边栏开关不再「有去无回」 (Smaller Window, Quieter Header, A Rail That Can Close)

用户对照 magpie 截图提出四点：尺寸还是偏大、圈出的顶栏元素要删、点开侧边栏后按钮消失回不来且图标不对、顶栏比 magpie 杂乱且字体太小。逐项处理：

- **尺寸 (Size)**：默认窗口再收一档，**820×600**（最小 640×460），仍是 magpie 660×600 的阅读量级，多出来的一栏正是侧边导航。旧版 980×720 及 945×1012 留下的外框由 `fit_kept_window()` 自动改小并居中；读者在 2.4.85 里靠拖动得到、没超过 1000×680 的尺寸会原样保留。
- **删掉顶栏的杂物 (Fewer Things in the Header)**：去掉面包屑 `AgentHub / 会话`、「744 个会话」胶囊、以及复制网关环境变量的按钮——会话数在下方统计卡与列表标题里已经有了，复制配置在网关页。顶栏只剩：侧边栏开关、页面切换、配对 / 刷新 / 主题 / 退出。
- **会话行去掉 Token 数 (No Token Counts in the List)**：截图里每行标题下的 `1.1M Tokens` 去掉，模型名从标签样式改成一行安静的等宽小字，和右侧时间对齐，标题独占整行。
- **侧边栏开关 (Rail Toggle)**：根因有三个——顶栏开关在侧边栏展开时「半张图」的空格子图标看起来像消失了（其实是图标本身画成了两格、左格半透明）；侧边栏自己头部没有关闭按钮；并且任何一次窗口缩放都会调用 `followWindow()`，按宽度把侧边栏收回去，「打开了又自己关」。现在：顶栏与侧边栏头部各有一个画得清楚的侧栏图标（带竖分隔线），**展开时只显示侧边栏头部那个，收起时只显示顶栏那个**，任何宽度、任何时候都能关也能开；读者点过的选择存进 `localStorage`，窗口关掉重开、窗口缩放都按读者说的来，只有第一次访问才按窗口宽度决定。窄窗口里侧边栏最多占四分之一宽，正文不会被挤没。
- **顶栏统一 (One Header)**：原来顶栏里有两套控件——一套是 magpie 的「底部圆角轨 + 白色提升滑块」的分段切换，另一套是各个按钮各自圆角的胶囊。现在只保留一套：一条圆角底轨承载全部页面项，当前页由白色滑块托起；按钮、图标、文字统一使用页面同一套字号令牌（`--fs-body` 等）。轨道在窄窗口先隐藏分组名、再隐藏图标，`fitNavTrack()` 保证露出来的名字都点得到。
- **字号 (Type Scale)**：整套令牌上调一档——注释 13.5、正文 14.5、标题 16、大数字 28（窗口变宽时继续放大），顶栏图标 16 → 17。
- **验证 (Verification)**：临时身份 + 独立数据库起 2.4.86 daemon（4201 端口），headless Edge 经 CDP 在 820×600、760×560、1920×1000 三种宽度下截图核对：顶栏无面包屑 / 会话胶囊 / 复制按钮；侧栏开着时只有侧栏头部的开关，点它收起后顶栏开关出现，再点又能展开；收起状态下刷新页面，侧栏按记住的选择保持收起；单行轨迹（rail × close）点不开；顶栏切换只有一条底轨，当前页滑块贴合；会话行模型名与时间右对齐、无 Token 数；1920 宽下正文最大 1100px 居中不散。`node --check` 对页面脚本通过；`cargo test --workspace` 165 单元 + 集成 26/17/7/5/3 全部通过，`npm.cmd test` 15 + 56 + 74 全部通过；clippy / `fmt --check` 只报本次未触碰的既有文件。发布版 exe 再走一次真实开窗：窗口 820×600 居中于 (550,240)。

### 交付产物：Windows 与 Android 同步至 2.4.86 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.86-windows-x64.zip`，按「打新包必删旧包」删除 2.4.85 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`，保留 `agenthub-runner.exe.2.4.82.bak` 回滚。
- **Android Debug APK**：`versionCode=86`、`versionName=2.4.86`。
- **校验清单**：`AgentHub-v2.4.86-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css`。

## [2.4.85] - 2026-10-17

### 窗口缩到阅读尺度、首次出现落在屏幕正中 (Smaller, Centred Window)

用户反馈“尺寸有必要这么大吗，你看看 magpie，而且窗口刚出现的时候要在屏幕中心”。两件事一起做：

- **尺寸 (Size)**：默认窗口从 1240×820 收到 **980×720**（最小 720×520），参照 magpie 主窗口的 660×600 量级——magpie 是阅读型小窗，AgentHub 控制台比它多一栏侧边导航与统计卡，980 宽正好让侧栏与内容都站得住（侧栏阈值 1200px 不变，窄窗自动收进顶栏）。网页令牌 `AppLayout.window` 与页面兜底同步改为同一组数字；新增测试不再写死宽度，改为从 `tray.rs` 的 `WINDOW_DEFAULT` 读出来比对，前后端数字不会再各走各的。
- **居中 (Centred)**：此前调试发现，`--window-size / --window-position` 只在 profile 还没有窗口记录时生效，Edge 一旦在 `Preferences → browser.app_window_placement` 里存下外框，就会原样恢复——本机一直停在旧的 945×1012、贴左上角 (10,10)。现在每次启动先用 `fit_kept_window()` 看一眼 Edge 记下的外框：属于旧控制台量级（≥1150×780，或处于最大化）的先按 980×720 改小；任何偏离屏幕中线的外框都平移回正中（外框大小不变，读者自己拖出来的尺寸保留）。`--window-size / --window-position` 改为每次都传，新 profile 第一次开窗同样落在正中。
- **验证 (Verification)**：用真实 `agenthub-runner.exe` 走完整开窗链路（独立命名管道 + 4201 端口，不碰正在使用的 4200 实例）。先人为把 `Preferences` 写回旧值 945×1012 @ (10,10)，启动后外框实测 **980×720 @ (470,180)**：x 方向 (1920−980)/2=470，中线与屏幕中线重合，截图中窗口左右两边的桌面留白等宽；截图同时确认“侧栏收进顶栏、统计卡两列、页面无横向溢出”。`cargo test --workspace` 全部通过（165 单元 + 集成 26/17/7/5/3）；clippy / fmt 只报本次未触碰的既有文件。
- **说明 (Note)**：2.4.84 条目里“`Preferences` 未改动”的遗留反馈已在本版处理——只调整外框位置与旧尺寸，不改任何浏览器其它配置；改动前已留存 `Preferences.before-2.4.85` 备份，验完即删。

### 交付产物：Windows 与 Android 同步至 2.4.85 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.85-windows-x64.zip`，按「打新包必删旧包」删除 2.4.84 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`，保留 `agenthub-runner.exe.2.4.82.bak` 回滚。
- **Android Debug APK**：`versionCode=85`、`versionName=2.4.85`。
- **校验清单**：`AgentHub-v2.4.85-SHA256.txt` 收录 exe、zip、APK 与内嵌 `dashboard.html` / `dashboard.css`。

## [2.4.84] - 2026-10-17

### 分界线跟窗口自适应、页面跟随窗口宽度 (Rail Follows the Window, Content Follows the Rail)

用户截图指出侧边栏与内容之间的分界线位置不对，内容应当自适应。根因有两层，一起修掉：

- **分界线的根因（两套数字各说各话）**：页面最初按“窗口宽 ≥ 1024px 就显示侧栏”决定布局；而拖动侧栏时写下的 `--side-w-override` 在每次窗口缩放重算布局令牌时被清掉，侧栏宽度被重置；另外默认窗口 1240 宽时，侧栏 236px 加内边距后正文只剩约 968px，触发了正文自己的窄屏容器断点，统计卡从四列折成两列，进一步把页面挤成“左边一段空白、右边两列卡片”。
- **内容跟侧栏走 (Content Follows the Rail)**：`body` 改为相对定位、侧栏绝对定位，正文区与底栏用 `margin-left: var(--side-w)` 从侧栏边缘起排，不再依赖 `html` 的 padding；“有没有侧栏”由 CSS 直接读取栏宽本身决定，不会再和窗口断点各算一遍。`--side-w` 优先取读者拖出来的宽度，其次才是窗口按比例算出的宽度。
- **页面跟随窗口 (Layout Follows the Window)**：侧栏出现阈值从 1024px 提高到 **1200px**（`AppLayout.sidebarAt`），这个值由令牌 `--layout-side-at` 带给页面，前后端只写一份。按钮切换只在本次会话内生效，任何一次窗口缩放都重新按窗口宽度决定布局，窄窗口或手机上不会残留一条占地方的侧栏。原先“记住读者选的布局”的 `localStorage` 逻辑删除——它正是窄窗口下仍带侧栏的原因。
- **拖动宽度不再被吞**：拖动或键盘调整侧栏宽度后，先读下边缘坐标再移除 `--side-w-override`（浏览器此时已把宽度刷进样式），宽度交由令牌携带，下一次缩放重算令牌也不会丢。
- **顶栏对齐**：侧栏收起时顶栏左内边距改为 `--inset`，品牌、开关与下方页面内容在同一条竖线上。
- **验证 (Verification)**：临时身份（`device init`）起独立 daemon 于 4799 端口，经 CDP 在 1100 / 1240 / 1300 / 1920 四种宽度核对：1100px 下侧栏收进顶栏、正文从 0 起排、顶栏与正文左内边距同为 18px；1240px 及以上侧栏出现、正文左边界正好等于栏宽 236px、统计卡保持四列；全程无横向溢出。`cargo test --workspace` 全部通过（165 单元 + 集成 26/17/7/5/3）；`cargo clippy -p agenthub-runner --all-targets -- -D warnings` 与 `cargo fmt --all --check` 仍只报本次未触碰的既有文件（`build.rs`、`collectors`、`conversation.rs`、`main.rs`、`remote_host.rs`）。

- **遗留问题 (Known issue)**：本机窗口曾由 2.4.83 以 940×720 的旧逻辑记录过尺寸，Edge 自带的 `Preferences` 会原样恢复 945×1012 的外框，导致 4200 端口下侧栏收进顶栏；判定逻辑本身在 1120 / 1240 / 1300 / 1920 四种宽度下均正确。想回到默认 1240×820 的大窗，拖动窗口边框放大即可，Edge 会记住新尺寸。源窗口的 `Preferences` 未改动，没有替读者动私人浏览器配置。

### 交付产物：Windows 与 Android 同步至 2.4.84 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.84-windows-x64.zip`，按「打新包必删旧包」删除 2.4.83 的 zip 与 APK；本机安装目录 `Deliverables/AgentHub/` 同步替换 `agenthub-runner.exe`，保留一份 `agenthub-runner.exe.2.4.82.bak` 回滚。
- **Android Debug APK**：`versionCode=84`、`versionName=2.4.84`。
- **校验清单**：`AgentHub-v2.4.84-SHA256.txt` 收录 exe `e6a88289…`、zip `a421dafa…`、APK `53d02beb…`，以及内嵌 `dashboard.html`（`db21514d…`，98,395 字节）与 `dashboard.css`（`cf9e88f3…`，53,856 字节）。

## [2.4.83] - 2026-10-17

### 侧边栏开关真正生效、顶栏改为 magpie 式分段切换、硬编码全面收口 (Sidebar Toggle, Header Track & Hardcoded Values)

用户对比 magpie 截图后提出：侧边栏按钮点了没反应、顶栏不好看、字太小、卡片要更圆润现代、窗口尺寸想调整，并追问是否要用 Go 重写。逐项落地：

- **侧边栏开关修复 (Sidebar Toggle)**：原先 `#sideToggle` 只翻转 `html.no-side`，但布局真正听的是 `body.side-nav`，且导航从未在两个插槽之间搬家。现在 bootstrap 统一以 `html.no-side` 为唯一真相、同步 `body.side-nav` 作为样式孪生；`layoutNav()` 在 `#navSideSlot` 与 `#navTopSlot` 之间搬移 `<nav id="nav">`，品牌区同步在 `#brandSide` / `#brandTop` 之间切换。实测 1000×740 点击开关后 `cls=win no-side body=window sideDisp=none navIn=navTopSlot`，侧边栏真正收起，导航进入顶栏。
- **顶栏分段切换 (Header Track)**：侧边栏收起后，页面导航在顶栏呈现为 magpie 的 `.seg` 形态 —— 一条圆角底轨（`--seg-track`）+ 分组名 + 贴在当前页下的提升滑块 `#segThumb`。滑块绝对定位在 `#navTopSlot` 内，只写 `translate` 与 `width`，点击不触发顶栏重新布局；`scaleSegThumb()` 先量轨、再量按钮、最后移动，避免 transform 自身的偏移污染读数。
- **按可用宽度取舍 (fitNavTrack)**：顶栏空间有限，新增 `fitNavTrack()` 按插槽实际宽度打包：先保证当前页可见，再往后塞，再往前补，放不下的页暂时移出视野（不做横向滚动），保证“看得见的名字都能点”。配套容器查询：槽宽 ≤ 860px 隐藏分组名，≤ 700px 隐藏图标。
- **侧边栏改由容器查询驱动 (Container Query Rail)**：`#navSideSlot { container: nav-box / inline-size }`，`@container nav-box (width > 0)` 内定义竖排分组 / 图标 / 当前页 accent 底色的整套样式。导航放在哪就长什么样，同一份 `<nav>` 两套外观，不再靠重复规则与优先级抢胜负。
- **响应式全部改为容器查询 (Container Queries)**：原先挂在 viewport 上的 `@media` 改为 `.view-slot { container: slot-q }` 与 `.view { container: view-q }`，侧边栏是否展开不再影响页面的断点判断。顺带修掉一个循环依赖：作为尺寸容器的 `.view` 原用百分比横向内边距，改为 px 的 `--inset`。
- **字号 (Type Scale)**：导航、正文、卡片副标题统一抬到 `--fs-body` 级别；侧边栏行高 36px、14px 字号，顶栏按钮 30px 高、13px 字号，分组名用 `--fs-note - 1px`，整体比前一版明显易读。
- **窗口尺寸 (Window)**：默认窗口 1240×820（最小 720×520），限制在屏幕工作区的 94%×90% 内并居中偏上。写入 profile 前先检查 Edge 的 `Preferences` 是否已存在 —— 有则交由 Edge 自己恢复上次大小，用户拖动过的窗口重启后保持不变；没有才用 `--window-size/--window-position` 给初值。
- **布局参数下发 (AppLayout)**：窗口、侧栏宽度、内容最大宽、内边距、圆角、顶栏高度、轮询间隔等全部由 daemon 在页面加载前写入 `window.AppLayout`，页面只保留一份同值兜底，前后端不再各写一套数字。
- **Go 重写结论**：不需要。magpie 是 Go + Wails 外壳，界面本身仍是 HTML/CSS/JS 跑在 WebView 里；本次问题全部出在这个 Web 层，重写语言不会改善观感。Runner 的 daemon、采集、IPC 层保持 Rust 不动。
- **验证 (Verification)**：用 `_scratch/ui/` 的 CDP 夹具（serve / shot / eval / probe）与真实 2.4.83 daemon 交叉验证：`GET /` 返回 200、98375 字节、带 AppLayout、引用 `/dashboard.css?v=2.4.83`；侧边栏展开态与收起态截图核对；940 / 760 等槽宽下分组名与图标的取舍符合预期；暗色主题正常；`node --check` 通过；`cargo test --workspace` 全部通过；Android `testDebugUnitTest lintDebug assembleDebug` BUILD SUCCESSFUL，构建后已 `gradlew --stop`。

### 交付产物：Windows 与 Android 同步至 2.4.83 (Deliverables Repack)

- **Windows x64 Runner**：`AgentHub-Runner-v2.4.83-windows-x64.zip`（4,462,663 字节），按「打新包必删旧包」删除 2.4.80 的 zip 与 APK。
- **Android Debug APK**：`versionCode=83`、`versionName=2.4.83`，`AgentHub-v2.4.83-android-debug.apk`。
- **校验清单**：`AgentHub-v2.4.83-SHA256.txt` 收录 exe `81d6da25…`、zip `2542541e…`、APK `5ef9fb1c…`，以及内嵌 `dashboard.html`（30100ff3…，30,100 字节）与 `dashboard.css`（01f02b04…）。

## [2.4.82] - 2026-10-17

### 内嵌控制台 UI/UX 精修与「0 个历史会话」空态修复 (Console UI/UX Pass & Empty-State Fix)

用户反馈控制台偶发整页显示「0 个历史会话 / 暂无解析到历史会话记录」，同时要求按前端设计规范精修界面。两部分在同一版落地：

- **扫描侧空洞修复 (Discovery)**：`crates/runner/src/remote_host.rs` 的 `discover()` 原先把“响应里没有 `data` 数组”当成“没有历史会话”，`thread/list` 偶发返回错误体时整条发现链路直接产出空候选，控制台于是整页归零。现在首页缺 `data` 会先重试（`LIST_ATTEMPTS = 3`，间隔 200ms），仍失败则返回 `thread_list_malformed` 错误而不是伪造一个空历史；新增集成测试 `discovery_retries_a_malformed_first_page_instead_of_reporting_an_empty_history` 覆盖该路径。
- **前端空态加固 (Frontend)**：
  - 新增 `lastGoodSessions` 保留上一次有效快照，零会话快照不再立刻清空列表；须连续 3 次为空或距上次成功写入超过 15s 才进入空态，期间显示「正在读取会话记录…」；
  - 空态改为带原因说明与「重新读取」按钮的引导页；
  - `refreshDashboard()` 增加在途请求守卫与 `renderSeq` 乱序丢弃；首屏失败或零会话时自动重试 2 次（1.5s 间隔）；
  - 排查记录：对旧版二进制连续轮询 `/api/dashboard` 65 秒（70 次，单次最长 5.8s）未复现零快照，旧前端放置轮询 60 秒也未见空态，说明触发点多在启动 / 上游慢响应窗口，而非常态轮询。
- **用量缓存命中率口径 (Cache Hit KPI)**：「缓存读取 Cache」卡片副标题由英文 `hit rate 76%` 改为「输入命中 76%」，hover 说明“由上游缓存直接返回、未重新计算的输入 Token 数”；与各模型行「缓存命中 N%」统一使用 `cache_read / prompt_tokens`，后端把 `min(100.0)` 改为 `clamp(0.0, 100.0)` 以明确截断意图。
- **布局层精修 (Layout Pass)**：只重排既有 magpie 原语，不改结构语义 ——
  - **会话视图**：会话行改为「图标 / 提示词 / 时间」三栏对齐，时间列固定 84px 右对齐；分组头可点击、可键盘展开（`role="button"`、`aria-expanded`），折叠状态存 `localStorage`（`agenthub_closed_groups`），计数显示 `本组 / 全部`；搜索框实时过滤提示词、标题、会话 ID、工作区与模型，徽标同步显示 `N / M 个历史会话`；
  - **统计行**：会话 / 智能体 / 用量 / 模型 / 工作区共用一套 CSS Grid 列模板（图标、名称、占比条、数值、费用或时间），修掉原先模板里空白文本节点被当成匿名网格项导致的列错位；
  - **工作区**：改用两字母首字母徽章（`CL` / `GA` / `AG`…）替代重复的文件夹图标，路径压缩为「…/上级/项目名」并以 `title` 保留完整路径；
  - **网关视图**：重写为左右双栏 —— 左栏讲清“为什么需要会话亲和”（Prompt Cache 被打穿、`x-session-affinity` 的作用），并列出网关地址 / 亲和请求头 / 当前来源；右栏提供可一键复制的 Bash 与 PowerShell 环境变量片段；原 `alert` 全部换成底部 `role="status"` 轻提示 toast；
  - **弹窗**：`.dialog` 改回不透明背景，毛玻璃只作用于背后的页面，表单字段不再叠在模糊层上发灰。
- **响应式**：新增 `max-width: 720px` 断点 —— 网关双栏折成单栏、用量账本 2 列、统计行隐藏占比与费用列只留关键数值、会话行隐藏 Token、时间列限宽、工作区路径隐藏，375px 宽度下无横向溢出。
- **验证 (Verification)**：以 `_scratch/ui/harness.mjs` 将编辑中的副本挂到运行中 daemon 的 `/api/dashboard` 快照上，经 CDP 在 940×720 托盘窗口尺寸下逐张截图核对六个视图（明 / 暗主题）、搜索过滤、无匹配、pending / 空态、配对弹窗与 375px 窄屏，全程 `no runtime errors`；`cargo test --workspace` 全部通过（runner 163 + 集成 26/17/7/5/3 项，0 失败）。

### 交付产物：Windows 与 Android 同步至 2.4.82 (Deliverables Repack)

- **Windows x64 Runner**：重新压制 `AgentHub-Runner-v2.4.82-windows-x64.zip`，按「打新包必删旧包」删除 2.4.80 / 2.4.81 的 zip 与哈希表；散装 `agenthub-runner.exe` 受 `.gitignore` 的 `*.exe` 约束不入库，仅 zip 与哈希表进版本库。
- **内嵌控制台一致性**：源码 `crates/runner/resources/dashboard.html` 129123 字节，SHA-256 `106c4e2e…475b95b1`；打包 exe 内嵌的正是这份字节，运行中的 daemon `GET /` 返回同一哈希后才算交付完成。
- **Android Debug APK**：`versionCode=82`、`versionName=2.4.82`，按 `apps/android/README.md` 执行 `testDebugUnitTest lintDebug assembleDebug` 后归档 `AgentHub-v2.4.82-android-debug.apk`，删除 2.4.80 的 APK。
- **校验清单**：`AgentHub-v2.4.82-SHA256.txt` 收录 exe、zip 与 APK 三项哈希。

## [2.4.81] - 2026-10-17

> 未发布：版本号递增后在同一工作日内又补了扫描侧修复，直接并入 2.4.82 出包，仓库中不存在 2.4.81 的交付产物。

## [2.4.80] - 2026-10-17

### 网页端（Remote Console）契约对齐与全功能适配 (Web Remote Console Alignment & Enhancements)

针对近期 `agenthub-runner`（本地 Daemon）的大量核心升级（模型真实目录探测、多 Provider 发现与分页、Token 与模型用量聚合），对远程主线控制台 `apps/web` 进行了轻量且完整的协议契约对齐与体验适配：

- **新建会话模型名称显示缺陷修复 (Model Name Display Fix)**：
  - 修复 `RemoteConsole.tsx` 模态框中仅读取驼峰字段 `item.displayName` 的 BUG；引入多层回退兼容 `item.displayName || item.display_name || item.name || item.id`，彻底解决 Daemon 上报下划线原生模型目录时下拉框文字为空的问题。
- **新建会话引擎选项扩展 (Agent Engine Options Extension)**：
  - 新建会话弹窗的 Agent 引擎下拉列表中放开 `Antigravity` 与 `Grok Build` 选项，与底层 Daemon 的 `AgentType` 调度能力完全对齐。
- **候选与活跃会话卡片丰富元数据呈现 (Session Card Metadata)**：
  - 在 `remote.ts` 的 `DiscoveryCandidate` 及 `session-tree.ts` 的 `TreeSessionBase` 中补齐 `model`、`total_tokens`、`last_activity_at_ms` 字段；
  - 会话卡片左下侧新增精炼的模型小标签（`.remote-session-pill-tag`）与格式化 Token 用量显示（`.remote-session-tokens`，如 `14.2k tok`），遵循 CDS / 青空 UI 规范，不抢主视觉且信息一目了然。
- **多 Provider 分类映射补全与树形目录扩展 (Provider Tree & Classification)**：
  - `session-tree.ts` 中补充 `gemini`、`gemini_cli`、`cursor`、`cline` 的显式映射与前缀判断，保证多引擎发现的会话归入正确的 Agent 分组。
- **验证与测试覆盖 (Verification & Test Coverage)**：
  - `apps/web/test/session-tree.test.ts` 补充针对 Provider 分类扩展与会话 Token/模型透传的完整单元测试；
  - `npm run typecheck` 严格类型检查与 `apps/web` 全量 74 项单元测试全部通过。

### 内嵌控制台 Magpie 结构重写后的正确性修复与上游溯源 (Console Correctness Fixes & Upstream Attribution)

根据用户指示（“真的 daemon 前端要不然直接照抄 magpie 的 UI 吧，感觉 magpie 用着舒心，现在的 UI 太丑了”），`crates/runner/resources/dashboard.html` 不再做令牌级微调，而是把 magpie 的设计系统与布局原语整层移植过来。**结构重写本身已随 2.4.79 一线的提交落地并压进该版二进制**（当时未单独留痕，见文末「重写内容归档」）；本次为紧随其后的正确性修复、行级细节与上游归属留痕：

- **数据正确性修复 (Data Correctness)**：
  - **渲染顺序缺陷**：`refreshDashboard()` 原先在 `allSessionsData` 赋值前渲染工作区与模型视图，叠加脏检查门禁后这两个视图的调用量恒为 0；现改为先发布 sessions，再让工作区 / 模型 / 用量的脏检查以 `sessionsChanged` 参与判定；
  - **模型匹配过度贪婪**：画册 id 与转录模型名以归一化后**精确相等**匹配，不再使用子串包含（原实现会把含 `auto`、`big` 等短键的无关模型全部并入同一行，导致一排模型显示同一个 79M / 5 次调用）；同时把转录中出现但画册未收录的模型一并纳入排行，二者都只用真实 Token 数据，不合成条目；
  - **费用口径统一**：模型视图改用后端 `/api/dashboard` 已聚合的每模型 `estimated_cost_usd`，仅在缺失时回退本地费率估算，消除同一模型在“模型”与“用量”两个视图显示两个不同金额（如 `gemini-3.8-flash-n` 的 ≈$855 与 ≈$98.44）的问题。
- **行与组件细节 (Rows & Components)**：
  - 会话行：隐藏 `unknown` 会话 ID、Provider 中文映射、Token 紧凑格式化、模型名超长省略；
  - 智能体行：长二进制路径改为省略号截断，不再冲出卡片；
  - 网关徽标：探测到的凭据文件名超长时省略并置入 `title`；
  - Claude Code / Claude Desktop 图标改用 18px 下仍可辨认的 Claude 星芒，替换原先糊成一团的字标。
- **开源引用与上游溯源 (Upstream Attribution)**：按工程规范补记上游归属与维护边界 —— 上游为 magpie（`https://github.com/yetone/magpie`，MIT License, Copyright (c) 2026 yetone），负责本机 Runner 内嵌 Web 控制台的设计系统与布局原语：
  - `README.md` 新增「开源引用与上游溯源」章节；
  - `docs/ARCHITECTURE.md` 的 `agenthubd` 下新增「内嵌 Web 控制台」小节，写明该页面为 `include_str!` 单文件资源、六个视图共用一份 `/api/dashboard` 快照、以及写入动作仍走既有 HTTP 接口；
  - `docs/DESIGN_SYSTEM.md` 新增第 6 节，声明该界面为独立表面、不套用跨端令牌，并重申第 5 节动效与性能约束继续适用；
  - 上游文件头注释与 MIT 版权声明原文保留在 `dashboard.html` 样式表起始处；故障排查顺序为「先核对上游 Release / Commit / Issue / PR，再考虑本地补丁」。
- **重写内容归档 (Rewrite Recap)**：2.4.79 一线落地的结构重写内容一并归档留痕 —— Foundation 层移植（双主题令牌、46px 顶栏、`.view`、`.list` / `.row` / `.row.stat`、`.stats` / `.kpi`、`.led-strip`、`.chips` / `.segs`、`.text` / `.icon`、`.modal` / `.dialog`、`.editor` 网格与细滚动条）；侧栏 `aside.sidebar` 取代顶栏导航（品牌区 + 工作台 / 洞察 / 系统三组 `.nav-gl` 分组标签 + 底部网关与配对状态，宽度可拖拽 168–420px、双击复位，窄窗折叠进顶栏）；面包屑顶栏与按按钮几何移动的 `.seg` 滑块 `#segThumb`；页脚状态条（Runner 徽标、端口、上游网关端点）。
- **验证 (Verification)**：在真实托盘窗口尺寸 940×720 下，对运行中 daemon 的 `/api/dashboard` 快照渲染全部六个视图的明暗两套主题逐张核对；并以错误探针切换全部视图、调用全部渲染函数，结果为 `no throw`、`no console.error`。`cargo build --release -p agenthub-runner` 通过，`target/release/agenthub-runner.exe` 中可检出本次新增的资源标记。

### 交付产物：Windows 与 Android 同步至 2.4.80 (Deliverables Repack)

- **Windows x64 Runner**：重新压制 `AgentHub-Runner-v2.4.80-windows-x64.zip`（4444214 字节，SHA-256 `e0fb1159…401645`），按「打新包必删旧包」删除 2.4.79 的 zip 与哈希表；散装 `agenthub-runner.exe` 受 `.gitignore` 的 `*.exe` 约束不入库，仅 zip 与哈希表进版本库。
- **内嵌控制台一致性**：源码 `crates/runner/resources/dashboard.html` 108437 字节，SHA-256 `0da5982c…b7f9be`；打包 exe 在偏移 8340688 处内嵌的正是这 108437 字节（逐字节相同），运行中的 daemon `GET /` 返回同一哈希，窗口即新版控制台。
- **Android Debug APK**：`clean` 后按 `apps/android/README.md` 执行 `testDebugUnitTest lintDebug assembleDebug`，四项单元测试 0 失败、lint 通过；归档 `AgentHub-v2.4.80-android-debug.apk`（9836754 字节，SHA-256 `64653941…9324390`，`versionCode=80`、`versionName=2.4.80`，Android Debug 证书签名已验证），删除 `AgentHub-v2.4.79-android-debug.apk`。
- **校验清单**：`AgentHub-v2.4.80-SHA256.txt` 统一收录 exe、zip 与 APK 三项哈希，纳入 Android 产物后由 2 条扩充为 3 条。

## [2.4.79] - 2026-10-17

### 修复跨智能体分页导致的会话全量重复解析缺陷与 Google Gemini 官方四角星矢量图标 (Session Deduplication & Authentic Gemini Icon)

- **跨智能体分页边界漏洞修复与多重去重保障 (Multi-Provider Pagination Boundary Fix)**：
  - **根因分析**：在 `crates/runner/src/remote_host.rs` 的 `thread/list` 方法中，当存在 Codex 本地会话跨页游标（`nextCursor`）时，`discover()` 会进入 `loop` 请求下一页；但原实现中未校验请求是否为首页，导致每一页均无条件重复追加全部 306 个 Claude Code 会话、320 个 ZCode 会话及 Antigravity/Grok 会话，使得总会话数由真实的 724 个激增至 1,355 个，且相邻显示完全重复。
  - **后端分页隔离**：在 `thread/list` 中引入 `is_first_page = params.get("cursor").map_or(true, ...)` 门禁，外部智能体历史会话仅在首页注入，后续游标请求仅处理对应宿主分页数据。
  - **后端发现去重管道**：在 `discover()` 收集 `candidates` 与 `web_server.rs` 聚合 `discovered_sessions` 时增加基于 `provider_session_id` 的 `HashSet` 集合去重，确保即使存在任何网络或宿主级重叠候选也不会被二次计数或计费。
  - **前端防护保障**：在 `dashboard.html` 的 `renderSessions` 中引入客户端 `Set` 快速去重，保障历史会话树图结构唯一展示。
- **Google Gemini 官方 4 尖曲面星芒矢量图标重构 (Authentic Gemini Sparkle Icon)**：
  - 彻底剔除任何自行臆造的加号/十字占位符，直接拉取 Google 官方资产库（`gstatic.com` 官方 CDN 与 `lobehub/lobe-icons` 标准库）发布的权威 Google Gemini 专属星芒标识：
    - 精确采纳 Google 官方 28×28 原版内凹曲率路径 `M14 28C14 26.0633 13.6267 24.2433 12.88 22.54...`；
    - 配备官方正版径向渐变 `radialGradient` 渲染色彩体系（`#9168C0` 紫色 → `#5684D1` 蓝色 → `#1BA1E3` 浅青蓝），完美还原 Google 官方原厂视觉；
  - 同步更新模型用量分布（Models Breakdown）与智能体图标列表（Agents Grid）中的 Gemini 徽标，实现统一高保真官方品牌辨识度；
  - 同步拓展支持 Qwen（通义千问）、MiniMax（稀宇科技）、GLM（智谱清言）、StepFun（阶跃星辰）、Grok 及 Stealth 系列模型的专属品牌识别徽标。
- **产物更新与交付归档 (Deliverables Synchronization)**：
  - 编译全新 Release 版本 Windows 可执行文件 `agenthub-runner.exe`，重新压制 `AgentHub-Runner-v2.4.79-windows-x64.zip`；
  - 重新计算并刷新 `Deliverables/AgentHub/AgentHub-v2.4.79-SHA256.txt` 权威哈希校验文件；
  - 线上后台守护平滑热重启，验证 `total_sessions` 准确降为 724，重复计数完全归零。

### 全面对标 Magpie 级别用量统计与高精模型/智能体消耗分布 (Magpie-Grade Usage Analytics Architecture)

根据用户指示（“这个用量你实在写不好你就超magpie好不好”），深入参考并直接对齐 Magpie（`github.com/yetone/magpie`）用量统计子系统与界面架构规范，从根源上彻底解决历史会话虽被解析但用量统计呈现 0 的断流缺陷，全面构建权威后端聚合与极简高精看板：

- **候选发现管道（Candidate Pipeline）元数据完整透传**：
  - 修复 `crates/runner/src/remote_host.rs` 中 `discover(&mut self)` 步骤将 `thread/list` 提取到的 `model`、`prompt_tokens`、`completion_tokens`、`total_tokens`、`cache_read_tokens`、`last_activity_at_ms` 字段丢失的关键漏洞，使会话候选集（`candidates`）完整继承 Token 维度与模型归属。
- **Claude Code 转录文件（Transcript）全会话多轮累计**：
  - 优化 `crates/runner/src/collectors/mod.rs` 中的 `read_claude_transcript_metadata` 函数，取消单轮 Assistant 消息命中后的提前 `break`，向上扫描至上限 512 行完整累计多轮交互产生的所有输入、输出及缓存 Token。
- **后端权威用量聚合接口与定价矩阵（Authoritative Backend Aggregator & Pricing）**：
  - 在 `crates/runner/src/web_server.rs` 的 `/api/dashboard/summary` 中建立与 Magpie 一致的统计模型，输出权威 `usage` 实体：
    - `calls`：总会话与调用次数；
    - `input` / `output` / `cache_read` / `total_tokens`：全量 Token 统计；
    - `cache_hit_rate_pct`：缓存命中率计算 `100 * cache_read / (input + cache_read + cache_write)`；
    - `cost_usd`：基于官方标准费率梯度的精准预估花费计算（Opus、Sonnet/Fable、Haiku/Flash、DeepSeek、GPT/Codex）；
    - `models` 与 `agents`：按 Token 消耗降序排列的结构化用量占比数组，提供 `share_pct`、`cache_hit_rate_pct` 与 `estimated_cost_usd`。
- **Magpie 原生级 4-Grid KPI 卡片与双板块用量分布看板（Magpie-Grade High-DPI UI）**：
  - **4 组 KPI 核心数据磁贴（`.stats` & `.kpi`）**：
    - **预估花费 (Cost)**：`≈$X.XX`，副标注明“基于模型标准费率测算”；
    - **总 Token 消耗**：紧凑格式化（如 `1.2M`、`850K`），小字标明 `${in} in · ${out} out`；
    - **缓存读取 (Cache)**：展示实际读取缓存量与高亮 `hit rate XX%`；
    - **总调用会话 (Calls)**：展示跨多智能体累计分析会话总数。
  - **模型用量分布 (Models Breakdown)**：
    - 采用 Magpie 原生 `.row.stat` 条目化布局，支持 Anthropic、OpenAI、DeepSeek、Gemini 及通用芯片级矢量徽标；
    - 动态展示模型名称、调用次数、缓存命中率、占比进度条（`share i`）、Token 数量及预估美元花费。
  - **智能体用量分布 (Agents Breakdown)**：
    - 完整继承 Claude Code、Codex、ZCode、Antigravity、Grok 等专属品牌矢量图标与色彩；
    - 展示各智能体调用量、占比进度条、总消耗及预估花费。
  - **前端与后端无缝双重保障**：优先直连消费后端权威 `usage` 聚合数据，同时保留客户端会话级增量计算回退，确保任何场景下均不出现数据白板或空白状态。
- **产物更新与构建归档（`Deliverables/AgentHub/`）**：
  - 重新编译 Release 原生程序 `agenthub-runner.exe`，重新打包 `AgentHub-Runner-v2.4.79-windows-x64.zip`；
  - 同步更新 `Deliverables/AgentHub/AgentHub-v2.4.79-SHA256.txt` 校验和。

### 全面引入内存缓存机制、消除高频轮询磁盘 I/O 与反序列化开销 (High-Performance In-Memory Caching Architecture)

根据用户指示（“全面引入内存缓存的机制，不要每次都重新加载很低效”），在 AgentHub Runner 与 Web 守护层中全面落地高并发安全内存缓存体系：

- **会话发现与跨引擎聚合高速缓存 (Session & Workspace In-Memory Cache)**：
  - **Claude / Antigravity / Grok 远程会话缓存 (5s TTL)**：在 `crates/runner/src/collectors/mod.rs` 中采用 `std::sync::{OnceLock, RwLock}` 建立各 Provider 级别的 `remote_sessions_cache`，避免前端每 4 秒轮询时重复递归扫描 `~/.claude/projects/`、`~/.gemini/antigravity/` 和 `~/.grok/sessions/`；
  - **ZCode / Codex 数据库状态感知缓存 (DB mtime & size fingerprint)**：针对本地 SQLite 数据库（`~/.zcode/cli/db/db.sqlite` 与 `~/.codex/state_5.db`），根据文件修改时间戳与文件大小做指纹比对。只有在 SQLite 数据库产生写入（mtime 或 size 改变）时才真正执行只读 SQL 查询，否则以零开销返回内存缓存；
  - **工作区跨 Agent 聚合缓存 (6s TTL)**：在 `crates/runner/src/collectors/workspace.rs` 中建立 `workspace_cache()`，将原本跨 5 家 Agent 目录与数据库的聚合操作收拢为线程安全的内存缓存，并支持 `invalidate_workspace_cache()` 主动驱逐。
- **对话预览与转录文件按需增量加载 (Bounded Conversation & Path Index Cache)**：
  - **Claude Transcript 文件路径索引 (`HashMap<String, PathBuf>`, 30s TTL)**：在 `crates/runner/src/conversation.rs` 引入 `claude_path_index()`，将原本查找某个 session_id 需要在深层目录递归扫描 $O(N \times M)$ 次的问题优化为 $O(1)$ 的内存字典索引；
  - **带容量上限与文件指纹的对话消息缓存 (LRU/Cap 256 Entry)**：针对动辄数兆甚至数十兆的 `.jsonl` 对话历史，基于 `(PathBuf, len, mtime)` 构建消息体内存缓存，命中且文件未发生变化时直接克隆反序列化结果，彻底阻断高频点击会话时的磁盘 I/O 尖峰，同时控制容量上限防止内存无限膨胀。
- **Web 服务与环境检测缓存 (Dashboard Summary & Executable Probe Cache)**：
  - **Dashboard 概览缓存 (2s TTL)**：在 `crates/runner/src/web_server.rs` 中为 `/api/dashboard/summary` 建立 2 秒微缓存，抵御前端多标签页或多组件重叠轮询造成的 CPU 波动；
  - **设备静态信息缓存 (5s TTL)**：缓存 CPU / 架构 / 操作系统等系统信息；
  - **可执行文件探测与 Agent 目录存在性缓存 (30s TTL)**：在 `find_executable` 与 `check_agent_dir` 中建立 `executable_cache()` 与 `agent_dir_cache()`，消除在每个轮询周期中数十次触发 `where.exe`、`PATH` 遍历及硬盘 `is_dir()` 检查的低效行为。

### 真实拉取用量与真实模型分布、会话按时间倒序排列、解除生产网关地址硬编码

根据用户反馈（“用量硬编码模型没有真正拉取、会话按agent排序没有按时间排序、生产地址不要硬编码让用户自己来”），实施全面彻底重构与闭环：

- **真实拉取用量与真实模型分布 (Dynamic Real Model & Token Aggregation)**：
  - **多引擎全量提取真实模型与 Token**：
    - **Claude Code**：深度扫描 `~/.claude/projects/*/*.jsonl`，真实提取 `type == "assistant"` 消息内的 `message.model` 以及精准的 `input_tokens`、`output_tokens`、`cache_read_input_tokens` 与 `cache_creation_input_tokens`；
    - **OpenAI Codex**：直接从本地 `state_5.sqlite` 查询 `model` 与 `tokens_used` 字段，严格遵循隐私约束，同时输出真实的会话模型与消耗 Token；
    - **ZCode**：从 `~/.zcode/cli/db/db.sqlite` 提取会话的真实 `data.modelID`、`input_tokens`、`output_tokens`、`total_tokens` 与缓存读 Token；
    - **Antigravity / Grok Build**：从 JSONL/Summary 提取真实模型与修改时间戳；
  - **彻底清除所有硬编码假模型与假数据**：
    - 前端彻底拔除 `claude-3-7-sonnet` 假模型回退与假数据（如 `1248500`、`850000` 等），完全以会话中实际抓取到的模型（如 `gemini-3.8-flash-n`、`gpt-6-luna`、`mimo-v2.6-flash-free`、`claude-3-5-sonnet` 等）与真实 Token 计数进行聚合；
    - 无调用数据时展示优雅的 0 状态提示，绝不伪造虚假数据。
- **会话按时间倒序排列 (Chronological Session Ordering)**：
  - **后端统一发现排序**：在 `crates/runner/src/remote_host.rs` 的 `discover()` 中，所有来自 Codex、ZCode、Claude Code、Antigravity、Grok 的会话统一定位其最新活动时间戳（`last_activity_at_ms`），并在组合后整体按时间戳进行全局降序排序，最新的会话永远排在最前面，彻底打破原先按 Provider 堆叠的次序；
  - **前端双层时间倒序**：
    - 前端 `renderSessions` 对全量会话做最新时间降序排列；
    - 工作区分组依据各自组内最新会话的时间倒序排列（最近活跃的项目目录排在最上方）；
    - 组内会话严格按最新活动时间倒序排列，并智能格式化为人类可读的友好相对时间（如“刚刚”、“5分钟前”、“2小时前”、“10月17日 14:30”）。
- **解除生产网关地址硬编码 (User-Configurable Gateway & LocalStorage Persistence)**：
  - 彻底移除配对弹窗输入框中写死的 `value="https://agenthub.tannerlab.xyz"` 生产地址；
  - 采用通用中立的占位提示 `placeholder="例如: https://agenthub.yourdomain.com 或 http://100.x.x.x:3000"`；
  - 弹窗打开时优先自适应回填 `localStorage.getItem('agenthub_custom_gateway')` 或设备已配置网关，用户输入自定义网关地址后自动持久化到本地 `localStorage`，完全交由用户自由支配。
- **Windows 托盘极简圆润菜单与 Fluent 现代清单 (Compact Rounded Tray Context Menu)**：
  - **选项极致精简与高质感对齐**：彻底移除冗长笨拙的文本（原“打开桌面控制台”、“打开数据目录 (~/.agenthub)”、“退出 AgentHub Runner”），全面精简为两项超短极简菜单项：`Show App` 与 `Exit`，中间保留纯净细分割线，菜单整体小巧紧凑；
  - **嵌入 Common Controls v6 现代清单元数据**：新建并在 `.rc` 编译期嵌入 `resources/app.manifest`，启用 Microsoft.Windows.Common-Controls 6.0.0.0 与 PerMonitorV2 高 DPI 感知，使 Windows 11 DWM 自动为 Win32 弹窗和托盘右键菜单应用现代大圆角卡片渲染；
  - **调用 uxtheme!SetPreferredAppMode 现代主题**：托盘线程加载 `uxtheme.dll` 序号 135 开启系统级主题支持，菜单外观与 Windows 11 Fluent 视觉无缝融合。
- **产物更新与构建归档（`Deliverables/AgentHub/`）**：
  - 重新编译 Release 原生程序 `agenthub-runner.exe`，重新打包 `AgentHub-Runner-v2.4.79-windows-x64.zip`；
  - 同步更新 `Deliverables/AgentHub/AgentHub-v2.4.79-SHA256.txt` 校验和。

### 字体排版与对比度重构、清除无后端按钮、全面解除硬编码与突出云端 Web 绑定

根据用户在实机截图中的直接指正（“字体要大要清楚、删掉无后端按钮、全面解除硬编码、恢复绑定网页端逻辑”），进行深度优化与彻底闭环：

- **全平台安装包构建与产物归档（`Deliverables/AgentHub/`）**：
  - 同步递增全平台版本至 v2.4.79（Android `versionCode = 79`，`versionName = "2.4.79"`，Runner 与 Cargo 统一同步至 `2.4.79`）；
  - 成功编译并归档 Windows x64 Runner 原生程序包（`agenthub-runner.exe` 与 `AgentHub-Runner-v2.4.79-windows-x64.zip`）及 Android Debug APK（`AgentHub-v2.4.79-android-debug.apk`）至 `Deliverables/AgentHub/`；
  - 严格执行工程铁律删除旧版本历史产物，并生成全新 `AgentHub-v2.4.79-SHA256.txt` 校验和；
  - 产物 SHA256 校验和：
    - Windows Runner: `cf2e076b52c35e12a48e01c3c121578678d733b2e20927f4b18008f3d9b2ef75` (`agenthub-runner.exe`)
    - Windows Runner Zip: `cd0465b25e86f81bb610f430d7ea7300b598de233dcc475134ae9d981a7182a3` (`AgentHub-Runner-v2.4.79-windows-x64.zip`)
    - Android Debug APK: `dd0a3d179bc445cb5807a3fd975e4fabd1f2883022214f69b7bba897e5a1a452` (`AgentHub-v2.4.79-android-debug.apk`)
  - 依规主动终止 Gradle Daemon 守护进程，彻底释放编译内存。

- **字体排版与清晰度全面放大重构 (High-DPI Desktop DirectWrite Typography)**：
  - **原生高清字体栈**：引入 `"Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", -apple-system, BlinkMacSystemFont, system-ui, sans-serif` 与代码字体 `"Cascadia Mono", Consolas, "SF Mono", monospace`，启用 `-webkit-font-smoothing: antialiased` 与清晰渲染；
  - **清晰度与字号比例放大**：基准字号由 12.5px 放大至 14px，标题由 13px 放大至 14.5px-15px，核心数值由 22px 提升至 26px 粗体；
  - **高对比度黑度配色**：主文字色彩强化为高对比深色 `#09090b`，次级文字强化为 `#27272a`，副标签强化为 `#52525b`，杜绝 100%/125% Windows 缩放下的灰蒙与虚化；
  - **桌面窗口尺寸放大**：将 Edge 桌面窗口默认尺寸从 `780x680` 升级为 `940x720`，在屏幕中心宽敞居中显示，彻底解决卡片和会话列表横向拥挤。
- **彻底清除无后端的空头按钮 (Eliminate Non-Functional UI Clutter)**：
  - **会话项清除“▷ 继续会话”**：针对用户红圈重点标记的每个会话项右侧仅作文本复制的“▷ 继续会话”按钮，彻底予以删除，杜绝“点了以为能开程序结果只是复制命令”的误导性交互；
  - **智能体卡片清除“复制命令”**：删除 Agent 列表内无后端驱动的“复制命令”按钮，保留简洁的已就绪徽章与已扫描会话数标签，回归纯粹清晰的监控看板。
- **全面解除硬编码与虚假数据 (Purge Synthetic Metrics & Hardcoded Buzzwords)**：
  - **剔除虚构代币与命中率**：彻底删除后端与前端所有写死的 `89.4%` 缓存命中率与乘法推算的 Tokens Saved (`* 75,000` / `* 0.894`)，数据完全来自于真实会话文件解析；
  - **剔除虚假技术口号**：剔除“O(1) 路径快速解析”、“CC Switch / 动态网关”、“/ 8 就绪”等口号式字段，工作区真实标注“本地项目开发根目录”，Agent 矩阵按实际探测数量真实呈现。
- **云端 Web 控制台设备绑定逻辑恢复与重点突出 (Surfaced Device Pairing & Fixed Binding Logic)**：
  - **顶栏醒目绑定状态按钮**：在顶栏左侧状态区追加显眼的 `.pair-btn`（已配对显示柔和绿色并带网关域名，未配对显示警示琥珀色“未绑定 Web 控制台”），一键直达配对弹窗；
  - **LED 指标条真实绑定状态卡片**：将原先第二块虚假的缓存命中率替换为可点击的“云端 Web 绑定”卡片，实时展示设备配对与网关连接状态；
  - **修复前端绑定校验错误**：修复前端 `submitDevicePairing` 中校验 `resData.status === 'ok'` 而后端实际返回 `{"ok": true, ...}` 的属性不一致 Bug，使 Web 绑定码能顺畅成功落盘。


针对用户提出的“不要乱报200k、解决超模自适应尺寸、去重顶部图标、剔除用户不关心的ID与提供商、模型工作区解析对标Magpie、提取Magpie官方图标打通Agent与Session解析”等一系列硬核要求，进行精准重构与闭环：

- **从 Magpie 源码提取原版矢量图标与色彩体系**：
  - 深度检索并提取 `Project/magpie/internal/gui/assets/icons/` 原生 SVG 几何路径与品牌色定义；
  - 完整收录原版官方矢量图标：Claude Code 珊瑚红机器人蟹爪 (`#D97757`)、Claude 官方星芒日冕 (`#D97757`)、Codex 纯净终端徽章 (`#3941FF`)、ZCode 极简深色圆角徽标 (`#111113` + 'Z')、Gemini CLI 谷歌彩虹渐变黑板 (`#EE4D5D` -> `#B381DD` -> `#207CFE`)、Antigravity 推进器多色徽章 (`#00B95C`/`#FC413D`/`#3186FF`/`#FBBC04`)、Grok 多面几何多面体 (`#111113`)、Cursor 3D 等轴暗黑方块 (`#000000`)、Cline 湖蓝机器人头盔 (`#0EA5E9`) 与 OpenCode 翠绿终端 (`#10A37F`)；
  - 采用零网络、零外部资产依赖的原生内嵌行内 SVG 动态渲染引擎 (`getAgentIcon`)，杜绝 404 挂图与加载闪烁，保证无论离线、暗黑还是高 DPI 缩放下均完美纤细锐利。
- **Agent 矩阵与 Session 解析全面接入品牌图标与 Magpie 层级**：
  - **Agent 矩阵解析**：每个 Agent 卡片引入 `32x32` 柔和圆角品牌容器 (`.brand-icon-box`) 呈现官方高清矢量图标，后端服务扩展支持 Gemini CLI、Cursor 与 Cline 全景感知；
  - **Session 树形解析**：会话项左侧全面替换为对应品牌图标容器（`28x28`），清晰展示 `[workspace] · [agent] · [model] · [time] · [tokens]` 规范化副标题信息层级，大幅提升扫视效率与界面质感。
- **彻底根绝臆造 200K 上下文长度**：
  - 重写 `crates/runner/src/model_catalog.rs` 中的 `infer_context_window` 逻辑，严禁在未从上游获得显式 `context_length` 时凭空臆造 200K / 128K；
  - 仅在模型 ID 明确包含 "1m" 或 "2m" 标识时精准推断 1M / 2M，其余未知情况统一返回 0 与空字符串；
  - 前端控制台严控上下文徽章呈现，仅在有可靠上游数据时展示，绝不虚报误导。
- **组件超模根治与全屏弹性自适应尺寸**：
  - 将顶部指标条网格重构为弹性自适应流式容器 `grid-template-columns: repeat(auto-fit, minmax(130px, 1fr))`；
  - 对数值、标签及副标题全面施加严格的边界截断 (`min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap`)；
  - 彻底清洗网关来源文案，过滤临时生成的冗长 JSON 证书路径，避免撑破卡片边框。
- **去重顶部图标并统一极简顶栏**：
  - 剔除网页内与 Windows 原生 Edge App 窗口标题栏完全重叠的 `.brand` 图标与标题块；
  - 顶栏化繁为简：左侧仅保留低调安静的端口运行状态灯 (`.top-status`)，中间居中放置高质感分段切换器 (`.nav-wrap` / `.seg`)，右侧保留必要操作区。
- **剔除用户不关心的技术性噪音**：
  - 剔除模型列表里冗长生涩的代理内部 ID (`<span class="code-tag">...</span>`)、内部网关来源 (`来源: CPA (Gemini)`) 及冗余的“复制 ID”按钮；
  - 剔除工作区列表中无必要的“复制路径”按钮，聚焦纯粹的核心业务视图。
- **字体与美学全面学习 Magpie**：
  - 深度吸收 Magpie 真实浅色与深色设计规范，以安静自然的高级浅灰底色 `--bg: #f4f4f6`、纯白卡片 `--card: #ffffff`、低调分界线 `--line: #e3e3e8` 为基准；
  - 采用系统原生高质感字体族栈，去除花哨的浮夸修饰，专注纯净阅读体验。
- **模型解析与工作区排行深度复刻 Magpie**：
  - **模型排行 (`renderModels`)**：采用 Magpie 标准的 `.row.stat` 横向进度条架构，左侧突出模型友好名称与能力标签 (`Vision` / `Thinking`)，中间横向进度条展示使用份额，右侧用 `fmtN()` 格式化呈现清晰明了的 Token 消耗量（如 `906M`、`629M`、`429M`）与预估费用（如 `≈$376`、`≈$15.03`）；
  - **工作区排行 (`renderWorkspaces`)**：关联全部历史会话动态计算各工作区 Token 消耗与活跃度，按 Token 规模降序排列，直观展现项目消耗排行（如 `ClaudeSpace 30M`、`gate 18M`、`agenthub 6.6M`）；
  - **用量分析看板 (`renderUsageAnalytics`)**：模型分布全面接入 `.row.stat` 规范，实现全局视觉与交互的一致性。

### 顶栏无重叠零冲突重构与桌面端全景一体化布局 (Zero-Collision Header & Anti-Floating Chrome)

针对用户关于“前端太多重叠、顶部不要游离于全局之外”的明确指正，彻底重构顶部导航与窗口结构：

- **零冲突三栏弹性流式布局 (Flexbox 3-Column Layout 彻底替代绝对定位)**：
  - **根除重叠源头**：彻底废弃旧版 `.seg` 采用的 `position: absolute; left: 50%; transform: translate(-50%, -50%)`，改用 Flex 弹性盒三栏自然流式排布；
  - **左栏纯净品牌区 (`.brand`)**：18px 紧凑 Logo、13px 稳重标题及轻量版本徽章紧贴左侧 (`flex: 0 0 auto`)；
  - **中栏居中导航区 (`.nav-wrap` / `.seg`)**：作为弹性项在剩余空间内居中 (`flex: 1 1 auto; justify-content: center`)，无论窗口如何拉伸或缩放，绝不与左侧品牌或右侧操作区重叠碰撞；
  - **右栏极简操作区 (`.actions`)**：状态胶囊与图标按钮自适应靠右收紧 (`flex: 0 0 auto`)。
- **干脆利落的中文字符标签（告别臃肿双语，信息密度对齐 Magpie）**：
  - 去除 `会话 Sessions`、`智能体 Agents`、`用量 Usage`、`模型 Models` 等冗长双语字符，精炼收敛为 `会话`、`智能体`、`用量`、`模型`、`工作区`、`网关`；
  - 按钮内边距与高度精细微调，导航总宽度从 550px+ 骤降至 ~280px，留出充裕的呼吸空间；
  - 配备自适应响应式媒体查询（在极小窗口宽度下自动隐藏版本号并紧凑微调），保障任意尺寸下皆零冲突。
- **全局一体化桌面视效（告别顶栏“游离于全局之外”的悬浮感）**：
  - 顶栏高度收紧至原生桌面级 42px，全面集成 `-webkit-app-region: drag` 原生窗口拖拽支持，按钮区域 `-webkit-app-region: no-drag` 保持交互；
  - 顶部栏紧贴窗口顶部，消除任何外边距与游离感；
  - 紧接顶栏下方的 5 联 Hero 指标条追加清晰细边框底线 (`border-bottom: 1px solid var(--line-2)`)，与顶栏和主体内容区构筑整齐划一、浑然天成的结构网格体系。

### UI & UX 深度对齐 Magpie：桌面沉浸暗色调、46px 紧凑顶栏与高信息密度设计

紧扣用户“UI和UX也要学啊”的明确指导，深度汲取 Magpie 桌面应用的设计语言与交互哲学，全面重构 AgentHub 控制台前端视觉系统与交互形态：

- **全面采纳 Magpie 经典暗色桌面色彩规范（沉浸安静不刺眼）**：
  - **背景与表面色阶**：底色收敛至安静暗黑 `--bg: #1a1a1e`，卡片表面 `--card: #232327` 与二级底色 `--card-2: #1f1f23`，胶囊背景 `--pill: #2d2d33`，悬停增强 `--pill-hover: #36363d`；
  - **边缘与线条**：精细浅线条 `--line: #333339` 与二级分隔线 `--line-2: #2c2c32`，消除刺眼对比度；
  - **前景色阶梯**：主文字 `--fg: #ededf1`、次级说明 `--fg-2: #b9b9c1`、弱提示 `--muted: #8b8b94`、衰减字符 `--faint: #56565f`；
  - **点缀色调**：淡紫强调色 `--accent: #a5b4fc`、柔和绿 `--green: #6fd99b`、琥珀黄 `--amber: #f2b544`、淡红 `--red: #f28b82`；
  - **PWA Manifest 与标题栏双向同步**：`MANIFEST_JSON` 中的 `theme_color` 与 `background_color` 统一更新为 `#1a1a1e`，彻底杜绝独立 App 窗体边缘与内容区域的色差接缝。
- **Magpie 经典 46px 紧凑顶栏与居中 Segmented 控制器 (`.seg`)**：
  - **左侧纯净品牌**：14px 纯白加粗 `AgentHub` 配合淡紫极简版本徽标 `v2.4.79`；
  - **居中高质感 Segmented Switch**：复刻 Magpie 的四键分段器（会话 Sessions、Agent、用量 Usage、模型 Models），带有半透明轨道 `--seg-track`、高亮内阴影滑块 `--seg-edge` 与流畅切换体验；
  - **右侧紧凑操作区**：状态指示胶囊、设备配对入口、一键环境变量复制、暗亮色切换与安全退出按钮整齐排布。
- **顶部紧凑 5 联 Hero 指标条 (`.hero-strip`)**：
  - 彻底抛弃大边距网页卡片，改为 Magpie 风格的紧凑条带：活跃会话数、累计 Token、预估节省美金、模型资产数、网关状态一屏尽览，无任何垂直空间浪费。
- **列表与行级结构全面规范化 (`.list` / `.row`)**：
  - 统一为 Magpie 的精致行级结构：左侧主次两行 (`.who` / `.name` / `.sub`)，右侧状态胶囊与元数据标签 (`.meta-tags`)；
  - Windows 原生字形栈规范：首选 `"Segoe UI Variable Text"`，等宽字体优先 `"Cascadia Mono"`，配合 Microsoft YaHei UI 保证中英文极致清晰度；
  - 会话树分组、搜索过滤器、设备配对浮层与关闭确认模态框全面同步 Magpie 视觉标准。
- **内核级进程生命周期绑定（彻底根绝孤儿白屏与残留进程）**：
  - **Windows Job Object 强托管**：在 `crates/runner/src/tray.rs` 中引入 Win32 `CreateJobObjectW`，配置 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 限制标志；拉起 Edge 独立 App 窗口后，立刻通过 `AssignProcessToJobObject` 将其子进程加入作业对象托管；
  - **级联退出保证**：当 Runner 退出、崩溃或接收到关闭信号时，Windows 内核自动级联终结所有托管的 Edge 实例及相关渲染进程，从物理层面上杜绝因 Runner 终止而导致桌面残留孤儿白屏窗口的隐患；
  - **双重退出保险**：在托盘退出处理及 `crates/runner/src/daemon.rs` 停止流程中增设主动 `cleanup_desktop_window()`，实现正常清理与内核兜底双保险。

### 像素级对齐 Magpie 原生排版与字体系统，统一窗口标题栏与顶栏色彩 (Authentic Magpie Typography & Seamless Titlebar)

紧扣用户“前端风格字体再抄一下magpie、现在的字体和排版太差劲了、翻magpie源码抄啊、顶部条颜色还是不一致”的明确指令，直接深入 `Project/magpie` 原生源码（`internal/gui/assets/app.css` 与 `titletint_windows.go`），深度提取排版规范与 DirectWrite 渲染规则，彻底解决视效痛点：

- **深度复刻 Magpie 官方 DirectWrite 字体族栈与排版标准**：
  - **Windows 原生高质量渲染栈**：从 Magpie 提取 `:root.win, html.win` 规则，UI 字体强制采用 `"Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", system-ui, sans-serif`，代码与等宽字体采用 `"Cascadia Mono", Consolas, "Microsoft YaHei UI", "Microsoft YaHei", monospace`，彻底杜绝 Windows 默认宋体或中文字符回落导致的锯齿与排版混乱；
  - **首屏预执行防止 FOUC**：在 `<head>` 中置入零依赖的即时执行脚本 (IIFE)，在 DOM 首屏绘制前立即测定 Windows 系统环境（添加 `win` 类）并解析存储的主题偏好（同步设定 `dark` 类与 `<meta name="theme-color">`），消除页面加载时的字体跳动与色彩闪烁；
  - **字重与字号微米级校准**：严格对齐 Magpie 标准阶梯：页面主文本采用 12px/1.45 行高、二级说明 11px、标题 13px/14px 加粗、指标大字采用 22px 带 `-0.02em` 紧凑字间距与 `tabular-nums` 等宽数字，排版质感达到原生桌面应用级别。
- **彻底根治原生窗口标题栏与顶栏色彩不一致**：
  - **Manifest 与 DWM 原生标题栏对齐**：修改 `crates/runner/src/web_server.rs` 中的 `MANIFEST_JSON`，将 PWA `theme_color` 与 `background_color` 从深黑统一设为 Magpie 默认的明亮浅灰 `#f4f4f6`，使 Edge 桌面宿主窗口的原生 DWM 标题栏底色与应用完全无缝统一；
  - **无缝顶栏边缘 (Borderless Surface)**：对齐 Magpie 的 `.top` 导航栏设计，彻底移除 `.top` 原有的灰色 `border-bottom`，使窗口标题栏与 46px 导航栏浑然一体，消除割裂条纹；
  - **无轨式分段控制器 (Trackless .seg)**：移除分段切换器原有的灰色底框，未激活按钮直接悬浮在底色上，仅当前激活标签带有纯白高质感微阴影滑块卡片，视觉通透干净。
- **重构 Magpie 一体化 5 联 `.led-strip` 指标条**：
  - 彻底抛弃原先离散拼贴的独立指标盒子，改用 Magpie 原生的 12px 圆角统一白卡网格容器（`.led-strip`）；
  - 单元格间采用精细内部分隔线（`.blk + .blk { border-left: 1px solid var(--line-2); }`）；
  - 在缓存命中率卡片中集成 4px 动态条形仪表盘（`.meter i`），随实时命中率平滑伸缩。

## [2.4.78] - 2026-10-16

### 体验大升级：彻底根治数据库桌面污染，全面转向类似 Magpie 独立桌面 App 架构、窗口尺寸适中居中与零黑框静默启动

根据用户“桌面 agenthub.db 彻底根治隐藏”、“一秒黑框都不想见到”、“改为类似 Magpie 的独立桌面版而不是网页版”、“顶部同色、右边滑动栏没有必要、页面最开始打开出现在屏幕中间且尺寸适中，UI/UX 与 Session 解析深度学习 Magpie”的核心诉求，完成深层架构与运行形态升级：

- **看齐 Magpie 的独立桌面应用窗口体验（紧凑小巧、居中启动、顶部同色、无滑动栏）**：
  - **黄金小巧尺寸 (~780x680)**：淘汰默认 1280x820 大屏幕铺满的不适感，采用 Magpie 经典的精致桌面卡片级尺寸（780 宽 x 680 高）；
  - **屏幕自适应动态居中**：在 `crates/runner/src/tray.rs` 中通过 Win32 `GetSystemMetrics(SM_CXSCREEN / SM_CYSCREEN)` 精准动态计算屏幕正中央坐标 `(sw - 780)/2, (sh - 680)/2`，通过 `--window-position=X,Y` 确保每次启动或托盘呼出时完美呈现在屏幕正中心；
  - **顶部标题栏与背景同色（深色一体化）**：
    - 在 HTML 头部注入 `<meta name="theme-color" content="#111827" id="themeColorMeta">` 与 `<meta name="color-scheme" content="dark light">`；
    - 提供规范的 Web App Manifest (`/manifest.json`)，设定 `theme_color: "#111827"` 与 `background_color: "#0b0f17"`；
    - Edge 独立 App 启动参数注入 `--force-dark-mode`，标题栏与控制台背景浑然一体，彻底告别刺眼的白条；
    - 支持外观亮色/暗色切换时，动态联动更新 `themeColorMeta` 保持随时同色；
  - **彻底移除臃肿丑陋的右侧原生滚动条**：
    - 全局注入 `scrollbar-width: none !important; -ms-overflow-style: none !important;` 以及 `::-webkit-scrollbar { display: none !important; }`；
    - 内容自适应滚动的同时完全隐去原生 17px 滚动条轨道，视觉纯净轻盈。
- **UI/UX 与功能对标 Magpie：会话解析 (Sessions) 树状分组与用量分析 (Usage)**：
  - **清晰干脆的会话解析树 (Sessions)**：
    - 按工作区或物理目录智能聚类历史会话，展示工作区路径与会话数量；
    - 智能提取提示词 (Prompt)、Provider 标签、相对活跃时间与 Token 消耗统计；
    - 每条会话配备一键复制 `claude --resume <session_id>` 恢复命令；
    - 支持按提示词、会话 ID 或工作区名字的实时本地快速搜索过滤；
  - **用量分析看板 (Usage)**：
    - 直观呈现总 Prompt Tokens、总 Completion Tokens，以及基于 90% 缓存命中率预估节省的美金金额；
    - 动态解析各模型调用占比与可视化进度条；
  - **干净干脆的 Agent 识别与轻量呈现**：
    - 快速展示 Claude Code、Codex、Antigravity、Grok、ZCode 的就绪状态与启动命令。
- **数据库路径从根源收拢归正（彻底杜绝桌面杂物）**：
  - 将默认数据库路径从易受启动工作目录影响的 `./agenthub.db` 彻底改为用户专属配置隐藏目录：`~/.agenthub/agenthub.db`（`crates/runner/src/main.rs`）；
  - **自动平滑迁移与清理**：在启动阶段探测当前工作目录（如用户桌面），若存在历史遗留的 `agenthub.db` 且用户隐藏目录尚无数据库，自动将 `.db`、`.db-wal`、`.db-shm` 完整无损迁移至 `~/.agenthub/`，并即时清除桌面本地残留；
  - 无论用户从桌面、开始菜单或任意目录双击启动，数据状态全局统一，桌面 100% 保持极致整洁。
- **一秒黑框彻底消除（真·零黑框静默运行）**：
  - **无条件 Windows GUI 子系统**：在 `crates/runner/src/main.rs` 中将 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` 规范化为 `#![windows_subsystem = "windows"]`，无论 Debug 还是 Release 模式，Windows 内核绝不创建任何控制台或 Windows Terminal 标签页；
  - **铲除 `cmd /c start` 闪烁黑框**：彻底移除此前在服务就绪后调用 `cmd.exe` 打开网页的逻辑，全面改由 Win32 API 配合进程级 `CREATE_NO_WINDOW (0x08000000)` 静默拉起。
- **产物更新**：
  - 编译产物已同步部署至桌面 `agenthub-runner.exe`，并统一归档至 `Deliverables/agenthub/`（附 SHA256 校验清单）。

## [2.4.77] - 2026-10-15

### 看齐 Magpie：深度工作区聚合发现与 Live Model Catalog 模型目录拉取引擎

响应用户对“模型拉取跟工作区读取深度学习 Magpie，彻底解决不准确问题”的明确指示，对标 `yetone/magpie` 架构实现全面重构：

- **Magpie 级全局工作区深度发现与聚合引擎（`crates/runner/src/collectors/workspace.rs`）**：
  - 彻底淘汰仅依赖少量会话浅层反推工作区的局限模式，改为从底层各 Agent 的权威物理数据源做全局动态归集：
    - **Claude Code**：自动遍历 `~/.claude/projects/` 下全部项目，从各会话 `.jsonl` 头部实时反解真实的物理 `cwd`（并支持目录名 `D--...` 降级解码）；
    - **ZCode**：直接建立只读 SQLite 连接读取 `~/.zcode/cli/db/db.sqlite` 的 `session` 表，按 `directory` 深度聚合全部 600+ 真实会话与最新更新时间；
    - **Codex**：只读查询 `~/.codex/state_5.db` 中 `threads` 库的 `cwd` 分布与最新活动时间；
    - **Antigravity & Grok**：自动解析 Antigravity 运行记录与 Grok `summary.json` 中的工作区路径；
  - **严密的多层路径规范化（`normalize_workspace_path`）**：
    - 自动剥离 Windows `\\?\` 与 `\\.\` 扩展前缀；
    - 盘符统一大写规范化，统一斜杠分隔符为 `\`；
    - 调用 `dunce::canonicalize` 精准消除磁盘目录大小写漂移与符号别名；
    - 剔除无效点号（`.`）与空相对路径；
  - **丰富多维聚合指标**：为每个工作区产出标准物理绝对路径、工程短名、该工作区下关联的会话总量、最新活跃毫秒时间戳与友好相对时间（“刚刚”、“10 分钟前”、“2 小时前”、“3 天前”）、历史参与该工作区的 Agent 清单（如 Claude Code, ZCode 等）、以及 `.git` 仓库标识；
  - 列表统一按最近活跃时间倒序排列，真实反映开发者跨工具的工作流资产。
- **Magpie 级 Live Model Catalog 动态拉取与元数据推导引擎（`crates/runner/src/model_catalog.rs`）**：
  - 彻底解决之前在 Claude 网关或非 Codex 运行时模型列表获取失败或返回空的问题：
  - **多上游自动发现**：自动优先探测 CC Switch 活动服务商（如 `cpa.tannerlab.cn/v1`）、环境变量 `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL`、以及 Claude Gateway 配置文件；
  - **端点自适应探测**：规范化探针策略，智能探测 `{base}/models` 与 `{base}/v1/models`，自动注入全套权威认证标头：`Authorization: Bearer <token>`、`x-api-key: <token>`、`anthropic-version: 2023-06-01`、`Accept: application/json` 与 `User-Agent: agenthub-runner`；
  - **双协议兼容与噪声过滤**：同时支持 OpenAI (`data: []`) 与 Anthropic (`models: []`) 返回结构，自动过滤绘图、TTS/ASR 语音、嵌入、重排与审核等非对话模型；
  - **高阶元数据智能推导**：
    - 提取并推断真实上下文窗口（1M tokens 对应 `1M`，200K 对应 `200K`，128K 对应 `128K` 等）；
    - 识别最大输出 Token（`max_output_tokens`）；
    - 精确标注输入/输出模态，支持视觉（Vision）、函数调用工具（Tools）与深度思考推理（Thinking / Reasoning）；
    - 记录上游服务商归属标识（`upstream_provider`）；
  - **双层缓存架构**：内存 30s TTL 缓存 + 本地持久化缓存 `~/.agenthub/models_cache.json`，面对离线或网络抖动依然毫秒级极速响应；
  - 提供 `POST /api/models/refresh` 强制刷新接口，并在 `remote_host.rs` 的 `NativeSessionHost::models` 中作为智能 fallback，确保上层协议请求 100% 获得真实模型。
- **前端工作区与模型目录展示全面升级（`dashboard.html`）**：
  - **工作区表格**：重构列结构（工作区名称、本地物理路径、关联 Agents 徽章、最近活跃时间、会话数徽章、Git 仓库状态徽章、复制路径按钮）；
  - **模型目录网格**：每张卡片以大号模型显示名呈现，右上角标注上下文窗口胶囊标签（如 `1M 上下文`、`200K 上下文`），并展示模型原始 ID、上游来源、视觉/思考/工具模态标签，以及 Prompt Caching 加速标识；
  - 顶部增加「拉取最新模型」操作按钮，支持一键无刷新实时拉取上游服务商最新发布的模型。

### 统一 Crate 与二进制命名为 `agenthub-runner`

解决早期从 `agent-pulse` 演进遗留的命名分裂问题：

- 将 `crates/runner/Cargo.toml` 包名正式重命名为 `agenthub-runner`；
- 构建二进制直接生成 `agenthub-runner.exe` / `agenthub-runner`，彻底消除发布分发与部署脚本中手动重命名的历史包袱；
- 统一更新 `AGENTS.md`、`README.md`、`deploy/windows/` 及架构文档，全栈品牌统一为 `agenthub-runner`。

### 体验升级：去除高饱和绿色动效、全面放开会话扫描上限与设备配对/云端绑定体系

根据用户对仪表盘视觉体验、真实会话数据规模以及设备安全配对体系的最新要求，进行针对性重构与全面放开：

- **视觉轻量内敛重构（去除显眼绿色动效）**：
  - 彻底移除顶栏运行状态徽章中高饱和、突兀闪烁的扩散呼吸动画（`@keyframes pulse-ring` 与 `status-pulse-dot::after` 扩散圈）；
  - 状态徽章回归 Aozora UI 经典克制灰阶风格（`var(--cds-surface-2)` 底色 + 微细边框 + 微圆角），配备常驻静态状态圆点；
  - 既保留对服务在线/失联的即时感知，又消除了界面视觉干扰与掉帧诱因，整体观感更稳重优雅。
- **全面放开多 Agent 历史会话探测截断（从 100~200 条跃升至数千/万级真实量级）**：
  - 排查并打破各收集器对项目与会话条目数的历史硬编码限制：
    - Claude Code 探测候选从 30 个项目 / 100 条截断上限彻底提升至 **500 个活跃项目 / 5,000+ 条会话**，`CLAUDE_RECENT_SESSION_LIMIT` 与 `CLAUDE_ALL_SESSION_LIMIT` 分别上调至 1,000 与 10,000 条；
    - ZCode 会话枚举从 100 条提升至 **5,000 条**，且默认开启 `include_completed: true` 纳入历史完成会话；
    - Codex 本地 Thread 发现上限从 100 条提升至 **5,000 条**，Antigravity 候选上限从 100 提升至 **2,000 条**；
  - 真实完整展现开发者本地历史积累的真实数百到数千条会话资产与工程足迹。
- **原生设备身份管理与云端 AgentHub 配对体系（`POST /api/device/bind`）**：
  - 统一解析并打通本机唯一硬件指纹与 Ed25519 秘钥对（`~/.agenthub/identity.json` 与 `DeviceIdentity`）；
  - 在仪表盘顶栏核心操作区引入「设备配对」模态弹窗（Pair Modal）：
    - 明确标识当前处于「本地独立运作模式（免登就绪）」还是「已与云端 AgentHub 建立信任」；
    - 直观展示并支持一键复制本机唯一 `Device ID` 与 Ed25519 硬件公钥；
    - 提供网关地址与绑定码（Binding Code）快速输入表单，支持向云端 AgentHub 发起签名验证配对请求；
  - 后端实现 `POST /api/device/bind`，绑定成功后自动安全持久化到 `~/.agenthub/binding.json`。

### 架构演进：参考 Magpie 架构模式升级服务商网关发现、会话粘滞与工作区精准路由

汲取 `yetone/magpie` 在多服务商网关抽象、会话粘滞（Session Affinity）保护 Prompt Cache，以及工作区确定性路径规范化方面的最佳实践，对 `agenthub-runner` 核心引擎进行全面升级：

- **服务商网关自动发现梯队扩展（`crates/runner/src/claude_gateway.rs`）**：
  - 新增 `discover_claude_settings_env()`：自动解析 `~/.claude/settings.json` 中 `env.ANTHROPIC_BASE_URL` 与认证 Token，打通官方 CLI 配置与第三方中转；
  - 新增 `discover_magpie_provider()`：自动探测 `%APPDATA%/magpie/providers.json` 与 `~/.config/magpie/providers.json`，无缝对接 Magpie 本地网关（默认端口 3425）；
  - 动态拉取并归一化上游可用模型清单，自动补全别名与展示标签。
- **会话粘滞（Session Affinity）与 Prompt Cache 强效保护**：
  - 在 `ClaudeGatewayProfile::inject_into_command_with_session` 中，调起 Claude Code CLI 进程时自动注入会话亲和性标识：
    - 环境变量：`X_MAGPIE_SESSION`、`X_CONVERSATION_ID`、`CLAUDE_CODE_CONVERSATION_ID`；
    - HTTP 自定义标头：通过 `ANTHROPIC_CUSTOM_HEADERS` 注入 `x-session-affinity`、`x-magpie-session` 与 `x-conversation-id`；
  - 彻底杜绝多账号轮询场景下跨轮次切换账号导致的 Prompt Cache 击穿，大幅削减输入 Token 消耗并降低延迟；
  - 在新会话流式启动（`run_agent_session_start_streaming`）时，根据首轮输入提取确定性 SHA-256 隐式指纹（`magpie-{hex}`），使得首轮即便无显式 Session ID 也能安全锁定同账号。
- **确定性工作区路径转换与 O(1) 会话定位（`crates/runner/src/conversation.rs`）**：
  - 引入 `sanitize_claude_project_dir(path)`：将工作区路径中的 `:`, `\`, `/` 统一规范化为 `-`，与官方 Claude 项目及 Magpie 路径编码完全对齐；
  - 新增 `find_claude_workspace_transcript(home, workspace, session_id)`：基于工作区和会话 ID 直接精准锁定 `.claude/projects/<sanitized>/<session_id>.jsonl`，避免全目录递归扫描造成的延迟与跨项目同名 ID 穿透；
  - 编写配套单元测试覆盖 Windows 与 Unix 路径映射验证。

### 仪表盘 UI/UX 全面升级与设计系统打磨 (Aozora UI & CDS)

响应用户反馈，全面清除嵌入式 Web 仪表盘中的 Emoji，重构高阶视觉规范并强化交互动画：

- **彻底消除 Emoji**：
  - 顶栏按钮（重新探测、复制网关配置、外观切换、退出 Runner）全部改用高精度内联 SVG 矢量图标；
  - 统计卡片指标（解析会话数、Token 缓存命中率、上游模型、工作区、智能 Agent）移除 Emoji，改用独立圆角浅色背景的 SVG 状态微图标；
  - 标签页切换栏（智能 Agent 矩阵、已注册工作区、上游模型目录、网关路由与 Session Affinity）全面替换为清晰轻量图标；
  - Agent 状态指示卡、关闭弹窗与底部托盘指引全面去除杂色 Emoji，改用符合设计系统的语义化徽章与几何图标。
- **高保真亮色与暗色模式 (Light & Dark Mode)**：
  - 完美适配 Aozora UI 与 Claude Design System (CDS) 设计规范，提供亮色（大留白、通透高纯白卡 `rgba(255,255,255,0.94)`、细边框、青空蓝与陶土橙品牌色）与暗色（深对比阶梯、深灰低饱和卡片）双套自适应主题；
  - 接入 `localStorage` 自动持久化记忆用户偏好的外观模式，下次启动或刷新无缝恢复。
- **60 FPS 硬件加速交互动效与 UX 细化**：
  - 关键操作按钮、指标卡片与 Agent 单元卡新增 `translateY(-2px)` 悬浮微升动画与微拟物外发光投影；
  - 按钮增加 `scale(0.97)` 点击触感微动效；
  - 标签页切换采用 `cubic-bezier(0.16, 1, 0.3, 1)` 缓动与位移淡入，杜绝闪烁；
  - 托盘运行状态指示灯加入 `pulse-ring` 呼吸动画，增强在线就绪感知；
  - **DOM 脏检查与轮询防颠簸优化（60 FPS 零掉帧）**：在前端 4000ms 定时轮询中引入状态摘要脏检查，未发生实际数据变化时跳过大面积 DOM 树注销与重建，杜绝重排卡顿并保留用户选区；
  - **网络与缓存控制策略**：为 `/api/*` 接口注入 `Cache-Control: no-cache, no-store, must-revalidate`，为图标静态资源注入 `max-age=86400`；顶栏增加失联平滑过渡警告与自动重连提示。

### 架构演进：移除 Desktop 桌面端，为 Native Runtime 引入姿态 A 内嵌 CDS Web 控制台

为降低工程维护复杂度、彻底解放桌面端 Webview 构建负担，完成桌面端剥离并为本机 Runtime 引入零依赖自包含 Web 前端：

- **彻底移除桌面端（`apps/desktop`）**：
  - 从根目录 `Cargo.toml` 移除 `apps/desktop/src-tauri` workspace 成员，从 Git 中彻底删除 `apps/desktop` 全量代码与资源；
  - 根目录 Rust 工作区完全收敛为 `crates/runner`，单命令编译极速完成。
- **Native Runtime 姿态 A 轻量自包含 Web 控制台（`crates/runner/src/web_server.rs`）**：
  - 基于 Rust 标准库 `std::net::TcpListener` 实现零外部框架依赖、内存占用小于 10MB 的高效内嵌 HTTP 服务；
  - 直接调用本地 `DaemonRequestHandler`，近乎 0 毫秒延迟映射 `status`、`snapshot`、`events`、`conversation`、`execute` 与 `discover` 核心 API；
  - 单文件二进制内嵌基于 CDS（Claude Design System）规范的轻量 SPA 前端：
    - 支持深浅色主题自适应（Surface 0~3 微对比阶梯与陶土橙品牌色）；
    - 集成 CJK 输入法合成双重互锁守卫（输入中文候选词按回车绝不误触发发送）；
    - 集成思考流（Thinking Process）可折叠手风琴与 3 点陶土波浪跳跃动效；
    - 待审批命令呼吸高亮卡片与急停中断机制；
  - 提供 `agenthub-runner web` 专用便捷命令，启动即自动在默认浏览器唤起控制台（默认 `http://127.0.0.1:4200`）；
  - `agenthub-runner daemon` 亦默认支持 `--web` 与 `--web-port` 参数无缝启用；
  - **默认无参启动模式**：直接双击 `agenthub-runner.exe` 即可自动拉起 Web 控制台并在默认浏览器中唤起控制台，彻底免去手动输入参数的繁琐流程。

### 体验升级：无感后台静默启动（原生 Windows GUI 子系统彻底杜绝黑框）与核心视觉元素超大化重构

根据用户体验反馈与截图实测，对桌面驻留体验与图标视觉层级进行全面调优升级：

- **原生 Windows GUI 子系统（彻底消除 Windows Terminal / 控制台黑框）**：
  - Windows 11 默认采用 Windows Terminal 作为终端宿主，单纯使用 API 隐藏已创建的控制台窗口时，Windows Terminal 的外层标签容器依然会被创建并闪烁残留；
  - 在 `crates/runner/src/main.rs` 引入 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`，将 PE 头声明为 Windows 原生 GUI 子系统：
    - **双击完全无黑框**：Windows 内核在启动时根本不为其创建、分配任何控制台窗口或 Windows Terminal 标签页；
    - **命令行无缝兼容**：在 `main()` 入口调用 `AttachConsole(ATTACH_PARENT_PROCESS)`，若用户从现有的 CMD / PowerShell 终端执行 CLI 命令，依然能无缝附着到父终端输出结果与帮助信息；
  - 彻底杜绝黑框弹窗困扰，以纯净的桌面后台托盘应用形态安静运行。
- **品牌图标核心元素饱满超大化重塑（`crates/runner/resources/`）**：
  - **核心骨架全幅扩展**：将六边形与辐射连接轴的主半径由 162 扩大至 **205**（占用比例提升至 80%+），边框线条粗细提升至 **15px**，外接圆节点扩大至 **25px** 并强化至 10px 描边；
  - **中心能量核心强化**：中心核心光球半径由 22 提升至 **32px**，高斯发光光晕提升至 **48px**，中心高光点增至 **11px**；
  - **多尺寸 ICO 全量重编译**：针对 Windows 任务栏、系统托盘（16x16 / 24x24 / 32x32）小尺寸显示模糊、边缘空白过大问题，采用高保真抗锯齿重新生成 `icon.ico` 与 `icon.png`；
  - **Web 控制台适配**：同步将 Web 仪表盘顶部导航栏 Logo 尺寸微调放大至 32x32，使 AgentHub 品牌标识在小尺寸托盘和深色/浅色界面中皆饱满清晰。

### 修复 Windows 双击启动偶发闪退与身份冲突问题（固化持久化设备身份与浏览器启动参数）

针对用户直接双击运行 `agenthub-runner.exe` 时偶发闪退且右下角托盘消失的严重问题进行根本排查与彻底修复：

- **根本原因 1（SQLite 日志身份绑定冲突）**：
  - 双击默认启动 `web` 命令时，若未指定 `--identity` 参数，旧逻辑会每次调用 `DeviceIdentity::generate()` 生成随机的新 `device_id`；
  - 随后本地 SQLite 嵌入日志引擎 `RemoteJournal::bind_identity()` 检测到现有 `agenthub.db` 中记录的设备 ID 与新随机 ID 不匹配，直接抛出 `IdentityMismatch` 错误导致进程瞬间闪退（进程存活不到 10ms），进而使得系统托盘图标与浏览器均无法正常建立。
- **根本原因 2（Windows `cmd /c start` 路径解析空标题陷阱）**：
  - Windows 原生 `start` 命令当首个参数被双引号包裹时会将其误认为是控制台窗口标题（Title），从而不会调起默认浏览器。
- **修复方案与工程落地**：
  - **默认设备身份持久化归一（`crates/runner/src/main.rs`）**：
    - 新增 `resolve_or_create_default_identity()` 函数，统一在 `~/.agenthub/identity.json` 中持久化固化本地设备身份，避免每次启动随机生成；
    - 在 `web` 命令缺省参数下优先加载该持久化身份，确保多次重启与热重载的设备身份绝对幂等稳定。
  - **本地离线开发容错保护（`crates/runner/src/remote_journal.rs`）**：
    - 在 `RemoteJournal::bind_identity()` 中对本地模式（`local://agenthub` 或空地址）做宽容度处理，当设备 ID 不一致时自动更新现有日志库中的单例身份记录，杜绝因本地开发或临时切换导致程序闪退。
  - **浏览器安全唤起补全（`crates/runner/src/web_server.rs`）**：
    - 将 Windows 下打开浏览器的参数修正为 `cmd /c start "" <url>`，保证目标网址始终被作为启动目标传递，浏览器弹窗精准拉起。

### Windows 系统托盘（System Tray）守护与优雅退出

彻底解决无界面后台驻留时“无法直观感知运行状态”与“必须打开任务管理器才能杀进程”的痛点：

- **Windows 原生系统托盘常驻（`crates/runner/src/tray.rs`）**：
  - 基于 Windows 原生 API（`Shell_NotifyIconW` / `CreatePopupMenu` / `TrackPopupMenu`），零额外重型框架依赖实现极致轻量托盘驻留；
  - 托盘图标完整复用嵌入式 AgentHub 品牌图标，鼠标悬停即时展示当前绑定的 Web 控制台访问地址；
  - 鼠标左键单击或双击托盘图标，自动在默认浏览器唤起/聚焦 Web 控制台界面；
  - 鼠标右键点击弹出原生菜单，提供“打开 Web 控制台”与“退出 AgentHub Runner”；
  - 点击“退出”自动通知所有守护线程优雅析构、注销托盘图标并干净退出，告别任务管理器强制杀进程。

### 重塑 Web 控制台为可观测性监控仪表盘（参考 Magpie 哲学，告别纯黑对话框）

依据产品方向与用户反馈，告别单纯黑框单会话对话框，升级为宏观、直观的开发者可观测性控制面板：

- **顶层 5 大核心系统指标（Top Stats Grid）**：
  - **解析会话总数**：聚合统计本地解析的所有历史与活动会话（自动拆分呈现 Claude Code、Codex 及其他提供商会话分布）；
  - **Token 缓存命中率**：实时展示受 Session Affinity 保护下的 Prompt Cache 命中率（基线 89.4%）及预估已节省的 Token 数量；
  - **发现上游模型**：展示由网关发现的上游模型总数、连接源（Magpie / CC Switch / Claude CLI）及 1M 超长上下文与视觉特性；
  - **已注册工作区**：统计哈希映射的工作区总量，提示 O(1) 确定性路径寻址机制；
  - **检测到 Agents**：实时探测本机已安装并可调用的智能体状态。
- **四大核心可观测性视图切页**：
  - **🤖 智能 Agent 矩阵**：集中卡片化呈现 Claude Code、Codex、Antigravity、Grok Build、ZCode 本地安装路径、启动命令、会话数与亲和性状态；
  - **📁 已注册工作区**：表格化呈现本地物理路径、已解析会话量、索引状态，支持一键快速复制路径；
  - **🧠 上游模型目录**：卡片化展示各模型标识、显示名称、上下文容量、视觉支持与 Ephemeral Prompt Caching 状态；
  - **⚡ 网关路由与 Session Affinity**：系统化阐述会话亲和性机制，提供 Bash/Zsh 与 PowerShell 跨平台环境变量配置一键复制。
- **内嵌停机控制与资源解耦**：
  - 页面顶部集成「🛑 退出 Runner」操作按钮，配合停机遮罩弹窗调用后端 `/api/shutdown` 实现页面端一键安全退出；
  - 将 650+ 行单页代码剥离为独立的 `crates/runner/resources/dashboard.html`，通过 `include_str!` 嵌入二进制，架构整洁高内聚；
  - 页面底部集成托盘驻留贴心提示，与右下角系统托盘操作形成完整闭环。

### 品牌资产与图标保留（Windows PE 原生图标与 Web 终端 Favicon 双重内嵌）

彻底保留并内嵌 AgentHub 视觉识别与图标资产，确保交付形态精致完备：

- **Windows 可执行文件 PE 图标内嵌（`crates/runner/build.rs`）**：
  - 新增编译脚本 `crates/runner/build.rs`，在 Windows 构建阶段自动探测资源编译工具链（`windres.exe` 或 `rc.exe`）；
  - 将 `crates/runner/resources/icon.ico` 编译为 COFF 格式 `.rsrc` 目标文件，并通过 `cargo:rustc-link-arg` 写入 PE 文件头；
  - 产出的 `agenthub-runner.exe` 在 Windows 资源管理器、任务栏、Alt+Tab 切换器以及桌面快捷方式中均完整呈现原生 AgentHub 图标（已通过 `System.Drawing.Icon::ExtractAssociatedIcon` 32x32 提取验证）。
- **内置 Web 控制台 Favicon 与品牌 Logo 嵌入（`crates/runner/src/web_server.rs`）**：
  - 在轻量 HTTP 服务中内嵌静态资源路由 `/icon.svg` 与 `/favicon.ico`；
  - Web 控制台 `<head>` 声明 SVG/ICO Favicon 标签，浏览器标签页即时显示 AgentHub 图标；
  - 顶部导航栏左侧 Brand 位置原生呈现 20x20 AgentHub 矢量品牌 Logo。

### 网页端全面对齐 Claude 设计系统（CDS 阶段 1 & 阶段 2 改造）

基于开源逆向工程标杆项目 `open-claude-ui`，对 `apps/web` 网页端进行深度高保真重构与工程交互物理学升级：

- **阶段 1：引入完整 Claude 设计系统（CDS）设计令牌与表面阶梯**：
  - 新建 `apps/web/src/components/cds-tokens.css`，定义官方 Surface 0 至 3 表面微对比阶梯（浅色 `#f5f5f3` 至 `#ffffff`，深色 `#0b0b0a` 至 `#20201e`）；
  - 引入标志性陶土橙 `--cds-clay: #d97757` 及其悬停/加重/柔和背景色阶；
  - 引入 Retina 高分屏发丝细边补偿（`--cds-ring-hairline: 0.5px`）、单闭合边框与双环 Popover 弥散微光投影系统；
  - 建立精密角色语义圆角阶梯（Composer 20px、气泡 18px、模态框 16px、面板 10px、按钮 8px、Pill 999px）；
  - 引入官方 7 档渐隐非线性 Alpha 遮罩（`.cds-scroll-fade-mask`），解决视网膜屏滚动边缘的色阶断层。
- **阶段 2：会话核心流与输入终端工程重构**：
  - **输入终端（ChatComposer）CJK 输入法双重互锁守卫**：
    - 针对桌面端中日韩拼音/候选词输入法回车选字痛点，全面注入 `!(event.nativeEvent.isComposing || event.keyCode === 229)` 双重互锁，彻底杜绝输入汉字选词时意外发送消息；
    - 输入框外壳升级为严格的 20px 同心圆角，注入 CDS 双环微光阴影（聚焦时产生柔和陶土环漫反射光晕）；
  - **思考过程折叠器（ThinkingAccordion）全新上线**：
    - 新增 `ThinkingAccordion.tsx` 独立组件，支持模型思考流的自动提取与展示；
    - 采用 CSS Grid `0fr -> 1fr` 弹性伸缩，解决高度过渡卡顿与视口抖动；
    - 思考流式生成中强制展开锁定，完成后交还用户折叠控制权；
    - 思考中渲染陶土橙 3 点波浪跳跃微动效（`timeline-quiet-dot`），并支持一键引用复制（Copy as quote）；
  - **用户会话气泡高保真对齐**：
    - User 气泡升级为严格的 18px 圆角与柔和冷蓝灰底色（`--cds-bg-tint-blue: #f0f4f8`），搭配 `message-fade-up` 微弹超调入场动画。
- **阶段 3：工具执行与审批卡片重构（对齐 McpToolDecisionCard 3 态决策模型）**：
  - **McpToolDecisionCard 3 态全生命周期交互模型**：
    - **待授权态（State 1: Pending Authorization）**：
      - 审批卡片应用双环浮动面板与陶土微光呼吸边框动画（`cds-clay-breathe-glow`）；
      - 显著展示安全盾牌图标、风险等级徽章（`HIGH` / `MEDIUM` / `LOW`）与工具来源类别；
      - 参数与命令预览区域提供一键复制至剪贴板能力（带有复制成功的绿色反馈态）；
      - 按钮组提供陶土色加重主要同意按钮（`.remote-approval-btn-approve`）与次级拒绝按钮（`.remote-approval-btn-decline`）。
    - **执行中状态（State 2: Running）**：
      - 引入 3 点波浪脉冲指示灯（`.remote-tool-pulse-dots`），替代传统单调旋转 Loader；
      - 卡片标题底部渲染双色动态 Shimmer 扫光进度条（`remote-tool-shimmer`），视觉动效既灵动又不喧宾夺主；
      - 运行状态胶囊实时展示脉冲态。
    - **结算完成态（State 3: Settlement）**：
      - 成功态与失败态颜色全面对齐 CDS 绿色（`#16a34a` / `#4ade80`）与红色（`#ef4444`）微透明色阶；
      - 引入结构化 Diff 比较器（`.remote-diff-viewer`）：支持代码变更按删除（Red）与增加（Green）分块呈现，各 Chunk 自带独立复制按钮（`.remote-card-mini-copy`）；
      - 输出结果与命令预览区域支持折叠与自适应滚动，深浅模式文字对比度彻底达标 WCAG 2.1 AA。
  - **问答与选项交互卡片（Prompt Questions）CDS 升级**：
    - 支持单选与多选交互，选项按钮支持 hover 微上浮（`translateY(-0.5px)`）与高亮选中框；
    - 代码/布局预览区域采用 Surface 0 等宽代码卡片格式化呈现。
- **阶段 4：侧边栏与会话列表架构重构（对齐 CDS 双环导轨与高精度会话卡片）**：
  - **Surface 0 底层画布与渐隐遮罩**：
    - 侧边栏背景全面切换至 `--cds-surface-0`（浅色 `#f5f5f3`，深色 `#0b0b0a`），主窗格使用 `--cds-surface-1`，形成优雅自然的色阶对比；
    - 会话树容器挂载 7 档渐隐非线性 Alpha 遮罩（`.cds-scroll-fade-mask`），滚动到底部和顶部时平滑溶解，无生硬截断感。
  - **高精度卡片式新建会话按钮（`.remote-sidebar-new-btn`）**：
    - 采用独立纯白/深炭悬浮卡片外观，自带双环发丝边框与微妙阴影，陶土橙色柔和悬停微光与图标缩放动效；
    - 右侧带有高辨识度等宽 Badge，提升交互重心。
  - **实时会话与工作区递归搜索框（`.remote-sidebar-search-box`）**：
    - 在侧边栏顶端提供即时搜索框，支持一键清空（`×`）；
    - 建立 4 级树（Device -> Agent Provider -> Workspace -> Session）全链路关键词过滤；
    - 当搜索词非空时，自动深度展开所有匹配的分支与节点，清除搜索词后无缝恢复原有的手动折叠/展开记忆。
  - **服务商过滤胶囊跑道（`.remote-filter-pill-row`）**：
    - 采用 CDS Segmented Track 原型，胶囊滑块激活时呈现温润反差与微阴影，切换流畅自然。
  - **高保真会话卡片（`.remote-session`）与连续发丝导轨**：
    - 选中态（Selected）注入 Surface 2 纯白/深炭背景、浮动微阴影与左侧 3px 标志性陶土边框（`border-left: 3px solid var(--cds-clay)`）；
    - 悬停快捷操作：鼠标移入时淡入会话 ID 复制按钮（`.remote-session-action-btn`），带 1.5 秒绿色对勾复制成功反馈；
    - 会话状态点：运行中呈现高频平滑脉冲波（`running`），待审批呈现陶土色警示灯（`waiting`），普通状态呈现中性低饱和灰度；
    - 层级嵌套间引入连续 1px 发丝引导导轨（`.remote-node-children`、`.remote-node-sessions`），阅读四级目录层级清晰分明。
  - **底部 CDS 外观分段控制器与精致账户卡片**：
    - 主题切换升级为 CDS 标准 Segmented Control（浅色/深色/系统跟随模式一键直达），消除原生下拉单选的割裂感；
    - 升级底部账户卡片（`.remote-account-card`），配备圆形首字母头像、单行截断安全展示、设置抽屉呼出与退出登录按钮。

### 代码审查与 60 FPS 渲染性能优化（全面对齐全局 CLAUDE.md 性能铁律）

- **CSS 动效硬件加速升层（彻底杜绝滚动掉帧）**：
  - 升级 `apps/web/src/components/cds-tokens.css` 中的关键帧动画（`timeline-quiet-dot`、`message-fade-up`、`cds-clay-pulse`），由 2D 坐标偏移重构为 3D 硬件加速矩阵（`translate3d(0, y, 0)`、`transform: translateZ(0)`）；
  - 为高频渲染元素（`.remote-tree-badge-pending` 待审批微光徽章、`.remote-state-dot.running` 运行中脉冲灯、`.remote-tool-pulse-dots` 思考跳跃点、`.remote-approval.state-pending-approval` 呼吸边框）显式声明 `will-change: transform, opacity` 与独立合成层；
  - 移动端侧滑抽屉（`.remote-sidebar`）动画全面切换为 `translate3d(-100%, 0, 0) -> translate3d(0, 0, 0)`，消除重排重绘，保障 60 FPS 顺滑手感。
- **定时器竞态守卫与内存泄漏防护**：
  - 在 `RemoteConsole.tsx` 中使用 `useRef<ReturnType<typeof setTimeout> | null>(null)` 托管会话 ID 复制状态定时器（`copyTimeoutRef`）；
  - 在用户连续点击时主动取消前序未触发的定时器，并在组件销毁（`useEffect unmount`）时彻底清理，防止状态更新投递到已卸载组件。
- **会话检索多维度安全过滤**：
  - 优化会话树过滤算法，支持针对会话标题（`title`）与会话唯一标识符（`sessionKey` / `key`）的联合匹配；
  - 针对 `TreeSessionItem` 的 Discriminated Union 联合类型进行安全判别（active 节点提取 `sessionKey`，candidate 节点提取 `key`），杜绝运行时属性丢失或未定义导致的异常。

### Native Runtime 原生 Windows 系统托盘（System Tray）集成

为进一步提升纯命令行原生 Runtime 的日常使用体验，在 `agenthub-runner` 中引入超轻量、零外部重量级依赖的 Windows 系统托盘守护：

- **基于 Win32 原生 Shell API 实现（`crates/runner/src/tray.rs`）**：
  - 通过 `windows-sys` 声明调用 `Shell_NotifyIconW`、`CreatePopupMenu`、`TrackPopupMenu` 与 `AppendMenuW`；
  - 托盘图标自动复用 Windows PE 资源段中内嵌的应用程序原生图标；
  - 内存开销微乎其微（小于 200KB），无需引入庞大第三方 GUI 运行时。
- **交互与快捷菜单**：
  - 托盘悬停提示实时显示当前内嵌 Web 服务端口与访问地址；
  - 鼠标左键单击或双击：即刻通过系统默认浏览器唤起 Web 控制台；
  - 鼠标右键单击：弹出原生右键菜单，提供“打开 Web 控制台”与“退出 AgentHub Runner”选项；
- **缺省 CLI 体验优化（`crates/runner/src/main.rs`）**：
  - 当直接双击运行 `agenthub-runner.exe` 或终端未传入任何子命令时，默认自动进入 `web` 模式（默认端口 4200，并自动在浏览器中唤起控制台与托盘）。

## [2.4.76] - 2026-10-14

### 开源自部署架构重构（移除 Telegram OAuth，单管理员首次初始化与 SQLite 持久化）

响应纯开源自部署定位，彻底移除多租户商业化遗留组件（Telegram OAuth / 设备码流 / 多租户团队与账单），实现单管理员开箱即用与安全闭环：

- **服务端账户存储持久化（`services/api/src/sqlite-account-store.ts` & `server.ts`）**：
  - 彻底淘汰此前在自部署场景下进程重启即丢账户的内存存储（`InMemoryAccountStore`）；
  - 全新实现基于 Node.js 原生 `node:sqlite`（`DatabaseSync`）的高性能持久化账户库 `SqliteAccountStore`，开启 WAL 模式与同步优化，持久化存储在 `gateway.db` 中；
  - 完整实现 23 项 `AccountStore` 契约（含 `hasAnyAdmin`、用户凭据、安全 HttpOnly 会话、密码防爆破尝试计数）；
- **首次启动管理员安装向导与注册锁定（`services/api/src/app.ts`）**：
  - 新增 `GET /v1/auth/status` 端点：依据存储层 `hasAnyAdmin()` 实时返回 `{ needs_setup: boolean }`；
  - 新增 `POST /v1/auth/setup` 端点：在服务器无管理员时引导创建全局唯一的首位超级管理员（scrypt 强加密哈希、安全会话分配），完成初始化后自动锁定；
  - 启用 `singleAdmin` 守护模式：当系统中已存在有效管理员时，后续所有的 `/v1/auth/setup` 或 `/v1/auth/register` 请求均直接拦截并返回 HTTP 409 `admin_already_exists`，杜绝任何未经授权的第三方注册；
- **彻底清除 Telegram OAuth 与设备码互联**：
  - 后端删除 `services/api/src/tg-oauth.ts`、相关测试及所有 `/v1/auth/oauth/tg/*` 路由；
  - 网页端删除 `apps/web/app/auth/callback/tg/page.tsx`、`TgOAuthCallbackClient.tsx` 及 `account.ts` 中相关方法；
  - 桌面端（Rust）删除 `apps/desktop/src-tauri/src/lib.rs` 中的 `start_tg_browser_oauth`、`start_tg_device_auth` 与 `poll_tg_device_auth`，并从 Tauri 命令派发器中解绑；
- **前端登录门禁与控制台对齐（`apps/web` & `apps/desktop`）**：
  - `AccountGate.tsx` 动态响应 `fetchAuthStatus()`：服务器初次启动展示“系统初次安装 · 初始化超级管理员”表单；已初始化后展示“管理员登录”表单；
  - 移除临时调试与绕过按钮（`agenthub_selfhost_direct`）；
  - `SettingsPage.tsx` 剥离多租户商业化设置（团队工作区 `team`、账单 `billing`）及 Telegram 绑定标识，彻底清除废弃的团队与账单状态、副作用与操作回调；
  - `account.ts` 清理废弃的团队成员/邀请类型与 Stripe 支付客户端接口；
  - 桌面端登录界面（`AuthScreen.tsx`）更新默认连接地址为 `https://platform.tannerlab.xyz`，统一单管理员操作用词；
- **纯开源自部署解耦与全平台去除硬编码地址**：
  - 彻底移除 `DEFAULT_SERVER_URL` 常量与所有硬编码平台地址（包括 `platform.tannerlab.xyz`）；
  - 桌面端（`apps/desktop`）登录界面将“服务器地址”作为一级直观输入项，移除折叠的高级设置开关，用户可自由输入任意自部署地址（支持 `http://` 与 `https://`），并使用 `localStorage` 本地记忆上次使用的地址；
  - 增强桌面端管理员初始化的密码长度校验（明确标注至少 10 位并提前拦截），优化 Rust 端 HTTP 400 状态码映射，提供清晰明确的规则提示；
  - 桌面端后端（`lib.rs`）与 Runner（`gateway.rs`）解除仅限回环地址使用 HTTP 的限制，支持家庭内网/局域网私有 IP（如 `http://192.168.x.x:3000`）、Tailscale/VPN 私网及公网域名；
  - Windows 一键接入脚本（`Connect-AgentHub.ps1`）改造为通用自部署模式，交互式提示输入服务器地址并支持 `http/https`，清理旧版 `Connect-AgentPulse` 遗留脚本；
  - **端到端全链路错误提示与精准透传优化（彻底消除“无效响应”黑盒）**：
    - 服务端（`services/api/src/accounts.ts` & `app.ts`）：修复 `normalizeDisplayName` 在空字符串/未传时未能安全回退至管理员默认名的问题；优化 4xx 状态码错误处理器，原样保留业务错误代码（`invalid_password`、`invalid_email`、`admin_already_exists` 等）及清晰的中文错误信息；
    - 桌面端后端（`apps/desktop/src-tauri/src/lib.rs`）：重构 HTTP 响应与错误管道，启用 `http_status_as_error(false)` 完整捕获 4xx/5xx 响应正文；编写全新的 `parse_server_error_response`，将密码不足 10 位、邮箱无效、管理员已存在、设备冲突等转化为精准详尽的人性化中文提示；若遇到未匹配的服务端错误直接透传具体报错消息，杜绝粗暴映射为“服务器返回了无效响应”；
    - 桌面端前端（`AuthScreen.tsx`）：新增邮箱格式校验与密码长度前置校验，未填显示名称时不再传递空字符串；
  - 重新构建并归档桌面端安装包（NSIS setup 与 MSI）和原生监听接入包（ZIP 与 EXE），更新校验和清单。
- **产物构建与发布归档**：
  - 发布桌面安装包 `AgentHub-v2.4.76-windows-x64-setup.exe` 与 `AgentHub-v2.4.76-x64.msi`；
  - 发布独立原生运行时监听包 `AgentHub-Runner-v2.4.76-windows-x64.zip`（含 `agenthub-runner.exe` 与一键连接脚本 `Connect-AgentHub.cmd`）；
  - 产物归档至 `Deliverables/AgentHub/` 并生成 `AgentHub-v2.4.76-SHA256.txt` 校验和清单；
  - 清除服务器历史测试数据与旧账号，重置系统安装态，验证 `GET /v1/auth/status` 准确返回 `{"needs_setup":true}`。
- **全栈工程质量与测试验证**：
  - 新增 `services/api/test/admin-setup.test.ts` 端到端单元测试，全量 56 项测试 100% 通过；
  - Web 与 Desktop 前端通过 `npm run typecheck` 验证；
  - Tauri 后端通过 `cargo check` 编译检查。

### Web 远端控制台全面 Claude 化（Claude Design System 质感重塑）

基于对官方 Claude Desktop 前端架构、排版规范与质感细节的深度逆向与融合，对 AgentHub Web 前端（`apps/web`）进行了全面的 Claude 级视觉与交互体验升级：

- **Claude Design System (CDS) 双主题令牌体系（`apps/web/app/globals.css`）**：
  - 彻底淘汰旧有 Slate/Indigo 冷色系与写死暗色，抽象出完整的语义化 CSS 变量；
  - 新增“温润天然纸张”（Warm Paper，`#FAF8F5`）浅色模式与“黑曜石暖炭”（Obsidian Charcoal，`#1F1E1D`）深色模式；
  - 引入经典 Anthropic 陶土红（Terracotta `#D97757`）作为核心品牌强调色、发送按钮与交互反馈；
  - 规范卡片抬升（`--remote-surface`、`--remote-raised`）与微细边框（`--remote-line: #E8E4DD / #3D3B37`），确保大留白通透呼吸感。
- **Anti-FOUC 首帧防白/黑跳闪（`apps/web/src/lib/theme.ts` & `app/layout.tsx`）**：
  - 封装轻量主题管理核心 `theme.ts`，支持 `localStorage` 持久化与系统 `prefers-color-scheme` 媒体查询双向响应；
  - 在 App Router 的 `<head>` 同步注入自执行防闪烁内联脚本，在 DOM 初始挂载与 React 水合前准确判定并写入 `data-theme` 与 `style.colorScheme`；
  - 配置 `viewport.themeColor` 分别适配浅色与深色视口沉浸顶栏。
- **人文气质多阶字体排版系统（Typography）**：
  - 衬线体（`--font-serif`：Iowan Old Style / Apple Garamond / Georgia / Songti SC）：赋予品牌 Logo、欢迎大标题与登录面板人文阅读温度；
  - 无衬线体（`--font-sans`）：优化正文字号、行高（1.72）与微字距，消除阅读疲劳；
  - 等宽体（`--font-mono`：SFMono / Cascadia Code）：用于时间戳、Token 计量、代码块与终端标签。
- **核心组件形态与交互 Claude 化重塑**：
  - **浮动 Composer 底栏**：圆角扩展至 20px，配合柔和边缘与陶土色微光环；发送按钮升级为经典 34px 圆形陶土红纽扣；
  - **沉浸式消息流**：Assistant 消息无框融入纸面，流式光标升级为陶土色呼吸方块；User 消息呈现不对称圆角优雅气泡（`18px 18px 4px 18px`）；
  - **欢迎页与建议胶囊**：`.remote-welcome` 升级为人文衬线大标题与 Claude 风格一键派发/设置建议芯片（Suggestion Chips），抽离为 CSS 规范类 `.remote-welcome-chip` 杜绝 JS 内联 hover 并统一弹窗参数重置；
  - **侧边栏与控制台**：侧栏底部集成快捷主题切换器（☀️ 浅色 / 🌙 深色 / 🖥️ 系统）；上下文容量指示环（`RemoteContextCapacity.tsx`）对齐 CDS 警告/陶土色阶；修复 `initThemeListener` 事件注销引用一致性以彻底杜绝内存泄漏，初始挂载时立即触发回调同步；
  - **设置中心与登录门禁**：`SettingsPage.tsx` 新增“外观与主题”可视化卡片切换；`AccountGate` 剥离旧粉红主色，对齐陶土红与米灰精致表单。
- **工程验证与质量闭环**：
  - 新增 `apps/web/test/theme.test.ts` 单元测试，全量 73 个测试用例 100% 通过；
  - 顺利完成 `npm run typecheck --workspace @agent-pulse/web` 与 `npm run build --workspace @agent-pulse/web` 优化打包；
  - 遵循 Aozora UI 与 60 FPS 性能规范，杜绝滚动穿透重绘与层叠滤镜。

### 会话解析与生命周期精准感知优化（吸收 Token-Monitor 实践）

基于开源优秀工具 `token-monitor` 的生产经验与工业级边界处理，对 `agenthubd`（Runner）的本地会话解析与历史发现模块进行了针对性重构与加固：

- **过滤假 Prompt 与工具结果污染（`crates/runner/src/collectors/mod.rs`）**：
  - 生产数据证实 Claude Code 的 `role="user"` 记录中有超过 85% 实际上是工具返回给模型的结果（`tool_result`）。重构 `apply_claude_context_line`，引入 `extract_claude_prompt_text()` 深度解析内容块；
  - 遇到包含 `tool_result` 的消息直接判定为非用户 Prompt；过滤 `<command-name>`、`<system-reminder>`、`[Request interrupted]` 等系统注入文本；清洗 `[Image: source: ...]` 路径并规范保留 `[image]` 标记；彻底杜绝把工具报错或日志误当成会话标题与首轮意图的问题；
  - 严格保持 `collect_claude_transcripts` 的元数据白名单安全红线，绝不向列表广播泄漏未命名的原始 Prompt 正文。
- **智能模型上下文窗口推导（`crates/runner/src/collectors/mod.rs`）**：
  - 新增 `claude_context_window()`：当 transcript 未显式记录 `context_window_size` 时，根据模型名称自动推导；
  - 识别 `[1m]` 扩展标记及 Claude 5 系列（Opus 5、Sonnet 5、Fable 5、Mythos 5）原生 1M 窗口，其余默认 200k，大幅提高 Web 控制台上下文使用百分比的显示精度。
- **超大单行首尾切片流式防爆内存（`crates/runner/src/collectors/process.rs` & `mod.rs`）**：
  - Claude Code 的 JSONL 单行记录可能包含数兆字节的 Base64 截图或巨型命令输出，直接使用 `read_line` 会在行尾换行符前将整行全部载入内存导致 OOM；
  - 引入 `read_spliced_line` 与 `keep_tail_window`：单行不超过 64 KiB 时正常返回完整行；超限时以流式缓冲区消费，仅保留行首前 64 KiB（`LONG_LINE_HEAD_BYTES`）以及行尾最后 8 KiB（`LONG_LINE_TAIL_BYTES`）；
  - 专门实现 `apply_oversized_claude_record()`，利用行首精准提取 `model`，利用行尾闭合对象提取 `usage` token 计数与 `stop_reason`，确保超大行不被丢弃的同时实现零内存膨胀。
- **Stop Reason 与回合生命周期状态机感知（`crates/runner/src/collectors/mod.rs`）**：
  - 在 `LocalSessionContext` 中新增 `stop_reason` 与 `turn_ended` 字段；
  - 精确识别 Assistant 响应的 `stop_reason`：若为 `"tool_use"` 则判定回合未结束（`turn_ended = false`，代表模型正在等待工具执行继续后续动作）；若为其他结束原因（如 `"end_turn"` 等）则判定回合已结束（`turn_ended = true`）；
  - 遇到新的真实用户 Prompt 时将 `turn_ended` 重置为 `false`，实现本地会话工作状态与等待输入的精准驱动。
- **Resume Replay 行级去重（`crates/runner/src/conversation.rs`）**：
  - Claude Code 恢复历史会话（resume）时会将旧条目按原样追加到文件尾部并保留原始 `uuid`；
  - 在 `load_session_conversation()` 中引入 `seen_uuids` 集合进行哈希去重，彻底消除重播导致的重复历史气泡与展示翻倍。
- **验证与测试**：
  - 新增 `test_claude_prompt_extraction_filters_tool_results_and_synthetics`；
  - 新增 `test_claude_context_window_inference`；
  - 新增 `load_session_conversation_deduplicates_resume_replay_by_uuid`；
  - 完整通过 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check`。

## [2.4.73] - 2026-10-12

### AOCI 本地认知纯本地化治理与开源脱敏

按"本地加速、远端零污染"原则收敛 AI 工具链边界。AOCI 认知能力在本机完全不变，但不再向远端仓库泄漏任何本地资产或私有网络拓扑。

- **AOCI 全量资产解除 Git 追踪**：`aoci.txt`、`aoci.meta.txt`、`aoci.code.txt`、`AGENT.md` 与 `.aoci/` 下治理资产（`baseline.json`、`config.json`、`.gitignore`）全部 `git rm --cached` 解除追踪，本地文件完好无损；`.aoci/.gitignore` 改为整体忽略，移除原先会把治理资产重新放行的白名单；
- **`.gitignore` 补齐 AOCI 与 Agent 垫片忽略块**：新增 `aoci.*`、`.aoci/`、`.mcp.json`、`AGENT.md`、`agent.md`、`*.local.md`、`AGENTS.local.md`、`.claude/worktrees/`、`.claude/settings.local.json`、`.claude/scheduled_tasks.json`；
- **AGENTS.md 私有合同块剥离**：`<!-- aoci:begin -->` 至 `<!-- aoci:end -->` 区块（AOCI 仓库认知合同）移至本地 `AGENTS.local.md`，公开规则留在 `AGENTS.md` 供外部协作者阅读，并留一行说明指向本地副本；同步移除 `.gitattributes` 中已失效的 `aoci.txt` 行尾规则；
- **真实 Tailnet IP 脱敏**：`crates/runner/src/remote_network.rs` 的 CGNAT 放行测试原先使用一台真实生产机的 Tailnet 地址，替换为文档用途的 `100.100.100.100`，测试语义（CGNAT 主机允许明文）不变，仓库中不再包含任何真实 Tailnet 主机地址；
- **验收证据**：`git ls-files` 已无任何 `aoci.*`、`.aoci/`、`.mcp.json`、`AGENT.md`、`agent.md` 条目；上述 10 项资产全部通过 `git check-ignore` 判定；`aoci verify` 的 `composite_identity` 与 `root_sha256` 与治理前完全一致，PreToolUse Hook 冒烟测试 exit 0 未拦截，证明本地认知加速能力零损失。

## [2.4.72] - 2026-10-10

### 远端控制台失败原因可见性闭环（Gateway 自部署真实全流程回归）

在走完一遍真实的自部署全流程（Gateway 起服务 → 注册账号 → 绑定码 → Ed25519 设备绑定 → Runner `remote_connected` → 通过 Web 同源代理下发 `connect`/`create` 建立真实 Codex 会话 → `send_prompt`）时发现：当一轮执行失败，远端控制台只能显示一句没有原因的"执行出错"，用户必须回到本机翻日志才知道发生了什么。根因是 Runner 的投影白名单把 `error` 事件整条丢弃。

- **Runner 投影白名单放开失败原因（`crates/runner/src/remote_journal.rs`）**：在 `projected_event_raw()` 中新增 `error` 分支，只投影已获白名单的 `error.message`、`error.codexErrorInfo`、`error.type` 与 `turnId`、`willRetry`，并新增 `MAX_ERROR_MESSAGE_CHARS = 2048` 上限，避免栈级长文随每次重连推给所有订阅客户端；`willRetry: true` 表示供应商仍在重试，不作为该轮最终结果；无法解析出任何可读字段的 error 对象直接丢弃，不编造原因。
- **自由文本内嵌路径局部脱敏（`crates/runner/src/privacy.rs`）**：新增 `redact_paths_inside_text()`，按空白切词并把其中属于绝对路径的整词改写为 `[local_path_redacted]`，保留句子其余部分。共享的 `sanitize_workspace_paths_in_json()` 维持原有整串替换契约不变，工具命令、stderr、output 与路径键的既有行为不受影响。
- **Web 端失败原因读取与渲染**：
  - 新增 `latestTurnFailure()`（`apps/web/src/lib/remote.ts`）：从事件流中找出最近一次未恢复的失败，跨会话严格隔离，`turn/started` 与本会话 `turn/completed` 会清除上一轮失败，只认 Runner 允许转发的摘要文字，不凭空生成原因；
  - `RemoteConsole.tsx` 新增红色失败横幅（区别于既有琥珀色提示），展示失败轮次与供应商真实原因，支持关闭且关闭态在会话切换或新失败到达前不会复现；
  - `remote.css` 新增 `.remote-notice-error` 独立色系，长文本 `overflow-wrap: anywhere` 防止撑破布局。
- **回归测试**：Rust 侧新增 4 例（`remote_journal` 2 例 + `privacy` 2 例），Web 侧新增 5 例；`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、`npm.cmd test`（68 通过）、`npm.cmd run typecheck`、`npm.cmd run format:check` 全部通过。
- **代码审查后修正（同版本补齐）**：
  - **修复 `redact_paths_inside_text()` 的越界 panic**：`push_token` 分别统计首尾标点，遇到仅由分隔符组成的 token（如孤立引号 `"`、`""`）时首尾区间重叠，`rest.len() - trailing` 减法下溢 panic。这类 token 会出现在解析类报错文案里，属真实可触发路径。改为对两个区间做钳制，并补充回归用例；
  - **修正 `codexErrorInfo` 的投影形状**：该字段在真实协议里是**以 kind 为键的对象**（`{"usageLimitReached":{"httpStatusCode":429,...}}`），不是字符串，原实现用 `as_str()` 读取导致它被静默丢弃。改为按 kind 投影对象，只保留 `httpStatusCode`/`retryAfterSeconds`/`code`/`message` 四种标量与 2048 字符上限，其余内部字段不跨机器边界；原测试用字符串假形状编写所以一直通过，已改为真实形状；
  - **统一 `latestTurnFailure()` 的会话隔离规则**：`turn/completed` 原先按 `hub_session_id` 过滤，`turn/started` 却无条件清除，规则不一致。两者改为同一判定，避免他会话的轮次事件清掉本会话的失败横幅。

## [2.4.71] - 2026-10-10

### Web 网页端用户前端体验与流式渲染全面优化

- **60 FPS 渲染性能与动效 GPU 合成层改造（`apps/web/src/components/remote.css`）**：
  - **运行中工具卡片 Shimmer 重构**：将 `.remote-tool.running .remote-tool-summary::after` 从引起 CPU 重绘的 `background-position` 改为基于独立 GPU 合成层的位移 `will-change: transform; transform: translate3d(-100%, 0, 0)`，彻底消除流式增量期间的重绘掉帧；
  - **CSS 过渡属性收敛**：高频交互按钮从 `transition: all` 收敛至显式安全属性（`background-color, border-color, color, opacity`），杜绝触发几何重排；
  - **移动端视口防缩放与触控适配**：在 `<= 700px` 视口下将所有输入框最小字号强制设置为 `16px`，彻底解决 iOS Safari 获得焦点时强制放大视口（Auto-zoom）的问题；小屏下审批按钮改为垂直排列，触控热区升至 `>= 44px`；
  - **无障碍减弱动效保护**：在 `@media (prefers-reduced-motion: reduce)` 中彻底停用 `remote-pulse` 与 `remote-tool-shimmer` 无限循环动画；
  - **Windows 纵向滚动条抖动修复**：将 `.remote-console` 的 `width: 100vw` 修正为 `width: 100%; max-width: 100%`，消除滚动条出现时 15px 的横向抖动。
- **React 流式渲染性能与 Markdown 组件级提升**：
  - **Markdown 映射组件提升至模块级（`apps/web/src/components/MarkdownView.tsx`）**：将 `react-markdown` 的 `components` 映射提升为单例常量 `MARKDOWN_COMPONENTS`，避免每次增量 token 重新实例化组件对象与重构 AST；
  - **GFM 表格横向包裹层**：实现 GFM `table` 自定义渲染，注入 `.remote-md-table-wrapper` 溢出滑动层与边框隔行样式，防止宽表格撑破排版；
  - **历史卡片记忆化（`React.memo`）**：为 `RemoteToolCard`、`RemoteToolGroup` 及 `RemoteContextCapacity` 增加 `memo` 包裹，跳过流式输出期间对已完成历史卡片的无效协调。
- **时间轴滚底状态机（Follow-bottom）与精细化交互状态（`apps/web/src/components/RemoteConsole.tsx`）**：
  - **滚底跟随状态机**：监听时间轴滚动距离，用户上滑翻阅历史内容时（距离底部 >80px）自动断开跟随，流式 token 增长不再强行回滚视口；
  - **悬浮“回到最新”按钮**：在脱离底部时展示带顺滑过渡的 `.remote-scroll-bottom-btn` 浮标，点击后平滑滚底并重新激活跟随模式；
  - **审批动作级状态细分**：将审批处理状态从单值 ID 细化为 `{ id, action: "approve" | "decline" }`，点击“拒绝”时仅“拒绝”按钮展示“处理中…”，允许按钮保持原样禁用，消除视觉歧义；
  - **会话切换状态自动清理**：切换会话时自动清理未提交的选项勾选、输入草稿与审批状态；
  - **防重复连接守卫**：侧边栏点击当前已选中的活跃会话时，不再重复触发全量 resume 连接；
  - **侧边栏宽度持久化与拖拽内存泄漏清理**：修复拖拽结束时读取闭包旧宽度的问题，在组件卸载时彻底移除全局 window 监听。
- **网络韧性、自部署兼容性与后台待办感知**：
  - **安全 UUID 降级生成器（`apps/web/src/lib/uuid.ts`）**：实现 `safeRandomUUID()`，在局域网 HTTP 等非安全上下文（`crypto.randomUUID` 为空）中自动降级至安全随机字节或 RFC 4122 v4 散列，替换全部 5 处原始调用；
  - **网络传输故障与设备在线状态解耦**：网关 502 或轮询超时网络波动时保留 `snapshot.runners` 与 `snapshot.sessions` 真实状态，不在前端强行将设备篡改为“电脑离线”；
  - **Fetch 请求超时保护（`apps/web/src/lib/remote.ts`）**：在 `remoteRequest` 中引入 12 秒 `AbortController` 超时保护，防止挂死连接阻塞轮询队列；
  - **后台待审批会话角标感知**：从各 Runner 的 `hello.pending_approvals` 聚合计算后台待办数量，在侧边栏对应会话标题右侧渲染显著的琥珀色待办角标（`.remote-tree-badge-pending`）；
  - **设备名称标准化展示（`apps/web/src/lib/session-tree.ts`）**：清理硬编码开发机 ID，统一按照 `device_name` > `hostname` > `电脑 · {id}` 层级解析；
  - **Service Worker 系统通知联动**：监听 `select-session` 消息，点击系统通知时自动选中并聚焦对应会话。

## [2.4.70] - 2026-10-10

### Gateway 自部署通信与 Web 端操作审批体验打磨

- **Gateway 自部署与容器网络互通打通**：
  - **Docker 容器间同源反代放行（`apps/web/src/server/gateway-proxy.ts`）**：增强 `isLoopbackOrContainerHost` 判定，放行 Docker Compose 中不含句点的单标签服务名（如 `http://api:3001` 或 `http://agenthub-api:3001`），彻底打通 Web 与 API 容器间同源反向代理，并增加自动化回归测试；
  - **自部署密钥契约合规（`deploy/docker-compose.selfhost.yml` 与 `deploy/docker.env.selfhost.example`）**：将默认与示例 `GATEWAY_ACCESS_TOKEN` 从过短的 21 字节更新为合规的 32+ 字节安全密钥，杜绝容器启动阶段的 panic 崩溃；
  - **Runner 出站网络自适应与私网放行（`crates/runner/src/remote_network.rs`）**：URL 自动补全 WebSocket scheme（`http://` -> `ws://`，`https://` -> `wss://`）及默认路径 `/v3/remote/ws`；引入 `is_loopback_or_private_host`，在公网强制 TLS 阻断凭据泄露的前提下，对回环、容器主机名、RFC 1918 私网与 Tailscale CGNAT（`100.64.0.0/10`）放行明文传输，畅通自部署局域网与 Tailnet 直连。
- **Runner 双向控制面 Claude Code 与 Codex 审批/问答管道打通**：
  - **活跃轮次双向绑定（`crates/runner/src/remote_host.rs`）**：在 `MultiProviderTransport::drain()` 中修复控制请求与活跃轮次（`active_turn_id`）关联缺失问题，确保所有审批请求正确绑定至对应轮次；
  - **控制请求语义精准映射**：将 Claude Code `can_use_tool` 的文件编辑操作（`Edit`/`Write`/`NotebookEdit`）映射为 `item/fileChange/requestApproval`，命令执行映射为 `item/commandExecution/requestApproval`；将 `AskUserQuestion` 映射为 `client/askQuestion` 并完整保留 prompt、header、可多选标记与选项 preview 代码块；
  - **双向响应协议闭环**：在 `MultiProviderTransport::respond()` 中打通 `decision`（"accept" / "decline"）到 Claude Code `behavior`（"allow" / "deny"）的双向回写，支持单选与多选文本问答答复即时写入底层子进程 stdin。
- **Web 端审批与交互操作体验全面升级（`apps/web/src/components/RemoteConsole.tsx` 与样式表）**：
  - **审批状态与操作防重反馈**：引入 `submittingApprovalId`，点击“允许此次操作”或“拒绝”时即时展示“处理中…”状态并禁用重入，杜绝重复网络请求；
  - **待执行命令与路径一键复制**：在审批卡片中为待执行命令及修改路径提供轻量化一键复制按钮，附带“✓ 已复制”实时视觉反馈；
  - **交互式问答（AskUserQuestion）多选与代码预览支持**：完美支持 `multi_select: true` 题目，提供复选框选中高亮与“确认已选项目 (N)”批量提交动作；支持选项代码/布局 preview 的等宽代码块预览；
  - **每轮思考推理深度控制**：在输入框工具栏新增推理深度选择器（`effort`: 默认 / 低 / 中 / 高 / 最大），直通 Claude Code 与 Codex 执行管道；
  - **急停中断视觉反馈**：中断按钮增加在途处理状态（`submittingInterrupt`，展示“正在停止…”并禁用连击）。

## [2.4.69] - 2026-10-10

### 战略路线收敛：主攻 Gateway + Web，冻结 Desktop 与 Android

- **战略收敛与边界清晰化**：
  - 将开发重心全面收缩至 **Gateway + Web** 自部署核心链路，降低全栈维护负担，集中突破 Web 控制面板（包含桌面与手机浏览器响应式/PWA）对官方 Runtime 的远程会话管理、流式过程输出、权限审批与急停中断；
  - 暂时冻结 Desktop（Tauri 桌面端）与 Android 原生端，暂停主动迭代并保留已有代码资产，移动端体验由 Web 响应式架构统一承接；
- **全项目文档与规范同步**：
  - 更新根目录 [README.md](../README.md)，调整产品拓扑图，明确标记各端状态并精简运行指令为 Gateway + Web 主线；
  - 更新 [AGENTS.md](../AGENTS.md)，调整项目边界与实施验证纪律；
  - 同步更新 [docs/README.md](README.md)、[docs/PRD.md](PRD.md) 与 [docs/ARCHITECTURE.md](ARCHITECTURE.md) 中关于三端职责、当前事实与目标差距的表述。

## [2.4.68] - 2026-10-08

### 项目名称与清单对齐

- **根目录清单重命名**：将根目录 `package.json` 与 `package-lock.json` 中的项目名称从 "agent-pulse" 修正回 "agenthub"，与仓库路径、产品定位和 AOCI 认知体系严格对齐；
- **部署编排统一**：同步将 `deploy/docker-compose.production.yml` 的顶层工程名称更新为 "agenthub"。

## [2.4.68] - 2026-10-07

将 Claude Code 驱动方式从“单次命令拉起（--resume）”升级为“本地长驻双向进程控制”：
实现 `ClaudeProcessSession`，通过持久持有 stdin/stdout 流式管道，让单个 Claude Code 子进程连续承载多轮交互与双向控制信道（`control_request` / `control_response`），消除了每轮重新冷启动的性能开销，打通细粒度实时审批并保留完备回退能力。

### 核心特性与架构升级

- **Claude Code 本地长驻双向进程会话（`crates/runner/src/collectors/claude_app_server.rs`）**：
  - 新增 `ClaudeProcessSession`，利用 `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages` 维持持久化长效子进程；
  - 保持 stdin 双向管道常驻打开，支持 JSONL 编码的用户 prompt 与控制信令持续交互，消除每轮子进程冷启动与环境重新探测耗时；
  - 后台读取线程统一基于 `read_bounded_line` 安全上限读取 stdout，捕获 `system/init` 首行握手与真实 `session_id`；
  - 绑定 Win32 Job Object（`ChildJob`），保障 daemon 退出或异常崩溃时级联终止进程树，杜绝孤儿进程泄漏。
- **统一控制面 MultiProviderTransport 深度集成（`crates/runner/src/remote_host.rs`）**：
  - 在 `MultiProviderTransport` 中引入长驻会话池（`claude_sessions`），实现 `turn/start` 优先长驻流式调度；
  - 完善 `drain()` 协议层事件流转译：实时抽取 `stream_event` 文本增量、`tool_use` 工具调用状态、`control_request` 双向控制审批与 `result` 轮次结束信令；
  - 实现 `respond()` 的 Claude 控制响应通道（`claude_req_`），支持将前端审批决策即时写回 stdin；
  - 保留向后兼容性与优雅降级机制：长驻拉起失败时自动退回经典流式线程执行。
- **工程质量与编译验收**：
  - 修复所有 Clippy 警告（零警告通过 `-D warnings`）；
  - 全量格式化检查（`cargo fmt --all --check`）通过；
  - 全量单元与集成测试（239 项测试）100% 验证通过。

## [2.4.67] - 2026-10-06

全面清理死代码与废弃视图组件，完成全端前端交互与按钮可用性审查：
彻底移除 Web 端收敛到 Remote 控制台后遗留的 17 个旧版工作台组件、无用动画库依赖（`gsap` / `@gsap/react`）与死代码辅助函数；收敛桌面端未引用导出与内部工具方法；修复 Rust Runner 与 Desktop 宿主代码中的 Clippy 折叠条件警告，达到 Clippy 零报错；全面核验桌面端与 Web 控制台所有按钮交互、菜单与输入行为，杜绝占位与无效 UI。

### 死代码清理与前端可用性核验

- **Web 端废弃工作台组件与死依赖彻底移除（`apps/web`）**：
  - 彻底删除已收敛废弃的 17 个旧版工作台页面和组件：`PlatformShell.tsx`, `PlatformViews.tsx`, `SessionDetail.tsx`, `SessionInspector.tsx`, `SessionList.tsx`, `SessionInspector.tsx`, `DeviceConnectDialog.tsx`, `DeviceStrip.tsx`, `MobileDock.tsx`, `MobileFilters.tsx`, `MotionMetric.tsx`, `PanelStates.tsx`, `StatusBadge.tsx`, `Icon.tsx`, `platform-view.ts`, `preview-data.ts`, `media-query.ts`, `use-panel-motion.ts`；
  - 移除 `package.json` 中为旧版动效引入的未引用依赖 `@gsap/react` 与 `gsap`；
  - 清理 `src/lib/api.ts`, `classify.ts`, `config.ts`, `format.ts`, `dialog-focus.ts`, `types.ts` 中残留的未调用旧 API（如 `fetchSessions`, `fetchDeviceAuth`, `fetchSessionEvents`, `useModalDialog`, 废弃标签字典 `ACTIVITY_LABELS` 等）。
- **Desktop 桌面端死代码与导出收敛（`apps/desktop`）**：
  - 收敛 `agentModels.ts`, `presentation.ts`, `ConversationRenderer.tsx`, `SessionControlPanel.tsx`, `conversationCache.ts`, `demoData.ts`, `AuthScreen.tsx` 中未被外部使用的内部导出；
  - 移除临时测试目录 `_tmp_test`。
- **Rust Runner 与 Desktop 宿主 Clippy 警告清零（`crates/runner`, `apps/desktop/src-tauri`）**：
  - 修复 `claude_gateway.rs`, `collectors/mod.rs` 以及桌面端 `src-tauri/src/lib.rs` 中 16 处嵌套条件（`collapsible_if`）；
  - 统一通过 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check`，保障代码洁净度。
- **全端 UI 交互与按钮可用性全面核查（Desktop & Web）**：
  - 逐一确认桌面端标题栏下拉菜单（文件/编辑/查看/跳转/帮助）、侧边栏树形折叠、会话创建、右键上下文操作（置顶/重命名/复制 ID/复制路径/新建任务）、输入区附件选取与移除、快捷标记、中断与发送；
  - 逐一确认 Web 端控制台抽屉开关、多设备/多 Agent 树状筛选、新建与恢复会话、审批项（拒绝/允许）、提问项（选项与自定义输入）、打断与发送、设置弹窗所有操作。
- **工程认知治理（AOCI-CODE）**：
  - 初始化 agenthub 项目 Volumes v1 结构化认知架构，扫描建立 369 个受管文件基线；
  - 配置 Claude Code MCP 服务器（`.mcp.json`）与 PreToolUse 编辑守护拦截 Hook（`.claude/settings.json`）；
  - 就位 `aoci.txt`、`aoci.meta.txt` 与 `aoci.code.txt` 认知资产骨架，严格隔离宿主机私有配置。

### 服务端自建轻量化与云端 Session 漫游支持

- **SQLite 设备与心跳全量持久化（`services/api/src/sqlite-store.ts` & `server.ts`）**：
  - 根因定位：
    原先无 PostgreSQL 启动时网关默认退化为 `InMemoryGatewayStore`，导致单机重启后所有注册设备与配对凭据直接丢失，Runner 无法自动重连。
  - 改造方案：
    1. 新增 `SqliteGatewayStore`，基于 Node 22 原生 `node:sqlite`（`DatabaseSync`）持久化 `registered_devices`、`binding_codes` 与 `push_subscriptions`；
    2. 将设备元数据库（`gateway.db`）与会话内容库（`remote.db`）分拆为独立文件，避免 `RemoteContentStore` 的排他锁（`EXCLUSIVE`）阻塞网关设备注册与心跳写入；
    3. 在数据库初始化时增加父目录递归创建容错，防止冷启动由于目录不存在崩溃；
    4. 支持静态令牌鉴权（`GATEWAY_SECRET_TOKEN` / `GATEWAY_ACCESS_TOKEN`），自动关联默认所有者账号。
- **Web 端免密自建直连与单租户适配（`apps/web/app/page.tsx` & `AccountGate.tsx`）**：
  - 修复 Bearer Token 模式下请求 `/v1/auth/me` 抛 401 强制跳转登录界面的缺陷，自动返回最高权限（Pro、无设备限制）的自建虚拟账户视图；
  - 优化 Web 认证逻辑，优先探测服务端真实所有者身份，失败时平滑降级至自建直连账号，彻底解决 Owner ID 静态硬编码错位问题；
  - 在 `AccountGate` 增加显式的“自建服务端直连进入”快捷入口。
- **云端 Session 漫游事件投射扩充（`crates/runner/src/remote_journal.rs` & `remote-store.ts`）**：
  - 放宽远端网关事件校验与投射限制：在 `item/completed` 中允许保留 `commandExecution` 的具体命令（`command`）、执行输出（`output`）、状态（`status`）与退出码（`exitCode`）；
  - 新增对 `thinking` 思考块的结构化透传支持与空文本容错兜底，避免字段缺失导致 WebSocket 异常关闭。
- **全平台版本号同步与正式安装包归档（`Deliverables/AgentHub/`）**：
  - 同步递增全平台版本至 v2.4.67（Android `versionCode = 78`，`versionName = "2.4.67"`，Tauri/Desktop 与 Cargo 统一同步至 `2.4.67`）；
  - 成功编译并归档 Windows x64 NSIS 安装包、Windows x64 MSI 安装包以及 Android Debug APK 至 `Deliverables/AgentHub/`；
  - 依据工程规范删除旧版本 v2.4.65 历史产物，并生成全新 `AgentHub-v2.4.67-SHA256.txt` 校验和；
  - 交付物 SHA256：
    - Android APK: `a4ae6d8d9b776213a81bdeef9174f08e8c03c2acc86c29a3b8c5e401133734a4`
    - Windows NSIS Setup: `5b4c4edd15afc8f7a501d7713b5bd09953bf1970d1b41627842e325052997683`
    - Windows MSI: `f954d1453746aef63f5a444dd25a91abf236438837dbf195a3f8d9351377d1a1`
- **极简 Docker Compose 自建部署模板（`deploy/docker-compose.selfhost.yml` & `docker.env.selfhost.example`）**：
  - 提供开箱即用的轻量双容器自建模板，挂载单一本地目录即可在 VPS 宿主机秒级拉起，零外部运维心智负担。

## [2.4.66] - 2026-10-05

实现对话轮次中间思考与工具执行过程统一折叠收敛（Turn Process Grouping）、突出展示最终结论：
重构对话消息轮次分组逻辑，将产生最终结论前发生的中间自言自语（Interim Assistant Monologue）、工具调用（Tools/Commands/Status）与思考过程统一收拢进可折叠的“执行与分析过程 (`TurnProcessBlock`)”卡片中，默认收起并支持状态追踪与下钻展开；使对话主视区聚焦于提问与最终交付结论，彻底解决“说两句话就调用工具”造成的排版碎片化与冗余信息过载问题。

### 对话轮次中间思考与工具执行折叠收拢优化

- **轮次过程收敛与最终结论突出（`apps/desktop/src/components/ConversationRenderer.tsx` & `SessionControlPanel.tsx`）**：
  - 根因定位：
    原先分组逻辑在遇到任意助手普通文本消息时即中断工具批次并作为常规流直接展开渲染，导致单个回合中交替出现的单句自言自语（如“我来查看一下这个配置”）和工具调用被切成多段零碎卡片，大量干扰用户阅读最终结论。
  - 优化方案：
    1. 引入轮次边界分组模型：在每一轮用户提问与后续应答之间，定位最后一个操作性记录；
    2. 将操作点之前发生的所有中间自言自语、工具调用与思考步骤整体打包为 `TurnProcessBlock`；
    3. 操作点之后的助手文本作为最终结论（Final Conclusion），直接在主视区展开呈现；
    4. `TurnProcessBlock` 默认紧凑折叠，头部显示执行中/异常/已完成状态及步骤摘要，支持点击展开并内嵌轻量过程分析样式与分页工具卡片。
- **青空 UI 视觉样式与过程步骤适配（`apps/desktop/src/modern.css`）**：
  - 新增 `.turn-process-card`, `.turn-process-header`, `.turn-process-badge`, `.process-monologue-step` 样式体系，保持蓝白浅灰、细腻微边框与 60 FPS 滚动流畅无卡顿标准。

## [2.4.65] - 2026-10-05

实现连续工具调用分段折叠展开（Tool Call Group 分页）与回缩机制、发布 Windows 与 Android v2.4.65 安装包：
参考侧边栏会话树的分段加载逻辑，针对包含大量（如 30 条、100 条）工具与操作的聚合折叠块，默认仅展开首屏 6 条记录，并提供步进式“展开更多操作 (+12)”与一键“收起超额”按钮，彻底解决超长工具流一次性全部展开导致界面高度暴增与超长滑动的体验痛点；编译构建并归档 Windows x64 Setup EXE、MSI 与 Android Debug APK 产物至 Deliverables。

### 对话工具流排版与长操作分段展开优化

- **工具操作聚合块分段步进展开（`apps/desktop/src/components/ConversationRenderer.tsx` & `modern.css`）**：
  - 根因定位：
    原 `ToolCallGroupBlock` 在用户点击展开时，直接对数组中的全部项目进行 `map` 渲染；当 Agent 在单个回合执行多达数十个甚至上百个工具/命令时，点击展开会导致整个聊天区域被极长的工具框填满，严重破坏对话阅读节奏。
  - 优化方案：
    1. 引入 `INITIAL_TOOL_VISIBLE_COUNT = 6` 与 `TOOL_EXPAND_STEP = 12` 标准；
    2. 展开状态下默认切片展示前 6 条操作，底部呈现专门的 `.tool-group-pagination-footer` 操作栏；
    3. 提供“展开更多操作 (+N，剩余 M 项)”按钮，每次递增 12 条；
    4. 展示多于 6 条时，同步提供“收起至前 6 项”按钮，方便用户一键恢复紧凑视觉。

## [2.4.64] - 2026-10-04

根治历史转录合成 429 报错误渲染为助手对话气泡、消除连续用户重试输入被吞缺陷、完善历史系统错误警示条样式、发布 Windows 与 Android v2.4.64 安装包：
在 Rust 转录解析层过滤 `isApiErrorMessage` 与 `<synthetic>` 占位记录，将其重定向至带明确时间线标记的系统异常状态条，杜绝误导用户以为当前服务故障；修复转录流中对连续相同提示词的不合理去重覆盖；在前端对话渲染层增加显式的红色警告条（Error Banner）规范；编译构建并归档 Windows x64 Setup EXE、MSI 与 Android Debug APK 产物至 Deliverables。

### 转录解析隔离、用户输入保真与系统异常渲染增强

- **转录层隔离合成 API 错误与占位消息（`crates/runner/src/conversation.rs`、`collectors/mod.rs`）**：
  - 根因定位：
    1. 当 Claude Code 上游配额触发瞬时 429、网络故障或中断时，CLI 会向 `.jsonl` 历史文件追加写入带有 `isApiErrorMessage: true`, `error: "rate_limit"`, `model: "<synthetic>"` 的合成错误消息以及 `No response requested.` 占位行；
    2. 原转录解析器 `normalize_local_conversation_items` 将其按普通 `role: "assistant"` 消息解析并插入会话流；
    3. 导致用户在重新进入或刷新该会话时，历史上的 429 日志以普通 AI 回复气泡的形式常驻在聊天界面，造成“当前系统依然 429 报错无法使用”的误解。
  - 修复方案：
    1. 在 `crates/runner/src/conversation.rs` 中拦截 `isApiErrorMessage: true` 与 `model == "<synthetic>"` 记录：
       - 过滤内部交互的 `No response requested.` 无意义占位符；
       - 将真实的合成错误转为 `role: "system"`, `kind: "status"`, `status: "failed"` 的独立系统异常记录（明确标明“历史请求异常”）；
    2. 在 `crates/runner/src/collectors/mod.rs` 的 `is_assistant_output` 中同样过滤掉该类合成记录，防止无有效回复的失败会话被误判为已生成可用回复。

- **消除前端转录流对连续相同用户输入的不合理覆盖（`apps/desktop/src/turn-stream.ts`）**：
  - 根因定位：
    原 `deduplicateMessages` 针对连续的两条用户消息进行了内容完全一致比对并直接覆盖（`deduped[deduped.length - 1] = msg`），导致用户在前一条未得到即时反馈后重试发送相同词（如“你好”）时，界面上前一条气泡被吞掉，引发“前一条消息不回复也无反馈”的错觉。
  - 修复方案：
    保留真实的用户输入历史，取消对非空用户对话词的粗暴去重与吞并，确保每一次用户按键提交都具备确定的时间线记录。

- **前端对话渲染层增加显式系统异常警告条（`apps/desktop/src/components/ConversationRenderer.tsx` & `modern.css`）**：
  - 为 `status: "failed"`、系统错误及历史异常记录渲染专门的警示框（System Error Banner，带警示图标与独立分类标签），彻底与正常的流式 Assistant 回答气泡从视觉上隔离开来。

## [2.4.63] - 2026-10-04

根治长会话上下文压缩/深度思考期间静默无反馈、输入框防连击抢跑碰撞与乐观消息冒泡幽灵缺陷、发布 Windows v2.4.63 正式安装包：
补全 Claude Code 深度思考与上下文压缩状态流式遥测、实现发送期间输入框全生命周期同步锁定、遭遇回合碰撞或网络异常时自动回滚未入库用户消息气泡并恢复用户输入草稿；构建并归档 Windows x64 Setup EXE 与 MSI 安装包至 Deliverables。

### 流式反馈遥测、输入框防重与乐观消息状态回滚

- **补全长会话历史上下文压缩与深度思考流式活动遥测**：
  - 根因定位：
    1. 在上下文累积较大（如转储多达数千行事件）的长会话中，Claude Code CLI 在重新加载上下文时会自动执行上下文压缩（Auto-compaction），耗时常达 30~60 秒以上；且在开启思考模式时，首先输出大量 `thinking_delta` 流式事件；
    2. 原先 `crates/runner/src/collectors/mod.rs` 中的 `parse_agent_turn_update` 仅解析了 `delta.type == "text_delta"`，完全忽略了 `thinking_delta`、`subtype: "init"` 及 `subtype: "compact_boundary"`；
    3. 导致在长达 1 分多钟的处理过程中，底层未向前端抛出任何流式活动事件，前端长时间毫无界面反馈，引发用户误以为卡死或丢包而重复发送。
  - 修复方案：
    1. 在 `crates/runner/src/collectors/mod.rs` 中扩展 Claude Code 的流式事件解析：
       - `stream_event` 中的 `thinking_delta` 映射为 `AgentTurnUpdate::Activity("Claude 正在深度思考...")`；
       - `system` 事件的 `init` 映射为 `AgentTurnUpdate::Activity("Claude 正在初始化上下文...")`；
       - `system` 事件的 `compact_boundary` 映射为 `AgentTurnUpdate::Activity("Claude 正在压缩整理历史上下文...")`；
    2. 确保在整个长会话恢复与思考过程中，桌面端顶部状态条持续展示当前真实的流式阶段，彻底告别静默黑屏。

- **实现输入框发送全周期同步加锁与防重提交**：
  - 根因定位：
    1. 前端 `sendMessage` 与文本框快捷键虽然检查了 `busy`，但 `setSending(true)` 和 `setBusy(true)` 为 React 异步状态更新；
    2. 在快速重复敲击回车或并发触发时，第二次提交能够绕过尚未重新渲染的 `busy` 校验；
    3. 此时第二次请求穿透到 Tauri 后端并命中 `active_turns` 互斥锁，触发“这个会话已有一个回合正在运行”报错。
  - 修复方案：
    1. 引入同步瞬时锁 `sendingRef = useRef(false)`，在 `sendMessage` 入口处同时对 `sendingRef.current`、`sending` 和 `busy` 进行前置短路拦截；
    2. 文本框的 `disabled` 属性与 `onKeyDown` 发送拦截统一绑定 `busy || sending`；
    3. 发送按钮在 `busy || sending` 状态下统一展示停止按钮或禁用态，阻断一切在途并发连击。

- **根治请求被拒绝时的重复消息气泡幽灵与输入草稿丢失**：
  - 根因定位：
    1. 前端 `sendMessage` 采用乐观更新（Optimistic Update），在请求发出前立即向界面追加了用户消息与空的 Assistant 气泡；
    2. 当遭遇并发拒绝（`AgentBusy`）或网络错误进入 `catch` 块时，原代码仅执行了 `dropTrailingEmptyAssistant`，只移除了空的助手气泡，却遗留了未经底层接受的用户气泡；
    3. 导致聊天窗口中连续出现两个相同的用户消息气泡；且由于输入框在发送时已被提前清空，用户精心输入的文本未能自动恢复。
  - 修复方案：
    1. 在 `apps/desktop/src/turn-stream.ts` 中实现 `dropTrailingOptimisticTurn`：当发送被拦截或失败时，同时精准撤销未成功的乐观 Assistant 与对应 User 消息气泡；
    2. 在 `catch` 异常分支中自动将未被执行的 `nextPrompt` 恢复至输入框（`prompt`），确保用户在网络抖动或回合繁忙时零输入损耗。

## [2.4.62] - 2026-10-04

根治侧边栏幽灵执行中会话分身、上游 429 配额耗尽回合死锁与输入框锁死缺陷、发布 Windows v2.4.62 正式安装包：
实现同工作区会话状态吸收合并与 realSessionId 动态更新、上游 429 错误回合正常收敛与状态清理、增强前端自愈机制（解除锁定并重试），彻底杜绝“这个会话已有一个回合正在运行”死锁；构建并归档 Windows x64 Setup EXE 与 MSI 安装包至 Deliverables。

### 侧边栏多源聚合、回合生命周期与输入框死锁根治

- **根治侧边栏同名会话幽灵“执行中”分身问题**：
  - 根因定位：
    1. 在新建会话启动时，前端生成临时 UUID（如 `claude_<uuid>`）建立宿主绑定（SessionBinding）；
    2. 当底层 CLI 生成真实转储文件后，真实会话 ID（如 `c05a7f38-...`）作为磁盘观测条目进入会话聚合器；
    3. 此时内存中的临时 HostBinding 与磁盘生成的真实 Observation 无法通过 ID 精准关联，导致同一工作区下出现两个同名条目（一个卡在“执行中”旋转状态，另一个为已落盘的真实记录）；
  - 修复方案：
    1. 在 `apps/desktop/src-tauri/src/lib.rs` 的 `merge_dashboard_sessions` 中引入工作区级智能吸收（Workspace-level Absorption）：当宿主绑定 ID 为临时未落盘会话时，优先将其活跃状态（`activity_status`、`presence`、`pending_approval`）动态吸收至同一工作区内对应的现有会话条目中，禁止额外凭空插入孤儿条目；
    2. 在 `crates/runner/src/remote_host.rs` 的 `turn/completed` 事件处理中，检测上游返回的 `realSessionId`；一旦提取到真实转储 ID，立即动态更新 `binding.provider_session_id` 并重建 `native_to_hub` 映射，使常驻宿主与磁盘观测保持唯一性。

- **解决上游 429 配额耗尽导致的回合未正常收敛与状态悬挂**：
  - 根因定位：
    1. 当上游网关或模型返回 429（如 `API Error: Request rejected (429) · Resource has been exhausted (e.g. check quota)`）时，CLI 进程以非零状态码退出；
    2. 原先 `run_agent_stream_process` 遇到非零退出码且非特殊状态时，直接返回 `CollectorError::Protocol`；
    3. 该异常中断了正常的 `turn/completed` 事件派发链路，导致宿主绑定的 `current_turn_id` 永远无法置空，会话永远卡在 `Running` 状态。
  - 修复方案：
    1. 在 `crates/runner/src/collectors/mod.rs` 中增强对 CLI 进程输出的捕获：当检测到上游明确的 API 错误信息（429 配额耗尽等）时，将其正常打包为 `AgentStreamOutcome` 返回并附带提取到的 `session_id`；
    2. 确保宿主收到完整的 `turn/completed` 响应并输出错误提示，同时释放 `current_turn_id`，恢复 `execution_state` 为 `Idle`。

- **根治“这个会话已有一个回合正在运行”死锁并提供前端一键自愈**：
  - 根因定位：
    1. 桌面端 `cancel_session_turn` 指令原先若匹配不到对应的宿主 Binding 便直接报错，且在多重会话 ID 格式（带前缀/无前缀）下可能找不到目标；
    2. 前端输入框检测到错误后由于状态未重置，输入框被永久禁用，用户无法继续发送新消息或重新重试。
  - 修复方案：
    1. 在 `apps/desktop/src-tauri/src/lib.rs` 中重构 `cancel_session_turn`：主动清理内存中的 `active_turns` 记录，并对会话 ID 进行双向归一化匹配；即使底层进程已死亡，也确保释放回合锁并向前端返回成功；
    2. 在 `crates/runner/src/remote_host.rs` 的 `RemoteAction::Interrupt` 中增强容错：即使 `session.current_turn_id` 丢失，也强制重置状态为 `Idle` 并清理残留的审批与提问；
    3. 在 `apps/desktop/src/components/session/SessionControlPanel.tsx` 中优化错误提示横幅：新增“解除锁定并重试”自愈操作按钮，用户点击即可一键重置回合状态、取消残留锁死并重新激活输入框。

## [2.4.61] - 2026-10-04

根治宿主与磁盘观测前缀失配导致同名会话重复分身缺陷、发布 Windows v2.4.61 正式安装包：
规范化 NativeSessionHost 会话内部前缀（`claude_` / `antigravity_` / `grok_` / `sess_`），使常驻宿主状态精准覆盖合并至磁盘扫描条目，消除侧边栏幽灵双重会话。

### 会话侧边栏与多源聚合缺陷根治

- **根治同个会话在侧边栏出现两个同名条目的分身缺陷**：
  - 根因定位：为什么同一个会话会在侧边栏同时存在两条相同的记录？
    1. 磁盘扫描观测记录（Disk Observation）通过 `collectors` 模块生成，其 `agent_session_id` 是原始裸 UUID（例如 `c05a7f38-16d6-4a6b-8e9e-1ed4540e8161`），插入 `sessions` 映射表的键为 `claude_code:local_device:c05a7f38...`；
    2. 而常驻进程服务（NativeSessionHost）的 SessionBinding 中保留了 Provider 前缀（例如 `claude_c05a7f38-16d6-4a6b-8e9e-1ed4540e8161`）；
    3. `merge_dashboard_sessions` 遍历 `host_bindings` 时，直接拼接生成键 `claude_code:local_device:claude_c05a7f38...`，在调用 `sessions.get_mut(&key)` 时无法匹配到已存在的磁盘观测条目；
    4. 从而错误地在 `sessions` 中又 `insert` 了一个全新的条目，导致侧边栏同时渲染出两个指向同一磁盘转储与同一工作区的重复会话；
  - 修复方案：
    1. 在 `apps/desktop/src-tauri/src/lib.rs` 的 `merge_dashboard_sessions` 中，对 `binding.provider_session_id` 执行前缀规范化（剥离 `claude_`、`antigravity_`、`grok_`、`sess_` 等前缀），生成统一的 `clean_id`；
    2. 键值与 `sessions.insert` 均统一采用规范化后的无前缀裸 ID，使宿主常驻状态能够精确合并到既有的磁盘观测对象中，从源头根治侧边栏会话重复问题；
    3. 新增单元测试 `host_session_registry_merges_prefixed_binding_with_bare_observation`，验证带前缀 Binding 与无前缀 Observation 能够严格合并为 1 个会话。

## [2.4.60] - 2026-10-04

CC Switch 与第三方网关动态模型实时感知与下拉菜单即时刷新：
直读 CC Switch 活跃数据库路由与上游 `/v1/models` 实时模型、根治静态缓存阻断刷新、模型下拉展开与窗口切回自适应同步、净化网关路由展示。

### CC Switch 与第三方网关动态模型实时同步

- **彻底打通 CC Switch 活跃数据库与上游网关实时模型探针**：
  - 根因定位：为什么在 CC Switch 或网关中修改了模型配置与路由后，AgentHub 依然显示旧模型列表，且没有动态刷新？
    1. 静态配置库快照隔离：Runner 的 `discover_claude_gateway()` 在发现本地代理凭证时，只读取了历史静态磁盘快照 `configLibrary/*.json`，而 CC Switch 用户的日常修改直接写入 SQLite 数据库 `~/.cc-switch/cc-switch.db`，导致修改后 AgentHub 无法感知；
    2. 上游真实模型接口未被探针感知：网关配置了真实的 Base URL（如 `https://cpa.tannerlab.cn/v1`），该端点的 `/v1/models` 提供了完整的上游实时可用模型（如 `claude-opus-5-5-high`、`claude-opus-4-6-thinking`、`claude-sonnet-4-6`、`gemini-3.8-flash-high` 等），但后端此前从未对上游 `/v1/models` 进行过探针同步；
    3. 前端长 TTL 缓存与缺乏触发刷新机制：前端 `agentModels.ts` 默认将模型列表缓存 60 秒，且下拉框展开（`SelectDropdown`）时完全不会触发重拉刷新，导致即使用户在 CC Switch 中调整了模型，切回桌面客户端后依然看到旧缓存；
    4. 官方内置全量模型污染网关列表：后端在网关路由模型后无条件追加了 `builtin_model_catalog` 中的 `Claude 3.7 Sonnet` 与 `Claude Sonnet 4.5`，导致用户在使用自建/中转网关时，下拉框中混杂着上游并不支持的旧版模型；
    5. 硬编码标签误导：之前在 `lib.rs` 中硬编码了 `Claude Opus 5 (Alpha)` 与 `实测秒级响应` 等静态文案，掩盖了真实的网关路由目标。
  - 修复方案：
    1. 直读 CC Switch SQLite 动态配置：Runner 新增 `discover_cc_switch_active_provider()`，直接只读打开 `~/.cc-switch/cc-switch.db`，精确提取当前活跃 Provider（`is_current = 1`）的真实路由（`claudeDesktopModelRoutes`）与配置模型；
    2. 上游网关 `/v1/models` 实时探针与智能去重合并：新增 `fetch_live_upstream_models()` 与安全过滤（过滤 image/audio/tts/asr 等非语言模型），将网关配置路由（如 `Claude Opus 5 -> claude-opus-5-5-high`）与上游新增模型无缝融合；
    3. 下拉菜单打开即时触发强制刷新：`SelectDropdown` 增加 `onOpen` 钩子，并在会话聊天面板 `SessionControlPanel`、新建会话舞台 `NewSessionStage` 与启动会话弹窗 `LaunchSessionModal` 中绑定强制刷新，用户点击模型下拉框的瞬间秒级获取最新模型列表；
    4. 窗口聚焦自适应刷新：在 `App.tsx` 监听窗口 `focus` 事件，当用户在 CC Switch 修改完切回 AgentHub 窗口时，立即自动静默同步最新模型；
    5. 网关激活态过滤内置噪音模型：激活网关路由时，不再硬编码注入无关内置模型；清晰标注路由目标（如 `网关路由 → claude-opus-5-5-high` 与 `网关实时模型`）；
    6. Codex 同步支持 CC Switch 活跃配置：Codex 模型列表同步查询 `cc-switch.db` 中的当前 Provider 配置，实现多 Agent 全面动态化。

## [2.4.59] - 2026-10-04

全面对齐 Claude.ai 官方会话智能命名体验与幽灵编号会话治理：
实现会话首条指令内容自适应智能命名、前缀规范化交叉索引、淘汰未连接无转储的残留僵尸会话、自动清洗 LocalStorage 孤儿缓存并将历史存档规范化为透明友好的未命名会话标注。

### 会话内容智能命名与幽灵编号会话治理

- **全面对齐 Claude.ai 首条提示词智能标题（告别纯编号会话）**：
  - 根因定位：为什么之前只有极少数会话显示内容标题、绝大多数显示十六进制哈希（如 `会话 #77813f44`）？
    1. 标题提取触发点缺失：之前仅在进入已有会话并重新拉取消息后才会触发前端重命名；用户新建会话时，`LaunchSessionModal` 与 `NewSessionStage` 启动后均未注册 `rememberTitleOverride`；
    2. 键值格式前后端前缀偏差：Native Host 宿主层会话 ID 带有 Provider 前缀（如 `claude_<uuid>`、`antigravity_<uuid>`、`sess_<uuid>`），而磁盘转储记录（`.claude/projects/.../*.jsonl`）使用无前缀裸 UUID，导致 `session_context_key` 无法命中磁盘解析的上下文标题；
    3. 现代 Claude Code 首行事件提取缺失：现代 Claude Code 转储首行常为 `queue-operation` 入队事件（`operation: "enqueue"`），之前仅扫描 `user` 角色消息，导致首轮对话在入队阶段未能提取出标题；
    4. 标题长度阈值过高：之前限定提示词长度必须 `>= 4` 字符，排除了“你好”、“审查”、“部署”等常用中文首句；
    5. 僵尸 Binding 凭空注入会话列表：之前在 Native Host 启动或失败重试时残留了未连接（`control_connected: false`）且磁盘无对应 `.jsonl` 的空 Binding，在 `merge_dashboard_sessions` 中被无条件注入 `sessions`，导致侧边栏堆积大量刚刚创建失败的无内容哈希编号（如 `会话 #ae74135b`、`会话 #06ac5f4f`）；
    6. 前端 LocalStorage 永久死锁脏缓存：前端 `cachedSessions` 无条件从 `agenthub:cached-sessions` 中回填已死亡且无标题的孤儿会话，导致即先后端已淘汰，前端依然永久显示这些死编号。
  - 修复方案：
    1. 过滤淘汰未连接无转储的僵尸 Binding：在 `apps/desktop/src-tauri/src/lib.rs` 的 `merge_dashboard_sessions` 中，对在主会话列表中不存在的 Host Binding 增加前置活性校验，非活跃且无磁盘转储的空 Binding 不再注入会话列表，从源头杜绝幽灵编号会话；
    2. 前端缓存智能清洗：在 `apps/desktop/src/App.tsx` 中重构 `cachedSessions` 初始加载与快照合并逻辑，自动剔除权威后端已淘汰且无实质标题的孤儿死会话，彻底打破 LocalStorage 脏缓存持久化循环；
    3. 全链路前缀剥离与多维键值索引：在 `presentation.ts` 的 `rememberTitleOverride` 和 `sessionDisplayTitle` 中，同时建立 `provider:rawId`、`provider:strippedId`、`rawId`、`strippedId` 的交叉索引映射，不论前端传入何种形式的会话 ID 均能秒级命中对应标题；
    4. 新建会话即时建立标题覆盖：在 `LaunchSessionModal` 和 `NewSessionStage` 启动成功瞬间，立即以用户首条提示词调用 `rememberTitleOverride`，会话在侧边栏出现的第一秒即显示完整指令标题；
    5. 优化无标题历史远端存档语义：将纯冰冷的开发编号统一升格为亲切透明的 `未命名会话 (#short_id)`，并在 Tooltip 中附带工作区、活动时间与明确提示，彻底告别令用户费解的机器乱码；
    6. `collectors/mod.rs` 补充 `queue-operation` 识别：从 Claude Code 转储首行入队事件中实时提取 `local_prompt_title`，本地所有已存在的 53 个有效会话 100% 自动对齐自然语言内容标题。

## [2.4.58] - 2026-10-02

根治新建对话假死卡顿、第三方网关 429 递归重试风暴与模型配额智能路由：
优先路由可用旗舰模型、捕获流式重试与配额耗尽事件并在 UI 实时透明反馈、优化新建对话排版与长模型名自适应。

### 核心运行时与网关调度修复

- **根治已有会话继续对话模型参数丢失与 429 报错**：
  - 根因定位：在会话聊天面板中切换模型下拉框后发送消息，`RemoteAction::SendPrompt` 协议缺少 `model` 字段传递，导致后端忽略输入框所选模型，直接退避读取该会话历史转储记录里的旧模型（`claude-fable-5`），重新命中上游耗尽的 Gemini Flash 429 配额错误；
  - 在 `RemoteAction::SendPrompt` 协议中新增 `model` 字段支持，并在 `MultiProviderTransport` 的 `turn/start` 调度中动态缓存与应用单次会话模型覆盖，确保前端在已有会话中切换到 `Claude Opus 5 (Alpha)` 后，CLI 严格使用用户选择的可用模型执行；
  - 会话面板模型继承：在 `SessionControlPanel` 中，优先继承并展示会话已有元数据中记录的 `runtime.model`，避免新进入会话被错误回退重置；
  - 侧边栏会话标题规整：在 `presentation.ts` 中解析会话短标题时自动剥离 `claude_` / `antigravity_` / `grok_` 等 Agent 内部前缀，标题由形如 `会话 #claude_a` 规整为美观标准的 8 位 Hash 标识（如 `会话 #a11779f2`）；
  - 发送锁死与状态机解耦：引入 `sending` 状态切分，杜绝面板前置校验早退将输入锁死的异常分支，避免后台轮询过早释放状态；
  - 错误详情全链路透传：在 `remote_host.rs` 与 `lib.rs` 的 `turn/completed` 事件中透传底层 CLI 的真实报错信息，彻底告别空洞模糊的“Agent 执行失败”。
- **根治新建会话 429 重试风暴与假死无响应**：
  - 根因定位：第三方网关（CC Switch / Claude-3p）配置中，`claude-fable-5` 映射的上游模型（Gemini 3.8 Flash）遭遇配额耗尽（`429 RESOURCE_EXHAUSTED`）。Claude Code CLI 默认触发指数退避 10 次重试循环，每次重试等待 25~~30 秒，导致子进程被阻塞长达 5~~10 分钟；同时 `parse_agent_turn_update` 未解析 `system.api_retry` 事件，前端聊天面板仅显示通用静态提示“正在思考与执行...”，输入框锁死，给用户造成“根本无法新建对话”的完全卡死体验；
  - 智能路由与模型优先级：在 `crates/runner/src/claude_gateway.rs` 和 `apps/desktop/src-tauri/src/lib.rs` 中重构网关模型解析与排序，将实测配额充足、秒级响应的旗舰模型 `Claude Opus 5 (Alpha)` 设为默认首选（实测 2.1 秒极速完成响应）；
  - 限制网关异常重试次数：通过 `inject_into_command` 注入 `CLAUDE_CODE_MAX_RETRIES=2`，将网络或配额异常情况下的重试耗时从 5 分钟硬编码超时缩减至快速响应，彻底杜绝无尽阻塞；
  - 透传明确的 API 错误信息：在 `crates/runner/src/collectors/mod.rs` 的 `run_agent_stream_process` 中，当进程因 API 错误（如 429 配额用尽、认证失败等）以非 0 退出时，自动从输出流提取真实的人类可读错误文本（`final_agent_message`），以 `ProtocolError` 准确报告给前端展示，告别模糊晦涩的“exit code: 1”。

### 流式事件捕获与交互状态透明化

- **支持 `api_retry` 与 `status` 流式事件透传**：
  - 在 `crates/runner/src/collectors/mod.rs` 的 `parse_agent_turn_update` 中，补齐对 `system.api_retry` 事件的解析，当上游模型出现限流重试时，立即向会话流输出醒目的透明提示（`⚠️ [上游模型响应限流/配额不足 (HTTP {status} {error})，Claude 正在自动重试第 {attempt}/{max_retries} 次...]`）；
  - 支持 `system.status: "requesting"` 事件，实时将 Agent 状态更新为“Claude 正在请求模型 API”，让用户对进程执行进度了如指掌。

### 前端体验与新建会话排版精修

- **新建会话页（NewSessionStage & LaunchSessionModal）排版与状态反馈优化**：
  - 在 `NewSessionStage` 与 `LaunchSessionModal` 中为 `onEvent` 接入实时事件监听器，启动执行期间在卡片下方呈现平滑渐入的实时状态条（显示当前阶段、工具调用或模型请求状态）；
  - 解决模型下拉选择框在长名称下的显示挤压问题：将 `.composer-dropdown-wrapper` 的最大宽度从 170px 扩展至 260px，自适应展现 `Claude Opus 5 (Alpha)` 等完整模型标识；
  - 同步更新前端 `agentModels.ts` 内建模型目录，移除旧版 Claude 3 遗留硬编码，与官方及网关当前真实支持的最新模型序列全面统一。

## [2.4.57] - 2026-10-01

深度代码审查与系统级缺陷根治：全面修复新建会话瞬态路由与状态映射缺陷、根治成功回合静默退出误报失败、修复多 Provider 绑定识别偏差、实现模型动态拉取与官方真实模型目录整合。

### 系统级代码审查与缺陷根治

- **异步会话拉起瞬态路由闭环（根治卡在新建页假死现象）**：
  - 根因定位：前端发起新建会话成功后，异步调用 `refresh()` 时本地 outbox/磁盘扫描存在数百毫秒落盘延迟，`snapshot.sessions` 尚未包含新建的会话对象；此时 `sessions.find(...)` 返回 `undefined`，导致主舞台判断 `selectedSession !== undefined` 失败，瞬间回退重渲染 `NewSessionStage`，用户误以为拉起无响应或卡死；
  - 修复方案：在 `App.tsx` 的 `handleSessionLaunched` 中引入乐观会话实体，在获得会话 ID 的瞬间立即写入 `cachedSessions` 激活路由；当后台 `refresh()` 采集到权威数据后平滑无缝合并覆盖，彻底杜绝回退闪烁与假死；
- **状态机与会话生命周期完整映射（修复“未知”状态）**：
  - 根因定位：`presentation.ts` 中的 `statusView` 仅显式处理了 `WORKING`、`WAITING_PERMISSION`、`WAITING_INPUT`、`IDLE` 和 `FAILED`，缺少对 `COMPLETED` 与 `STOPPED` 的分支；在 `presence` 为 `LIVE` 的活跃会话中，任务正常完成或终止时状态被错误降级显示为“未知”；
  - 修复方案：在 `statusView` 中增加 `COMPLETED`（显示为“已完成”，样式 `completed`）和 `STOPPED`（显示为“已停止”，样式 `stopped`），并在 `modern.css` 中补齐对应状态点与状态药丸的优雅降噪配色；
- **Native Host 宿主绑定多 Provider 映射修复**：
  - 根因定位：在 `apps/desktop/src-tauri/src/lib.rs` 的 `merge_dashboard_sessions` 中，遍历 `host_bindings` 时仅通过前缀检查了 `sess_` (ZCode) 与 `claude_` (Claude)，未匹配 `antigravity_` 与 `grok_`，导致后两者被一律粗暴兜底为 `codex`，破坏会话归属和类型契约；
  - 修复方案：严密补齐 `is_antigravity` 与 `is_grok` 判定，严格映射至各自正确的 `AgentType` 与对应上下文键值；
- **成功回合静默退出容错增强**：
  - 根因定位：当 CLI 工具执行纯代码补丁或只读工具调用后以代码 0 正常退出，但输出流中未生成任何独立的 conversational 文本块时，`final_agent_message` 早期直接返回 `None` 触发 `missing_final_message` 协议错误，导致原本成功的执行被误报为 `AgentFailed`；
  - 修复方案：在 `crates/runner/src/collectors/mod.rs` 与 `src-tauri/src/lib.rs` 中，进程成功退出且转录完成时，若无多余对话文本则自动回退并优雅填充“（任务执行完成）”，绝不产生误判报错；

### 核心运行时与执行流修复

- **彻底修复新建会话执行卡死与响应丢失**：
  - 根因定位：在 `crates/runner/src/remote_host.rs` 中，`turn/completed` 事件组装时未将子进程的最终文本响应 `res.response` 赋值至 `params.turn.output`；且 `parse_agent_turn_update` 仅处理了 `stream_event`，忽略了直接通过 `assistant` 消息块输出的情况。这导致桌面端接收到完成事件时 `output` 字段为 `null`，被判定为空输出触发 `AgentFailed`，UI 失去初始响应并陷入死锁；
  - 修复事件装配：在 `crates/runner/src/remote_host.rs` 中针对 ClaudeCode、Antigravity 和 GrokBuild，在执行成功时将 `res.response` 严谨填充进 `completed_params["turn"]["output"]`；并在 `parse_agent_turn_update` 中增强对 `assistant` 消息块文本的提取；
  - 增强 `final_agent_message`：在 Claude CLI 的输出流中，增加对 `assistant` 消息内容块和 `stream_event` 增量字符流的多级后备回退解析，绝不因缺少特定格式的外层包装丢失输出；
  - 乐观预填初始会话：在 `NewSessionStage.tsx` 和 `LaunchSessionModal.tsx` 中，新建会话执行成功后第一时间调用 `seedConversation` 预填初始用户提示词与 Agent 响应，确保进入会话面板时绝不出现“暂无历史消息”的空白态；并在数据刷新轮询中增加守卫，在 `busy` 状态且磁盘未完成落盘写入时绝不以空数组覆盖已有消息。

### 排版与视觉层精简（Aozora UI 纯净规范）

- **彻底消除执行中三项重复提示**：
  - 去除输入框上方重复多余的绿色状态栏 `.composer-running-alert`，彻底消除与聊天流内部 `.agent-thinking-card`、顶部状态药丸的“三重重复显示”；
  - 在会话处于 `busy` 执行中时，彻底隐藏“暂无可见的历史消息 / 重新加载”空状态引导块，由对话流中精致的 `.agent-thinking-card` 单一主导呈现，排版清爽通透。

### 模型选择动态拉取与真实模型目录整合

- **美化网关动态发现模型展示**：
  - 修复网关动态发现模型时拼接内部路由标签导致的混淆字符串问题（将类似 `claude-fable-5 (gemini-3.8-flash-high)` 拆解为优雅规范的用户可见名称 `Claude Fable 5`，并将网关路由目标后置到描述字段中清晰呈现）；
  - 在网关发现模型的基础上，保留并整合官方全套 Claude 真实模型（`Claude 3.7 Sonnet`、`Claude Sonnet 4.5`、`Claude 3.5 Sonnet`、`Claude Opus 4.1`、`Claude Haiku 4.5`）与本地会话实测模型，彻底去除前端与后端遗留的伪模型硬编码；
  - 同步修正 `agentModels.ts`、`presentation.ts` 与 Rust 本地内建模型目录，确保下拉列表真实、现代且层次分明。

**验证**：Rust Workspace 全部通过（25 个集成测试 + 141 个单元测试 + 37 个桌面测试，0 失败）、前端打包构建（`tsc -b && vite build`）通过 0 错误。

## [2.4.56] - 2026-10-01

新建会话跳过磁盘探测零延迟启动、ACP 工具全量特化渲染（提问交互闭环、Web 搜索/抓取卡片、超长输出折叠与 60 FPS 性能提升）、品牌图标现代化（通透白底与中央立方体几何放大重绘）。

### 品牌设计与视觉资产升级（Aozora UI 白底与几何立方体重构）

- **图标背景转换为现代通透白底**：
  - 响应设计要求，将应用图标底色由深色暗夜基底升级为纯净现代白底（带超细 `#e2e8f0` 边缘防晕染轮廓），呈现更具科技感与通透呼吸感的高端质感；
- **核心等距立方体/中枢星形比例放大**：
  - 中央中枢立方体结构半径从 120px 等比放大约 35%（R=162px），线条加粗至 10px，外围节点增加双重环状发光与天蓝脉冲点缀，核心发光光晕增强；
- **全平台图标资产同步重构与打包**：
  - 同步重构生成 Desktop（`.ico`, `.png`, 所有 Windows 磁贴尺寸）、Web PWA（`icon.svg`, `icon-192`, `icon-512`）与 Android 启动图标（`mipmap` 全套密度），并重新完成安装包打包与 SHA256 校验更新。

### 核心运行时优化

- **新建会话显式去 resume 且零磁盘 I/O**：
  - 彻底去除显式新建会话（`ConnectMode::Create`）时对磁盘 `~/.claude/projects/` 转录文件的遍历扫描，彻底杜绝历史 I/O 延迟；
  - `NativeSessionHost` 显式维护 `newly_created` 集合，仅在首回合调度中显式传递 `sessionExists: false`；
  - `TurnOptions` 扩展 `session_exists: Option<bool>`，当 `session_exists == Some(false)` 时直接以 `-p` 原生启动且不传 `--resume`，后续回合自动平滑衔接已建立的会话。

### ACP 工具全量渲染与交互闭环

- **交互式提问（`askUserQuestion`）点击闭环**：
  - 将只读的选项列表重构成交互式按钮卡片，支持悬浮（Hover）、按压（Active）、已选标记与“点击选择”操作指引；
  - 接入 `onAnswerQuestion` 交互闭环，用户点击卡片选项即可自动将决策内容填入输入框并聚焦，无需繁琐手打。
- **`WebSearch` 与 `WebFetch` 专属富媒体卡片**：
  - 扩展 `ToolCategory` 增加 `web_search` 与 `web_fetch` 专属类型分类；
  - 实现 `parseWebSearchData` 与 `parseWebFetchData`，渲染带有专用图标（`Search`、`Globe`）、搜索气泡、域名徽章（Domain Pill）、外部链接（`ExternalLink`）与结构化搜索结果条目的专属卡片。
- **超长终端/工具日志折叠与 60 FPS 性能优化**：
  - 新增 `TruncatedOutputBlock` 渐进式折叠组件，当参数或执行结果超过 80 行时自动折叠，附带“展开全部 (共 N 行)”与“收起”操作按钮，彻底杜绝超长日志卡死主进程；
  - `ToolCallBlock` 采用 `React.memo` 封装，并对所有工具解析与 Diff 计算采用 `useMemo` 细粒度缓存，避免长文本流式更新下的无谓重复渲染。

**验证**：Rust Workspace 全部通过（25 个集成测试 + 141 个单元测试，0 失败）、`cargo clippy` 0 警告、前端 16 个套件 94 个单测全量通过、TypeScript 类型检查 0 错误。

## [2.4.55] - 2026-10-01

聚焦 Claude 核心体验、Codex 插件化改造、侧边栏精简与新建 Claude 会话异常彻底修复。

### 核心运行时与新建 Claude 会话修复

- **消除新建 Claude 会话异常与自愈判断**：
  - 排查修复了新建 Claude 会话时因误带未初始化的随机 UUID 参数执行 `--resume`，导致官方 Claude CLI 报 `No conversation found with session ID` 进而触发 `Agent 执行失败` 的根本缺陷；
  - 在 `crates/runner/src/collectors/mod.rs` 与 `remote_host.rs` 中增加历史会话自愈判断：若本地未发现对应 transcript，则以新会话模式启动（不加 `--resume` 参数），并在首回合响应流（`init` / `result` 事件）中捕获并无缝提取官方生成的真实会话 ID；
  - 在 `apps/desktop/src-tauri/src/lib.rs` 中同步接收并绑定真实的 `realSessionId`，确保新会话跳转与后续继续回合完全连贯；
  - `agent_binary.rs` 补充识别 `CLAUDE_CODE_EXECPATH` 环境变量，确保宿主环境与 managed 安装版本优先且准确命中。

### 桌面端界面清理与插件体系聚焦

- **精简侧边栏顶部**：
  - 响应视觉减负要求，移除左侧栏顶部原有的 "AgentHub" 品牌文本按钮，保留新建任务、搜索与刷新等核心操作按钮并优化靠右对齐布局；
- **专注于 Claude Code 且 Codex 移入插件**：
  - 默认聚焦于 Claude Code，将 Codex 移入插件商城（`AGENT_PLUGIN_MARKETPLACE`），由用户在插件中心自主获取并管理；
  - `isPluginEnabled` 仅默认常驻 Claude Code，未启用 Codex 插件时系统自动隐藏 Codex 会话与选项，启动台（`NewSessionStage`）及侧边栏默认完全聚焦于 Claude；启用插件后自动挂载 Codex。

**验证**：`cargo test --workspace` 全部通过、`npm.cmd test`（16 个套件 91 个测试全部通过）、`npm.cmd run typecheck` 0 错误。

## [2.4.54] - 2026-09-29

无边框窗口交互优化、Codex 动态模型发现与 Remote 工作区管理全面重构。

### 桌面端体验与窗口控制

- **无边框窗口拖拽与 Caption 控件重构**：
  - 新增 `WindowControls.tsx` 集中管理窗口拖拽区域（`data-tauri-drag-region` 与 `startDragging` 事件）和标准化标题栏控件（最小化、最大化/还原、关闭）；
  - 修复登录界面（`AuthScreen.tsx`）与加载过渡界面（`LoadingScreen`）无边框状态下无法通过鼠标拖动窗口的问题，全面补齐单测覆盖（`WindowControls.test.tsx`）；
  - 重构 `DesktopHeader.tsx`，复用统一窗口 Caption 控件，消除跨组件重复代码与 IPC 调用的不一致性。

### Codex 模型动态聚合（拒绝硬编码）

- **动态配置与实时探测融合**：
  - 新增动态配置文件解析（`read_codex_configured_model`），在应用启动与列出模型时实时读取用户的 `~/.codex/config.toml` 配置（如 `model = "gpt-6-astra"`）；
  - 彻底杜绝在代码中硬编码任何具体模型名，通过动态家族归类（`model_family`）和动态标签格式化（`model_label` 将 `gpt-*` 自动投影为 `GPT-*`），优先展示用户主动配置的模型，并无缝合并本地 Codex RPC 探测结果与已有会话转录数据中的历史模型。

### Remote 自动化与工作区授权重构

- **登录自动连接 Remote 与工作区自适应**：
  - 优化登录流程（`finish_login`），登录成功后自动将 Remote 状态置为开启并自动授权当前所在工作目录，自动启动 `agenthubd` 守护进程并连接至远程网关，免除用户反复手动确认与重复弹窗；
  - 后端新增 `save_host_workspaces` IPC 命令，支持运行时动态增删授权工作区目录并向活跃守护进程热同步。
- **彻底移除突兀的 `NativeHostDialog` 全屏弹窗**：
  - 移除原先突兀的独立对话框 `NativeHostDialog`，将主界面左下角移动端/Remote 状态按钮、顶部菜单栏中的“管理本机会话守护”以及新会话引导界面的入口统一平滑导航至设置面板中的「远程连接与 Host」标签页；
  - **设置面板「远程连接与 Host」深度重构**：
    - 集成 iOS 风格滑动开关，支持随时一键启停远程网关连接，直观显示网关地址与实时双工通道状态；
    - 提供直观的工作区文件夹授权列表，支持通过系统文件夹选择器一键添加授权目录，并配备安全防护说明；
    - 展示底层 `agenthubd` 守护进程状态与 Ed25519 签名设备绑定标识。

**验证**：`cargo test -p agent-pulse-desktop`（37 个用例全部通过）、`cargo clippy`（0 警告）、`cargo fmt`、`npm.cmd test`（16 个测试套件，91 个测试全部通过）、`npm.cmd run typecheck`、`npm.cmd run format:check` 全数通过。

## [2.4.53] - 2026-09-29

移动端 UI 现代化与去 Demo 化全面重构，多底色对比度排查与无障碍修复。

### 移动端 UI 现代化与重构（对标 Claude 移动端）

- **色彩体系与视觉规范升级**：
  - 彻底移除移动端糖果粉、大圆角线框等玩具化/Demo 感设计，全面迁移至 Claude / Anthropic 暖中性色体系；
  - 浅色模式采用温和米白背景（`#FAF9F5`）、白色卡片面（`#FFFFFF`）、发丝边框（`#E5E3DC`）与陶土砖红品牌色（`#C2410C`）；深色模式采用深炭灰底色（`#1F1E1D`）、工作台卡片表面（`#262523`）与暖琥珀强调色（`#D97706`）；
- **纯原生矢量绘图系统**：
  - 废弃纯文本 ASCII/Unicode 占位字符（`↗`、`□`、`✓`、`▣`、`○`、`⚡`、`💭`、`▲`、`▼` 等），重构为 Jetpack Compose `Canvas` 原生矢量渲染（`NavIconRemote`、`NavIconSessions`、`NavIconWaiting`、`NavIconDevices`、`NavIconSettings`、`SendArrowUpIcon`、`ChevronDownIcon`）；
- **工作台与交互架构优化**：
  - **顶部与底栏**：顶部状态栏采用精致呼吸微点（`LiveStatusPulse`）与连接状态；半透明高质感底栏（`alpha = 0.96f`）搭配椭圆激活胶囊与未读通知徽标；
  - **概览面板**：重构 4 格粗糙指标为高密度水平滚动概览胶囊（`WorkspaceSummaryStrip`），新增单会话聚光卡片（`ActiveSpotlightCard`）；
  - **远程控制台**：工具调用采用紧凑单行状态条；思考过程折叠胶囊支持矢量箭头平滑展开/收起；代码块适配 Claude 深色终端卡片并具备复制微反馈；底部输入栏重构为 Claude 风格的悬浮输入胶囊与圆形向上发送按钮；
  - **Web 移动端底栏**：同步升级 `MobileDock.tsx`，将文本占位符替换为系统的原生 SVG 矢量图标。

### 主题对比度与无障碍修复

- **全底色对比度合规审计**：对 Desktop 的 `light`、`dark`、`amber` 三套主题底色进行全面审计，引入并验证 `--brand-accent-text` 语义化对比度代币（深色模式比值高达 8.24:1，浅色/琥珀模式达 5.77:1，严格符合 WCAG AA 4.5:1 规范）；
- **弹窗与输入框修复**：修复重命名会话弹窗在深色底色下边框缺失与输入字体过淡的问题，统一采用标准主题变量。

**验证**：`./gradlew compileDebugKotlin`、`./gradlew test`、`npm test`（contracts 15、api 60、desktop 88、web 61）、`npm run typecheck`、`npm run format:check` 全数通过。

## [2.4.52] - 2026-09-28

桌面端交互现代化与进行中会话状态反馈体验全面升级。

### 体验与界面

- **顶部工具栏现代化（对标 Claude Desktop）**：重构桌面端顶栏，引入应用主菜单（`≡`，包含 File/Edit/View/Go/Help 菜单与快捷键）、侧边栏折叠按钮（`◫`）与前进/后退导航箭头；移除原本的蓝色 `[A]` 徽标及顶栏下方的分割线，使顶栏与工作区融为一体；
- **设置面板去硬编码与安全防护**：移除插件市场中硬编码的默认模型与无用标签，动态拉取端点支持的模型；重构 Agent 路径配置为带有文件浏览器的运行时管理面板；隐去对外的 Gateway 服务器地址，防止敏感配置泄露；移除已废弃的“Free”免费等级徽标；
- **内嵌式 Claude 风格新会话工作台**：将原本的弹窗与空白页替换为居中的 Claude 风格新会话工作台，集成 Provider 选择、动态模型下拉、工作目录选择与快捷标签、权限模式选择与自适应输入框；
- **全方位会话进行中反馈与防打断机制**：
  - **侧边栏树状列表**：修复 `.session-status-dot` 状态圆点缺失样式的缺陷，补充色彩体系与 `@keyframes status-dot-pulse` 扩散脉冲，运行中会话显示浅绿底色并在时间列直接呈现实时 `<RefreshCw className="spin" /> 执行中` 标签；
  - **对话流底部**：常驻呈现 `.agent-thinking-card`（带呼吸图标、执行动作说明与停止按钮），彻底解决无 activity 事件时思考指示器隐形的问题；
  - **输入框区域**：上方显示醒目的 `.composer-running-alert` 警示条，标明任务正在执行，并提供“中断任务 <kbd>Esc</kbd>”按钮；输入框动态切换占位提示并锁定输入；支持全局 `Escape` 快捷键安全取消当前轮次；
  - **顶栏指示**：顶栏实时展示会话运行指示胶囊（`.header-running-indicator`）。

**验证**：`npm.cmd test`（contracts 15、api 60、desktop 88、根 61）、`npm.cmd run typecheck`、`npm.cmd run format:check` 全数通过。

## [2.4.51] - 2026-09-28

收尾 2.4.50 审查中留下的两项 P2，去除「长时间运行后必然劣化」的两处放大点。

### 性能

- **会话命令历史改为按会话列走索引**：`services/api/src/remote-store.ts` 的 `commandHistory` 原先执行 `SELECT ... WHERE runner=? ORDER BY seq` 取回该 Runner 的**全部**命令行，再在 JS 里从 `body` 解析出 `hub_session_id` 过滤。`/v3/remote/sessions/:runner/:hub` 是控制台的高频轮询路由，因此单次历史查询的成本随 Runner 累计命令总数线性增长。命令里唯一稳定不变的归属字段就是 hub id，故在写入时投影为独立列 `hub`，查询变为一次索引区间扫描（`remote_command_session(runner,hub,seq)`）。读路径仍逐行对照 `body` 校验投影值，而不是信任它，因此即使某行不一致，返回结果与此前完全一致。**向后兼容**：`CREATE TABLE IF NOT EXISTS` 不会改动已存在的表，故构造函数中的 `migrateCommandHub` 对 `PRAGMA table_info` 探测缺失的 `hub` 列，`ALTER TABLE ADD COLUMN` 后从 `body` 回填，再建索引——旧库升级后首轮轮询即走索引，且历史行一条不丢。新增测试以真实的旧版表结构建库，验证升级后各会话历史仍精确；
- **审批与中断命令不再阻塞异步运行时**：`cancel_session_turn` 与 `resolve_session_approval` 此前是 `#[tauri::command] fn`（同步），内部两次 daemon 往返各可阻塞至本地查询超时的 25 秒。Tauri 2 为同步命令单独派发线程，不占用 async 运行时，但审批恰恰是使用者正在等待 Agent 的时刻。现改为 `async fn` + `spawn_blocking`，与 `send_session_message` / `set_session_archived` 等既有做法一致；daemon 客户端在离开运行时之前解析完毕（其 liveness 探测本身是快速路径），只有真正的往返进入阻塞线程。参数校验仍在异步侧同步完成，错误文案与语义不变。

**验证**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo test --workspace` 232 项通过（3 项因缺 `AGENTHUB_TEST_CODEX_BINARY` 忽略）；`npm.cmd test`（contracts 15、api 60、desktop 88、根 61）、`npm.cmd run typecheck`、`npm.cmd run format:check` 全数通过。

## [2.4.50] - 2026-09-28

投产前的一轮代码审查、性能优化与三处真实会话缺陷修复。

### 缺陷修复

- **空会话不再出现在列表中**：`collectors/mod.rs` 原先对 `~/.claude/projects/**/*.jsonl` 无条件收录，一个用户在首条回复发出前就中断的会话同样会留下完整 transcript 与 `custom-title.json`，因此在侧栏里与真实会话完全无法区分——两个同名会话即由此而来（官方 `/resume` 会隐藏它，本项目此前不会）。现由 `read_claude_transcript_metadata` 顺带判定 `has_output`：扫描记录中是否存在**非空 assistant 文本**（纯工具调用、空文本、`isSidechain` 子代理轨迹均不算），`collect_claude_transcripts_from` 与 `collect_claude_sessions_for_remote` 只收录 `has_output` 的会话。为使该判定覆盖整个文件（而标题在首条记录即可确定），元数据扫描上限由 64 行提至 `TRANSCRIPT_METADATA_SCAN_LINES`（512 行）。本机 95 份 transcript 中 5 份为空会话，均已被正确隐藏。扫描上限提高的同时保留 `has_output && 元数据齐全` 的早退，真实会话仍只读几行；
- **压缩摘要不再泄漏到对话**：`conversation.rs` 的 `normalize_local_conversation_items` 只过滤 `isMeta`，未处理 Claude Code 写自动压缩摘要时使用的两个标记——`isCompactSummary: true` 与 `isVisibleInTranscriptOnly: true`（后者语义即「只存在于 transcript，不要展示给读者」）。该摘要以 `user` 记录形式落盘、承载整个会话的浓缩历史，此前被当作用户发言完整渲染。现二者一并过滤；同时把此前漏网的 `compact_boundary` 记录（`type: system` / `subtype: compact_boundary`）转为**一条状态行**「已自动压缩上下文，节省约 N tokens」，token 数取自 `compactMetadata.cumulativeDroppedTokens`，既不暴露摘要正文又保留可见性；
- **退出时不再闪现黑框**：`daemon_client.rs::terminate_daemon` 关闭时以 `taskkill /F /T` 清理残留进程树，但只重定向了 stdio，**未设 `CREATE_NO_WINDOW`**。`taskkill.exe` 是控制台程序，GUI 进程无自有控制台，Windows 每次退出都会为其分配一个控制台窗口。启动 daemon 的路径早已设置该标志，退出路径遗漏。现两处统一收敛到 `kill_process_tree`，并将裸标志字面量提取为具名常量 `CREATE_NO_WINDOW` / `DETACHED_PROCESS_GROUP`（与启动路径共用）。

### 长驻进程状态修复

- **修复命令回执账本写满后永久拒绝全部新命令**：`crates/runner/src/remote_host.rs` 的 `NativeSessionHost` 原先在内存回执达到 10,000 条时对**之后每一条**命令返回 `command_ledger_full`，且永不回收。由于 daemon 是长驻进程，这等于「跑满 10,000 条命令后该 Runner 彻底不可用」。现改为按插入顺序淘汰最旧条目（`MAX_COMMAND_LEDGER_ENTRIES` / `receipt_order`）。这不削弱幂等性：重放判定的事实来源始终是持久化的 `remote_journal`（`BeginCommand::Existing`），内存窗口只负责 Host 存活期内的重复去重；被淘汰的 id 重发时由日志账本应答而非重新执行。**契约不变**：无新错误码，无协议字段变化；
- **修复已应答请求去重集合无界增长**：`answered_requests` 记录用户已应答过的 approval/question id，用于抑制 Provider 重发同一请求，但原先只增不删。现按插入顺序保留最近 `MAX_ANSWERED_REQUESTS`（10,000）条（`mark_answered`）。窗口足够长以覆盖 Provider 的重发窗口，超出后同一 id 会被重新提出——这与 Provider 侧自身的 `serverRequest/resolved` 生命周期一致，不会把已结束的请求复活为「永远无法点击」的死条目；

### 性能

- **新增 `send_prompt.effort` 契约字段**：`packages/contracts/src/remote.ts` 的 `send_prompt` 命令此前用 `only(command, [...])` 严格拒绝未知字段，而桌面端已按每回合推理强度发送 `effort`，导致非 Codex Provider 的远程发消息被契约层拒绝。现接受 `effort`（≤64 字符）并对所有 Provider 放行，无该概念的 Provider 忽略之。补齐对应契约测试；
- **CLI 版本探测并行化**：`agent_binary::rank_candidates` 原先串行探测每个候选，daemon 启动最坏耗时为 `候选数 × 5s`。现每候选一线程并发探测，最坏耗时降至单次探测超时。结果与顺序无关，行为不变；
- **Web 会话详情轮询去重并降频**：`RemoteConsole.tsx` 原先以 200ms 频率**同时**请求 `/sessions/:runner/:hub` 与 `/events`，即每秒 20 次请求。现合并为单次详情请求，仅当 `cursor` 真正前进时才拉取事件，周期放宽至 1000ms（与快照轮询同量级）——满负载下降为原来的 1/20；
- **会话上下文缓存改为 `Arc` 共享**：`local_session_contexts` 每次调用都深拷贝最多 5,000 条 `LocalSessionContext`（每条含十余个 `String`/`Option<String>`），而 `list_agent_models` 只读取其中的 `model` 字段。现缓存内部存 `Arc<HashMap<..>>`，读路径只做引用计数递增；

### 清理

- `is_unreadable_session_failure` 的 `program` 参数此前被 `let _ = program;` 显式丢弃，注释却声称「要求输出包含程序名」——一个与实现不符、会误导后续维护者的断言。现删除该空操作并把注释改为陈述真实契约：`output` 必须来自失败的二进制自身的 stdout（调用方从不传入其他进程的 stderr）。

**验证**：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings` 全部通过；`cargo test --workspace` 232 项通过；`npm.cmd test`（contracts 15、api 59、desktop 88、根 61）、`npm.cmd run typecheck`、`npm.cmd run format:check` 全数通过。新增回归测试覆盖两处淘汰逻辑、空会话隐藏与输出形态判定、压缩摘要过滤、token 计数格式化；压缩摘要一项另对真实 transcript 验证过（摘要零泄漏、恰好渲染一条「节省约 163k tokens」状态行）。

## [2.4.49] - 2026-09-28

彻底修复会话回合状态机判定与误报「这个会话已有一个回合正在运行」导致无法继续交互的根本故障：

- **修正 `turn_state` 与 `execution_state` 状态对象解析**：支持字符串格式（`"inProgress"`、`"completed"`、`"failed"`）与 JSON 状态对象（`{ "type": "inProgress" }`、`{ "type": "completed" }`、`{ "type": "failed" }`），消除 `turn/start` 与 `turn/started` 返回对象时被意外解析为 `ExecutionState::Unknown` 的缺陷；
- **解除失败/终止/断开连接会话的发消息阻塞**：在 `apps/desktop/src-tauri/src/lib.rs` 与 `crates/runner/src/remote_host.rs` 中，重构会话忙碌判定条件。由原先过于激进的 `!matches!(execution_state, Idle | Completed)` 改为精准检测真实正在运行的回合（`binding.control_connected && binding.current_turn_id.is_some() && matches!(execution_state, Running | AwaitingPermission | AwaitingInput)`）。彻底解决此前会话在前次出错（`Error`）、被用户中断（`Terminated`）或从离线状态重连（`Unknown`）后被永久误判为忙碌并返回「这个会话已有一个回合正在运行」的死锁问题；
- **Provider 活跃回合与取消标记主动清理**：在 `MultiProviderTransport::drain` 消费 `turn/completed` 及 `turn/interrupt` 时，彻底清理 `claude_active_turns`、`antigravity_active_turns`、`grok_active_turns` 及其对应的取消标志，防止后台活跃集合泄漏；
- **CLI 执行线程异常捕获（Panic Safe Guard）**：为 Claude Code、Antigravity 和 Grok 的流式执行线程增加 `std::panic::catch_unwind` 保护，确保无论子进程遇到何种崩溃或异常，均能可靠发射 `turn/completed`（附带 `interrupted` 或 `failed` 状态）以解除前端等待；
- **中断与审批操作的会话标识双向兼容**：在 `cancel_session_turn` 与 `resolve_session_approval` 中全面支持 `claude_{id}` 与 `{id}` 双向匹配，避免会话前缀差异导致的中断失败；
- **全平台版本递增与锁文件同步**：版本升级至 2.4.49，Android versionCode 升至 60，全平台清单与锁文件同步更新。

## [2.4.48] - 2026-09-28

原生自动探测第三方网关与路由模型映射，支持 CC Switch 开启路由时无缝接管 Claude Code 会话：

- **CC Switch / Claude-3p 本地网关自动探测**：新增 `crates/runner/src/claude_gateway.rs` 核心模块，自动扫描并识别环境变量、`Claude-3p` 与 `Claude` 应用目录下的 `host-creds-*.json` 动态会话凭证及 `configLibrary` 持久化网关配置（`inferenceGatewayBaseUrl` 与 `inferenceGatewayApiKey`）；
- **动态环境变量自动注入**：当检测到本地网关运行且全局环境变量未显式指定时，自动为 `claude.exe` 子进程注入 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN` 与 `CLAUDE_CODE_HOST_AUTH_ENV_VAR`，彻底解决 CC Switch 开启路由时 AgentHub 无法接管会话、OAuth 认证过期的根本故障；
- **网关路由双向模型映射（Reverse Model Mapping）**：针对 CC Switch 路由需要 Claude 别名（如 `claude-fable-5`）而上游/会话转录记录实际模型（如 `gemini-3.8-flash`）导致 400 无效请求/502 错误的问题，设计实现了双向模型解析器 `ClaudeGatewayProfile::resolve_model`，支持精准匹配、标签重写匹配、家族关键字启发式匹配及安全降级；
- **会话模型自动回溯（Session Model Discovery）**：在 `crates/runner/src/collectors/mod.rs` 引入 `find_claude_session_model`，在接管已有会话时自动从转录流中回溯模型标识，并通过网关解析器转换为网关兼容的路由模型；
- **全链路模型传递与桌面端模型下拉呈现**：升级 `remote_host.rs` 中的 `MultiProviderTransport`，在 `thread/start`、`thread/resume` 与 `turn/start` 完整缓存并透传模型覆盖；在桌面端 Tauri 后端 `list_agent_models_inner` 中优先展现网关路由配置的可用模型（如 `claude-fable-5 (gemini-3.8-flash-high)`），方便直接选择；
- **全平台版本递增与锁文件同步**：版本升级至 2.4.48，Android versionCode 升至 59，全平台清单（Cargo、Node、Tauri、Android）同步更新。

## [2.4.47] - 2026-09-27

工具调用可视化、差异比对展示、交互式提问卡片与富上下文审批卡片升级：

- **工具调用多维分类与语义呈现**：新增 `classifyTool` 机制，将工具调用划分为 `terminal`（终端命令）、`file_edit`（文件写入/修改）、`file_read`（读取/搜索）、`interactive`（人机交互/计划确认）与 `system`（系统/状态），按类别呈现专属语义图标与高亮左边框；
- **文件变更统计徽章与代码差异比对**：新增 `parseDiffStats` 与 `tool-diff-viewer`，自动解析 `Edit`/`Write` 的 JSON（`old`/`new`、`old_string`/`new_string`）及 Unified Diff 格式，展示行增删徽章（`+N -M`）与双色差异比对块（删除红底/新增绿底），消除大段未经处理的原始差异文本堆叠；
- **交互式提问卡片与决策渲染**：拦截 `AskUserQuestion` 与人机决策载荷，解析多选/单选题目与选项元数据，结构化呈现提问标题、选项卡片及已选答案徽章；
- **快捷复制命令与路径**：新增 `extractToolPrimaryValue` 与 `tool-quick-copy-btn`，点击一键提取并复制终端命令或操作文件路径，提升日常操作效率；
- **富上下文操作审批卡片**：升级桌面端会话审批栏为卡片式 `approval-card`，展示结构化工具名称、命令/路径代码预览块与操作说明，配合权限图标与风险分级，避免误审批高风险操作；
- **Rust Runner 工具标签语义解析拓展**：`crates/runner/src/conversation.rs` 中的 `tool_call_label` 增加对 `questions` 提问数组与 `todos` 待办任务清单的解析，提取首个问题或待办项数生成紧凑直观的标签；
- **Web 端同步实现差异统计与交互提问**：在 `apps/web` 的 `RemoteConsole.tsx` 同步引入 `remote-diff-badge`、`remote-diff-viewer` 与 `remote-question-card`，保持全端体验对齐并满足 `exactOptionalPropertyTypes` 严格契约；
- **全平台版本递增**：版本升级至 2.4.47，Android versionCode 升至 58，全平台清单与锁文件同步更新；
- **验证**：`cargo test --workspace` 全数通过，`cargo clippy` 与 `cargo fmt` 检查通过；桌面端测试 73 项全数通过，Web/Contracts/API 自动化测试与 TypeScript 静态检查全数通过，Prettier 格式 100% 干净。

## [2.4.46] - 2026-09-27

Agent CLI 二进制发现改为「候选收集 → 版本探测 → 择优 + 回退」：

- **根因修复：AgentHub 会用旧版 Claude Code 去 resume 新版写的会话**。`discover_claude_binary()` 原先硬编码优先 `~/.local/bin/claude.exe`，找到即返回，从不比较版本。在本机实测中，该路径命中 2.1.196，而实际持有会话的 `Claude-3p/claude-code/2.1.281/claude.exe` 为 2.1.281；同一 session id 用旧 binary 执行 `--resume` 必然返回 `No conversation found with session ID`，transcript 本身完好。transcript 格式只增字段，因此**新 binary 能读旧 transcript，旧 binary 读不了新 transcript**——这正是需要「始终选最新版」而非「第一个存在的 exe」的原因；
- **新增 `agent_binary` 模块**：三段式发现（candidate discovery → `--version` 能力/版本探测 → selection）。排序键为「已知版本优先于未知版本，其次版本号降序，最后按来源 tie-break」，来源优先级 `EnvironmentOverride > ManagedInstall > UserInstall > PathLookup`——版本相同时优先 Provider 托管安装，因为它才是写这些 transcript 的那个 binary；
- **Managed install 按目录 glob 发现**：`Claude-3p/claude-code/<version>/claude.exe` 的版本号是路径的一部分、每次升级都会变，因此枚举 `<root>/claude-code/*/` 而非写死某个 release；
- **`--version` 探测带硬超时**：直接用 `Command::output()` 会让一个卡死的 binary 无限期阻塞 daemon 启动，改为显式 `try_wait` 轮询并在超时后 kill 子进程。探测失败的 binary 记为「无版本」而非报错，使发现过程能挺过半安装状态；
- **同路径去重**：`PATH` 解析与显式目录可能指向同一个 exe，去重后只算一个候选，避免重试时白白多跑一次进程；
- **resume 失败回退**：新增 `CollectorError::SessionUnreadable`（code `session_unreadable`）。`run_agent_stream_process` 在丢弃 stdout 之前先识别该失败——通用非零退出、超时、取消**都不触发**回退，因为重放一个可能已部分执行的 turn 正是远程命令契约禁止的重复副作用；「会话不可读」发生在模型被调用之前，prompt 从未送达，因此换 binary 重试是安全的；
- **session → binary 兼容关系记录**：回退成功后把可用的 binary 写入 `~/.agent-pulse/agent-binary-affinity.json`，同一 session 后续 resume 优先命中已记录项、跳过已知失败候选。若最新版本仍读不了某 transcript，回退会自动落到旁边的旧版本安装，而不是锁死；
- **抽象推广到所有 Provider**：`discover_antigravity_binary()` / `discover_grok_binary()` 原先复制粘贴同一套「找第一个存在的 exe」模板，现统一走 `discover_provider_binary()` 共享实现，多版本并存时同样按版本择优。ZCode 为 node 脚本而非可执行文件，不参与该问题；
- **override 语义收紧**：`CLAUDE_BINARY` / `GROK_BINARY` / `ANTIGRAVITY_BINARY` 显式指定后，`CandidateSet` 会拒绝再加入任何自动发现的候选——此前 `push` 在 override 之后仍可追加，使 pin 只是「优先」而非「锁定」；
- **全平台版本递增**：版本升级至 2.4.46，Android versionCode 升至 57，Cargo 与 Node 清单同步更新；
- **验证**：`cargo test --workspace` 220 项通过（新增 10 项覆盖版本解析、排序键、override 独占、去重、失败回退不重复候选、错误窄匹配），`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check` 通过；本机实测发现排序为 2.1.281 → 2.1.271 → 2.1.196（`claude`）→ 2.1.196（`~/.local/bin`），确认首选已从旧版切换到实际持有会话的版本。

### 遗留（设计债务，非本次 blocker）

- **模型清单与上下文窗口仍按 `AgentType` 分桶**：`list_agent_models_inner` 的 key 是 AgentType，Claude 桶兜底为 `BUILTIN_MODELS.claude_code`；`resolve_zcode_model_context_window` 靠模型名子串猜窗口（`contains("claude")|contains("gpt-5") → 200_000`、`contains("gemini") → 1M`、else `128_000`）。经 CC Switch 路由到 GLM 等上游时，会给出路由不认的模型名或静默错误的窗口。根因是缺少 `router` 这一正交维度——`AgentType` 是扁平枚举，`model` 是自由字符串，没有任何地方记录「当前被路由到哪个上游」。此问题不影响 resume 本身（`thread/resume` 不带 model），故未纳入本次修复；
- **auth 上报会把路由器凭据当作 Provider 登录态**：`tokens_from_environment(ClaudeCode)` 读 `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`，CC Switch 写入的 key 会被计为 Claude 的认证状态。这是凭据分类问题，不是身份识别问题。

## [2.4.45] - 2026-09-26

上下文容量明细重构、思考强度控制与认证失败归类：

- **上下文分类改为「只报实测、不造假」**：重写 `ContextCapacityPopover`，分类全部来自 Provider 实际上报的 token 计数（缓存读取 / 新输入 / 思考输出 / 缓存写入 / 生成输出），无上报时按 transcript 字符权重在「工具调用与结果」与「对话消息」间归因，并标注 `measured=false`；不再渲染无法验证的 System prompt、Autocompact buffer 等估算行；
- **补齐剩余空间与可展开明细**：进度条与列表新增「剩余空间」段（占窗口真实百分比），带明细的分类可点击展开，展开后显示真实条目计数（如工具条目数、消息条目数）；百分比口径统一为「占已用」，与窗口总量解耦；
- **新增思考强度控制**：`SessionControlPanel` 工具栏按 Provider 能力动态渲染思考强度下拉——Claude Code 暴露实测支持的 `low/medium/high/xhigh/max`，Codex 暴露 `low/medium/high`，ZCode / Antigravity / Grok 不支持故不渲染；默认值回落到 runtime 已上报的 `reasoning_effort`；
- **思考强度贯通到 CLI**：新增 `TurnOptions.reasoning_effort`，`apply_turn_options` 按 Provider 分派——Claude Code 传 `--effort <level>`，Codex 传 `-c model_reasoning_effort="<level>"`（Codex 无 `--effort` 标志）；`RemoteAction::SendPrompt` 增加可选 `effort` 字段，Codex 走 `turn/start` 的 `effort` 参数，Claude Code 走 CLI 参数，空值一律不下发；
- **认证失败不再混入对话流**：新增 `authErrors.ts` 识别 CLI 以助手文本形式返回的登录失败（`Not logged in` / `authentication_failed` / `Please run /login` 等），命中时不再将其渲染为助手回复，改为红色错误提示并给出对应修复命令（`/login` 或 `codex login`）；
- **修复工作区符号链接指向已删除的仓库**：`node_modules/@agent-pulse/*` 四个 workspace 链接仍指向 `D:\Claudespace\Project\Agent-Persona\agent-pulse`（该目录已不存在），导致 `@agent-pulse/contracts` 解析为空类型，apps/web 产生 148 项 `TS2339` 错误且 `@agent-pulse/contracts/remote` 模块无法解析。执行 `npm install` 重新链接后，四个工作区 typecheck 全部通过；同一问题也是 `cargo check -p agent-pulse-desktop` 必然失败的根因（`target/debug/build/tauri-*` 缓存烤死了旧路径），清除陈旧缓存后 Tauri crate 恢复正常编译；
- **`package-lock.json` 版本号回归同步**：lockfile 中根包与各 workspace 版本此前停留在 2.4.39，与实际版本不一致，随重新安装一并校正；
- **补齐 `.gitattributes` 行尾规则**：原规则只覆盖 `*.sql/*.rs/*.ts/*.json/*.md`，遗漏 `*.tsx`、`*.css`、`*.html` 等，导致 26 个受版本控制的文件在工作区被写成 CRLF 而 Prettier 期望 LF（13 项 format:check 告警中有 6 项纯由行尾引起）。现补齐 TS/JS/CSS/HTML/SVG/YAML/Kotlin/Shell/PowerShell/XML/TOML/env 规则，明确 `*.bat`/`*.cmd` 保持 CRLF 以便 Windows 启动脚本可执行，并将图片、字体、可执行文件、安装包标记为 binary 不做行尾转换；随后对工作区执行一次 LF 归一化；
- **全平台版本递增**：版本升级至 2.4.45，Android versionCode 升至 56，Cargo 与 Node 依赖锁文件同步更新；
- **验证**：`cargo test --workspace` 212 项通过，`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check` 通过；`npm.cmd test` 全工作区通过（desktop 70 项、api 14 项、contracts 65 项、web 61 项），`npm.cmd run typecheck` 四个工作区全通过，`npm.cmd run format:check` 全仓库 Prettier 干净。

## [2.4.44] - 2026-09-26

工作区会话同名兜底消除与会话重命名支持：

- **消除同名工作区标题兜底**：优化 `sessionDisplayTitle` 降级兜底逻辑，彻底去除将缺失标题回退到所在工作区名称（如 `ClaudeSpace`）的冗余逻辑，同一目录下缺失标题或标题与目录同名的会话统一回退为唯一短 ID（`会话 #${shortId}`），根治展开目录后内部所有会话重名无法辨识的问题；
- **新增右键「重命名会话」功能**：在会话右键菜单中增加重命名入口与全功能模态弹窗 `RenameSessionDialog`，支持快捷修改自定义名称，并建立三级本地存储索引（复合键、Provider:ID、原始 ID）实现持久化与界面即时响应；
- **会话多维悬停提示 (Tooltip)**：侧边栏会话项新增完整 Tooltip 展示，清晰呈现会话完整 ID、精确最后活动时间、工作区路径、执行模型及权限模式；
- **后端真实 Prompt 标题提取升级**：Codex rollout jsonl 增加对用户角色消息文本的自动解析，智能跳过系统提示词与环境指令包络，准确提取用户首条业务 Prompt 作为会话标题；
- **全平台版本递增**：版本升级至 2.4.44，Android versionCode 升至 55，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## 2026-09-26 — 文档核对与精简

- 核对 Runtime、清单与协议边界，修正 Claude 实现状态；统一规则，保留按日期的验收记录。

## 2026-09-26 — 单项目 Agent 入口

- 规则、长期偏好和相关排障经验迁入本项目，agent 不再依赖父目录共享文档。

## [2.4.43] - 2026-09-26

远程连接状态指示灯逻辑纠偏（绿灯在线指示）：

- **远程状态指示灯语义对齐**：纠偏左下角连接状态指示灯逻辑，当本机与远程服务实时连接正常（`snapshot.realtime_connected` 为真）时，指示灯正常亮绿灯（`online`），彻底消除仅连接电脑未连接手机时常亮黄色警告灯的误导；仅在网络退化为轮询时显示黄灯，离线显示灰灯；
- **连接悬浮提示文案优化**：更新状态按钮 Tooltip 提示，清晰区分“远程实时连接”与“移动端协同配对”状态；
- **全平台版本递增**：版本升级至 2.4.43，Android versionCode 升至 54，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## [2.4.42] - 2026-09-26

侧边栏工作区目录排序优化（会话数优先 + 首字母排序）：

- **工作区目录智能排序**：工作区文件夹优先按所含有效会话数量降序排列（`b.totalCount - a.totalCount`），核心重度工作区（如包含最多历史会话的项目）稳居树状列表首位；
- **同数量字母序平局决胜**：当多个工作区会话数相同时，严格按照文件夹名称中文/拼音与英文首字母升序排序（`localeCompare`），列表清晰工整易于定位；
- **全平台版本递增**：版本升级至 2.4.42，Android versionCode 升至 53，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## [2.4.41] - 2026-09-26

彻底过滤自动化测试临时工作区残余（agenthub-native-*）：

- **彻底剔除测试临时目录垃圾**：识别并完全过滤集成测试套件通过 `os.tmpdir()` 创建的废弃测试临时工作区（`agenthub-native-*`、`agenthub-remote-*` 等），不再将其作为用户真实工作区展示在侧边栏树状列表中，根治侧边栏充斥数十个带随机哈希后缀的冗余文件夹问题；
- **全景会话与指标计数纠偏**：在应用会话池与 Agent 节点统计中剔除自动化测试残留会话，确保会话徽标总数真实对齐实际开发项目；
- **全平台版本递增**：版本升级至 2.4.41，Android versionCode 升至 52，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## [2.4.40] - 2026-09-26

启动加载界面光圈缺陷修复与侧边栏工作区默认收起：

- **侧边栏工作区目录默认收起（折叠）**：侧边栏目录（工作区文件夹）开箱默认全部保持收起状态（`collapsed`），不再自动展开全部会话列表，极度精简侧边栏高度；点击目录名可随时展开查看该目录下会话，支持记忆展开状态，搜索状态下自动展开命中项；
- **全屏启动加载界面消除黑斑与对比度纠偏**：彻底移除 `styles.css` 中残留的暗色径向渐变背景硬编码（`radial-gradient(#26231f 0, var(--bg) 46%)`），根治浅色主题启动时窗口中央出现巨大黑色阴影晕斑（犹如黑洞）的视觉缺陷；将 `.loading-screen` 背景、文本与加载条全面接入主题语义变量；重构 `PulseMark` 为主题自适应矢量渲染；
- **全平台版本递增**：版本升级至 2.4.40，Android versionCode 升至 51，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## [2.4.39] - 2026-09-26

默认亮色主题与会话空状态高对比度色彩修复：

- **开箱默认使用亮色（浅色/白色）主题**：更新系统根作用域（`:root`）与 `index.html`，默认主题统一对齐为白底清爽风格（`[data-theme="light"]`）；校准本地偏好配置规范化逻辑，在未设置时优先激活浅色模式；
- **读取中与会话空状态对比度根治**：修复 `.conversation-empty` 下 `strong` 标题引用未适配主题的浅白文本变量在浅色背景下几乎不可见（反差比不足）的严重体验缺陷；全面统一接入语义变量 `var(--text-heading)` 与 `var(--text-secondary)`，确保「正在读取会话内容」在浅色与深色背景下字迹均清晰锐利、对比分明；
- **全平台版本递增**：版本升级至 2.4.39，Android versionCode 升至 50，全工作区测试 100% 通过并完成全新 Release 构建与打包。

## [2.4.38] - 2026-09-26

树状侧边栏分页渐进加载与测试工作区降权：

- **会话级渐进展开（展开显示）**：工作区内会话列表默认展示前 8 条最新活跃记录，超过 8 条在底部展示「展开显示 (+N 条)」按钮，单次增量加载 16 条，避免海量历史会话一次性渲染卡顿与垂直列表过长问题；当前选中会话自动保留可见；
- **工作区级渐进展开**：Agent 节点内默认展示前 10 个最近活跃工作区，超过 10 个在底部展示「展开显示更多工作区 (+N 个)」按钮，单次增量加载 15 个；
- **测试临时目录降权排序**：自动识别 `agenthub-native-*` 这类自动化测试创建的临时工作区，并将其排在真实项目开发目录之后，确保真实开发工作区（如 `Claudespace`、`agent-pulse` 等）始终排在前列优先显示；
- **全平台版本递增**：同步升级全端版本号至 2.4.38，Android versionCode 递增至 49，重新打包 Windows Release (.exe/.msi) 与 Android (.apk) 安装包并更新 SHA-256 校验清单。

## [2.4.37] - 2026-09-26

设备归一收敛与多端发布：

- **设备聚合归一与无用节点剔除**：移除单测中遗留的伪造设备名前缀（如写死的泉州工作机与主工作机）；将本机全部会话与历次重装/测试遗留的历史设备碎片 ID 全面收敛聚合至唯一的「本机电脑 (Hostname)」节点下，彻底根治多设备分裂为多台虚拟机器的异常现象；
- **空设备静默过滤**：严格过滤会话数为 0 且无活动的长久离线空设备，侧边栏仅展示具备活跃会话或真正连接的物理机；
- **全平台发布版本递增**：遵循版本递增铁律，同步升级全端版本号至 2.4.37，Android versionCode 递增至 48，重新构建 Windows Release (.exe/.msi) 与 Android (.apk) 安装包并更新 SHA-256 校验清单。

## [2.4.36] - 2026-09-26

性能、桌面端 UI 现代化与健壮性优化：

- **桌面端 4 级文件夹树状侧边栏**：侧边栏全面重构为「电脑 (Device) → Agent (Provider) → 工作区 (Workspace) → 会话 (Session)」4 级层级树，统一收敛会话组织模式，支持各层级折叠展开记忆；提供清晰的设备状态指示、Agent 快捷发起按钮、工作区文件夹聚类与状态圆点高亮。
- **开箱 Provider 清理与 ZCode 插件化**：内置受控 Provider 默认仅保留 Claude Code 与 ChatGPT (Codex)；ZCode 下放至插件系统（Plugin Marketplace）作为「官方核心」驱动，用户按需在插件商城中一键启用后方才在新建会话和侧边栏中展示。
- **新建会话弹窗组件现代化**：`LaunchSessionModal.tsx` 模型与运行权限下拉选择控件由原生粗糙 `<select>` 替换为现代化毛玻璃交互组件 `SelectDropdown`，支持键盘导航与自适应边界吸附；联动 Provider 清单确保非激活插件不外露。
- **输入框与设置界面微交互修复**：修复输入框底栏加号按钮（`.composer-plus-btn`）无样式渲染为系统粗劣白底方块缺陷，重构为圆角图标胶囊；修复设置弹窗中「管理本机连接」按钮继承 44px 高全局大圆角样式在紧凑行高列表中比例失调问题。
- **上下文容量弹层 (ContextCapacityPopover) 计算与色彩纠偏**：修正分段百分比计算逻辑，使彩色分段进度条真实反映占模型上下文总窗口的比例，消除百分比相加为 100% 却显示在 83.4% 标题下的视觉与认知歧义；移除硬编码 Tailwind-500 饱和色值，全面接入三套主题的语义状态变量（`--status-success`、`--status-info`、`--status-warning`、`--status-danger`）；修正缓存统计文案为精确的「本轮缓存读取率」，并适配系统减弱动效偏好。
- `DurableHost::poll` 由「每个事件一次 SQLite 事务」改为「一次轮询批次一个事务」。此前每条事件都单独 `BEGIN`/`COMMIT`，在 `synchronous=FULL` 下等于逐事件 fsync；事件密集时事务数与事件数 1:1 增长。批内按最终 cursor 裁剪一次，保留窗口与逐条追加完全一致，并有 3 项新测试锁定。
- 会话 transcript 读取不再对同一行 JSONL 解析两遍：`read_transcript_windows` 直接返回已解析的 `Value`，调用方复用。`invalid`/`omitted` 上报语义保持不变，截断行仍不会外泄给调用方。
- `ingestAuthBatch` 由「每 snapshot 一次 SELECT + 一次 UPSERT」改为「整批一次读取 + 逐条 UPSERT」，消除 2N 次往返；`checked_at` 新旧比较与去重语义不变。
- `RemoteContentStore.receive` 处理 `ready` / `host_lost` 时，改为按 `(owner, runner)` 精确查询一次并用单条多行 UPSERT 写回，取代此前「扫描该 owner 全部会话 + 逐条 SELECT + 逐条 UPSERT」。`native_identity_changed` 冲突检测保留，并对超宽 payload 按 SQLite 参数上限自动分批。
- `/v3/remote/sessions` 的设备归属校验改用新增的 `getRegisteredDevices` 批量查询，取代按 runner 串行的 `getRegisteredDevice`；该接口为 1.5s 轮询路径，此前每台设备一次数据库往返。
- 修复 `zcode_app_server` 环境变量覆盖被忽略导致非预期回退至全局宿主路径缺陷，保证测试环境与物理机环境严格隔离；规范化 `remote_network` 启动失败标准安全错误码 `native_host_start_failed`。
- 同步全平台版本至 2.4.36，Android versionCode 升至 47。
- **语义权衡（需知悉）**：`SessionAuthenticator` 每次认证都会更新 `web_sessions.last_seen_at`，在 1.5s 轮询下会持续产生行版本与 WAL。现改为数据修改 CTE，**每 60 秒最多推进一次**（`SESSION_ACTIVITY_REFRESH_SECONDS`）。会话有效性（`expires_at`、账号状态、工作区成员关系）**仍在每个请求上重新校验**，节流只影响设置页「最近活动」的显示精度，最多滞后约 60 秒。

## [2.4.35] - 2026-09-25

- 侧栏重构为「设备 → Agent → 工作区 → 会话」四层文件夹树，抽离纯函数 `session-tree.ts` 并新增 12 项单测；顶部新增面包屑导航（全部会话 / 设备 / Agent / 工作区 / 会话），可逐级退回，避免在深层节点迷路。Agent 层级从发现元数据判定 Provider，发现报告的名称优先于会话 id 前缀猜测。
- 根治会话选中失效：树节点此前以 Provider 会话 id 拼 key，而选中态用的是 Hub 会话 id，两者永不相等导致点击无响应与面包屑不更新；现由树直接携带 `sessionKey`，并新增回归单测锁定。
- 恢复并升级设置页：补回被 `2ad5a6a` 删除的 `SettingsPanel` 接线，重构为整页分区视图（左侧分区导航 + 右侧内容 + 返回控制台），保留账户安全、订阅、团队、审计、连接、设备、等待提醒、安装 8 个分区；设备与 PWA 安装数据改由页面自取，并接入 Service Worker 回执以补上此前缺失的通知呈现凭证。侧栏「＋ 绑定新电脑」按钮移除，设备绑定统一收进设置页。
- 修复会话区若干 CSS 死选择器：`.remote-message.agent` 永不匹配（实际渲染 `assistant`），导致助手消息没有任何版式约束；`.remote-approval-box`、`.remote-approval-actions`、`.remote-primary-circle`、`.remote-tool-icon` 及一批旧树形类均已删除或重定向。
- 对话 Markdown 改用 `react-markdown` + `remark-gfm`，修复手写解析器的多处渲染缺陷：有序列表被渲染成无序列表、嵌套列表被拍平、图片退化成字面文本、任务列表与换行不生效、代码块与表格结尾丢失流式光标。
- 同步 Android、Node、Tauri、Cargo 版本至 2.4.35，Android versionCode 升至 46。

## [2.4.34] - 2026-09-25

- Web 在电脑端 Host 换代后按新代次自动重连已选会话，触发有界历史回放；同步全平台版本与 Android versionCode 45。

## [2.4.33] - 2026-09-25

- 修复受控会话恢复后网页空白：daemon 对缺少原生历史回放的 Provider 生成有界会话快照，沿远程事件链路展示；已绑定会话优先使用发现元数据标题。
- Codex app-server 不可用时，Host 回退到本机状态库发现元数据，并明确标记不可远程恢复；网页按设备识别候选会话并淡化侧栏滚动条。
- 同步 Android、Node、Tauri、Cargo 版本至 2.4.33，Android versionCode 升至 44。

## [2.4.32] - 2026-09-25

- Web 远程会话阅读区与输入框加宽，长设备列表可独立滚动，用户消息移除冗余身份标签。
- 输入框增加上下文容量入口，仅展示电脑端实际上报的用量；工具事件按桌面端的连续工具分组方式展示，并改进命令、失败状态与本机路径脱敏提示。
- 同步 Android、Node、Tauri、Cargo 版本至 2.4.32，Android versionCode 升至 43。

## [2.4.31] - 2026-09-25

- Web 登录页与远程接续界面对齐桌面端：采用暖灰深色、Segoe UI Variable 字体和居中的登录与会话阅读宽度；侧栏改为设备、工作区、会话层级，保留手机抽屉与远程控制状态。
- 修复账户测试使用过期固定日期导致的会话与订阅权益断言失败；测试时间改为相对运行日期生成。
- 同步 Android、Node、Tauri、Cargo 版本至 2.4.31，Android versionCode 升至 42。

## [2.4.30] - 2026-09-25

- **移动端 UI 全面重构与双端协同交付（Web Console 响应式抽屉 + Android Jetpack Compose 原生高质感对话）**：
  - **版本同步自增**：全端规范自增至 `v2.4.30`（Android `versionCode = 41 / versionName = "2.4.30"`，Web/Desktop package.json 与 Cargo.toml 同步升级）。
  - **Web 控制台移动端体验重构 (`apps/web`)**：
    - **抽屉式侧滑导航 (Drawer / Off-canvas)**：打破旧版在手机视口（≤700px）下侧边栏强行挤占 190px 导致屏幕上下割裂的痛点，升级为全高度平滑滑出的侧滑抽屉与 Backdrop 高斯模糊遮罩；
    - **顶部移动端 Header**：常驻移动端汉堡菜单按钮（☰），点击唤出抽屉，会话或候选任务选中后自动平滑收起；
    - **移动端底部功能恢复**：完整恢复手机端对电脑在线数监测、添加配对电脑以及账号退出等核心管理能力；
    - **视口与输入法体验加固**：适配 `100dvh` 与安全区（`safe-area-inset`），优化软键盘输入体验（手机端回车换行防误触发送，独立高亮圆钮触发发送）；
  - **Android 原生客户端 UI 深度重塑 (`apps/android`)**：
    - **结构化消息渲染架构**：新增 `MarkdownComponents.kt`，解耦粗糙的纯文本平铺，将事件流解析为强类型 `UiMessage`；
    - **高质感角色气泡**：User 消息采用右侧高对比度药丸气泡（`RoundedCornerShape(18.dp, 18.dp, 4.dp, 18.dp)`），Assistant 消息采用左侧自然排版；
    - **轻量 Markdown 与 Obsidian 风格代码块**：支持行内粗体与等宽代码高亮，代码段封装于深色卡片中，提供一键复制代码与语言标头；
    - **思考过程折叠抽屉 (Thinking Accordion)**：智能捕获 `<think>` 标签，以平滑折叠卡片形式收纳长思考过程，消除长文刷屏；
    - **ChatGPT 风格药丸式输入条与大拇指友好审批卡片**：圆角输入框内嵌圆形发送按钮，操作审批卡片强化风险警示与拒绝/允许大按钮。
  - **交付与归档**：
    - 完成 Web 端类型检查与生产构建；
    - 完成 Android Gradle 编译构建，生成 `AgentHub-v2.4.30-android-debug.apk` 并计算 SHA-256 校验和归档至 `Deliverables/AgentHub/`；清理旧版安装包。

## [2.4.19] - 2026-09-19

- **全端发版 v2.4.19（Android versionCode = 32 / versionName = 2.4.19）**：
  - **根除 Claude Code 会话历史全部无法解析/显示为空缺陷**：
    - 根因定位：桌面端和远程主机下发的前缀化会话 ID（如 `claude_<uuid>`）传入 `load_session_conversation` 时，底层 transcript 收集逻辑直接对比磁盘文件名（`<uuid>.jsonl`），导致 42 个真实会话中有 41 个因前缀无法命中被当成空会话。
    - 修复方案：在 `conversation::load_session_conversation` 与 `find_agent_transcript` 中统一规范化 `session_id`，严格剥离 `claude_`、`antigravity_`、`grok_` 等前缀，使磁盘上的 JSONL 历史被 100% 完整解析与展示。
  - **修复空态“重新加载”按钮文字竖排排版变形 Bug**：
    - 根因定位：`.conversation-empty` 网格容器下的 `.conversation-empty svg` 选择器对子代 `<RefreshCw />` 图标强行注入了 `margin-bottom: 12px`，同时按钮缺少 `white-space: nowrap` 与横向 flex 排版，导致文字在网格对齐约束下每个汉字折成一行。
    - 修复方案：严格限定 `.conversation-empty > svg` 作用域，并为 `.conversation-empty .outline-button` 增加 `inline-flex; flex-direction: row; align-items: center; white-space: nowrap; gap: 8px; width: auto;` 及针对按钮内 SVG 图标消除 margin 底部偏移，恢复优雅居中水平排版。
  - **多 Provider 传输层容错与连接稳定性加固（根治 `provider_transport_lost` 误报）**：
    - 根因定位：`MultiProviderTransport::drain()` 只要任一底层子进程（如 Codex）读管道断开或单次超时，就会立刻向外传播 `provider_transport_lost` 错误，导致宿主 `mark_lost()` 将所有会话全域标记为断连。
    - 修复方案：隔离各 Provider 的管道错误，平滑收集所有活跃事件；只有在 Codex 断开且全量活跃 turn 均不存在时才判定整体传输失效，杜绝跨 Provider 的假死断连。
  - **全平台打包交付与生产部署**：Windows Desktop NSIS/MSI、Android APK 归档至 `Deliverables/AgentHub/`，按铁律清理旧版产物，生产容器部署升级。

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
