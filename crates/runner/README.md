# AgentHub Runner

`agent-pulse-runner` 是当前可执行文件名，承载本机会话 Host、设备身份、Remote 连接和只读 Provider 发现。`agenthubd` 是目标架构角色；Desktop 与独立后台进程尚未完全统一前，不得宣称已实现系统级常驻 daemon。

## 运行模式

- `remote-run`：持久 Remote 入口。使用设备身份、授权工作区和本地 SQLite journal，经 WSS 连接 Gateway；支持命令去重、断线回放、审批隔离和 Codex 原生会话控制。
- `remote-host`：同一 Host 的本地 JSONL 调试入口，不提供远程持久恢复。
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

## Remote 示例

完成设备绑定后，可为明确授权的工作区启动 Remote：

```powershell
cargo run -p agent-pulse-runner -- --identity runner-identity.json remote-run `
  --codex-binary <codex可执行文件> `
  --workspace D:\Projects\Example `
  --db runner.db `
  --server wss://<Gateway域名>/v3/remote/ws
```

非回环地址必须使用 WSS。journal 绑定设备和 Gateway，不得跨身份或服务复用。停止 Runner 会失去实时控制；下次必须显式恢复。当前不支持任意 attach 到由其他官方客户端持有的可写会话。

## 隐私与执行边界

- 只读发现只同步白名单元数据，不保留 Prompt、消息、命令、工具载荷、完整路径、transcript、rollout、Token 或 Cookie。
- managed Remote 可以在用户授权后同步正文、工具事件和审批；原始 Provider 事件与统一 Envelope 分层保存。
- 原始凭据不得进入 Gateway、Outbox 或诊断日志。`auth token --reveal` 只允许显式本地操作。
- `discover`、`resume`、`attach` 和新建 Session 是不同语义；发现 PID、JSONL 或状态库不代表取得控制权。
- 远程命令必须幂等；排队、Provider 接受和执行完成是不同状态，未知结果先对齐再处理。

## 文档

- [系统架构](../../docs/ARCHITECTURE.md)：Host、Provider、Gateway 与客户端边界。
- [Remote 协议](../../docs/SYNC_PROTOCOL.md)：身份、序列、ACK、重连和命令状态。
- [部署](../../docs/DEPLOYMENT.md)与[已知问题](../../docs/PITFALLS.md)。
- [仓库入口](../../README.md)与[文档索引](../../docs/README.md)。
