# Changelog

## [Unreleased]

### 2026-10-19 — Web 控制台 UI/UX 全面重构（遵循 `/ui-design` 设计指南）

- **全面去 Emoji 与纯矢量图标系统**：全站移除原生 Emoji 与伪字符符号，统一使用 2.2px 圆角几何线宽的内联 SVG 图标（`PaletteIcon`、`ShieldIcon`、`AuditIcon`、`ConnectionIcon`、`DevicesIcon`、`NotificationIcon`、`InstallIcon`、`ArrowLeftIcon`、`SunIcon`、`MoonIcon`、`MonitorIcon`、`ChevronDownIcon`、`CheckIcon`、`CopyIcon` 等）。
- **根治图一字体模糊发虚与主题污染**：拔除 `.remote-session-tree` 顶部渐变遮罩，使设备列表与顶部节点清晰可辨；消除 `.settings-page` 强行覆盖暗色变量的硬编码缺陷，完整支持 Warm Paper 浅色与 Obsidian Charcoal 暗色主题自然继承；修复浅色模式下问题卡片、引用块与启动项的深底黑块问题。
- **排版尺寸与组件呼吸感升级**：表单标签提升至 13.5px 加粗，导航与元信息统一至 12~13px，输入框高度扩充至 42~46px（圆角 10px），操作按钮与提交纽扩大至 38~48px；拉开卡片、侧边栏检索框与弹窗的留白间距。
- **60 FPS 流畅微动效与视觉对齐**：为核心按钮、导航项、气泡弹窗与折叠手风琴注入平滑 GPU 硬件加速过渡（悬停微浮 `translateY(-1.5px)`、弹性按压 `scale(0.98)`、平滑入场微动效与旋转）；强化文字前景色阶与 WCAG AA 对比度，统一 Claude Terracotta 陶土品牌色。

### 2026-10-19 — 项目文档精简与单一事实源对齐

- 清理 `docs/` 下重复的 8 份单版本 `RELEASE_NOTES_*.md` 及 4 份宣发模板文件（`COMMUNITY_SUBMISSIONS.md`、`DISCUSSIONS_QA_SEEDS.md`、`LAUNCH_COPYWRITING.md`、`OPEN_SOURCE_GUIDE.md`）；
- 将 `M4_VALIDATION.md` 与 `ANDROID_VALIDATION.md` 的按日期验收证据与边界合并至 `docs/README.md`，消除跨文件重复；
- 精简 `AGENTS.md`、`README.md`、`README_CN.md`、`llms.txt`、`llms-full.txt`、`docs/{PRD,ARCHITECTURE,SYNC_PROTOCOL,PITFALLS,DEPLOYMENT,DESIGN_SYSTEM,CHANGELOG}.md` 及各子包 `README.md`，移除已废弃 `apps/desktop` 的过时描述，修正 `@agent-pulse/api` 启动命令，统一 `2.5.1` 版本与 `magpie`/`orca` 开源溯源声明。

## [2.5.1] - 2026-10-19

- **内嵌控制台导航收敛 (Four Pages, One Settings)**：
  - 主界面收敛为 **会话 / 智能体 / 用量 / 工作区** 四页；移除"模型"与"网关"独立视图（路由与拓扑归上游 CPA 管理）及页脚常驻端口显示；
  - "能力"并入设置页 **能力生态** 分区（全部/技能/MCP/钩子筛选、跨分区搜索、注册 MCP 与重探测）；"设备与配对"并入设置页 **设备与网关** 分区；设置页固定为 **通用 / 外观 / 设备与网关 / 能力生态 / 关于** 五个标签。
- **逐请求用量账本 (`crates/runner/src/usage_ledger.rs`)**：
  - 解析本地会话转录，按 `message.id + requestId` 去重并以末行写入为准，严格区分 `claude_code`（Coding Assistant）与 `claude_desktop`；
  - 识别合成 API 错误行（`isApiErrorMessage` / `<synthetic>`），将同会话 10 分钟窗口内的失败后成功配对为 `retried/recovered`，按消息首尾时间戳测算输出速度；
  - 新增 `GET /api/usage/ledger?period=today|7d|30d|all`（5 秒缓存），驱动用量页四段标签（概览 / 请求 / 会话 / 上下文）、五格 KPI、堆叠柱状趋势图、三套排行、53 周（周一起）热力图与 UTF-8 BOM CSV 导出。
- **原版厂商图标与智能体收敛 (`crates/runner/src/console_icons.rs`)**：
  - `include_bytes!` 内嵌 `crates/runner/resources/icons/` 下 20 个逐字节取自上游 `magpie` 的 SVG/PNG 图标，经 `/icons/<name>` 提供并按模型前缀匹配；
  - 智能体视图收敛为 Coding Assistant、Claude Desktop、Codex、Grok、Gemini CLI 五款产品（其余仅计入会话来源）。
- **构建与发版**：全工作区版本递增至 `2.5.1`（258 项 Rust 测试通过），归档 `AgentHub-Runner-v2.5.1-windows-x64.zip` 与 `AgentHub-v2.5.1-SHA256.txt` 并清理旧包。

## [2.5.0] - 2026-10-19

- **设置页独立视图化 (`#view-settings`)**：移除设置与配对弹窗，改为五分区独立页面，支持 `#setQ` 跨分区实时检索、`Ctrl+,` / `Esc` 切换及 `switchView()` 序号防竞态保护。
- **开发构建热重载 (`web_server.rs`)**：debug 构建直读 `resources/` 磁盘文件并提供 `GET /api/dev/wait` 长轮询自动刷新，release 构建保持 `include_str!` 单文件内嵌。
- **构建与发版**：全工作区版本升至 `2.5.0`（Android `versionCode = 100`），归档 Windows Runner 与 Android Debug APK 基线。

## [2.4.96 – 2.4.99] - 2026-10-19

- **并发与协议加固 (v2.4.99)**：修复 `remote_host.rs` 中 `events_queue.lock()` 重入死锁；规范化 ACP `usage_update`（仅 `used > 0` 时派发）；修复 Android `MarkdownSegmentCache` 闭合代码块尾部反引号判定与 `Locale.US` 数字格式化。
- **联邦能力生态与上下文容量管线 (v2.4.99)**：
  - 新增 `crates/runner/src/collectors/capabilities.rs`：跨 Claude Code、Codex、工作区与 `~/.agenthub/mcp.json` 集中扫描 Skill、MCP Server 与 Hook，支持多源去重与 `POST /api/capabilities/sync`；
  - 打通 Claude `result`、Codex `thread/tokenUsage/updated` 与 ACP `usage_update` 的 `{used, window}` 上下文容量远端白名单投影，并在客户端渲染会话容量环与多选问答卡片（`multi_select`）。
- **流式转录、工具卡片与高 DPI 托盘图标 (v2.4.98)**：
  - 统一 Web 与 Android 会话转录解析（`item/agentMessage/delta` 增量聚合与 `item/completed` 原位替换、`history/snapshot` 最新快照回放、`item/started` 白名单窄投影、`<think>` 折叠手风琴）；
  - 重绘 16×16 至 256×256 多帧 `icon.ico`，升级 Win32 `LoadImageW` 高分屏托盘渲染与现代右键菜单。
- **移动端 Orca 令牌对齐与全机 Token 聚合修正 (v2.4.97)**：
  - `apps/android` 统一 `OrcaSpace`/`OrcaRadius`/`OrcaType` 深色令牌与 `OrcaRow`/`OrcaListGroup` 组件，轮询绑定 `repeatOnLifecycle(RESUMED)`；
  - 修复 ZCode 历史消息 `LIMIT 1` 截断与子代理折叠漏计，联动 Codex 本地缓存读取，全机吞吐统计与 `magpie` 基准对齐。
- **审批 Diff 预览与通用 ACP 适配器 (v2.4.96)**：
  - 对文件修改审批提取结构化 Diff（≤64 KiB，本地路径脱敏为 `[local_path_redacted]`），Web 与移动端支持 `+add -del` 折叠预览；
  - 新增 `crates/runner/src/collectors/acp_app_server.rs` 标准 JSON-RPC 2.0 stdio ACP 适配器，支持 `acp_` 会话路由与 `ChildJob` 进程树隔离。

## [2.4.77 – 2.4.95] - 2026-10-15 ~ 2026-10-19

- **移除 Tauri Desktop，落地 Runner 内嵌 Web 控制台 (v2.4.77 – v2.4.95)**：
  - 删除 `apps/desktop`（Tauri），Rust 工作区收敛为单一 `crates/runner`（`agenthub-runner`）；
  - 新增 `crates/runner/src/web_server.rs` 与内嵌静态资源 `dashboard.{html,css,js}`（默认 `127.0.0.1:4200`，支持 Win32 系统托盘与 Edge `--app` 独立窗口染色）；
  - 完整移植上游 `yetone/magpie` 设计系统与布局原语；脚本拆分为独立 `/dashboard.js?v=<version>`，Token 计价唯一权威收口至守护进程后端，未定价模型显示 `—`；
  - 引入各收集器内存 TTL / SQLite mtime 指纹缓存、工作区深度聚合（`collectors/workspace.rs`）、上游模型目录探测（`model_catalog.rs`）与 Claude 会话亲和标头注入（保护 Prompt Cache）。
- **开源合规、CI 与 AI 检索规范 (v2.4.93 – v2.4.95)**：
  - 补齐 `LICENSE`（MIT）、英文 `README.md`、中文 `README_CN.md`、`CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、`SECURITY.md`、`.github/workflows/ci.yml` 及 `llms.txt` / `llms-full.txt`；
  - `apps/android` 按 `stablyai/orca` 移动端控制舱重构 `Home -> Host -> Session` 三级栈式导航与终端虚拟按键行。

## [2.4.67 – 2.4.76] - 2026-10-06 ~ 2026-10-14

- **自部署单管理员化与 SQLite 持久化 (v2.4.76)**：
  - Gateway 新增基于 `node:sqlite` 的 `SqliteAccountStore` 与 `SqliteGatewayStore`（`gateway.db` 与 `remote.db` 分离），支持首次启动 `/v1/auth/setup` 单管理员初始化锁定，移除 Telegram OAuth、多租户团队与商业化计费页面遺留；
  - `crates/runner` 优化 Claude 转录解析：过滤 `tool_result` 假用户 Prompt、支持超大单行首尾流式切片（防 Base64 撑爆内存）、识别 `stop_reason` 生命周期并按 `uuid` 去重 resume 重放。
- **AOCI 本地认知脱敏治理 (v2.4.73)**：`aoci.*`、`.aoci/`、`.mcp.json`、`AGENTS.local.md` 全部纳入 `.gitignore` 并解除 Git 追踪，测试代码清除真实 Tailnet IP。
- **远端失败可见性、60 FPS 流式渲染与双向审批闭环 (v2.4.68 – v2.4.72)**：
  - `remote_journal.rs` 白名单放行脱敏后的 `error` 事件与 `codexErrorInfo`，Web 端渲染会话隔离的失败原因横幅；
  - `apps/web` 重构为 Claude Design System 暖纸浅色 / 暖炭深色双主题，工具 Shimmer 改用 GPU `translate3d` 合成，新增时间轴滚底跟随状态机、安全 UUID 降级与同源容器代理放行；
  - 新增 `claude_app_server.rs`（`ClaudeProcessSession` 长驻双向 stdio 进程），打通 Claude Code `can_use_tool` / `AskUserQuestion` 实时审批、多选问答与每轮 `effort` 推理强度控制；
  - 清理 Web 端 17 个已废弃的旧监控工作台组件及无用 `gsap` 依赖。

## [2.4.1 – 2.4.66] - 2026-09-15 ~ 2026-10-05

- **会话流与工具体验演进 (v2.4.44 – v2.4.66)**：
  - 过滤 Claude 合成 429 占位消息（`isApiErrorMessage`）与自动压缩摘要（`isCompactSummary`），将其转为独立系统状态条；过滤无有效 assistant 输出的空会话；
  - 新增 `agent_binary.rs` 三段式 CLI 发现（候选枚举 → 带超时 `--version` 探测 → 版本择优 + `session_unreadable` 窄匹配回退与亲和记录），根治旧版 CLI 无法 resume 新版 transcript 的问题；
  - 新增 `claude_gateway.rs` 自动探测本地网关配置与模型映射；
  - `NativeSessionHost` 命令回执账本与已应答请求集合改为有界 FIFO 淘汰（上限 10,000），`remote-store.ts` 新增 `(runner, hub, seq)` 索引加速命令历史查询。
- **多 Provider 路由与早期迭代 (v2.4.1 – v2.4.43)**：
  - `MultiProviderTransport` 支持按会话前缀与 `agentType` 分流 Codex、Claude Code 与其他发现源；过滤 `agenthub-native-*` 测试临时工作区残留。

## [2.2.0 – 2.3.6] - 2026-09-02 ~ 2026-09-14

- **Remote-only 定位与进程外 `agenthubd` 收口 (v2.3.4 – v2.3.6)**：
  - 确立"官方 Agent 会话的统一远程继续入口（Remote-only）"定位，transcript 解析单点收敛至 `crates/runner/src/conversation.rs`（`GetSessionConversation` IPC 有界预算）；
  - 将 `NativeSessionHost` 提升为进程外常驻 daemon，落地本地 IPC v2（1 MiB 长度前缀 JSON frame、Windows Named Pipe 当前用户 ACL / Unix `0600` Socket、`snapshot + cursor` 重放）；
  - 落地 Remote v3：Ed25519 设备签名 WSS、Generation Fencing、`ws-{display}-{hash16}` 不可逆工作区标识、opaque `approval_id`、按事件种类严格白名单 DTO 投影及单实例 SQLite 内容库；
  - 清理早期自建任务代执行系统（`tasks.rs`、`/v1/tasks`）与废弃 v2 内存原型路由。
- **基础采集、加密同步与 M1–M4 基线 (v2.2.0 及更早)**：
  - 建立 Rust Runner、SQLite Outbox、Ed25519 设备身份与签名批次协议（`@agent-pulse/contracts`）、Fastify Gateway（PostgreSQL 迁移与行锁事务、SSE `/v1/stream`、RFC 8291/8292 Web Push）及 Next.js 响应式 Web 端。
