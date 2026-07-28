# Milestone 4 安全与稳定性验收

> 本文记录 M4 的可重复验收项、当前证据和仍需真实环境完成的边界。测试通过只代表
> 对应层级已验证，不把 Fixture、内存 Store 或回环网络模拟写成生产环境通过。

## 当前状态

M4 已于 2026-07-28 进入执行阶段，五类验收均已建立自动化基线；磁盘 Outbox
重启恢复与 PostgreSQL 压力用例也已进入测试套件。2026-07-28 使用本机
PostgreSQL 18.4 实际跑通了持久化重连契约和 12 设备/3000 Session 工作负载，
并用可执行的 Codex CLI 0.144.2 完成一次真实只读采集。阶段仍为**进行中**：
真实 Claude/Agent 升级前后状态矩阵、TLS 与长时间离线恢复，以及系统级 Web Push
呈现仍未完成。

## 自动化验收矩阵

| 验收项 | 自动化证据 | 当前结论 |
| --- | --- | --- |
| 字段泄漏 | Claude/Codex 含 Prompt、标题、预览、命令、消息和完整路径标记的 Fixture 经过 Adapter → Outbox → 签名批次，逐项断言标记与禁止字段键均未出现 | 通过 |
| 断网重放 | Runner 使用磁盘 SQLite Outbox，向已释放的回环端口上传失败后关闭并重开数据库，断言两条事件与序列 1..2 均保留；随后连接临时 Gateway，在合法 ACK 后清零，再次重开仍为零 | 通过（回环故障与进程重启模拟） |
| Agent 升级兼容 | 新版 Fixture 增加未知字段并混合已知/未知状态；已知状态继续识别，未知状态降为 `UNKNOWN + inferred`，新内容字段不进入事件 | 通过（Fixture） |
| Hook 禁用和未信任降级 | 无 Hook 的轮询源仍入队并保持健康；未知 Hook 只记来源、状态降为 `UNKNOWN + inferred`；缺少 Session ID 的 Hook 拒绝 | 通过 |
| 多设备和大量 Session | 12 个独立 Ed25519 设备经完整注册、验签、批次入口写入 3000 个 Session；验证 8/4 设备的双 owner 隔离、按设备筛选、重放不重复和读响应字段边界。相同工作负载使用 PostgreSQL 18.4 的随机 Schema 与 4 条连接实际并发执行 | 内存与 PostgreSQL 均通过 |

压力用例是确定性的回归工作负载，不设置机器相关的毫秒阈值。它用于发现无界增长、
串号、隔离失效和重放重复，不冒充容量基准。

## 仍需真实环境验收

以下项目没有被自动化基线替代：

1. 在可执行的真实 Claude Code 与 Codex CLI 版本上跑状态矩阵，至少覆盖一次升级前后
   的工作、等待输入、等待权限、完成、失败与停止。
2. 使用真实 TLS 故障和长时间离线验证恢复；当前测试已证明磁盘 Outbox 跨进程重启
   不丢序列，且外部不可达端点超时后事件仍保留、显式再次推送可恢复。自动后台同步
   仍属于 M2 遗留能力。
3. 在普通浏览器验证 Web Push 的系统级呈现。Chrome 已授予 localhost 通知权限，
   Service Worker 已受控，Push 订阅成功登记，等待权限事件也已触发；但 Chrome/
   Windows 没有留下可观察的活动通知，不能把订阅成功写成实际弹窗通过。

## 本机环境探测

2026-07-28 重新执行了工具和运行时探测及现场验收：

- `claude`：命令不存在，无法执行真实 Claude 状态矩阵。
- `codex`：PATH 仍优先解析到不可直接执行的 WindowsApps 路径；但 Desktop 的
  `AppData\Local\OpenAI\Codex\bin\<version>\codex.exe` 可执行，版本为
  `codex-cli 0.144.2`。Runner 对它执行 `collect-once codex`，真实接收并白名单化
  35 个 Session，Adapter 健康记录为空。该证据只覆盖当前版本的一次性 Thread
  列表，不等于升级前后完整状态矩阵或跨客户端实时事件覆盖。
- PostgreSQL：Windows 服务 `postgresql-x64-18` 正在运行，二进制位于
  `D:\PostgresSQL\bin`（未加入 PATH），服务端版本为 18.4。设置隔离测试连接串后，
  API 的 16 项测试全部通过、0 跳过；随机测试 Schema 已在清理阶段删除。
- 容器：`docker` 与 `podman` 均不存在，无法临时启动专用测试数据库。
- 外部不可达：Runner 向保留的 `.invalid` HTTPS 主机推送时在 3 秒后返回稳定
  `gateway_timeout`，磁盘 Outbox 保留 1 条事件；随后改回本地 Gateway，合法 ACK
  后待发送事件归零。
- Chrome：localhost 通知权限为 `granted`，Service Worker 已控制页面，Push
  订阅登记返回 204；验收结束已注销临时订阅并删除临时 VAPID 密钥与 Runner 数据。

现场测试没有向仓库写入数据库密码、VAPID 私钥、真实 Session 样本或临时数据库。

## 质量门

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```
