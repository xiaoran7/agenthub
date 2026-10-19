# AgentHub Contracts (`@agent-pulse/contracts`)

Runner、Gateway 与客户端共享的协议校验与类型定义包（零运行时外部依赖）：

- **v1 只读发现同步 (`src/index.ts`)**：严格字段白名单与未知字段拒绝、设备 ID 与 Ed25519 公钥派生校验、跨语言确定性签名字节生成与批次验签。
- **v3 原生 Remote 契约 (`src/remote.ts`)**：`RemoteCommand`、`RemoteCommandReceipt`、`RemotePendingApproval`（含 ≤64 KiB 脱敏 `diff`）、按事件种类的 `validateRemoteEventRaw` 白名单校验，以及 `ws-{display}-{hash16}` 工作区标识校验（拒绝物理绝对路径）。

```powershell
npm.cmd run typecheck --workspace @agent-pulse/contracts
npm.cmd test --workspace @agent-pulse/contracts

# 重新生成 Rust/Node 跨语言签名测试向量（仅限公开测试密钥）
npm.cmd run generate:test-vector --workspace @agent-pulse/contracts
```
