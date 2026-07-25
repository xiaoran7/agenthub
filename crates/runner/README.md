# Agent-Platform Runner

本地只读 Session 元数据采集器。目前实现：

- Claude `agents --json` 状态标准化
- Claude 生命周期 Hook 标准化
- Codex app-server Thread 与实时事件标准化
- Codex 生命周期 Hook 标准化
- Claude CLI 一次性 Session 采集
- Codex app-server stdio 握手与一次性 Thread 采集
- Codex app-server 被动状态通知采集与轮询状态合并
- Claude/Codex 持续采集、活动/空闲自适应间隔与失败指数退避
- 采集命令和协议响应强制超时；Windows 下终止完整子进程树
- 适配器健康状态持久化和安全诊断错误码
- 工作目录显示名与 HMAC 指纹脱敏
- SQLite Outbox、事件顺序、幂等和 Session 快照
- Ed25519 本地设备身份、签名同步批次和有界确认
- Fixture 驱动的隐私回归测试

## 开发命令

```powershell
$env:AGENT_PLATFORM_FINGERPRINT_KEY = 'local-development-only'

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

初始化设备身份：

```powershell
cargo run -p agent-platform-runner -- `
  device init `
  --identity runner-identity.json
```

身份文件包含设备私钥，必须保存在仅当前用户可读的目录中，不得提交到 Git、上传或
与其他设备共用。重复初始化不会覆盖已有文件。后续命令传入全局
`--identity runner-identity.json` 后，会自动使用该设备 ID，并由设备私钥派生项目
指纹密钥；也可以设置 `AGENT_PLATFORM_IDENTITY`。

标准化 Claude Agent 列表 Fixture：

```powershell
cargo run -p agent-platform-runner -- `
  --identity runner-identity.json `
  normalize claude-agents `
  --input crates/runner/fixtures/claude-agents.json
```

接收 Claude Hook 并写入本地 Outbox：

```powershell
Get-Content -Raw crates/runner/fixtures/claude-hook-permission.json |
  cargo run -p agent-platform-runner -- `
    --device-id local-dev `
    ingest-hook claude `
    --db runner.db
```

从本机 Claude 或 Codex 采集一次，并写入本地 Outbox：

```powershell
cargo run -p agent-platform-runner -- `
  --identity runner-identity.json `
  collect-once claude `
  --db runner.db

cargo run -p agent-platform-runner -- `
  --device-id local-dev `
  collect-once codex `
  --db runner.db `
  --limit 100
```

如 CLI 不在 `PATH` 中，可通过 `--binary` 传入可执行文件路径。Claude 可使用
`--include-completed` 同时读取已完成的 Session。一次性采集默认在 15 秒后超时，
可通过 `--timeout-seconds` 调整。

持续运行两个 Adapter：

```powershell
cargo run -p agent-platform-runner -- `
  --identity runner-identity.json `
  run all `
  --db runner.db
```

默认有活动 Session 时每 2 秒采集，无活动时每 10 秒采集；失败后按
2、4、8……秒退避，最长 60 秒。可以通过 `--active-interval-seconds`、
`--idle-interval-seconds`、`--failure-initial-seconds` 和
`--failure-max-seconds` 调整。每次采集默认最多等待 15 秒，可通过
`--collector-timeout-seconds` 调整。`Ctrl-C` 会请求 Runner 安全退出。

Codex Runner 会处理当前 app-server 连接收到的状态、Turn、审批和用户输入通知，
并与 `thread/list` / `thread/loaded/list` 的轮询结果合并。为保持只读，它不会调用
`thread/resume` 或 `thread/start`；因此其他客户端已启动的 Thread 是否会向这条连接
发送完整实时通知，仍需在可执行的真实 Codex CLI 环境中验证，轮询始终作为兜底。
Runner 在初始化时退出消息、推理、计划、Diff、命令输出等内容型通知，避免读取不属于
Session 元数据的载荷。

只运行一个 Adapter：

```powershell
cargo run -p agent-platform-runner -- `
  --device-id local-dev `
  run codex `
  --db runner.db `
  --codex-binary C:\Tools\codex.exe
```

诊断或测试时可使用 `--max-attempts N` 自动停止。这个数字表示 Adapter 尝试次数；
`run all --max-attempts 2` 通常会让 Claude 和 Codex 各尝试一次。

查看 Outbox：

```powershell
cargo run -p agent-platform-runner -- outbox stats --db runner.db
cargo run -p agent-platform-runner -- `
  outbox pending --db runner.db --device-id local-dev
cargo run -p agent-platform-runner -- outbox health --db runner.db
```

健康状态只保存 `executable_not_found`、`permission_denied`、
`protocol_invalid_json` 等固定错误码，不保存可执行文件路径或原始系统错误文本。
状态未变化的轮询不会重复写入 Outbox。

准备并验证签名同步批次：

```powershell
cargo run -p agent-platform-runner -- `
  sync prepare `
  --identity runner-identity.json `
  --db runner.db `
  --limit 100

cargo run -p agent-platform-runner -- `
  sync verify `
  --batch batch.json
```

`sync prepare` 只输出白名单 Session 元数据，不输出设备私钥。服务端确认写入文件后，
可用 `sync ack --db runner.db --batch batch.json --acknowledgement ack.json`
清理已确认事件。确认的设备、批次和序列必须落在实际发送范围内；重复确认是幂等的。
协议字段和签名规则见 [SYNC_PROTOCOL.md](../../docs/SYNC_PROTOCOL.md)。

## 安全约束

标准化输出禁止包含：

- Prompt 与 Assistant 消息
- 未经明确授权的 Agent 名称、Thread 标题和任务摘要
- Shell 命令正文
- 工具输入与输出
- 完整工作目录
- transcript 或 rollout 内容
- Token、Cookie 和环境变量

Fixture 中带有敏感标记，测试会确认这些标记不会进入标准化事件或 Outbox。

## 尚未接入

- Codex 跨客户端实时通知覆盖验证与缺失事件补偿
- Hook 插件安装器
- 云端设备绑定与 WebSocket 传输
- Windows 身份文件 ACL 主动加固和系统密钥库接入
- OS 原生服务安装

这些功能将在核心协议与隐私测试稳定后逐步接入。
