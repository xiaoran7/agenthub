# AgentHub Runner

`agent-pulse-runner` 是当前可执行文件名，承载本机会话 daemon、设备身份、Remote 连接和只读 Provider 发现。`agenthubd` 是已经落地的进程职责；当前没有独立命名的二进制或 OS 服务安装包。

## 运行模式

- `daemon`：本机唯一持久 Session 控制面。独占设备作用域 SQLite、Codex app-server、Session Registry、命令账本和事件日志，同时服务本地 IPC 与可选 Gateway Remote。
- `remote-run`：兼容入口，带 Gateway 地址进入同一个 daemon 主流程，不创建第二种 Host。
- `remote-host`：非持久 JSONL 诊断入口，不提供本地 IPC、命令恢复或 Gateway Remote。
- `run` / `collect-once`：迁移期只读发现与元数据同步，覆盖 Claude、Codex、Grok 和 Antigravity 适配器。
- `device` / `sync` / `outbox`：设备绑定、签名批次和本地事件队列诊断。
- `auth`：显式 opt-in 的本地兼容能力；默认不读取 Provider 凭据。

精确子命令和参数以 `cargo run -p agent-pulse-runner -- --help` 及各子命令 `--help` 为准，不在 README 复制完整 CLI 手册。

## 最短验证

从仓库根运行：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

初始化开发设备身份并查看命令：

```powershell
cargo run -p agent-pulse-runner -- device init --identity runner-identity.json
cargo run -p agent-pulse-runner -- --identity runner-identity.json device --help
```

身份文件包含设备私钥，只能保存在当前用户可读目录，不得提交、上传或跨设备复用。

## daemon 示例

```powershell
cargo run -p agent-pulse-runner -- --identity runner-identity.json daemon `
  --codex-binary <codex可执行文件> `
  --workspace D:\Projects\Example `
  --db runner.db
```

Desktop 使用设备 ID 派生的当前用户 IPC endpoint；手工启动时通常无需传 `--ipc-endpoint`。Windows 可配置官方 npm `codex.cmd`/`codex.ps1` shim，Runner 只解析已知 `@openai/codex` 包布局并直接启动原生 `codex.exe`，不执行 shell。

## Remote 示例

完成设备绑定后，可为明确授权的工作区启动 Remote：

```powershell
cargo run -p agent-pulse-runner -- --identity runner-identity.json remote-run `
  --codex-binary <codex可执行文件> `
  --workspace D:\Projects\Example `
  --db runner.db `
  --server wss://<Gateway域名>/v3/remote/ws
```

非回环地址必须使用 WSS。journal 绑定设备，并在首次开启 Remote 时绑定一个 Gateway；已有历史不能切换到另一 Gateway。关闭 Remote 不停止本地 daemon 或 Runtime。daemon 重启后既有绑定先标记 detached，必须通过 `thread/read` 验证后显式 resume。当前不支持任意 attach 到由其他官方客户端持有的可写会话。

## 隐私与执行边界

- 只读发现只同步白名单元数据，不保留 Prompt、消息、命令、工具载荷、完整路径、transcript、rollout、Token 或 Cookie。
- managed Remote 可以在用户授权后同步消息正文、工具完成状态和 opaque 审批描述符；完整 Provider 事件、收据、工具命令/参数/输出只保存在本机。
- 原始凭据不得进入 Gateway、Outbox 或诊断日志。`auth token --reveal` 只允许显式本地操作。
- `discover`、`resume`、`attach` 和新建 Session 是不同语义；发现 PID、JSONL 或状态库不代表取得控制权。
- 远程命令必须幂等；排队、Provider 接受和执行完成是不同状态，未知结果先对齐再处理。

## 文档

- [系统架构](../../docs/ARCHITECTURE.md)：Host、Provider、Gateway 与客户端边界。
- [Remote 协议](../../docs/SYNC_PROTOCOL.md)：身份、序列、ACK、重连和命令状态。
- [部署](../../docs/DEPLOYMENT.md)与[已知问题](../../docs/PITFALLS.md)。
- [仓库入口](../../README.md)与[文档索引](../../docs/README.md)。
