# Milestone 4 安全与稳定性验收

> 记录 M4 的自动化基线、本机实测证据与待真实环境验收边界；严格区分 Fixture、回环模拟与生产环境结论。

## 1. 2026-09-08 v2.3.2 原生 Remote 闭环验收

- **原生链路（Runner 2.3.1 + Codex 0.153.4）**：`remote-native.test.ts` 跑通真实签名连接、两轮同原生 ID 续聊、重复命令幂等拦截、Gateway 停止 6 秒后 SQLite 水位回放（约 33s）及 `remote_embedded` 原生中断（1.30s）。
- **安装包与移动端**：Windows NSIS 安装包覆盖安装与登录页窄窗口校验通过；Android Debug APK 续聊、审批、中断与 Host 重启恢复见 [ANDROID_VALIDATION.md](ANDROID_VALIDATION.md)。

## 2. 自动化安全与稳定性矩阵

| 验收项                | 自动化证据摘要                                                                                                     | 结论                     |
| :-------------------- | :----------------------------------------------------------------------------------------------------------------- | :----------------------- |
| **字段泄漏防护**      | Claude/Codex 含路径与敏感标记 Fixture 经 Adapter -> Outbox -> 签名批次，断言敏感标记与非白名单键零泄漏             | 通过                     |
| **断网重放与积压**    | 磁盘 SQLite Outbox 上传失败后跨进程重开保留序列 `1..2`；退避报告携带 `pending_events`，连接恢复并获合法 ACK 后清零 | 通过                     |
| **Agent 升级兼容**    | 混合已知/未知状态的 Fixture 降级为 `UNKNOWN + inferred`，未知内容字段不进入同步事件                                | 通过（Fixture）          |
| **Hook 降级防护**     | 无 Hook 轮询源保持健康；未知 Hook 降级为 `UNKNOWN + inferred`；缺失 Session ID 的 Hook 直接拒绝                    | 通过                     |
| **多设备与 PG 并发**  | 12 台 Ed25519 设备并发写入 3000 Session（PostgreSQL 18.4 随机 Schema + 4 连接），验证双 owner 隔离与重放幂等       | 内存与 PostgreSQL 均通过 |
| **面板访问认证**      | 缺失/伪造令牌或仅伪造 `x-user-id` 均返回 401；Web 同源代理丢弃客户端 Cookie 并仅由服务端注入 Bearer 令牌           | 通过                     |
| **设备生命周期**      | 绑定码仅存 SHA-256 摘要，覆盖 TTL、幂等重试、双设备抢码与软撤销（撤销后历史保留、新上传返回 403）                  | 内存与 PostgreSQL 均通过 |
| **TLS 与私有 CA**     | 本机 rustls 自签根证书握手：未配 CA 时拒绝且保留 Outbox，配置 `AGENT_PLATFORM_GATEWAY_CA_CERT` 后完成严格校验与 ACK | 通过（本机 TLS）         |

## 3. 2026-07-28 ~ 07-29 本机现场实测摘要

- **Codex CLI 与后台同步**：`codex-cli 0.144.2` 执行 `collect-once codex` 真实采集并白名单化 35 个 Session；预置磁盘 Outbox 事件后启动 `run codex` 自动上传并归零。
- **Web Push 链路**：Chrome 授予通知权限并登记 VAPID 订阅，`WAITING_PERMISSION` 触发后 Gateway 记录 `delivered=1, gone=0, failed=0`，Service Worker 回传解密与 `showNotification()` 时间戳（注：未观测到 Windows 系统级 Toast 弹窗）。
- **Chrome 移动 PWA 与无障碍**：在 `390×844`、`320×568`、`844×390` 视口验证 `scrollWidth === innerWidth`、44px 触控目标、`dialog`/`aria-modal` 焦点圈定、断网提示与 `getInstallabilityErrors() === []`。

## 4. 仍需真实环境验收

1. 真实 Claude Code 与 Codex CLI 跨版本升级前后的完整状态矩阵；
2. 公网生产环境证书轮换/过期故障与长时离线恢复；
3. Windows / 移动操作系统级的原生通知弹窗呈现。
