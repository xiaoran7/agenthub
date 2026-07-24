# Agent-Platform Runner

本地只读 Session 元数据采集器。目前实现：

- Claude `agents --json` 状态标准化
- Claude 生命周期 Hook 标准化
- Codex app-server Thread 与实时事件标准化
- Codex 生命周期 Hook 标准化
- Claude CLI 一次性 Session 采集
- Codex app-server stdio 握手与一次性 Thread 采集
- 工作目录显示名与 HMAC 指纹脱敏
- SQLite Outbox、事件顺序、幂等和 Session 快照
- Fixture 驱动的隐私回归测试

## 开发命令

```powershell
$env:AGENT_PLATFORM_FINGERPRINT_KEY = 'local-development-only'

cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

标准化 Claude Agent 列表 Fixture：

```powershell
cargo run -p agent-platform-runner -- `
  --device-id local-dev `
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
  --device-id local-dev `
  collect-once claude `
  --db runner.db

cargo run -p agent-platform-runner -- `
  --device-id local-dev `
  collect-once codex `
  --db runner.db `
  --limit 100
```

如 CLI 不在 `PATH` 中，可通过 `--binary` 传入可执行文件路径。Claude 可使用
`--include-completed` 同时读取已完成的 Session。

查看 Outbox：

```powershell
cargo run -p agent-platform-runner -- outbox stats --db runner.db
cargo run -p agent-platform-runner -- `
  outbox pending --db runner.db --device-id local-dev
```

## 安全约束

标准化输出禁止包含：

- Prompt 与 Assistant 消息
- Shell 命令正文
- 工具输入与输出
- 完整工作目录
- transcript 或 rollout 内容
- Token、Cookie 和环境变量

Fixture 中带有敏感标记，测试会确认这些标记不会进入标准化事件或 Outbox。

## 尚未接入

- Claude/Codex 持续轮询与实时订阅
- Hook 插件安装器
- 云端 WebSocket 同步
- OS 原生服务安装

这些功能将在核心协议与隐私测试稳定后逐步接入。
