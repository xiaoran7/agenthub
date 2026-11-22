# AgentHub 系统架构

## 1. 总体拓扑

系统唯一会话真相源为开发机上的 `agenthubd`（`agenthub-runner`）；远端控制不启动第二套推理进程，网络抖动与客户端启停不影响本地 Runtime 执行。

```text
┌────────────────────────────── 开发机 (Workstation) ──────────────────────────────┐
│  agenthub-runner (agenthubd) ── Provider Adapter ──► Codex / Claude / agy / ACP │
│    ├─ 内嵌控制台 (:4200) + 按需隔离 `ui` WebView2 子进程 + 原生 Win32 托盘       │
│    ├─ 本地 IPC v2 (Named Pipe ACL / Unix 0600) + RemoteJournal (INCREMENTAL)    │
│    └─ UsageIndexDb 冷热分层账本 + 联邦能力生态 (Skill / MCP / Hook)              │
└────────────────────────────────────────┬─────────────────────────────────────────┘
                                         │ 主动出站 TLS / WSS (零公网入站端口)
                                         ▼
                        ┌─── Gateway (@agent-pulse/api) ───┐
                        │ Ed25519 验签 / 断线宽限 / 幂等账本│
                        │ PostgreSQL|SQLite + remote.db    │
                        └────────────────┬─────────────────┘
                                         ▼
              Web 静态前门 (Vite 8 + Nginx :3000/:3100) & Android 原生端
```

## 2. 核心子系统职责

### 2.1 `agenthubd` (`crates/runner`)
- **控制核心与断网自治**：独占本机 `RemoteJournal`、Session Registry、单写者锁与 `host_generation`；断网期间继续执行并按连续水位补发，单点有界转录解析收敛于 `crates/runner/src/conversation/`。
- **内存与进程隔离**：Linux x86_64 稳态常驻 RSS 约 `13.47 MB`；重度历史用量经 `Arc<str>` 驻留池与 `UsageIndexDb`（`>35` 天或超出单源 `256` 热文件自动溢出 SQLite）将稳态 WorkingSet 压至数 MB。Windows 桌面通过 `agenthub-runner ui` 按需拉起独立 WebView2 子进程（关闭或最小化空闲即彻底释放，由 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` 兜底回收），托盘右键采用零 WebView2 的纯 Win32 GDI 分层卡片（`tray_menu/`）。
- **代码健康门禁**：全仓 190+ 个 Rust 文件零超限（单文件 `<=800` 行），与 `packages/contracts`、`services/api`、`apps/web` 的 TypeScript/TSX 棘轮上限统一由 `scripts/check-line-limits.py` 守护。

### 2.2 内嵌本机控制台 (`127.0.0.1:4200`)
- **零外部依赖资源**：`crates/runner/resources/`（HTML/CSS/JS 与 20 枚厂商图标）编译期嵌入二进制（Debug 构建支持磁盘 mtime 热重载）；导航固定为 **会话、智能体、用量、工作区** 四主视图与独立五分区 **设置** 视图（上游模型路由拓扑由外部网关只读承载）。
- **用量与能力生态**：`usage_ledger/` 按 `message.id + requestId` 去重、配对 10 分钟内失败重试并统一在 `calc_cost` 计价；`collectors/capabilities/` 跨 Claude、Codex、Antigravity 与工作区扫描去重 Skill、MCP Server 及 Hook。视觉原语与厂商图标源自 [yetone/magpie](https://github.com/yetone/magpie)（MIT）。

### 2.3 Provider Adapters 与 ACP 插件体系
- **5 大核心原生槽位**：
  - **OpenAI Codex**：常驻 `app-server`，支持 `thread/start|resume|read`、流式通知与结构化审批。
  - **Claude Code / Claude Desktop**：区分对话式 `claude_desktop` 与 CLI `claude_code`，提供历史发现、流式续聊、取消与会话亲和头注入。
  - **Google Antigravity (`collectors/antigravity/`)**：多运行根与 Subagent 树发现、实时 daemon 模型目录探测（`discover_live_antigravity_models`），以及基于官方 `agy` 的持久化 `stream-json` 双向续聊、权限审批与中断；无 CLI 时自动降级只读发现。
  - **DeepSeek Harness**：保留原生槽位、图标与契约定义（未接入本地运行时前显式返回 `deepseek_harness_runtime_not_implemented`）。
- **Runner 权威 ACP 插件运行时 (`crates/runner/src/plugins/`)**：原 `ZCode`、`Grok Build`、`Gemini CLI` 等非核心引擎统一剥离至独立插件仓库 `agenthub-plugins`。本机 Runner 独占 `~/.agenthub/plugins/` 目录、状态文件 `state.json` 与 SHA-256 安装校验，通过标准 ACP JSON-RPC 2.0 + `agenthub/*` 扩展 RPC（`probe` / `discover` / `models` / `history`）将已启用插件动态并入智能体列表与会话启动选择器。

### 2.4 Gateway (`services/api`) 与跨端客户端 (`apps/web`, `apps/android`)
- **Gateway 存储与防抖**：`presence.ts` 提供可配置断线宽限（默认 `6000ms` 防瞬时重连闪烁）；`remote-store.ts` 与 `remote-store-validation.ts` 在入库前剥离 `ready` 帧大字段、借助 `StatementSync` 预编译缓存与 `remote_event_runner_id` 索引执行 `O(log N)` 窗口裁剪，并通过 `thinRunners()` 内存缓存与双路游标加速会话轮询；提供 `GET /healthz` 探针且 `/v3/remote/stream` SSE 支持背压 `drain` 等待。
- **Web 静态前门 (`apps/web`)**：Vite 8 产出四入口静态包（`/`、`/remote`、`/verify-email`、`/reset-password`），由 `nginx:1.29-alpine`（镜像 `96.7 MB`，常驻 `~8.4 MiB`）承担静态托管与同源反代（会话 Cookie 白名单、服务端 Bearer 注入、API `no-store`、SSE 禁缓冲）。
- **Android 原生端 (`apps/android`)**：Kotlin + Jetpack Compose 实现，与 Web 共用 `@agent-pulse/contracts/remote` 强类型契约，支持增量转录折叠（`IncrementalTranscriptFold`）与 Keystore 加密存储。

## 3. 所有权、状态机与安全边界

- **稳定映射**：`(owner_id, daemon_id, provider, provider_session_id) -> hub_session_id`。执行态仅由原生 Runtime 事件推进，Runner 离线仅更新 `Presence` 与 `control_state`，绝不篡改任务终态。
- **隐私白名单**：主动出站 TLS/WSS；物理路径映射为 `ws-{display}-{hash16}`，审批 Diff 限 `64 KiB` 且路径脱敏为 `[local_path_redacted]`；Provider 凭据与设备私钥永不离开开发机。
