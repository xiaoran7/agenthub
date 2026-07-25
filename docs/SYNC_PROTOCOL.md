# 设备同步协议

> 当前文档描述 Runner 已实现的协议版本 1。它是云端 Device Gateway 的输入契约，
> 不是已经上线的网络服务。

## 1. 设备身份

Runner 使用 Ed25519 密钥对：

- 私钥只保存在本地身份文件。
- 公钥随同步批次发送，供首次绑定和签名验证使用。
- `device_id = "dev_" + hex(SHA-256(public_key)[0..16])`。
- 项目路径 HMAC 密钥由私钥和固定域分隔标签派生，不进入同步批次。

`device init` 使用仅新建语义，不覆盖已有身份。Unix 新文件权限为 `0600`；Windows
当前继承身份文件所在目录的 ACL，因此身份应放在仅当前用户可读的应用数据目录。

## 2. SyncBatch

批次顶层字段：

| 字段 | 含义 |
| --- | --- |
| `protocol_version` | 当前固定为 `1` |
| `batch_id` | 每个发送批次的 UUID |
| `device_id` | 由公钥派生的设备 ID |
| `public_key` | 32 字节 Ed25519 公钥的十六进制编码 |
| `created_at` | Runner 创建批次的 RFC 3339 时间 |
| `first_sequence` | 本批次首个 Outbox 序列 |
| `last_sequence` | 本批次最后一个 Outbox 序列 |
| `events` | 1 到 1000 个连续、升序事件 |
| `signature` | 64 字节 Ed25519 签名的十六进制编码 |

每个事件是固定结构：

```text
sequence
event_id
schema_version
device_id
agent_type
agent_session_id
event_type = session_observed
observed_at
source
confidence
adapter_version
payload
```

`payload` 只包含项目显示名与指纹、统一状态、白名单原始状态、等待原因以及允许的时间
字段。它不包含 Prompt、Assistant 消息、命令、工具输入输出、代码、Diff、完整路径、
环境变量、transcript、Agent 名称、Thread 标题或任务摘要。

## 3. 签名

签名输入是删除 `signature` 字段后，按上述结构声明顺序生成的紧凑 UTF-8 JSON。
所有 Map 按键名升序。签名覆盖批次元数据和全部事件。

验证顺序：

1. 拒绝未知 `protocol_version`。
2. 验证事件非空、设备一致、序列连续且首末序列匹配。
3. 从公钥重新派生 `device_id` 并比较。
4. 验证 Ed25519 签名。
5. 执行服务端字段白名单和业务约束。

版本 1 已包含由 TypeScript 生成、Rust Runner 验证的固定跨语言签名向量，Node 与
Rust 的质量门都会读取同一 Fixture。任何规范化规则变化都必须升级协议版本，不能让
相同版本产生不同签名字节。

## 4. SyncAck

服务端成功持久化事件后返回：

```json
{
  "protocol_version": 1,
  "batch_id": "00000000-0000-0000-0000-000000000000",
  "device_id": "dev_example",
  "accepted_through": 42
}
```

Runner 只有在以下条件全部成立时才清理 Outbox：

- 协议版本受支持。
- `batch_id` 和 `device_id` 与已发送批次一致。
- `accepted_through` 位于 `[first_sequence, last_sequence]`。
- 原批次签名仍然有效。

重复确认是幂等操作，第二次删除数量为零。服务端不得确认尚未持久化或未包含在批次中
的序列。

## 5. 尚未实现

- 正式账号认证下的一次性绑定码生成、过期和持久化
- TLS WebSocket Device Gateway
- 服务端公钥注册、撤销和轮换
- 心跳、离线判断与重连
- 在途批次持久化
- Windows ACL 主动加固或系统密钥库托管

开发期 Gateway 已实现内存型一次性绑定码、公钥注册、批次验签、幂等接收和有界确认
响应；上述清单中的“绑定”指正式账号认证、过期、撤销和持久化流程。
