# AgentHub 文档索引

实现描述以源码为准；设备与生产结论只引用带日期的实测证据。

## 权威文档

| 文档                              | 职责                                                                |
| :-------------------------------- | :------------------------------------------------------------------ |
| [PRD](PRD.md)                     | 产品定义、组件职责、Session 语义与验收标准                          |
| [ARCHITECTURE](ARCHITECTURE.md)   | `agenthubd`、内嵌控制台、Provider Adapter、Gateway 与客户端协作边界 |
| [SYNC_PROTOCOL](SYNC_PROTOCOL.md) | 本地 IPC v2、Remote 身份、幂等命令、事件 DTO、水位 ACK 与重连对齐   |
| [DESIGN_SYSTEM](DESIGN_SYSTEM.md) | 跨端视觉令牌、动效约束与本机内嵌控制台（magpie）边界                |
| [DEPLOYMENT](DEPLOYMENT.md)       | Gateway + Web 单机/Docker/1Panel 部署与安全边界                     |
| [PITFALLS](PITFALLS.md)                     | 实现与验收前必查的已知问题清单                                      |
| [M4_VALIDATION](M4_VALIDATION.md)           | Milestone 4 安全、脱敏、断网重放与稳定性验收矩阵                    |
| [ANDROID_VALIDATION](ANDROID_VALIDATION.md) | 原生 Android 客户端（冻结基线）历史实测与自动化验收记录             |
| [OPEN_SOURCE_GUIDE](OPEN_SOURCE_GUIDE.md)   | 开源仓库元数据、社区收录（[PR 指南](COMMUNITY_SUBMISSIONS.md)）与宣发索引 |
| [CHANGELOG](CHANGELOG.md)                   | 历史版本变更记录（不作为当前能力说明）                              |

## 当前实现与目标差距（2026-10）

> **战略主线**：聚焦 **Runner + Gateway + Web** 自部署闭环；`apps/desktop`（Tauri）已移除并由 Runner 内嵌 Web 控制台（`127.0.0.1:4200`）承接，原生 Android 暂时冻结。

| 模块                         | 当前事实                                                                                           | 下一步                                    |
| :--------------------------- | :------------------------------------------------------------------------------------------------- | :---------------------------------------- |
| **Runner (`agenthubd`)**     | 单二进制常驻控制面，内嵌本机 Web 控制台（会话/智能体/用量账本/工作区/设置）与本地 IPC v2           | 升级为可安装、可诊断的用户级 OS 服务      |
| **Gateway**                  | v3 单实例 SQLite 内容库、Ed25519 设备验签、命令账本、水位回放与 PostgreSQL 元数据存储              | 强化多实例横向扩展与长连接稳定性          |
| **Web（主力端）**            | v3 Remote：工作区/会话管理、流式输出、Diff 审批、多选问答、中断、移动端响应式 PWA                  | 持续打磨手机端 PWA 与通知体验             |
| **Codex / Claude / ACP**     | Codex 常驻 app-server、Claude Code 流式续聊、通用 ACP stdio 适配器可用；其他 Provider 保持只读发现 | 持续对齐新版 CLI 协议、审批与异常恢复     |
| **Android (`apps/android`)** | Kotlin + Compose 原生客户端（v2.5.0 基线）                                                         | **暂时冻结**（移动端由 Web PWA 主力承接） |

## 历史验收证据与边界摘要

- **本机与自动化基线（2026-07 ~ 2026-10）**：
  - 隐私与字段泄漏：Claude/Codex/ACP 含路径与敏感标记的 Fixture 经 Outbox/DTO 白名单过滤，0 物理路径或未授权字段外泄；
  - 断网重放与存储：磁盘 SQLite Outbox 跨进程重启保留序列、ACK 后清零；PostgreSQL 18.4 通过 12 设备 / 3000 Session 并发隔离与重放测试；
  - 本机 TLS 与认证：rustls 自签 CA 严格校验、Bearer/Cookie 同源代理注入、一次性绑定码 SHA-256 摘要与设备软撤销均通过验证。
- **原生 Remote 与移动端实测（2026-09 ~ 2026-10）**：
  - 真实 Runner + Codex / Claude / ACP 跑通创建、恢复、连续多轮、Diff 审批、多选问答、中断及 Gateway 重启后 SQLite 回放；
  - Android 模拟器（`Medium_Phone` 1080×2400）与 Chrome 移动视口（390×844 / 320×568）实测通过流式转录、增量 Markdown、上下文容量环与 1.3× 字体缩放。
- **未覆盖边界**：Android 实体真机与商店正式签名、公网多实例 Gateway 容量压测、Windows 系统级 Web Push 弹窗呈现仍需在实际环境中独立验收。
