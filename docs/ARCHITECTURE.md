# AgentHub 系统架构

更新：2026-10-19。

## 1. 架构拓扑

AgentHub 只有一个会话真相源：运行在开发机上的 `agenthubd`（`agenthub-runner`）。远端控制不创建第二个 Runtime，网络、客户端与执行状态相互独立。

```text
┌──────────────────────────── 开发机 ────────────────────────────┐
│  agenthub-runner (agenthubd) ── Provider Adapter ──► Runtime   │
│    ├─ 内嵌控制台 (http://127.0.0.1:4200)             Codex     │
│    ├─ 本地 IPC v2 (Named Pipe / Unix Socket)         Claude    │
│    ├─ Session Registry / Ownership / Command Ledger  ACP ...   │
│    └─ Usage Ledger & Capability Library (只读聚合)             │
└──────────────────────────────┬─────────────────────────────────┘
                               │ 主动出站 TLS / WebSocket
                               ▼
                         ┌── Gateway ──┐
                         │ 鉴权 / 路由 │
                         │ 账本 / 回放 │
                         └──────┬──────┘
                                ▼
                    Web 控制端 (PC / 手机 PWA)
                    [原生 Android 端暂时冻结]
```

## 2. 组件职责

### 2.1 `agenthubd` (`agenthub-runner`)

- **守护与控制核心**：通过 `agenthub-runner daemon`（或 `web` / `remote-run`）运行，独占本机 SQLite、Session Registry、写者所有权、Host generation、命令账本和事件日志；
- **断网自治**：客户端退出、登出或关闭 Remote 均不停止本地 Runtime；断网期间继续执行，Remote 开启时按持久水位补发，关闭期间不积累待上传正文；
- **单点 Transcript 解析**：历史会话预览只在 `crates/runner/src/conversation.rs` 单点有界解析，客户端与 Gateway 绝不直接解析 Provider 本地文件。

### 2.2 内嵌本机 Web 控制台 (`127.0.0.1:4200`)

- **零构建内嵌资源**：`crates/runner/resources/` 下的 `dashboard.html`、`dashboard.css`、`dashboard.js` 与 `icons/`（20 个厂商图标）经 `include_str!` / `include_bytes!` 编入二进制（debug 构建支持磁盘热重载）；
- **页面结构**：主导航包含 **会话、智能体、用量、工作区** 四页；**设置**（通用 / 外观 / 设备与网关 / 能力生态 / 关于）为第五个独立视图。路由拓扑归上游 CPA 管理，本机只读消费；
- **逐请求用量账本 (`usage_ledger.rs`)**：扫描本地会话转录（按 `message.id + requestId` 去重，区分 `claude_code` 与 `claude_desktop`，配对 10 分钟窗口内 `retried/recovered` 失败重试，测算首尾时间戳输出速度），经 `GET /api/usage/ledger` 输出分桶序列、三套排行与 53 周热力图。Token 计价唯一权威在 `usage_ledger::calc_cost`，前端只读渲染、不自带费率表；
- **联邦能力生态 (`collectors/capabilities.rs`)**：集中扫描 Claude Code、Codex、工作区及 `~/.agenthub/mcp.json` 的 Skill、MCP Server 与 Hook，支持多源去重与交互式 MCP 注册；
- **开源溯源**：设计系统、布局原语、厂商图标集、用量视图与能力资料库架构移植自上游 [magpie](https://github.com/yetone/magpie)（MIT License, Copyright (c) 2026 yetone），详见 [README](../README.md#upstream-attribution--open-source-traceability)。

### 2.3 Provider Adapter 与 Runtime

- 将 `create`、`resume`、`send_prompt`、`resolve_approval`、`interrupt` 映射为官方协议动作，隔离进程树（Windows Job Object / Unix 进程组）、超时与协议错误；
- **Codex**：常驻 `app-server`，支持 thread start/resume/read、流式通知与审批请求；
- **Claude Code**：支持历史发现、流式续聊、取消与会话亲和标头注入（保护 Prompt Cache）；
- **通用 ACP (`acp_app_server.rs`)**：标准 JSON-RPC 2.0 stdio 适配器，支持符合 ACP 规范的智能体；
- **其他来源（ZCode / Antigravity / Grok 等）**：仅作只读发现或本地用量归集，不伪造可写 attach。

### 2.4 Gateway (`services/api`) 与客户端 (`apps/web`, `apps/android`)

- **Gateway**：负责账户/设备身份、命令路由、单实例 SQLite v3 内容库（与 PostgreSQL 元数据库）及 SSE/WSS 分发；不运行代码、不持有 Provider 凭据、不在 daemon 离线时伪造成功；
- **Web / Android**：通过 Gateway 消费统一 Remote API，只能在 daemon 已授权的 `ws-*` 工作区内操作；daemon 离线时历史只读、写操作禁用。

## 3. Session 所有权与状态机

稳定映射：`(owner_id, daemon_id, provider, provider_session_id) -> hub_session_id`。

- `SessionBinding` 记录：`origin`（`managed` / `discovered`）、`control_state`（`detached` / `connecting` / `connected` / `lost`）、`provider_session_id`、授权 `workspace_id`、`host_generation`、当前 turn、待审批集合、事件水位与探测到的 capability report；
- 执行态（`unknown / idle / running / awaiting_input / awaiting_permission / error / completed / terminated`）只由原生 Runtime 证据推进；daemon 离线只改变 `Presence` 与 `control_state`，不把 `running` 篡改为 `completed`；
- 新的 `host_generation` 拒绝旧代际的过期审批和不确定命令。

## 4. 安全与数据边界

- **主动出站**：daemon 主动发起 TLS/WSS 连接，本机不开放公网入站端口；本地 IPC 仅限当前 OS 用户访问（Windows Named Pipe ACL + 拒绝远程客户端，Unix `0600`）；
- **脱敏投影**：Provider 原始事件与完整工具输出只留本机；进入 Gateway 的事件、收据、Diff（≤64 KiB 且路径脱敏为 `[local_path_redacted]`）和 `ws-*` 工作区 ID 均经严格白名单校验；
- **凭据隔离**：Provider Token 与设备私钥永不进入 Gateway、浏览器或诊断日志；设备撤销或关闭 Remote 后旧连接立即失效。
