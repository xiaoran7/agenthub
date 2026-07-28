# Milestone 4 安全与稳定性验收

> 本文记录 M4 的可重复验收项、当前证据和仍需真实环境完成的边界。测试通过只代表
> 对应层级已验证，不把 Fixture、内存 Store 或回环网络模拟写成生产环境通过。

## 当前状态

M4 已于 2026-07-28 进入执行阶段，五类验收均已建立自动化基线；磁盘 Outbox
重启恢复与 PostgreSQL 压力用例也已进入测试套件。2026-07-28 使用本机
PostgreSQL 18.4 实际跑通了持久化重连契约和 12 设备/3000 Session 工作负载，
并用可执行的 Codex CLI 0.144.2 完成一次真实只读采集。个人部署的 Bearer
访问令牌、Web 服务端代理以及持久设备绑定/撤销也已进入自动化安全基线。阶段仍为
**进行中**：
真实 Claude/Agent 升级前后状态矩阵、部署环境 TLS/长时间离线恢复，以及系统级
Web Push 呈现仍未完成；本机真实 rustls 握手和私有 CA 恢复已进入自动化基线。
普通 Chrome 的新订阅已让推送服务实际接受一条加密通知，但该成功响应只证明服务
端投递，不等于 Windows 已展示。

## 自动化验收矩阵

| 验收项                | 自动化证据                                                                                                                                                                                                         | 当前结论                       |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------ |
| 字段泄漏              | Claude/Codex 含 Prompt、标题、预览、命令、消息和完整路径标记的 Fixture 经过 Adapter → Outbox → 签名批次，逐项断言标记与禁止字段键均未出现                                                                          | 通过                           |
| 断网重放              | Runner 使用磁盘 SQLite Outbox，向已释放的回环端口上传失败后关闭并重开数据库，断言两条事件与序列 1..2 均保留；随后连接临时 Gateway，在合法 ACK 后清零，再次重开仍为零                                               | 通过（回环故障与进程重启模拟） |
| Agent 升级兼容        | 新版 Fixture 增加未知字段并混合已知/未知状态；已知状态继续识别，未知状态降为 `UNKNOWN + inferred`，新内容字段不进入事件                                                                                            | 通过（Fixture）                |
| Hook 禁用和未信任降级 | 无 Hook 的轮询源仍入队并保持健康；未知 Hook 只记来源、状态降为 `UNKNOWN + inferred`；缺少 Session ID 的 Hook 拒绝                                                                                                  | 通过                           |
| 多设备和大量 Session  | 12 个独立 Ed25519 设备经完整注册、验签、批次入口写入 3000 个 Session；验证 8/4 设备的双 owner 隔离、按设备筛选、重放不重复和读响应字段边界。相同工作负载使用 PostgreSQL 18.4 的随机 Schema 与 4 条连接实际并发执行 | 内存与 PostgreSQL 均通过       |
| 面板访问认证          | 无令牌、错误 Scheme、错误令牌、尾随内容和仅伪造 `x-user-id` 均返回相同 401；正确 Bearer 令牌绑定固定 owner。Web 同源代理覆盖浏览器自带凭据、丢弃 Cookie，并只从服务端注入令牌                                      | 通过                           |
| 设备生命周期          | 绑定码只存 SHA-256 摘要，覆盖认证签发、TTL 边界、过期、同设备幂等重试、双设备并发抢码和 owner 隔离软撤销；撤销后历史保留且新同步拒绝。相同竞态与撤销契约在 PostgreSQL 随机 Schema 执行                             | 内存与 PostgreSQL 均通过       |
| TLS 与私有 CA         | 本机 rustls 服务端使用临时自签根证书完成真实握手；默认客户端因未知签发者拒绝且 Outbox 不清理，显式配置同一根证书后严格校验证书并完成 ACK。无效/不可读 PEM 返回稳定错误且不泄露路径或内容                         | 通过（本机 TLS）               |
| 离线积压可观测性      | 后台同步传输失败后从磁盘 Outbox 读取统计，退避报告持续携带 `pending_events`；成功恢复后归零，采集唤醒仍不能绕过失败退避                                                                                           | 通过                           |

压力用例是确定性的回归工作负载，不设置机器相关的毫秒阈值。它用于发现无界增长、
串号、隔离失效和重放重复，不冒充容量基准。

## 仍需真实环境验收

以下项目没有被自动化基线替代：

1. 在可执行的真实 Claude Code 与 Codex CLI 版本上跑状态矩阵，至少覆盖一次升级前后
   的工作、等待输入、等待权限、完成、失败与停止。
2. 在部署网络中使用真实证书轮换/过期故障和长时间离线验证恢复；本机测试已证明
   rustls 未知签发者失败时不清 Outbox、配置私有 CA 后完成真实 TLS ACK，磁盘 Outbox
   跨进程重启不丢序列，后台同步在传输失败后独立退避并持续报告积压。
3. 在普通浏览器验证 Web Push 的系统级呈现。Chrome 已授予 localhost 通知权限，
   Service Worker 已受控，Push 订阅成功登记，等待权限事件也已触发；Gateway 的
   隐私安全聚合日志确认推送服务返回成功（`delivered=1, gone=0, failed=0`），但
   Chrome/Windows 没有留下可观察的活动通知，不能把服务端投递成功写成实际弹窗
   通过。

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
  API 的 21 项测试全部通过、0 跳过；随机测试 Schema 已在清理阶段删除。
- 容器：`docker` 与 `podman` 均不存在，无法临时启动专用测试数据库。
- 外部不可达：Runner 向保留的 `.invalid` HTTPS 主机推送时在 3 秒后返回稳定
  `gateway_timeout`，磁盘 Outbox 保留 1 条事件；随后改回本地 Gateway，合法 ACK
  后待发送事件归零。
- TLS：本机临时 rustls 服务端以自签根证书完成真实握手。未配根证书时 Runner 返回
  `gateway_tls_failed` 且磁盘事件保留；通过 `AGENT_PLATFORM_GATEWAY_CA_CERT`
  配置根证书后同一事件获得合法 ACK 并清零。测试证书和私钥只在测试内存中生成。
- 后台同步现场：预先在磁盘 Outbox 留下 1 条事件，再用 Codex CLI 0.144.2 启动
  `run codex` 并连接本地 Gateway；无需执行手工 `sync push`，旧事件与本轮新观察
  均自动上传，Runner 退出时 `pending_events=0`，云端可查询 2 个 Session。
- 认证现场：实际启动 Gateway 与生产构建后的 Web 两个进程；直接匿名和仅伪造
  `x-user-id` 均返回 401，正确 Bearer 返回 200。浏览器不携带令牌经同源代理返回
  200，额外注入攻击者 Authorization/用户头仍由服务端令牌覆盖。
- 设备生命周期现场：实际启动 Gateway，由认证接口签发绑定码，Runner 创建身份、
  绑定并同步真实 Codex 只读观察；软撤销后既有 Session 仍可查询，设备显示已撤销，
  后续 Runner 上传稳定返回 403 `gateway_rejected`。临时身份、SQLite、日志和进程
  均在验收结束后清理。
- Chrome：localhost 通知权限为 `granted`，Service Worker 已控制页面，Push
  订阅登记返回 204。使用新临时 VAPID 密钥、新设备和
  `WAITING_PERMISSION` Fixture 完成 Runner 签名上传后，Outbox 归零且推送服务
  返回成功；Gateway 聚合日志为 `delivered=1, gone=0, failed=0`。Windows 未留下
  可观察的活动通知，因此证据止于服务端投递接受；验收结束已注销临时订阅并删除
  临时 VAPID 密钥与 Runner 数据。

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
