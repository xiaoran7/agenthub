# 设备同步协议

> 当前文档描述 Runner 和开发期 Gateway 已实现的协议版本 1。它已可通过本地 HTTP
> 完成真实同步闭环，但不是已经上线的生产网络服务。

## 1. 设备身份

Runner 使用 Ed25519 密钥对：

- 私钥只保存在本地身份文件。
- 公钥随同步批次发送，供首次绑定和签名验证使用。
- `device_id = "dev_" + hex(SHA-256(public_key)[0..16])`。
- 项目路径 HMAC 密钥由私钥和固定域分隔标签派生，不进入同步批次。

`device init` 使用仅新建语义，不覆盖已有身份；写入通过同目录临时文件加不覆盖式
落位保证原子性。Unix 新文件权限为 `0600`；Windows 当前继承身份文件所在目录的
ACL，因此身份应放在仅当前用户可读的应用数据目录。

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

字段约束（网关强制执行；Runner 侧对 `raw_status` 与 detail 值在标准化时主动
钳制，其余字段由数据源天然有界）：

- UUID（`batch_id`、`event_id`）只接受小写；PostgreSQL 会把 uuid 归一化为小写，
  大写变体会破坏重放幂等。
- 字符串必须是良构 UTF-16（无孤立代理项）、无控制字符，且各有长度上限
  （`raw_status` ≤ 128，`agent_session_id` ≤ 512，显示名 ≤ 256）。
- `raw_status_detail` 若出现必须含 1–16 个条目；键限定
  `^[a-z][a-z0-9_]{0,63}$`，值 ≤ 256 字符。空对象非法（Rust 侧空 map 直接省略
  字段，空对象只可能来自签名可塑性攻击面）。
- 双方反序列化均拒绝未声明字段（Rust `deny_unknown_fields` / Node 字段白名单），
  签名保护的是结构完全匹配的字节，不是已知字段投影。

## 3. 签名

签名输入是删除 `signature` 字段后，按上述结构声明顺序生成的紧凑 UTF-8 JSON。
所有 Map 按键名 **Unicode 码点升序**（等价于 UTF-8 字节序；Rust `BTreeMap`
的自然序）。禁止使用 locale 相关比较（如 `localeCompare`）——detail 键字符集
已限定为小写 ASCII，使 UTF-16 码元序与码点序在合法键上不可能分歧。签名覆盖
批次元数据和全部事件。

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

## 5. HTTP 传输

Runner 使用以下接口：

- `POST /v1/devices/register`：提交一次性绑定码、设备 ID、公钥和持有证明签名。
  签名输入是 `{binding_code, device_id, public_key}` 按此声明顺序的紧凑 UTF-8
  JSON，由设备私钥 Ed25519 签名；网关验签失败拒绝注册，公开可见的公钥无法被
  第三方抢注。
- `POST /v1/sync/batches`：提交完整 `SyncBatch` 并接收 `SyncAck`。

绑定码长度和控制字符在发出请求前校验。手工 `sync push` 与 `run` 的后台同步共用
同一套有界批次和 ACK 校验；只在响应能反序列化且通过第 4 节全部确认约束后清理
Outbox。4xx/5xx、超时、连接失败、TLS 失败、无效 JSON 或设备不匹配均保留原事件，
Runner 只报告稳定错误分类，不回显服务端响应正文。

后台同步启动时立即重放磁盘 Outbox，采集到新事件后立即唤醒；失败按独立指数退避，
退避期间不会停止 Agent 采集。唤醒信号是容量为 1 的合并通知，长时间离线不会因每次
轮询都排队一个通知而无界占用内存。当前不持久化“正在发送的 batch_id”；进程在响应
到达前退出时，下次会为相同事件生成新批次，服务端仍按事件 ID 幂等接收。

生产和远程地址必须使用 HTTPS。仅为本地开发允许 `localhost`、IPv4 回环网段和
IPv6 回环地址使用 HTTP；URL 不允许嵌入用户名、密码、查询参数或片段。客户端不
跟随任何 HTTP 重定向，避免请求被转发到未经上述校验的地址。

## 6. 尚未实现

- 正式账号认证下的一次性绑定码生成、过期和持久化（当前一次性语义仅在网关进程
  内存中，重启后失效）
- TLS WebSocket Device Gateway
- 服务端公钥撤销和轮换
- 心跳与可靠离线判断
- 在途批次持久化
- Windows ACL 主动加固或系统密钥库托管
- SyncAck 的服务端签名（文件式 `sync ack` 诊断流程因此只应在本地信任环境使用）
- Outbox 事件与会话快照的本地保留/清理策略（长期不同步时数据库单调增长）

开发期 Gateway 已实现内存型一次性绑定码、公钥注册、批次验签、幂等接收和有界确认
响应；上述清单中的“绑定”指正式账号认证、过期、撤销和持久化流程。
