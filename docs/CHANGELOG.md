# Changelog

## [2.8.1] - 2026-11-22

- **插件生态安全加固与多端协同修复（代码审查 #2～#9）**：
  - **ACP 网关凭据隔离**：`AcpAppServer::start_with_options` 新增 `inject_gateway_env: bool`，第三方插件进程强制传 `false`，阻断因路径/参数含 `claude`/`gemini`/`codex` 意外注入宿主机网关 Token。
  - **本地目录安装先验校验与防 symlink**：`install_from_local_directory` 在覆盖安装目录前先行校验 `expected_id` 与暂存清单，跳过符号链接并预热启用态缓存。
  - **大小写不敏感 Scheme 与加强反 SSRF**：Runner 与 Gateway 对 `HTTPS://` / `HTTP://` / `FILE://` 做大小写不敏感校验与 URL 规范化；Gateway `isForbiddenHost` 拦截 IPv4-mapped / IPv4-compatible / 完整展开回环 / 链路本地 / ULA / 组播 IPv6。
  - **多前缀会话路由一致性**：Runner `PluginView`、Contracts `AgentPlugin`、Gateway 序列化、Web `session-tree.ts` 与 Android `Models.kt`/`ModelCatalog.kt` 完整透传并匹配 `sessionPrefixes`（含 `zcode` 对存量 `sess_*` 会话识别）。
  - **多端状态同步与设备吊销清理**：Runner WebSocket 建连后主动发送首帧 `plugin_state`；Gateway 吊销设备同步删除 `remote_runner_plugins` 并于 `GET /v3/remote/plugins` 过滤已吊销 Runner，`ready` 携带插件帧时触发 SSE `event: plugins` 推送；Web 插件设置页订阅 `plugins` 事件静默刷新；Android 修复多 Runner 切换误全量覆盖状态与直接走 `performPluginAction(runnerId, "toggle")`。
  - **清理死代码**：移除 Web SSE 解码器未使用的 `framesByEvent` 临时映射。
  - **版本发布与公开脱敏**：全仓版本号自增至 `2.8.1`（Android `versionCode = 138`），同步已裁切脱敏的远程对话截图并清理文档/示例注释中的私域及本机工作目录引用。

## [2.8.0] - 2026-11-20

- **构建以本机 Runner 为权威源的 ACP 智能体插件生态（`crates/runner/`, `packages/contracts/`, `services/api/`, `apps/web/`, `apps/android/`）**：
  - **5 大原生核心与外部插件剥离**：核心智能体收敛为 `Antigravity`、`Codex`、`Claude Desktop`、`Claude Code` 与 `DeepSeek Harness`（预留原生槽位/图标/契约，未实现运行时显式返回 `deepseek_harness_runtime_not_implemented`）；将原内置的 `ZCode`、`Grok Build`、`Gemini CLI` 适配器完整剥离至独立公开插件仓库 `agenthub-plugins`。
  - **Runner 权威插件运行时 (`crates/runner/src/plugins/`)**：由本机 Runner 直接拉取远程或本地 `registry.json`，执行相对路径安全校验与文件大小/SHA-256 严格验证，原子安装至 `~/.agenthub/plugins/installed/<id>/` 并持久化 `state.json`；基于标准 ACP JSON-RPC 2.0 及 `agenthub/*` 扩展 RPC（`probe`、`discover`、`models`、`history`）统一驱动发现、模型列表、新建/恢复会话、流式输出与历史读取；新增本地 `:4200` `/api/plugins/*` 路由与控制台 "设置 -> 插件" 管理页。
  - **契约与网关动态中继 (`packages/contracts/`, `services/api/`)**：支持合法插件 slug（`^[a-z][a-z0-9_-]{1,63}$`）与 `"plugin_acp"` 观测来源；新增 PostgreSQL 迁移 `0018_dynamic_plugin_agent_types.sql` 放开 `agent_type` CHECK 约束，并在 SQLite `RemoteContentStore`（`services/api/src/remote-store-validation.ts`）中建立 `remote_runner_plugins` 表，移除服务端硬编码 `DEFAULT_PLUGINS`；通过 WebSocket `plugin_command` / `plugin_state` 控制帧及 `POST /v3/remote/runners/:runner/plugins/action` 实时中继安装/启停/卸载/源切换，支持 `downloading` 中间态上报、SSE `event: plugins` 推送、`503 runner_offline` 与 `504 plugin_action_timeout` 处理。
  - **安全加固与生命周期治理**：`/api/plugins/icon/:id` 强制输出 `Content-Security-Policy: default-src 'none'; sandbox` 沙箱头并拦截含 `<script>`/`<foreignObject>`/内联事件的 SVG；远端插件仓库强制校验 HTTPS 与反 SSRF 私网地址检查（禁用远端 `source_url`），运行时命令收敛至 `node` 相对路径入口与安全环境变量白名单；非阻塞缓存插件元信息防止主事件循环卡顿；修复带前缀 ACP `session/update` 的 `turnId` 查找、`zcode` 对存量 `sess_*` 会话前缀兼容、Web 按 Runner 严格隔离与 Android `SettingsScreen`/`RunnerPluginSnapshot` 修复；避免 Windows 下 Node 插件占用安装目录 CWD 句柄并扩展卸载目录共享冲突重试。
  - **Web 与 Android 多端适配 (`apps/web/`, `apps/android/`)**：按 5 个原生核心 + 已启用且就绪的扩展插件动态渲染智能体列表与新建会话引擎选项；Web 新增 `PluginSettingsSection` 支持按在线 Runner 分组管理插件；Android 升级 `EngineKind`（新增 `DEEPSEEK_HARNESS` 与 `PLUGIN`）并支持远端插件目录刷新与安装卸载切换。
  - **版本发布与端到端验证**：全仓版本号统一升至 `2.8.0`（Android `versionCode = 137`）；新增 `crates/runner/tests/plugin_ecosystem.rs` 覆盖模拟 HTTP 注册表拉取、SHA-256 篡改拦截、ACP 完整会话生命周期及停用/卸载回收。

## [2.7.17] - 2026-11-20

- 修复 Gateway 运行时依赖 Fastify 和 fast-uri，以及 Web 构建依赖 source-map-js 的高危漏洞。
- Gateway 按代理地址验证转发头。Docker 自托管的 Web 通过回环地址访问 Gateway，只对宿主机发布 Web 端口。

## [2.7.16] - 2026-11-20

- Gateway 核验 Runner 声明的 Codex 会话控制能力，再派发只读预览、接管或归还指令。未升级的 Runner 可继续通过原有的显式恢复入口工作。

## [2.7.15] - 2026-11-20

- 固定 Web 容器中静态资源的读取权限，避免构建检出使用严格权限时导致 Nginx 健康探针返回 403。

## [2.7.14] - 2026-11-20

- 修正 Codex 历史会话提前占用桌面端写入锁的问题。浏览会话和 Runner 重启仅恢复历史或绑定记录。主动接管或首次发送才申请原生控制权，归还时等待 Codex 确认卸载。接管的会话空闲 45 秒后自动归还，执行或等待审批时保留控制。Web 和 Android 遇到原生写锁冲突时保留草稿。

## [2.7.13] - 2026-11-19

- 将运行期间的模型和会话目录查询移到独立的只读 App Server 工作线程，避免目录刷新阻塞回复片段、轮次完成事件和新命令。目录结果继续更新工作区映射和会话类型缓存，刷新失败时保留上次成功的目录。

## [2.7.12] - 2026-11-19

- 修复 Claude Desktop 和 Claude Code 连续发送消息时，新回复和思考片段追加到前一轮的问题。Runner 按轮次生成流式消息 ID，Web 按轮次还原旧 Runner 和历史记录中的固定 ID，使后续片段继续更新当前回复。

## [2.7.11] - 2026-11-19

- **修复远程控制台首次拉取与二次同步 Agent 数量及分类不一致的竞态问题（`services/api/src/`, `apps/web/src/`）**：
  - **根因定位**：`GET /v3/remote/sessions` 返回的瘦身响应不含 `discovery.candidates` 且活跃绑定未携带 `provider`，导致首帧将同为 `claude_` 前缀的 `claude_desktop` 历史绑定按前缀盲猜并入 `Claude Code`；同时 `RemoteConsole` 拉取 `/runners/:id/hello` 的 `useEffect` 依赖随每次 `/sessions` 刷新而重建的 `runners` 引用，SSE `ready` 推送触发重拉时会提前 `stopped = true` 丢弃在途 `/hello` 并误写 30 秒缓存时间戳；
  - **后端精确归类 (`services/api/src/remote-store-validation.ts`, `services/api/src/remote-routes.ts`)**：新增 `extractDiscoveryProviders()` 从服务端内存缓存 `runnerHello` 的 `discovery.candidates` 提取权威 `provider`，在 `GET /v3/remote/sessions` 的每个会话行上附加 `provider` 字段，保持 `runner.hello` 瘦身结构不变；
  - **前端无竞态同步与首屏门禁 (`apps/web/src/lib/runner-hello-sync.ts`, `apps/web/src/components/RemoteConsole.tsx`, `apps/web/src/lib/remote.ts`, `apps/web/src/lib/remote-client.ts`, `apps/web/src/lib/session-tree.ts`)**：抽出 `RunnerHelloSyncController` 对在途 `/hello` 请求按 `runner_id` 去重、取消不污染 `fetchedAt`、瞬时失败保留已有 `hello` 并按 3 秒退避重试、`host_generation` 变更立即刷新；`remoteRequest` 将 `response.text()` 纳入超时/中止守卫；当前设备首次 `/hello` 落地前锁定 Agent 选择器并显示 `"正在同步 Agent…"` / `"正在同步会话列表…"`，落地后未手动选过 Agent 时自动对齐实际会话最多的引擎；
  - **单测验证**：新增 `services/api/test/runner-hello.test.ts`（1 项）、`apps/web/test/runner-hello-sync.test.ts`（7 项）、`apps/web/test/session-tree.test.ts`（1 项）与 `apps/web/test/remote.test.ts`（1 项），通过 `npm run format:check`、`npm run typecheck`、`npm test`、Vite 生产构建及 `scripts/check-line-limits.py`。

## [2.7.10] - 2026-11-19

- **托盘「配置」页去硬编码并改为客户端真实配置优先发现（`crates/runner/src/claude_gateway/`, `crates/runner/src/tray_menu/`, `crates/runner/src/usage_ledger/`）**：
  - **去掉固定 6 项硬编码与静态兜底**：将 `ClientRouteSummary.agent_id`/`title` 改为 `String`；移除 `route_summary.rs` 无条件塞入 6 个客户端 `"Official · 直连"` 的逻辑、移除 `tray_menu/builder.rs` 的 6 项 `is_core` 白名单与 3 行写死加载占位行，并将 `is_loading` 收敛为 `!is_warm`（避免空列表长期停在"加载中"）；移除 `detail_inspect.rs` 中 Antigravity 的静态默认模型列表；
  - **客户端实时生效配置优先，拒绝过期 CC Switch 劫持**：
    - `cc_switch.rs` 新增 `proxy_takeover` 真实接管判定（联合检查 `proxy_config.enabled`/`proxy_enabled`/`live_takeover_active`、`proxy_live_backup` 与 `settings.proxy_takeover`）、`discover_all_cc_switch_active_providers()` 及 `cc_switch_manages_live_url()`，仅在 CC Switch 实际处于代理接管或与客户端实时端点同源时采信其记录；
    - `config_library.rs` 抽出 `inspect_applied_config_library()` 供摘要与详情共用，Claude Desktop 优先读取真实 `configLibrary` / `host-creds`，Claude Code 优先读取 `ANTHROPIC_BASE_URL` / `~/.claude/settings.json`，Gemini CLI 优先读取环境变量 / `~/.gemini/.env` / `~/.gemini/settings.json`，并自动比对本地 `magpie/gateway.json` 解析真实网关名称；
    - 新增 `route_parsers.rs` 实现分节感知的 `parse_codex_toml_route()`，只解析顶层 `model_provider` 与对应的 `[model_providers.<key>]`，修复顶层未设路由时误抓未启用 `[model_providers.*]` 段地址的问题；
    - `antigravity` 不再回退继承 `cc-switch.db` 的 `gemini` 行，且补充 `is_official_provider_url` 识别；`discovery.rs` 仅在 `cc_switch_manages_live_url` 成立时复用 CC Switch 模型目录；
  - **动态接入新客户端与空态提示**：支持按真实本机安装/配置/会话证据动态探测本地 Agent（含 `grok_build`），并动态纳入 `cc-switch.db` 中处于活跃接管的任意 `app_type`；`tray_menu/win32/` 新增空态引导绘制与按中英文字符选择字体及省略号裁剪；
  - **单测与门禁验证**：新增 `claude_gateway/route_summary_tests.rs`（8 项）与 `tray_menu/tests.rs`（4 项）覆盖空白环境、Claude Desktop / Claude Code / Codex / Antigravity / 动态 `app_type` 与临时 SQLite 接管校验，并通过 `check-line-limits.py`、`cargo fmt`、`cargo clippy` 与 `cargo test --workspace`。

- **Android 启动器图标切换为透明背景传统 PNG 并发布全仓 `v2.7.10`（`apps/android` `v2.7.10` / `versionCode = 129`）**：
  - 根因与方案：Android `<adaptive-icon>` 规范强制要求不透明背景层（`#00000000` 在桌面离屏缓冲中被合成为纯黑方块，`#FFFFFF` 渲染为白底圆形），故移除 `mipmap-anydpi/`（`ic_launcher.xml`、`ic_launcher_round.xml`）及 `AndroidManifest.xml` 的 `android:roundIcon` 声明；
  - 资产更新：基于 `apps/web/public/icon-512.png` 清理零 Alpha 通道边缘残留后，在 `mipmap-mdpi` 至 `mipmap-xxxhdpi` 生成 48/72/96/144/192px 透明 RGBA `ic_launcher.png`；清理仅被自适应图标引用的 `ic_launcher_brand_monochrome.png` 与 `values/colors.xml`，保留 Android 12+ 启动图引用的 `ic_launcher_brand_foreground.png`。

- **CI 排障：Android 单测门禁在自建 Runner 上从未跑通（`.github/workflows/ci.yml`, `docs`）**：
  - 根因定位：`android-checks` 自 v2.7.9 上线起唯一一次运行（Gitea run #140）即失败，日志停在 `gradle/actions/setup-gradle@v4` 的 clone 阶段并报 `Unable to resolve v4: unsupported object type`。`gradle/actions` 仓库体积约 350MB，而 `act_runner` 对每个 `uses` 引用的外部 action 执行**全量 clone**；在自建 CI Runner 容器内手动全量 clone 同一仓库，6~8 分钟后被对端 `fatal: early EOF` 掐断，缓存目录只剩未完成的 `tmp_pack_*`，故非偶发网络抖动而是结构性不可用；
  - 修复：移除 `gradle/actions/setup-gradle@v4`，改用体积仅 2.4MB 的 `android-actions/setup-android@v3` 安装 `platforms;android-36` 与 `build-tools;35.0.0` / `36.0.0`（对齐 `compileSdk = 36` 与 AGP 8.13.0 默认 build-tools）；Gradle 发行版继续由 `gradle-wrapper.properties`（8.14.3）自行下载，JDK 仍由 `actions/setup-java@v4` 提供 temurin 17；
  - 权衡：暂不接入 `actions/cache` / `setup-java` 的 `cache: gradle`，避免在自建 Runner 上为一条本就偶发的链路再叠加缓存上传/磁盘增长这类第二故障源；如需提速可后续单独验证后再加；
  - 验证：宿主侧实跑 `./gradlew testDebugUnitTest --no-daemon`，`BUILD SUCCESSFUL`（2m46s，208 项单测 0 failure 0 error）；CI 侧从 `catthehacker/ubuntu:act-latest` 探活 `services.gradle.org` / `dl.google.com` / `repo.maven.apache.org` / `plugins.gradle.org` 均 200。
  - 结果：Gitea run #757 三个 job 中 `Node.js & Web Checks` 与 `Android Unit Tests` 均转 `success`（`Android Unit Tests` 自 v2.7.9 上线以来首次跑通）。

- **CI 排障：修复 `daemon_ipc` 超时单测在自建 Runner 上的偶发失败（`crates/runner/src/daemon_ipc/tests.rs`）**：
  - 现象：Gitea run #757 的 `Rust Workspace Checks` 在 `Run Workspace Tests` 步失败，`daemon_ipc::tests::request_timeout_returns_without_waiting_for_a_slow_handler` 断言 `error.to_string().contains("ipc_response_timeout")` 不成立，同树 308 passed / 1 failed；
  - 根因：该断言本身是竞态。`request_with_timeout`（`daemon_ipc/client.rs`）起一个 worker 线程执行 `platform::request(..., timeout)`，主线程用同一个 `timeout` 做 `recv_timeout`；Unix 侧 `cancel_request` 是空实现，双方都无法抢先，谁先到期就决定错误字符串——主线程赢是 `ipc_response_timeout`，worker 自己的 50ms Unix 读超时赢则是 `ipc_read_failed`（`types.rs:59`）。同一提交树在 run #750 全绿、宿主侧 `cargo test --workspace` 370 项全过、单测单独连跑 8/8 过，说明是负载诱发的偶发，而非回归；
  - 修复：断言改为接受上述两种合法超时结果（附错误串上下文便于失败时定位），真正约束行为的 `unwrap_err()`（不得返回 350ms 慢处理器的响应）与 `started.elapsed() < 250ms`（必须及时放弃而非阻塞）两条原样保留；未使用 `#[ignore]`、未删除用例、未放宽时间预算；
  - 验证：`cargo fmt --all --check` 通过；`cargo clippy -p agenthub-runner --all-targets -- -D warnings` 零告警；宿主侧 `cargo test --workspace` 全绿（309 + 5 + 7 + 29 + 17 + 3 = 370 项）；额外在 4 核宿主上挂 8 个 `yes` 自旋进程制造满载，连跑该单测 25/25 通过（修复前该条件下会周期性复现）。

## [2.7.9] - 2026-11-18

- **全栈安全加固、稳定性治理、交互细节优化与架构解耦（`packages/contracts`, `services/api`, `deploy`, `crates/runner`, `apps/web`, `apps/android` `v2.7.9` / `versionCode = 128`）**：
  - **部署前门、容器非 Root 与后端重构 (`deploy`, `packages/contracts`, `services/api`, `.github/workflows`)**：
    - `deploy/nginx/agenthub-web.conf.template` 与 `apps/web/public/theme-init.js`：落地面向静态前门的严格 CSP（移除 `unsafe-inline` 内联脚本依赖），补充 `X-Content-Type-Options` / `Referrer-Policy` 等安全响应头，优先转发 `__Host-agent_pulse_session` 安全 Cookie，并使用中性域名重命名样例配置（`deploy/1panel/platform.example.com.conf.example`）；
    - `deploy/api.Dockerfile`、`deploy/docker-compose.selfhost.yml`、`.dockerignore` 与 `.gitignore`：API 容器切换非 root `node` 用户，收敛自部署默认开发身份开关，完善构建忽略规则并在 `.github/workflows/ci.yml` 增加 `android-checks` 单测门禁；
    - 新增 `services/api/migrations/0017_expand_agent_types.sql` 将数据库 `agent_type` 约束与索引扩展至全量 5 类引擎（`claude_code`、`codex`、`antigravity`、`grok_build`、`zcode`）；拆出 `security-guards.ts`、`app-helpers.ts`、`account-helpers.ts`、`postgres-helpers.ts`、`remote-stream-helpers.ts`、`store-schemas.ts` 与 `migrations-list.ts`，新增 `services/api/test/security-hardening.test.ts` 验证账号及远程网关边界。
  - **Rust Runner 私钥隔离、Win32 托盘异步化与并发安全 (`crates/runner`)**：
    - 私钥与凭证保护（`device.rs`, `auth/cache.rs`, `auth/{types,manager}.rs`, `daemon_ipc/platform/windows.rs`）：新增 `write_private_file`（Windows 受保护独占 DACL `CreateFileW(CREATE_NEW)` / Unix `0o600`），为设备身份与缓存凭证启用 `Zeroize` / `ZeroizeOnDrop` 内存清零及私有 `.revoked-*.bak` 备份；
    - Win32 托盘防阻塞与异常处理（`tray_menu/win32/{gdi,frame,window,events}.rs`）：为 GDI/上传资源实现 `Drop` 与边界校验，弹窗消息过程包裹 `catch_unwind`，`WM_TIMER` 改为后台异步刷新（`WM_APP_TRAY_MENU_READY`）防止界面阻塞，`STATE`/`FRAME` 改用 `try_borrow*`；
    - `daemon.rs`、`remote_ipc.rs` 与 `remote_network/*` 跨线程原子变量统一使用 `Release`/`Acquire` 内存序；隔离桌面 Antigravity 守护进程避免干扰单元测试，`is_safe_model_id` 支持带方括号模型 ID。
  - **Web Console / PWA 与原生 Android 全链路对齐与加固 (`apps/web`, `apps/android`)**：
    - `apps/web`：扩展 `claude_desktop` / `gemini_cli` 会话引用解析，StrictMode 下幂等消费邮箱验证/重置令牌；`remote-client.ts` 安全处理 `AbortSignal`、非 JSON 错误及 `413 payload_too_large`；为外链 Markdown 图片加入 click-to-load 门禁并拦截 SVG `data:` URI；按会话键隔离事件、指令、队列、草稿及 `MAX_CACHED_SESSIONS = 24` 上限；补齐模态框焦点陷阱（`dialog-focus.ts`）、`<Select>` Tab 筛选与 ARIA 语义、`sw.js` 通知点击精确导航，并将 `RemoteConsole.tsx` 拆出 `remote-session-state.ts` 与 `RemoteToolCard.tsx`；
    - `apps/android`：`AgentGateway` 强制非回环远端使用 HTTPS（支持 IPv4/IPv6 回环开发）、补全路径编码、修复 `togglePlugin` 单对象 `{ plugin }` 解析并加入响应/SSE `SseLineParser` 长度上限；`SecureConfigStore` 收窄清库触发条件；`CommandLedger` 使用 SHA-256 摘要与 `512` 条 LRU；`EventWindow` 保留关键轮次事件并处理游标回退与 `invalid_cursor` 重置；将待发队列与首条指令迁入生命周期感知与会话级状态，主线程更新 UI 状态并处理 `401` 登出；修复 `QuestionCard` 防连点、配对码 `EXTRA_IS_SENSITIVE` 剪贴板标记、空文件与 `7MB` 附件总容量校验及 `data_extraction_rules.xml` 备份屏蔽；提取 `RemoteTranscriptArea` 使 `RemoteScreen.kt` 降至 `752` 行，全仓通过 `scripts/check-line-limits.py`。

## [2.7.8] - 2026-11-18

- **Google Antigravity (`antigravity`) 与 OpenAI Codex (`codex`) 全栈完整度全面对齐 Claude Desktop (`crates/runner`, `apps/web`, `apps/android`)**：
  - **Google Antigravity (`antigravity`) 全协议闭环、实时思考流与精准模型归因 (`crates/runner`)**：
    - 新增 `collectors/antigravity/daemon.rs` 与 `daemon_turn.rs`：在无独立 `agy` CLI 的 Antigravity 2.0 桌面端环境下，自动探测 `hub_*.json` (`language_server` ConnectRPC 端点)，打通 `StartCascade`、`SendUserCascadeMessage`（支持多模态图片附件与指定模型）、`GetCascadeTrajectorySteps` 实时流式轮询（思考过程、工具/命令执行、统一 Diff、Token 用量）、`HandleCascadeUserInteraction`（权限审批与多选提问应答）及 `CancelCascadeInvocation` 中断闭环；
    - 精准模型归因与状态校准：通过 `GetCascadeTrajectoryGeneratorMetadata`、`antigravity_state.pbtxt` 占位符映射（`MODEL_PLACEHOLDER_M*` 及展示名反查）与 `<USER_SETTINGS_CHANGE>` 动态解析每会话真实模型（如 `gemini-4-carbon-high`），消除 `"gemini-3.8-flash"` 静态误判；结合 `GetAllCascadeTrajectories` 实时校准 SQLite 陈旧运行态，修复 `conversation_parents.parent_id` 列名兼容，并从 `raw_summary` Protobuf 提取子代理 `Role` 职责描述；
    - 会话历史与回收站：`conversation/antigravity.rs` 按 FIFO 将 `PLANNER_RESPONSE.tool_calls` 与后续 `GENERIC` 工具输出配对合并为 `"{入参或Diff}\n\n〔结果〕\n{输出}"`，为 `edit_file` / `write_file` 自动合成标准统一 Diff，彻底消除 `Created At:` 原始工具输出泄漏为伪助手消息气泡；新增 `collectors/antigravity/trash.rs` 支持安全移入 `~/.agenthub/trash/sessions/` 并通知守护进程 `DeleteCascadeTrajectory`。
  - **OpenAI Codex (`codex`) 会话富化、子代理折叠、Diff 解析与审批补齐 (`crates/runner`)**：
    - 新增 `collectors/codex_enrich.rs`：基于 `state_5.sqlite` 与 rollout JSONL 缓存索引对 `codex app-server` `thread/list` 及本地会话实施全量富化，剥离 Windows `\\?\` 路径前缀、统一秒/毫秒时间戳为毫秒、将 `notLoaded`/`persisted` 归一为 `"idle"`、补齐真实 `model` / `prompt_tokens` / `completion_tokens` / `cache_read_tokens` (`cached_input_tokens`) / `context_window_tokens`，并将多智能体 `source` JSON (`thread_spawn.parent_thread_id`) 子会话折叠至父会话 `subagents: []`；
    - 历史对话、实时审批与安全回收：新增 `conversation/codex.rs` 保留 `"reasoning"` 思考块、将 `apply_patch` (`*** Begin Patch`) 转换为标准 `--- a/... +++ b/...` 统一 Diff、按 `call_id` 配对 `function_call_output` / `custom_tool_call_output`；`session_host.rs` 扩展 `turns` 回放覆盖思考与工具项并在空回合时自动回退本地 rollout 加载；`session_host_poll.rs` 与 `session_host_exec.rs` 补齐 `item/tool/requestUserInput` (`codex_question_id`)、`execCommandApproval`、`applyPatchApproval` 与 `item/mcpToolCall/requestApproval`；新增 `collectors/codex_trash.rs` 并在 `:4200` 面板开放 Codex 与 Antigravity 会话安全回收。
  - **Web PWA 与原生 Android 历史/流式思考、工具 Diff 及上下文容量全量对齐 (`apps/web`, `apps/android`)**：
    - 将 `apps/web/src/lib/remote.ts` (1,306 行) 拆解为 `remote.ts` (485 行)、`remote-transcript-fold.ts` (412 行) 与 `remote-transcript-events.ts` (571 行)，并将 `remote.ts` 正式移出 `scripts/check-line-limits.py` 历史豁免清单；同步将 Android `SessionTranscript.kt` 拆分为 `SessionTranscript.kt` (549 行) 与 `SessionTranscriptEvents.kt` (414 行)；
    - 双端 `history/snapshot` 全面支持 `kind === "thinking"` 思考折叠块，并将历史 `"tool"` 项按 `"\n\n〔结果〕\n"` 拆分为 `command` (入参/Diff) 与 `output` (执行结果)，使历史会话直接渲染语法高亮 `+add / -del` Diff 与提问卡片；
    - 双端实时流新增 `item/reasoning/delta`、`item/reasoning/textDelta`、`item/reasoning/summaryTextDelta`、`item/commandExecution/outputDelta`、`item/fileChange/outputDelta` 及 Codex `fileChange.changes` Diff 折叠，并在 `multi_drain_claude.rs` 转发 Claude `/event/delta/thinking`；修复 Android `SessionSignals.kt` 读取 Codex `tokenUsage.last` 与 `modelContextWindow` 上下文水位及 Antigravity `1M` 默认窗口推断。

## [2.7.7] - 2026-11-18

- **移动端双轨全面升级：Mobile Web / PWA 自适应打磨与原生 Android 客户端全量对齐（`apps/web`、`apps/android`、`scripts`）**：
  - **Mobile Web 触控人机工学、底部抽屉与全断点自适应（`Select.tsx`、`useMobileConsoleLayout.ts`、`MobileHeaderAccessories.tsx`、`RemoteConsole.tsx`、`SettingsPage.tsx`、`remote.css`、`remote-desktop.css`、`globals.css`）**：
    - 视口与软键盘避让：HTML 视口新增 `viewport-fit=cover, interactive-widget=resizes-content`，配合 `useMobileConsoleLayout.ts` 同步 `visualViewport` 动态高度 `--vvh` 与安全区域 `env(safe-area-inset-*)`，并使用 `matchMedia("(max-width: 700px)")` 同步抽屉无障碍语义（`role="dialog"`、`aria-modal="true"`、主画布 `<main inert>`）与 `100svh` 高度，支持左边缘右滑呼出与抽屉左滑关闭手势；
    - 移动端原生底部抽屉（Bottom Sheet）：窄屏下将 `<Select>` 下拉菜单（超过 7 项自动展示实时筛选框）、`"在电脑上开始"` 模态框与二次确认框统一升级为全不透明底部抽屉，修复浅色模式背景透字与 `.remote-context-popover` 左溢出裁剪；
    - 顶栏与跨设备感知：手机顶栏补齐一键 `"新建会话"`、单点复制会话 ID 与跨设备 `"要你处理"` 未读角标/置顶提醒条；设置页窄屏导航改为无滚轮胶囊流式换行，底部输入栏与平板分栏（`701px~1024px`）支持自适应换行，彻底消除 `320px` 小屏至 `768x1024` 平板竖屏的控件挤压与横向溢出；
    - 触控热区扩展：在 `@media (pointer: coarse)` 下通过透明伪元素扩展小图标点击热区至 `44x44` CSS 像素，移除桌面悬停隐藏陷阱，并为 Markdown 宽表格与代码对比补齐独立横向惯性滚动。
  - **PWA 安装与系统状态栏主题同步（`manifest.webmanifest`、`theme.ts`、`pwa-install.ts`、`sw.js`）**：
    - 补齐 `maskable` 图标声明、`id: "/"`、`display: "standalone"` 与快捷入口，切换明暗主题时实时同步 `<meta name="theme-color">`（浅色 `#faf8f5` / 深色 `#161514`）；
    - 新增 `pwa-install.ts` 统一支持标准 `navigator.install()`、Chromium `beforeinstallprompt` 单次消费防错与 iOS Safari 添加到主屏幕指引，并清洗 `sw.js` 推送通知引擎名称与正文。
  - **原生 Android 客户端全协议对齐、平板自适应与巨石文件拆解（`apps/android` `v2.7.7` / `versionCode = 126`）**：
    - 协议与安全对齐：补齐 `Antigravity`、`Gemini CLI`、`ACP` 及插件门控 `Grok` 引擎识别与独立模型目录，登录与刷新自动水合 `/v3/remote/runners/:runner/hello` 与 `/v3/remote/plugins`；修复 `mergeHostHello` 误清空 `hostGeneration` 导致会话状态永久冻结的缺陷，并在切换服务器源站时自动剥离跨源 Cookie 与 Bearer 令牌；
    - 会话与交互补齐：同步 Web 端 Codex 原生 `userMessage` 双气泡去重、`turn/completed` 失败态保留、`sanitizeSessionTitle` XML 标签清洗、`___`/`ws-__-<hash>` 路径解码、执行中 `QueuedPromptBar` 消息排队、跨设备 `"要你处理"` 内联批准/拒绝、可接管会话一键恢复与断线重连、配对码生成与设备吊销、插件扩展开关及 `"跟随系统"` DayNight 冷启动防黑屏闪白；
    - 平板分栏与架构治理：新增 `>= 720.dp` 宽屏 `SideNavigationRail` 侧边导航栏、`FlowRow` 筛选胶囊、高对比度深色气泡与代码块/表格横向滚动，将 3 个超限单体文件拆解为 17 个高内聚子模块并将 `apps/android` 正式纳入 `scripts/check-line-limits.py`（全仓 `300` 个 `.rs`/`.ts`/`.tsx`/`.kt` 源文件零超限，Android 45 项构建与 JUnit 单测全绿）。

## [2.7.6] - 2026-11-18

- **Web 远程控制台与本机面板全链路交互优化（`apps/web`、`services/api`、`crates/runner`）**：
  - **模态框下拉层级、按键捕获与主按钮修复（`Select.tsx`、`RemoteLaunchModal.tsx`、`remote.css`、`remote-desktop.css`）**：
    - 抬升 `.cds-select-menu` 层级至 `z-index: 1200`，彻底修复 `"在电脑上开始"` 模态框内电脑、工作区、引擎、模型与权限下拉菜单被背景层遮挡的问题；为展开态 `<Select>` 增加捕获阶段 `Escape` 拦截，按 `Escape` 仅收起下拉菜单而不误关父级模态框；
    - 使用 `:where(...)` 收敛 `.remote-console` 兜底按钮选择器优先级并排除 `.remote-primary` 与 `.settings-page *`，修复 `"创建会话"` 主按钮灰底无强调色、设置页深色模式白底黑字对比度崩坏及 `"撤销"` 危险按钮样式失效问题；
    - 拆出独立 `RemoteLaunchModal.tsx`，默认继承侧边栏当前电脑与引擎筛选、仅在 Codex 下展示文件沙箱选项、创建成功后同步侧边栏筛选与输入栏权限模式，并紧凑化模态框间距使桌面端无需内滚动即可一屏操作。
  - **Codex 提问双气泡去重、执行中消息排队与输入防丢字（`remote.ts`、`remote-client.ts`、`QueuedPromptBar.tsx`、`RemoteConsole.tsx`）**：
    - 拆出 `remote-client.ts`（`remote.ts` 由 `1653` 行压降至 `1306` 行），并在 `ConversationFold` 中记录 `promptedTurnIds` / `currentTurnId`，对已由 `turn/started` 渲染的本轮原生 `userMessage` 事件精准去重，同时保留跨轮重复文本提问与外部终端直接输入的真实气泡；
    - 新增 `QueuedPromptBar.tsx` 与会话级待发队列：Agent 执行期间按 `Enter` 或点击 `"排队"` 可将后续指令排队暂存（支持编辑、取消、立即发送），待本轮正常结束后自动续发，若用户点击 `"停止"` 或本轮报错则安全保留草稿不自动下发；移除异步 `submit()` 返回后的冗余 `setText("")`，杜绝发送后 50ms 内快速连打被清空。
  - **登录页、设置页、移动端抽屉、骨架屏与细节体验打磨（`AccountGate.tsx`、`SettingsPage.tsx`、`RemoteMessageCard.tsx`、`ConversationSkeleton.tsx`、`MarkdownView.tsx`、`session-tree.ts`、`globals.css`）**：
    - 修复桌面端登录卡片左偏双列网格覆盖、输入框双重焦点环与懒加载过渡屏无样式闪烁，新增密码显隐切换按钮、去极客化中文说明文案，并消除登录成功时重复调用 `/v1/account/me`；
    - 修复设置页 `.account-session-row` 网格列错位导致的 `"撤销"` 按钮横向拉满，按需懒加载设备、Web Push 公钥（记忆未启用网关免重复 `404`）、活跃会话与审计日志；Gateway 错误处理器对预期 `4xx` 响应停止打印 `level: 50` 堆栈噪音；
    - 同步初始化移动端视口状态并仅在用户交互打开抽屉时挂载过渡动画，根除 `390x844` 首屏与缩放时的侧边栏残影遮挡及底部工具栏重叠；
    - 侧边栏改为单列全宽选择器消除设备名截断，新增桌面端 `"收起侧边栏"`（`Ctrl+B`）、搜索快捷键（`Ctrl+K` / `Escape`）、会话标题内联 `<command-name>` XML 清洗、`___` 路径解码、会话 ID `"已复制"` 胶囊反馈、`"可接管"` 会话 `"恢复中…"` 状态与 `ConversationSkeleton` 骨架屏、AI 回复悬浮 `"复制"` 按钮，以及本地相对路径链接安全降级为代码标签。
  - **本机原生控制台修复与减负（`crates/runner`）**：
    - `collectors/capabilities/parsers.rs` 新增 `parse_toml_string_literal` 区分 TOML 单引号字面量与双引号转义串，修复 Windows 路径含 `\r` / `\n` / `\t`（如 `\resources\...\node_repl.exe`）被误转义为控制字符的问题；
    - `resources/dashboard.{js,css}` 为不可删除会话行补齐对齐占位列、对工作区会话列表实施分页展开（首屏 `40` 条）、折叠 PowerShell `-EncodedCommand` 超长 Base64 载荷（复制仍保留原文），并为用量面板增加 `5s` 内存缓存短路。
- **全仓文档无损瘦身与版本契约同步**：统一双语 `README`、`llms*.txt`、`AGENTS.md` 与 `docs/*` 至最新真实架构（Vite 8 + Nginx 静态 Web 前门、Gateway 索引裁剪、Remote v3 原生 Android、Antigravity `agy` 双向控制与 `UsageIndexDb` 冷热分层），100% 保留全部历史已发布版本节点。

## [2.7.5] - 2026-11-18

- **全栈代码审查硬化与性能优化（`services/api`、`apps/web`、`crates/runner`、`deploy`、`scripts`）**：
  - **Gateway 存储与数据库负载治理（`services/api`）**：
    - 拆解 `remote-store.ts` 协议校验与投影逻辑至 `remote-store-validation.ts`（`remote-store.ts` 由 `1460` 行压降至 `971` 行），引入 `StatementSync` 预编译语句缓存，并将 SQLite WAL 调优为 `synchronous=NORMAL` + `temp_store=MEMORY`；
    - 新增覆盖索引 `remote_event_runner_id(runner, id)` 与内存事件计数，将 `receive()` 裁剪从每帧 `TEMP B-TREE` 全表排序（`O(N log N)`，实测 `16.67 ms`）降为正向索引单点裁剪（`O(log N)`，实测 `< 0.01 ms`）；
    - 将 `resumePoint()` 拆分为 `hub=?` 与 `hub IS NULL` 双路 `remote_event_session` 倒序索引游标短路扫描（实测由 `23.44 ms` 降至 `0.033 ms`），并新增 `thinRunners()` 内存投影缓存消除 `/v3/remote/sessions` 每次轮询对百 KB 级 `hello` JSON 的重复拷贝与 `JSON.parse`；
    - 新增可配置 `deviceRevalidateMs` / `streamAuthRevalidateMs`（生产默认 `2000 ms`）与设备/会话吊销即时通知钩子（`revokeRunner` / `invalidateStreamAuth`），并在 `/v3/remote/stream` 无新事件时短路空转唤醒，消除高频流式输出下的 PostgreSQL 查询风暴；补齐 `GET /healthz` 探针路由并修复 `admin-setup.test.ts` 测试库残留。
  - **Web 控制台流式去噪、按需分包与渲染优化（`apps/web`）**：
    - `lib/stream.ts` 新增 `streamBatchAffectsSnapshot` 与 `streamBatchAffectsSession`，过滤纯流式文本增量对全局 `/v3/remote/sessions` 的无效轮询及跨设备/跨会话详情唤醒；
    - `PanelPage.tsx`、`RemotePage.tsx` 与 `RemoteConsole.tsx` 对 `RemoteConsole` 和 `SettingsPage` 实施 `React.lazy` 按需加载，配合 `vite.config.ts` 拆分 `vendor-react` / `vendor-markdown` 独立缓存块并移除 `<head>` 中 `281 KB` Base64 位图伪 SVG 图标引用，未登录首屏请求体积下降约 `70%`；
    - 为 `RemoteToolGroup` 补齐按元素引用相等的 `React.memo` 比较器并缓存 `RemoteToolCard` Diff/提问解析结果，消除流式吐字期间历史工具卡片组的全量重渲染。
  - **Runner 本机守护进程安全加固与 Outbox 批处理（`crates/runner`）**：
    - `web_server/http_io.rs` 与 `server.rs` 新增有界请求行/Header 行读取（`8 KiB`）、Header 数量上限（`64`）、严格 `Content-Length` 校验上限（`16 MiB`，超限返回 `413` 防无界 `vec!` OOM 崩溃）及最大并发连接守卫（`32`）；
    - `remote_journal/journal.rs` 高频查询切换至 `prepare_cached` 并新增 `mark_sent_through` 批量水位推进，配合 `remote_network` 单轮连续 `ack` 帧合并与主循环自适应空闲退避（活跃 `10 ms` / 空闲 `50 ms`），大幅降低流式期间磁盘事务次数与后台空转唤醒。
  - **部署模板与架构行数门禁扩展（`deploy`、`scripts`）**：
    - `deploy/nginx/agenthub-web.conf.template` 开启静态产物与 API JSON 的 `gzip` 压缩（排除 `text/event-stream`）；修正 `deploy/1panel/platform.example.com.conf.example` 的 ACME 跳转优先级与 `proxy_set_header` 继承配置；
    - 扩展 `scripts/check-line-limits.py` 同步纳管 `packages/contracts`、`services/api` 与 `apps/web` 的 `.ts`/`.tsx` 源码行数棘轮门禁。

## [2.7.4] - 2026-11-17
- **Antigravity 实时模型发现 (`crates/runner`)**：针对前端仅展示 3 个硬编码占位模型的问题，新增 `collectors/antigravity/models.rs`（`discover_live_antigravity_models`）从本地 daemon `hub_*.json` + CSRF 头动态拉取真实目录，仅在无实时源时回落静态清单。
- **离线闪断与风暴抑制 (`crates/runner`, `services/api`)**：WS `write_timeout` 放宽至 `15s` 并改用指数退避重连；`refresh.rs` 按结构指纹将纯 token/时间戳跳变的 `ready` 快照冷却至 `120s`，过滤无 `hub_session_id` 杂音；Gateway 新增 `offlineGraceMs`（默认 `6000ms`）且 `/v3/remote/stream` 遇写满改为等待 `drain` 不断连。
- **消息吞没竞态与轻量流式态 (`apps/web`)**：`settledConversationItems` 按已落盘事件计数退避乐观气泡，补充 `inFlight` 期间的 SSE `again` 重拉标记，并以轻量 `.remote-turn-live` 状态条替代巨型闪烁骨架（Web/API/Contracts 96 项单测全绿）。

## [2.7.3] - 2026-11-16
- **Antigravity (`agy`) 实时双向接管与审查硬化 (`crates/runner`, `apps/web`)**：新增 `AntigravityProcessSession` 持久化 `stream-json` 通道、多运行根/Subagent 发现与能力生态扫描；硬化 `antigravity_req_` 精确前缀匹配、缺失 `decision` 默认 fail-closed `"decline"`、`8192` 字节 UTF-8 安全 `stderr` 截断与提前异常子进程回收。
- **SQLite 冷热分层与峰值内存治理 (`crates/runner`)**：引入 `BoundedLruCache`、流式 Typed Serde 解析与 `usage_ledger/index_db.rs`（`UsageIndexDb`，`>35` 天或超出单源 `256` 热文件自动溢出 SQLite，`PRAGMA auto_vacuum = INCREMENTAL`），Windows Release 热重启扫描峰值 Commit 降至 `57.41 MB`（`-80.5%`）、稳态 WorkingSet `2.61 MB`。

## [2.7.2] - 2026-11-14
- **隔离 WebView2 UI 子进程与纯 Win32 托盘升级 (`crates/runner`)**：剥离常驻浏览器进程，新增 `agenthub-runner ui` 按需子进程（`tray/webview_host.rs` + Job Object 连根回收，关窗或最小化失活后释放 `425 MB` WebView2 内存，主守护保持 `WebView2Children=0`）。
- **零 WebView2 原生托盘卡片与模块拆解**：`tray_menu/win32/` 拆分为 7 个正交子模块，支持配置详情下钻、用量周期切换（`1d/7d/30d/All`）与全键盘 SDF 焦点环导航；全仓 174 个 Rust 文件零超限通过。

## [2.7.1] - 2026-11-12
- **Usage Ledger 历史日志内存暴降治理 (`crates/runner`)**：针对 6GB+ 本地 JSONL 历史导致 `224 万` 小字符串分配与全量 `clone` 占满 `566.8 MB` 内存的根因，落地 `Arc<str>` 全局字符串驻留池 `INTERNER`、就地缓存更新、冷文件索引释放与 `SetProcessWorkingSetSize` 修剪，稳态 WorkingSet 骤降至 `1.08~18.89 MB`（`-97%~99.8%`）。
- **CI 容器代理契约加固 (`scripts/test-web-proxy.sh`)**：改用 `docker cp` 注入配置模板并直连容器 IP，消除 Docker-out-of-Docker 路径挂载与内嵌 DNS 污染失败。

## [2.7.0] - 2026-11-11
- **Web 层静态化重构：Next.js 16 迁至 Vite 8 + Nginx 同源反代 (`apps/web`, `deploy/`)**：移除常驻 Node SSR，改为四入口静态 MPA + `nginx:1.29-alpine`（`agenthub-web.conf.template` 统一在 `server` 层收口 Cookie 白名单、服务端 Bearer 注入、API `no-store` 与协议透传），生产镜像由 `1.51 GB` 降至 `96.7 MB`（`-93.6%`）、常驻内存由 `64.93 MiB` 降至 `8.37 MiB`。
- **自动化反代门禁**：CI 新增静态构建、`nginx -t` 与 `scripts/test-web-proxy.sh` 13 项容器级安全回声测试；补充 `agenthubd` 稳态 `13.47 MB` RSS 基准记录。

## [2.6.23] - 2026-11-11
- **全工作区巨石清零里程碑 (`crates/runner`, `scripts/check-line-limits.py`)**：将 `collectors`（7,352 行）、`remote_host`（4,435 行）、`usage_ledger`（4,300 行）、`claude_gateway`（3,235 行）、`tray_menu`（2,796 行）、`web_server`（2,785 行）、`remote_network`、`conversation`、`remote_journal`、`auth`、`model_catalog` 等全部遗留巨石正交拆解为职责单一的子模块，`LEGACY_BASELINE_CAPS` 彻底归零（全仓单文件 `<=800` 行，329 项 Rust 测试全绿）。
- **跨机 `"要你处理"` 置顶条视觉与对比度返工 (`apps/web`)**：新增高对比文字令牌 `--cds-clay-ink`（WCAG AA `>=4.7:1`），对齐 32px 审批按钮、16px 外框递减圆角与 `scrollbar-gutter: stable` 窄滚动条。

## [2.6.22] - 2026-11-11
- Web 侧栏顶部新增跨设备 `"要你处理"` 队列（`AttentionQueue.tsx` + `attentionEntries`），取 `execution_state` 与 `pending_approvals` 并集并校验 `control_connected`，支持不切换设备直接内联批准/拒绝。

## [2.6.21] - 2026-11-11
- Web 与 Android 统一将乐观提问确认改为绝对用户行数阈值（`neededUserRows`）与 1s 独立超龄清理循环；Android `AgentGateway.kt` 改为读到首行字节才判 SSE 存活，并拆出 `SessionSignals.kt` 保持单文件行数合规。

## [2.6.20] - 2026-11-10
- 修复 Android 输入法遮挡与双重导航栏留白（`adjustResize` + `ime.union(navigationBars)`），按行数基线保留重复文本提问的乐观气泡与首响应等待提示，收拢双端冗余停止按钮与参数胶囊文案。

## [2.6.19] - 2026-11-10
- Runner 路径脱敏改为局部替换 `[local_path_redacted]` 保留语句上下文；Web 上线即时乐观气泡与紧凑工具抽屉；Android 引入 `IncrementalTranscriptFold` 后台增量折叠消除主线程卡顿。

## [2.6.18] - 2026-11-10
- 修复 Android Material 3 默认亮色（Paper）配色层级并重构 `RemoteScreen` 对话卡片留白与视觉层次。

## [2.6.17] - 2026-11-10
- 修复 Runner 误将 Claude Desktop DPAPI 加密密文当作明文 Bearer 注入导致的远程调用 `401` 认证失败。

## [2.6.16] - 2026-11-09
- 补全 Codex 与 Antigravity 运行时网关成对环境注入，落地服务端插件能力门禁并将 Grok 解耦为可选插件，修复新旧字段兼容导致的重连循环。

## [2.6.15] - 2026-11-09
- Android 持久化记忆网关地址与登录邮箱，修复会话事件解析异常并补齐按需拉取 Runner `hello` 目录。

## [2.6.14] - 2026-11-09
- 修复网页端选择 `claude-*` 模型别名直连裸上游网关时的 `503` 路由失配，自动按上游真实可用模型目录映射。

## [2.6.13] - 2026-11-09
- 隔离 Runner 子进程继承的宿主环境冲突变量，修复网页端下发首轮提示词时的网关 `401` 鉴权污染。

## [2.6.12] - 2026-11-08
- Runner 向平台 `hello.models.agents.claude_code` 完整上报上游网关真实模型清单，解除仅显示少量本地别名的限制。

## [2.6.11] - 2026-11-08
- Runner 直连裸上游网关时保留客户端所选真实模型 ID，不再强制套用本地路由器专用的别名改写。

## [2.6.10] - 2026-11-08
- Android 自适应启动图标背景改为全透明图层以贴合系统桌面视觉，并统一 Prettier 代码格式门禁。

## [2.6.10] Windows Runner 构建记录（2026-11-08）
- 同步构建归档 Windows `agenthub-runner` `v2.6.10` Release 二进制，替换停留在旧版的桌面运行实例。

## [2.6.9] - 2026-11-07
- 统一 Android 启动图标、启动画面与单色主题层至与 Runner 托盘、Web 控制台同源的品牌图形资产。

## [2.6.8] - 2026-11-07
- **根治 `remote.db` 9GB 膨胀 (`services/api`)**：新增 `storableFrame()` 在 `ready` 帧入 `remote_events` 前剥离 440KB 级 `models`/`discovery` 大字段（保留 `pending_approvals` 锚点），设置每设备 `EVENT_RETENTION = 20_000` 窗口并对存量库重写 `VACUUM`（`8.46 GiB -> 114.8 MiB`，回收 `98.68%`）。

## [2.6.7] - 2026-11-07
- `GET /v3/remote/sessions` 瘦身为轻量设备头（单设备响应从 `583 KB` 降至 `1.8 KB`，`-99.7%`），完整目录拆至按需端点 `GET /v3/remote/runners/:runner/hello`；反转 Android 附件 UTF-8 嗅探默认策略并统一引擎枚举。

## [2.6.6] - 2026-11-07
- Android 附件增加 8KB UTF-8 多字节字符边界嗅探防 UTF-16/Latin-1 乱码，合并 `ProviderKind` 与 `engineOfSession` 分类器并复用 `OrcaFilterPill`。

## [2.6.5] - 2026-11-07
- Android 修复 `TurnSeed` 跨会话串味为单次消费，补齐 SAF 空输入流与异常文件名报错提示及浅色主题底部导航栏深色图标。

## [2.6.4] - 2026-11-07
- Android 独立识别 `claude_desktop` 与 `gemini_cli` 模型目录，补齐首条指令失败反馈、实时 `detailState` 轮询节律、`readCapped` 流式长度预判与 SAF 句柄关闭。

## [2.6.3] - 2026-11-06
- Android 对齐模型/推理强度/权限选择、SAF 多附件上传、上下文容量环与三套高对比主题（石墨/午夜/纸墨）；将会话详情打开期间的全局列表轮询放宽至 `15~20s` 消除大包抢占导致的请求取消。

## [2.6.2] - 2026-11-06
- 清理 Android `minSdk=26` 下不可达的旧密度位图，合并 `mipmap-anydpi` 并补齐 Android 13 单色主题图标层。

## [2.6.1] - 2026-11-06
- 完成 Android v2.6.0 审查 15 项修复：修复全新安装登录校验死锁、单例缓存 `AgentGateway`、`seedResume` 快照首帧偏移、`EventWindow` 容量封顶、屏幕旋转 `Application` 级保活及密码移出 `savedState`。

## [2.6.0] - 2026-11-05
- **原生 Android 客户端整体重写 (`apps/android/`)**：全面迁移至 `@agent-pulse/contracts/remote` Remote v3 协议，落地 Cookie 会话认证、`EventWindow` 快照保护、`CommandLedger` 幂等键、四级会话树与 Jetpack Compose 固定底栏 + 全屏详情架构。

## [2.5.6] - 2026-11-05
- Runner 支持孤儿网关配置自动回退；Web 侧栏按活跃时间降序排列，引入会话级游标 `sessionCursor()`、`resumePoint` 复合索引与 `foldWithCache()` 尾部增量折叠（4 万事件归约降至毫秒级）。

## [2.5.5] - 2026-11-03
- 修复普通浏览器访问同名标题网页时误关 Runner 本地应用窗口的缺陷（绑定 `APP_WINDOW_PID` 精确匹配）；Windows 托盘监听 `TaskbarCreated`（穿透 UIPI）并增加冷启动挂载重试与退出 `NIM_DELETE` 清理。

## [2.5.4] - 2026-11-02
- 移植 `magpie` 句柄级变更探测与全客户端模型用量解析，将本地与远端链路统一为单节拍扫描加事件推送，升级自绘托盘交互防抖、品牌多色图标、多维用量筛选与 CSV 导出。

## [2.5.3] - 2026-10-20
- 修复多目录同名会话移入回收站时的覆盖冲突，`web_server.rs` 收紧 `Origin == http://{Host}` 同源校验，并为已完成子代理转录增加按长度与修改时间的缓存记忆。

## [2.5.2] - 2026-10-19
- 重建 9 帧高辨识度浅色六边形托盘图标（`16~256px`），将 Windows 托盘右键菜单升级为 `UpdateLayeredWindow` 自绘圆角亮色卡片并支持 `#settings/gateway` 哈希直达。

## [2.5.1] - 2026-10-19
- 内嵌控制台收敛为 `会话 / 智能体 / 用量 / 工作区` 四主视图加统一设置页，上线 `GET /api/usage/ledger` 逐请求去重账本与 53 周热力图，并嵌入 20 枚 `magpie` 厂商图标。

## [2.5.0] - 2026-10-19
- 对标上游 `magpie` 将设置弹窗重构为五分区独立视图 `#view-settings`，新增 Debug 构建磁盘静态资源热重载（`GET /api/dev/wait`）与单调递增视图切换防竞态保护。

## [2.4.99] - 2026-10-19
- 根除 `remote_host.rs` `events_queue` 可重入死锁，新增跨智能体 Skills/MCP/Hooks 联邦资料库（含零依赖 Codex TOML 解析）、上下文容量环与多选问答卡片。

## [2.4.98] - 2026-10-19
- 新增网关会话亲和卡片与 4 语言接入代码切换器、顶栏设置齿轮（`Ctrl+,`）、High-DPI Win32 托盘加载以及移动端可展开工具/思考手风琴卡片。

## [2.4.97] - 2026-10-19
- 修复 ZCode 子查询漏计与子代理汇总并将 9.8 万行聚合提速至 `0.39s`，补齐 Codex 缓存指标、5 行骨架屏与 Android `Orca` 视觉令牌统一。

## [2.4.96] - 2026-10-19
- 新增跨 Claude/Codex/ACP 的 `<=64 KiB` 路径脱敏审批 Diff 预览，落地通用 JSON-RPC 2.0 `acp_app_server.rs` 适配器与 `ChildJob` 进程树隔离。

## [2.4.95] - 2026-10-19

- 内嵌 Web 控制台引入 Shimmer 骨架屏、3 次轮询/15s 空状态宽限期与 12s 摘要缓存；Claude 转录改为全文件扫描并计入 `cache_read_input_tokens`，本地模式启动超时缩至 3s；Android 抽离 `OrcaComponents.kt`。

## [2.4.94] - 2026-10-18

- Android 引入对标 `stablyai/orca` 的多设备控制舱与虚拟终端按键行（`Esc`/`Tab`/`Ctrl-C`/方向键）；内嵌控制台补齐 `request_count`/`calls` 统计；完善 GitHub Actions CI、社区健康文件与 `llms.txt` / `llms-full.txt`。

## [2.4.93] - 2026-10-18

- 完成开源工程治理、仓库社区标准文件、大模型检索索引（`llms.txt` / `llms-full.txt`）与宣发文档体系建设，全工作区自动化测试通过。

## [2.4.92] - 2026-10-18

- 将 `crates/runner/resources/dashboard.html` 内联的 1700 行脚本拆分为独立 `app.js` 并纳入语法检查，统一模型费率计算表消除三处重复计费逻辑。

## [2.4.91] - 2026-10-18

- 对标 magpie 桌面极简规范，移除内嵌控制台顶栏冗余应用图标、装饰性 Emoji 与外层大白框容器，统一页面呼吸感与留白。

## [2.4.90] - 2026-10-18

- 修复内嵌控制台 `#sideToggle` 侧边栏开关被外层容器遮挡失效的问题，合并双层顶栏并移除右上角重复关闭按钮。

## [2.4.89] - 2026-10-18

- 为内嵌控制台引入 View Transitions 页面切换、分组折叠缓动、弹窗进出场过渡、数字插值滚动与悬停微交互，同时支持 `prefers-reduced-motion` 降级。

## [2.4.88] - 2026-10-17

- 移除 Edge App 模式下的异色系统标题栏割裂感，统一内嵌控制台顶栏与主画布背景色，对齐 magpie 原生窗口视觉一体性。

## [2.4.87] - 2026-10-17

- 删除内嵌控制台顶栏与侧栏冗余分组小标题（工作台/洞察/系统），统一顶栏与内容区底色。

## [2.4.86] - 2026-10-17

- 内嵌控制台默认窗口尺寸收敛至 820×600（最小 640×460），修复侧边栏展开后切换按钮消失与图标方向错误，精简顶栏元素并放大字号。

## [2.4.85] - 2026-10-17

- 内嵌控制台默认窗口尺寸由 1240×820 收敛至 980×720（最小 720×520），并确保窗口首次启动时在屏幕正中央居中呈现。

## [2.4.84] - 2026-10-17

- 修复内嵌控制台侧边栏拖拽宽度（`--side-w-override`）与响应式断点冲突导致的分界线错位问题，实现主内容区自适应伸缩。

## [2.4.83] - 2026-10-17

- 修复内嵌控制台 `#sideToggle` 窄屏切换失效，优化顶栏排版、全局字号、卡片圆角与窗口默认尺寸调节。

## [2.4.82] - 2026-10-17

- 修复 `crates/runner/src/remote_host.rs` `discover()` 扫描超时或瞬态空结果覆盖既有非空会话缓存导致偶发显示 0 条历史会话的缺陷，并精修控制台界面排版。

## [2.4.81] - 2026-10-17

- 内部过渡版本号，扫描侧空洞修复直接并入 v2.4.82 发布。

## [2.4.80] - 2026-10-17

- `apps/web` 对齐 `agenthub-runner` 最新模型目录探测、多 Provider 分页与 Token 用量聚合契约，优化新建会话模型名称展示。

## [2.4.79] - 2026-10-17

- 修复 `crates/runner/src/remote_host.rs` `thread/list` 跨 Provider 分页边界截断与重复问题，确保多智能体会话合流排序与游标分页稳定。

## [2.4.78] - 2026-10-16

- 根治桌面工作目录残留 `agenthub.db` 与启动控制台黑框闪烁；将本机控制台重构为对标 magpie 的独立桌面窗口体验，统一顶栏底色与居中尺寸。

## [2.4.77] - 2026-10-15

- 对标 `yetone/magpie` 重构 `crates/runner/src/collectors/workspace.rs` 全局工作区深度发现、路径还原与模型真实用量聚合引擎。

## [2.4.76] - 2026-10-14

- 移除多租户商业化遗留组件（Telegram OAuth / 设备码流 / 团队账单），在 `services/api/src/sqlite-account-store.ts` 实现单机 SQLite 账户与会话持久化闭环。

## [2.4.73] - 2026-10-12

- 收敛本地开发辅助资产与 `.gitignore` 规则，解除本地索引与私有网络配置追踪，确保远端开源仓库零本地环境残留。

## [2.4.72] - 2026-10-10

- 跑通 Gateway + Web + Runner + Codex 真实自部署全链路（注册、绑定码、Ed25519 绑定、同源反代 `connect`/`create`/`send_input`/`interrupt`），修复联调发现的边界问题。

## [2.4.71] - 2026-10-10

- 重构 `apps/web/src/components/remote.css` 运行中工具卡片 Shimmer 与滚动容器，全面迁移至 `transform`/`opacity` GPU 合成层以保障 60 FPS 渲染。

## [2.4.70] - 2026-10-10

- 增强 `apps/web/src/server/gateway-proxy.ts` `isLoopbackOrContainerHost` 判定以放行 Docker Compose 容器内网反代，补齐 `deploy/` 容器化编排与健康检查。

## [2.4.69] - 2026-10-10

- 战略重心收敛至 Runner + Gateway + Web 自部署核心链路，移除重型 `apps/desktop`（Tauri）客户端并由 Runner 内嵌轻量控制台与 Web 端承接。

## [2.4.68] - 2026-10-08

- 根目录 `package.json`、`package-lock.json` 与部署编排配置统一由 `agent-pulse` 规范命名为 `agenthub`。

## [2.4.68] - 2026-10-07

- 新增 `ClaudeProcessSession` 将 Claude Code 驱动方式由单次 `--resume` 拉起升级为本地常驻 stdin/stdout 流式双向进程控制。

## [2.4.67] - 2026-10-06

- 清理 Web 端废弃的 17 个旧版监控工作台组件与冗余动画库依赖（`gsap`），收敛未引用导出并完成全端按钮交互审查。

## [2.4.66] - 2026-10-05

- 实现对话轮次中间思考与工具执行过程统一折叠收敛（Turn Process Grouping），突出展示每轮最终回复结论。

## [2.4.65] - 2026-10-05

- 实现连续工具调用分组折叠与分段展开（默认展示首屏 6 条，支持增量展开与一键回缩），防止海量工具调用撑爆会话视图。

## [2.4.64] - 2026-10-04

- 在 Rust 转录解析层过滤 `isApiErrorMessage` 与 `<synthetic>` 合成 429 报错气泡，修复连续用户重试输入被吞缺陷并优化系统错误警示条样式。

## [2.4.63] - 2026-10-04

- 补全 Claude Code 深度思考与上下文压缩流式状态遥测，实现发送期间输入框全生命周期锁定与回合碰撞/网络异常时的乐观消息自动回滚。

## [2.4.62] - 2026-10-04

- 实现同工作区会话状态吸收合并与 `realSessionId` 动态更新，修复侧边栏幽灵执行中会话分身及上游 429 配额耗尽导致的回合死锁。

## [2.4.61] - 2026-10-04

- 规范化 `NativeSessionHost` 会话内部前缀（`claude_` / `antigravity_` / `grok_` / `sess_`），根治宿主内存状态与磁盘观测前缀失配导致的同名会话分身。

## [2.4.60] - 2026-10-04

- 打通 CC Switch 活跃数据库路由与上游 `/v1/models` 实时模型探测，消除静态模型缓存阻断并在下拉展开或窗口切回时自动刷新。

## [2.4.59] - 2026-10-04

- 实现会话首条指令自适应智能命名、前缀规范化交叉索引、无转储残留僵尸会话淘汰与 LocalStorage 孤儿缓存自动清洗。

## [2.4.58] - 2026-10-02

- 修复已有会话续聊模型参数丢失与第三方网关 429 递归重试风暴，支持流式重试与配额耗尽状态在 UI 透明反馈。

## [2.4.57] - 2026-10-01

- 修复新建会话异步拉起瞬态路由假死、成功回合静默退出误报失败与多 Provider 绑定识别偏差，整合官方真实模型动态目录。

## [2.4.56] - 2026-10-01

- 新建会话跳过磁盘探测实现零延迟启动；补齐 ACP 工具特化渲染（提问交互、Web 搜索/抓取卡片、超长输出折叠）与通透白底品牌图标重绘。

## [2.4.55] - 2026-10-01

- 修复新建 Claude 会话误带未初始化随机 UUID 执行 `--resume` 导致的报错，支持无效参数自愈回退与侧边栏精简。

## [2.4.54] - 2026-09-29

- 新增 `WindowControls.tsx` 优化无边框窗口拖拽与标题栏控件，打通 Codex 动态模型发现并重构 Remote 工作区管理。

## [2.4.53] - 2026-09-29

- 移动端 UI 全面去 Demo 化并迁移至暖中性色视觉体系，完成多底色对比度排查与无障碍触控修复。

## [2.4.52] - 2026-09-28

- 对标 Claude Desktop 重构顶栏工具条（应用主菜单 `≡`、侧边栏折叠 `◫`、前进/后退导航）并升级进行中会话实时状态反馈。

## [2.4.51] - 2026-09-28

- 优化 `services/api/src/remote-store.ts` `commandHistory` 按会话列走复合索引查询，消除长时运行后的全表扫描放大点。

## [2.4.50] - 2026-09-28

- 过滤 `~/.claude/projects/**/*.jsonl` 中无实质消息的空会话文件，完成投产前性能优化与真实会话恢复边界修复。

## [2.4.49] - 2026-09-28

- 修正 `turn_state` 与 `execution_state` 对字符串与 JSON 状态对象的双格式解析，根治误报回合正在运行导致无法继续输入的问题。

## [2.4.48] - 2026-09-28

- 新增 `crates/runner/src/claude_gateway.rs` 自动探测 CC Switch / Claude-3p 本地网关配置与路由模型映射，支持开启路由时无缝接管会话。

## [2.4.47] - 2026-09-27

- 新增 `classifyTool` 多维工具分类（`terminal`/`file_edit`/`file_read`/`interactive`）、差异比对展示、交互式提问卡片与富上下文审批卡片。

## [2.4.46] - 2026-09-27

- 重构 `discover_claude_binary()` 为候选收集、版本探测与择优回退机制，避免使用旧版 CLI 恢复新版创建的会话。

## [2.4.45] - 2026-09-26

- 重写 `ContextCapacityPopover` 仅展示 Provider 实测上报的分类 Token 计数，新增思考强度控制与认证失败错误归类。

## [2.4.44] - 2026-09-26

- 移除 `sessionDisplayTitle` 将缺失标题回退为工作区目录名的冗余逻辑（统一回退为短 ID），并支持会话自定义重命名。

## [2.4.43] - 2026-09-26

- 修正远程连接状态指示灯逻辑：当 `snapshot.realtime_connected` 为真时正常亮绿灯（`online`），仅在降级轮询或断连时提示警告。

## [2.4.42] - 2026-09-26

- 侧边栏工作区目录优先按所含有效会话数降序排列（`b.totalCount - a.totalCount`），同数量按字母序平局决胜。

## [2.4.41] - 2026-09-26

- 自动过滤集成测试通过 `os.tmpdir()` 产生的临时工作区（`agenthub-native-*`、`agenthub-remote-*`），防止测试目录污染侧边栏。

## [2.4.40] - 2026-09-26

- 侧边栏工作区目录默认保持折叠（`collapsed`）并记忆展开状态（搜索时自动展开命中项），修复启动加载光圈样式缺陷。

## [2.4.39] - 2026-09-26

- 默认主题统一对齐为清爽亮色模式（`[data-theme="light"]`），修复读取中与会话空状态在浅色背景下的对比度问题。

## [2.4.38] - 2026-09-26

- 树状侧边栏引入会话级（默认前 8 条，单次增量 +16 条）与工作区级渐进分页加载，避免海量历史会话一次性渲染卡顿。

## [2.4.37] - 2026-09-26

- 清理测试遗留伪造设备名并将本机历史碎片设备 ID 统一归集至唯一本机节点，静默隐藏无会话空设备。

## [2.4.36] - 2026-09-26

- 侧边栏重构为设备 -> Agent -> 工作区 -> 会话 4 级文件夹层级树，支持各层级折叠记忆与设备在线状态指示。

## [2.4.35] - 2026-09-25

- 抽离纯函数 `session-tree.ts`（含 12 项单测）构建四层会话树与顶部面包屑导航，支持按发现元数据判定 Provider 与工作区分组。

## [2.4.34] - 2026-09-25

- Web 端在 Host 换代（`host_generation` 变更）后自动按新代次重连已选会话并触发有界历史回放。

## [2.4.33] - 2026-09-25

- Daemon 为缺少原生历史回放的 Provider 生成有界会话快照并沿远程事件链路下发，修复受控会话恢复后网页空白问题。

## [2.4.32] - 2026-09-25

- 加宽 Web 远程会话阅读区与输入框，输入框新增实测上下文容量入口，连续工具事件按组折叠展示并强化本机路径脱敏提示。

## [2.4.31] - 2026-09-25

- Web 登录页与远程续聊界面视觉对齐，侧栏采用设备/工作区/会话层级；修复账户测试固定过期日期断言失败问题。

## [2.4.30] - 2026-09-25

- 完成 Web Console 响应式抽屉与 Android Jetpack Compose 原生对话界面双端移动端 UI 重构与全端版本同步。

## [2.4.19] - 2026-09-19

- 修复 `load_session_conversation` 传入前缀化会话 ID（如 `claude_<uuid>`）未剥离前缀导致 Claude Code 历史会话解析为空的缺陷。

## [2.4.18] - 2026-09-19

- 将 Google Antigravity 与 xAI Grok Build 接入后端统一 Remote 控制流，支持真实全双工远程接管与流式事件同步。

## [2.4.17] - 2026-09-18

- 设置面板新增插件商城（Plugin Marketplace），将实验性 Agent 驱动收敛至插件中心按需启用。

## [2.4.16] - 2026-09-18

- 修复 ZCode app-server 无头模式启动时内存模型目录为空导致自定义模型历史会话恢复报错本机会话不可用的缺陷。

## [2.4.15] - 2026-09-17

- 完成三套主题语义化配色重构（37 个语义令牌 × 3 主题），消除侧边栏徽章、表格与代码块二重边框对比度问题。

## [2.4.1] - 2026-09-15

- 新增 `ZCodeAppServer`（`crates/runner/src/collectors.rs`）直驱 `zcode.cjs app-server`，实现 ZCode 全双工远程控制与流式输出。

## [2.3.6] - 2026-09-14

- Web 端根路径 `/` 移除旧版 `LegacyPanelPage` 监控大屏，默认直出 Remote Console 并接入 Runner 在线指示灯与设备绑定入口。

## [2.3.5] - 2026-09-12

- Web/Android 对齐 Remote 创建状态机（HTTP 202 `queued` 显示为等待绑定，待真实 binding 或 receipt 终态后再收敛），统一将 `ws-*` 作为不透明工作区 ID 展示。

## [2.3.4] - 2026-09-10

- 对齐 Web/Android v3 Remote 创建、恢复、续聊、审批与中断契约文档，明确 `queued -> accepted -> session-bound` 状态流转与 `ws-*` 隐私边界。

## [2.2.0] - 2026-09-02

- 新增 `list_agent_models` 从本机 Agent 实测会话聚合可用模型列表（去硬编码），并在会话 Composer 支持模型、权限与上下文附件配置。

## [2.1.0] - 2026-09-01

- 支持多 Agent 模型动态切换与记忆持久化，新增暗色 `SelectDropdown` 组件与会话右键快捷菜单。

## [2.0.8] - 2026-09-01

- 发布 AgentHub 2.0.8 Windows x64 安装包（MSI 与 NSIS Setup）。

## [2.0.7] - 2026-09-01

- 发布 AgentHub 2.0.7 Windows x64 安装包（MSI 与 NSIS Setup）。

## [2.0.6] - 2026-09-01

- 重构双栏低噪音工作台 UI，引入暖纸色主舞台、深色导航与 Provider 信号色体系，并发布 2.0.6 Windows x64 安装包。

## [2.0.5] - 2026-09-01

- 侧边栏按 Provider 供应商分组组织会话，支持快捷搜索、前后导航与工作台模式切换。

## [2.0.4] - 2026-09-01

- 登录界面新增重新打开授权网页与取消重登控制面板，解决误关浏览器授权页后客户端阻塞问题。

## [2.0.3] - 2026-09-01

- Windows 端改用原生 `ShellExecuteW` API（`shell32.dll`）直调默认浏览器，消除 `cmd`/`powershell` 子进程黑框闪烁。

## [2.0.2] - 2026-09-01

- 修复 Windows 下 `cmd /c start` 调起浏览器时 `&` 字符截断 OAuth URL 查询参数的缺陷。

## [2.0.1] - 2026-09-01

- 桌面端 Rust 实现 RFC 8252 §7.3 本地临时回环端口（`http://127.0.0.1:<port>/callback`）PKCE 静默登录，修复 `TgOAuthClient` 空环境变量回退。

## [2.0.0] - 2026-09-01

- 落地 AgentHub v2.0 统一多 Agent 远程控制平面全栈基线，在 `packages/contracts` 实现 v2 Frame Envelope（`seq`/`ack_seq`/`hub_session_id`/`cmd_id`/`is_replay`）与端到端同步链路。
