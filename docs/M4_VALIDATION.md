# Milestone 4 安全与稳定性验收

> 本文记录 M4 的可重复验收项、当前证据和仍需真实环境完成的边界。测试通过只代表
> 对应层级已验证，不把 Fixture、内存 Store 或回环网络模拟写成生产环境通过。

## 当前状态

M4 已于 2026-07-28 进入执行阶段，五类验收均已建立自动化基线。阶段仍为
**进行中**：真实 PostgreSQL 压力、真实 Claude/Codex 升级和真实断网环境尚未验证。

## 自动化验收矩阵

| 验收项 | 自动化证据 | 当前结论 |
| --- | --- | --- |
| 字段泄漏 | Claude/Codex 含 Prompt、标题、预览、命令、消息和完整路径标记的 Fixture 经过 Adapter → Outbox → 签名批次，逐项断言标记与禁止字段键均未出现 | 通过 |
| 断网重放 | Runner 向已释放的回环端口上传失败，断言 Outbox 两条事件不减少；随后连接临时 Gateway，按原序列 1..2 上传并在合法 ACK 后清零 | 通过（回环故障模拟） |
| Agent 升级兼容 | 新版 Fixture 增加未知字段并混合已知/未知状态；已知状态继续识别，未知状态降为 `UNKNOWN + inferred`，新内容字段不进入事件 | 通过（Fixture） |
| Hook 禁用和未信任降级 | 无 Hook 的轮询源仍入队并保持健康；未知 Hook 只记来源、状态降为 `UNKNOWN + inferred`；缺少 Session ID 的 Hook 拒绝 | 通过 |
| 多设备和大量 Session | 12 个独立 Ed25519 设备经完整注册、验签、批次入口写入 3000 个 Session；验证 8/4 设备的双 owner 隔离、按设备筛选、重放不重复和读响应字段边界 | 通过（内存 Store） |

压力用例是确定性的回归工作负载，不设置机器相关的毫秒阈值。它用于发现无界增长、
串号、隔离失效和重放重复，不冒充容量基准。

## 仍需真实环境验收

以下项目没有被自动化基线替代：

1. 设置 `AGENT_PLATFORM_TEST_DATABASE_URL` 后运行 PostgreSQL 集成测试，并在专用
   测试数据库执行多设备/大量 Session 工作负载。本机仍无 PostgreSQL 或容器运行时。
2. 在可执行的真实 Claude Code 与 Codex CLI 版本上跑状态矩阵，至少覆盖一次升级前后
   的工作、等待输入、等待权限、完成、失败与停止。
3. 使用真实网络断开、DNS/TLS 故障和进程重启验证 Outbox 恢复；当前测试只证明
   传输失败不 ACK、显式再次推送可恢复，自动后台同步仍属于 M2 遗留能力。
4. 在普通浏览器验证 Web Push 实际订阅与投递。嵌入式浏览器固定拒绝通知权限，
   该限制不会因 M4 自动化测试消失。

## 质量门

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```
