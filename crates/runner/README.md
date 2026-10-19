# AgentHub Runner (`agenthub-runner`)

`agenthub-runner` 是本机唯一会话控制面（`agenthubd`），独占设备 SQLite、Session Registry、命令账本和事件日志，同时提供内嵌 Web 控制台（`127.0.0.1:4200`）、本地 IPC v2 与可选 Gateway Remote。

## 常用子命令

```powershell
# 启动内嵌本机 Web 控制台（默认 http://127.0.0.1:4200）
cargo run -p agenthub-runner -- web

# 启动常驻守护进程
cargo run -p agenthub-runner -- --identity runner-identity.json daemon `
  --codex-binary <codex可执行文件> --workspace D:\Projects\Example --db runner.db

# 连接远程 Gateway（非回环地址必须使用 wss://）
cargo run -p agenthub-runner -- --identity runner-identity.json remote-run `
  --codex-binary <codex可执行文件> --workspace D:\Projects\Example `
  --db runner.db --server wss://<Gateway域名>/v3/remote/ws

# 初始化设备身份（私钥仅限当前用户可读，严禁提交或跨机复用）
cargo run -p agenthub-runner -- device init --identity runner-identity.json
```

其余子命令（`remote-host`、`run`、`collect-once`、`device`、`sync`、`outbox`、`auth`）以 `cargo run -p agenthub-runner -- --help` 为准。

## 验证

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

架构、隐私与协议约束见 [ARCHITECTURE](../../docs/ARCHITECTURE.md)、[SYNC_PROTOCOL](../../docs/SYNC_PROTOCOL.md) 与 [PITFALLS](../../docs/PITFALLS.md)。
