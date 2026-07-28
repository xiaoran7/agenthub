# Milestone 4 安全与稳定性验收

> 本文记录 M4 的可重复验收项、当前证据和仍需真实环境完成的边界。测试通过只代表
> 对应层级已验证，不把 Fixture、内存 Store 或回环网络模拟写成生产环境通过。

## 当前状态

M4 已于 2026-07-28 进入执行阶段，五类验收均已建立自动化基线；磁盘 Outbox
重启恢复与 PostgreSQL 压力用例也已进入测试套件。阶段仍为**进行中**：本机没有
PostgreSQL，数据库压力尚未实际执行；真实 Claude/Codex 升级和真实外部网络故障
也尚未验证。

## 自动化验收矩阵

| 验收项 | 自动化证据 | 当前结论 |
| --- | --- | --- |
| 字段泄漏 | Claude/Codex 含 Prompt、标题、预览、命令、消息和完整路径标记的 Fixture 经过 Adapter → Outbox → 签名批次，逐项断言标记与禁止字段键均未出现 | 通过 |
| 断网重放 | Runner 使用磁盘 SQLite Outbox，向已释放的回环端口上传失败后关闭并重开数据库，断言两条事件与序列 1..2 均保留；随后连接临时 Gateway，在合法 ACK 后清零，再次重开仍为零 | 通过（回环故障与进程重启模拟） |
| Agent 升级兼容 | 新版 Fixture 增加未知字段并混合已知/未知状态；已知状态继续识别，未知状态降为 `UNKNOWN + inferred`，新内容字段不进入事件 | 通过（Fixture） |
| Hook 禁用和未信任降级 | 无 Hook 的轮询源仍入队并保持健康；未知 Hook 只记来源、状态降为 `UNKNOWN + inferred`；缺少 Session ID 的 Hook 拒绝 | 通过 |
| 多设备和大量 Session | 12 个独立 Ed25519 设备经完整注册、验签、批次入口写入 3000 个 Session；验证 8/4 设备的双 owner 隔离、按设备筛选、重放不重复和读响应字段边界。相同工作负载已接入随机 Schema 的 PostgreSQL 并发事务测试 | 内存通过；PostgreSQL 测试已建立、本机跳过 |

压力用例是确定性的回归工作负载，不设置机器相关的毫秒阈值。它用于发现无界增长、
串号、隔离失效和重放重复，不冒充容量基准。

## 仍需真实环境验收

以下项目没有被自动化基线替代：

1. 设置 `AGENT_PLATFORM_TEST_DATABASE_URL` 后实际运行已建立的 PostgreSQL
   12 设备/3000 Session 并发写入、owner 隔离与整批重放测试。本机仍无 PostgreSQL
   或容器运行时，所以该用例当前明确 `SKIP`。
2. 在可执行的真实 Claude Code 与 Codex CLI 版本上跑状态矩阵，至少覆盖一次升级前后
   的工作、等待输入、等待权限、完成、失败与停止。
3. 使用真实外部网络断开、DNS/TLS 故障和长时间离线验证恢复；当前测试已证明磁盘
   Outbox 跨进程重启不丢序列、显式再次推送可恢复，自动后台同步仍属于 M2 遗留能力。
4. 在普通浏览器验证 Web Push 实际订阅与投递。嵌入式浏览器固定拒绝通知权限，
   该限制不会因 M4 自动化测试消失。

## 本机环境探测

2026-07-28 重新执行了工具和运行时探测：

- `claude`：命令不存在，无法执行真实 Claude 状态矩阵。
- `codex`：可解析到 WindowsApps 内部 `codex.exe`，但实际运行
  `codex --version` 返回 `Access is denied`，不能把路径存在写成 CLI 可用。
- PostgreSQL：`psql` 不存在，没有匹配的 Windows 服务，
  `AGENT_PLATFORM_TEST_DATABASE_URL` 与 `AGENT_PLATFORM_DATABASE_URL` 均未设置。
- 容器：`docker` 与 `podman` 均不存在，无法临时启动专用测试数据库。

所以本机执行 `npm.cmd test` 时两项 PostgreSQL 测试明确显示 `SKIP`；这属于环境
阻塞，不是通过，也不是测试代码未接入。

## 质量门

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```
