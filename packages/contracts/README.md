# AgentPulse Contracts

Runner 与 Device Gateway 共享的同步协议版本 1 TypeScript 实现。

当前提供：

- 严格字段白名单和深层未知字段拒绝
- 设备 ID 与 Ed25519 公钥关联校验
- Rust 字段顺序兼容的签名字节生成
- Ed25519 批次验签
- Agent、状态、来源、等待原因和载荷类型
- Node 生成、Rust 验证的跨语言测试向量

```powershell
npm.cmd run typecheck --workspace @agent-pulse/contracts
npm.cmd test --workspace @agent-pulse/contracts
```

重新生成固定测试向量：

```powershell
npm.cmd run generate:test-vector --workspace @agent-pulse/contracts
```

生成脚本中的密钥只用于公开测试向量，不得用于真实设备。
