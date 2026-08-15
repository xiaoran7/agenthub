# AgentPulse Runner

本地只读 Session 元数据采集器。目前实现：

- Claude 本地 Session JSONL 元数据发现
- Claude 生命周期 Hook 标准化
- Codex app-server Thread 与实时事件标准化
- Codex 生命周期 Hook 标准化
- Grok Build `summary.json` / `active_sessions.json` 元数据发现
- Grok Build 生命周期 Hook 标准化
- Claude 配置目录一次性 Session 采集（无需 Claude CLI）
- Codex app-server stdio 握手与一次性 Thread 采集；不可执行时只读本地状态库兜底
- Grok Build 一次性 Session 采集（无需 `grok` 在 PATH 中）
- Codex app-server 被动状态通知采集与轮询状态合并
- Claude/Codex 持续采集、活动/空闲自适应间隔与失败指数退避
- 采集命令和协议响应强制超时；Windows 下终止完整子进程树
- 适配器健康状态持久化和安全诊断错误码
- 工作目录显示名与 HMAC 指纹脱敏
- SQLite Outbox、事件顺序、幂等和 Session 快照
- Ed25519 本地设备身份、签名同步批次和有界确认
- 独立后台同步、启动排空、新事件唤醒与网络失败指数退避
- Fixture 驱动的隐私回归测试
- Pro/Team 远程任务主动领取、本机项目指纹 allow-list、本地确认与 Claude/Codex/Grok CLI 执行

## 远程任务

任务 Worker 只建立出站连接。每个项目必须在本机显式配置指纹到路径的映射；Gateway
只看到指纹，不接收路径。当前只支持本地确认策略，提示词会在终端完整显示，并通过
stdin 交给 Agent CLI，不进入命令行参数：

```powershell
cargo run -p agent-pulse-runner -- tasks work `
  --identity runner-identity.json `
  --server http://127.0.0.1:3001 `
  --project <64位项目指纹>=D:\Projects\Example
```

Runner 在 `delivered`、`accepted` 或 `waiting_user` 阶段重启可安全恢复同一任务；若在
`running` 后中断，下次领取会回报 `execution_interrupted`，不会自动重复执行可能已产生
修改的提示词。`--once` 只轮询一次，供诊断与自动化验证。

## 开发命令

```powershell
$env:AGENT_PLATFORM_FINGERPRINT_KEY = 'local-development-only'

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

初始化设备身份：

```powershell
cargo run -p agent-pulse-runner -- `
  device init `
  --identity runner-identity.json
```

身份文件包含设备私钥，必须保存在仅当前用户可读的目录中，不得提交到 Git、上传或
与其他设备共用。重复初始化不会覆盖已有文件。后续命令传入全局
`--identity runner-identity.json` 后，会自动使用该设备 ID，并由设备私钥派生项目
指纹密钥；也可以设置 `AGENT_PLATFORM_IDENTITY`。

标准化 Claude Agent 列表 Fixture：

```powershell
cargo run -p agent-pulse-runner -- `
  --identity runner-identity.json `
  normalize claude-agents `
  --input crates/runner/fixtures/claude-agents.json
```

接收 Claude Hook 并写入本地 Outbox：

```powershell
Get-Content -Raw crates/runner/fixtures/claude-hook-permission.json |
  cargo run -p agent-pulse-runner -- `
    --device-id local-dev `
    ingest-hook claude `
    --db runner.db
```

从本机 Claude、Codex 或 Grok Build 采集一次，并写入本地 Outbox：

```powershell
cargo run -p agent-pulse-runner -- `
  --identity runner-identity.json `
  collect-once claude `
  --db runner.db

cargo run -p agent-pulse-runner -- `
  --device-id local-dev `
  collect-once codex `
  --db runner.db `
  --limit 100

cargo run -p agent-pulse-runner -- `
  --identity runner-identity.json `
  collect-once grok `
  --db runner.db
```

Claude 默认扫描 `CLAUDE_CONFIG_DIR`，未设置时扫描 `~/.claude/projects`，不要求
`claude` 在 `PATH` 中；`--include-completed` 会把最大历史读取量从 100 提高到 1000。
扫描只反序列化顶层 Session ID、工作目录与时间，不保留消息或工具载荷。Codex 优先
使用 app-server；其可执行文件不在 `PATH` 时可通过 `--binary` 指定，启动失败则自动
只读查询 `CODEX_HOME`（默认 `~/.codex`）最新状态库的 Thread 元数据。该兜底不读取
rollout 正文或 `auth.json`。Grok Build 扫描 `GROK_HOME`（默认 `~/.grok`）下的
`summary.json` 与 `active_sessions.json`，不读取对话正文、`auth.json` 或
`last_turn_summary`。一次性采集默认在 15 秒后超时，可通过
`--timeout-seconds` 调整。

持续运行两个 Adapter：

```powershell
$env:AGENT_PLATFORM_GATEWAY_URL = 'http://127.0.0.1:3001'

cargo run -p agent-pulse-runner -- `
  --identity runner-identity.json `
  run all `
  --db runner.db
```

默认有活动 Session 时每 2 秒采集，无活动时每 10 秒采集；失败后按
2、4、8……秒退避，最长 60 秒。可以通过 `--active-interval-seconds`、
`--idle-interval-seconds`、`--failure-initial-seconds` 和
`--failure-max-seconds` 调整。每次采集默认最多等待 15 秒，可通过
`--collector-timeout-seconds` 调整。`Ctrl-C` 会请求 Runner 安全退出。

设置 `AGENT_PLATFORM_GATEWAY_URL`（或 `run --sync-server`）后，`run` 会启动独立
同步线程：启动时先尝试排空上次进程留下的事件，采集到新事件时立即唤醒；成功后
默认每 5 秒检查一次，失败按 2、4、8……秒退避到 60 秒。同步线程使用独立 SQLite
连接，不会让 15 秒网络超时阻塞 Claude/Codex 采集。每轮仍受单批事件数和最大批次
数约束，只有合法 ACK 才清理 Outbox；可通过 `--sync-interval-seconds`、
`--sync-failure-initial-seconds`、`--sync-failure-max-seconds`、`--sync-limit`、
`--sync-max-batches` 和 `--sync-timeout-seconds` 调整。启用后台同步必须同时传入
`--identity` 或设置 `AGENT_PLATFORM_IDENTITY`，确保采集与签名使用同一个设备 ID。
若数据库只有其他设备 ID 的旧事件，后台日志会报告
`sync_pending_device_mismatch` 并退避，不会把“有积压但无可发送批次”写成成功。
传输失败期间的每条后台同步报告都会包含当前磁盘 `pending_events`，便于确认长期
离线时积压仍在增长而非丢失。

远程 Gateway 使用私有 CA 时，把根证书 PEM 文件传给
`AGENT_PLATFORM_GATEWAY_CA_CERT`（或 `--gateway-ca-cert`）。该选项适用于 `run`、
`device bind` 和 `sync push`，仍会严格验证证书链、主机名和有效期，不提供跳过
证书校验的开关：

```powershell
$env:AGENT_PLATFORM_GATEWAY_URL = 'https://agent.example.internal'
$env:AGENT_PLATFORM_GATEWAY_CA_CERT = 'C:\ProgramData\AgentPlatform\gateway-ca.pem'
```

Codex Runner 会处理当前 app-server 连接收到的状态、Turn、审批和用户输入通知，
并与 `thread/list` / `thread/loaded/list` 的轮询结果合并。为保持只读，它不会调用
`thread/resume` 或 `thread/start`；因此其他客户端已启动的 Thread 是否会向这条连接
发送完整实时通知，仍需在可执行的真实 Codex CLI 环境中验证，轮询始终作为兜底。
Runner 在初始化时退出消息、推理、计划、Diff、命令输出等内容型通知，避免读取不属于
Session 元数据的载荷。

Claude Runner 不再调用并不存在于当前官方 CLI 的 `claude agents --json`。磁盘发现的
Session 只标记为持久化历史；工作中、等待输入、等待审批等实时状态必须来自 Hook 或
后续受支持的官方事件接口。

只运行一个 Adapter：

```powershell
cargo run -p agent-pulse-runner -- `
  --device-id local-dev `
  run codex `
  --db runner.db `
  --codex-binary C:\Tools\codex.exe
```

诊断或测试时可使用 `--max-attempts N` 自动停止。这个数字表示 Adapter 尝试次数；
`run all --max-attempts 2` 通常会让 Claude 和 Codex 各尝试一次。

查看 Outbox：

```powershell
cargo run -p agent-pulse-runner -- outbox stats --db runner.db
cargo run -p agent-pulse-runner -- `
  outbox pending --db runner.db --device-id local-dev
cargo run -p agent-pulse-runner -- outbox health --db runner.db
```

健康状态只保存 `executable_not_found`、`permission_denied`、
`protocol_invalid_json` 等固定错误码，不保存可执行文件路径或原始系统错误文本。
状态未变化的轮询不会重复写入 Outbox。

准备并验证签名同步批次：

```powershell
cargo run -p agent-pulse-runner -- `
  sync prepare `
  --identity runner-identity.json `
  --db runner.db `
  --limit 100

cargo run -p agent-pulse-runner -- `
  sync verify `
  --batch batch.json
```

`sync prepare` 只输出白名单 Session 元数据，不输出设备私钥。服务端确认写入文件后，
可用 `sync ack --db runner.db --batch batch.json --acknowledgement ack.json`
清理已确认事件。确认的设备、批次和序列必须落在实际发送范围内；重复确认是幂等的。
协议字段和签名规则见 [SYNC_PROTOCOL.md](../../docs/SYNC_PROTOCOL.md)。

连接开发期 Gateway：

```powershell
  $env:AGENT_PLATFORM_GATEWAY_URL = 'http://127.0.0.1:3001'
  $env:AGENT_PLATFORM_BINDING_CODE = '<从 Web 设置面板取得的一次性绑定码>'

cargo run -p agent-pulse-runner -- `
  device bind `
  --identity runner-identity.json

cargo run -p agent-pulse-runner -- `
  sync push `
  --identity runner-identity.json `
  --db runner.db `
  --limit 100 `
  --max-batches 10
```

绑定码由 Gateway 的 Web 设置面板或受认证的 `POST /v1/device-bindings` 签发，
默认 10 分钟有效且只显示一次。建议通过环境变量提供，避免进入 Shell 历史。
私有 CA 部署还需设置 `AGENT_PLATFORM_GATEWAY_CA_CERT`；公共 CA 部署不设置即可。
`sync push` 保留为诊断和一次性
排空命令，每次只处理有界数量的批次；Gateway 返回的确认通过本地校验后才会删除
Outbox 事件。网络失败、超时或无效响应不会清理未确认事件。远程 Gateway 地址必须
使用 HTTPS，明文 HTTP 只允许 `localhost`、IPv4 或 IPv6 回环地址用于本地开发。

Windows 用户可以使用 [`deploy/windows`](../../deploy/windows) 中的一键接入脚本：将
`agent-pulse-runner.exe`、`Connect-AgentPulse.cmd` 和 `Connect-AgentPulse.ps1` 放在同一目录，
双击 `.cmd` 后输入 Web 页面生成的一次性连接码。脚本会自动初始化设备身份、绑定 Gateway、
创建本地 SQLite Outbox 并启动持续同步。

## 本地认证状态与配额

凭据读取是显式 opt-in 能力，默认关闭。启用后 Runner 只读取当前用户允许的 Claude Code
与 Codex 本地凭据来源，并将原始 token 保存在设备本地的 AES-GCM 加密缓存中；上传到 Gateway
的认证同步批次只包含状态、来源、脱敏账号、过期时间和可用配额窗口，不包含 token、cookie、
环境变量值或凭据文件正文。

```powershell
cargo run -p agent-pulse-runner -- `
  --allow-credential-access `
  --identity runner-identity.json `
  auth status claude

cargo run -p agent-pulse-runner -- `
  --allow-credential-access `
  --identity runner-identity.json `
  auth quota codex
```

如果 CLI 不在 `PATH`，在 `auth` 子命令上显式传入 `--claude-binary` 或
`--codex-binary`；这与 `run` 的 provider binary 覆盖选项相互独立。

`auth refresh` 会调用官方 CLI/app-server 的刷新路径；`auth login` 和 `auth logout` 是显式的
交互式本地操作。遇到 `reauth_required` 时，重新执行对应 provider 的 `auth login`。原始 token
只有在同时传入 `--allow-credential-access --reveal` 的 `auth token` 命令中输出到当前终端，
且不会进入日志、Outbox、同步批次或 Web 面板。`run` 启用凭据访问后会按
`--auth-interval-seconds`（默认 60 秒）后台刷新安全快照；后台循环不会自动启动交互式登录。

## 安全约束

标准化输出禁止包含：

- Prompt 与 Assistant 消息
- 未经明确授权的 Agent 名称、Thread 标题和任务摘要
- Shell 命令正文
- 工具输入与输出
- 完整工作目录
- transcript 或 rollout 内容
- Token、Cookie 和环境变量（除非用户显式执行本地 `auth token --reveal`）

Fixture 中带有敏感标记，测试会确认这些标记不会进入标准化事件或 Outbox。

## 尚未接入

- Codex 跨客户端实时通知覆盖验证与缺失事件补偿
- Hook 插件安装器
- 联合账号登录、设备恢复绑定、密钥轮换与 WebSocket 传输
- Windows 身份文件 ACL 主动加固
- OS 原生服务安装

这些功能将在核心协议与隐私测试稳定后逐步接入。
