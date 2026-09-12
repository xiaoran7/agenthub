# AgentHub 文档索引

更新：2026-09-12。

## 权威文档

| 文档                              | 解决的问题                                          |
| --------------------------------- | --------------------------------------------------- |
| [PRD](PRD.md)                     | AgentHub 是什么、三端职责、范围和验收标准           |
| [ARCHITECTURE](ARCHITECTURE.md)   | `agenthubd`、官方 Runtime、Gateway 与客户端如何协作 |
| [SYNC_PROTOCOL](SYNC_PROTOCOL.md) | Remote 身份、幂等、事件、ACK、重连与多端一致性      |
| [DESIGN_SYSTEM](DESIGN_SYSTEM.md) | 跨端视觉与交互规则                                  |
| [DEPLOYMENT](DEPLOYMENT.md)       | Gateway 单机部署与生产边界                          |
| [PITFALLS](PITFALLS.md)           | 实现与验收前必须检查的已知问题                      |
| [CHANGELOG](CHANGELOG.md)         | 历史变更，不作为当前能力说明                        |

[M4 验收](M4_VALIDATION.md) 与 [Android 验收](ANDROID_VALIDATION.md) 是按日期保存的证据。它们只证明记录中明确执行过的环境，不自动代表真机、生产或其他 Provider 已通过。

## 当前实现与目标差距

| 范围          | 当前事实                                                                 | 下一步                                      |
| ------------- | ------------------------------------------------------------------------ | ------------------------------------------- |
| Codex Host    | 进程外 daemon + 常驻 app-server；创建、恢复、多轮、审批、中断可用       | 强化状态对齐和版本兼容                      |
| Desktop       | 本机宿主客户端，经当前用户 IPC 使用 daemon；Remote 只是 Gateway 暴露开关；transcript 读取已收敛到 daemon 单点，仅托管能力 Provider 提供会话预览 | OS 服务安装、升级、诊断与独立停止控制       |
| Gateway       | v3 单实例 SQLite 内容库、设备签名、命令账本、回放                       | 统一连接管理；需要横向扩展时迁移 PostgreSQL |
| Web / Android | v3 Remote：Session 列表、Runner/Workspace 选择、Codex 创建/恢复/续聊、事件与命令收据、审批和中断；前台轮询、无原生推送和无离线命令队列 | 共用 capability report、通知与后台/离线同步体验 |
| Claude Code   | 发现仅作恢复候选（有界元数据）；managed 写未开放，无全文浏览             | 验证官方长时协议与认证后实现 Host           |
| 其他 Provider | 发现仅作恢复候选；不声称可写 attach                                      | 仅在 Provider 提供可靠所有权/订阅机制后开放 |

`agent-pulse-runner` 是当前可执行文件名，`agenthubd` 是已经落地的进程职责；Desktop 通过隐藏 `--agenthubd` 模式启动同一实现。当前没有独立命名的 daemon 二进制或 OS 服务安装包。旧 v1 元数据同步仍保留，但不得承载新的会话正文或被当成 Remote 完成证据。

## 文档规则

- PRD 只写稳定产品决定；架构写目标结构并明确当前差距；组件 README 写真实命令和接口；验收记录写证据。
- 不再维护重复的能力清单、可行性摘要和逐轮设计日记。Provider 能力证据统一放在架构文档，变化历史留在 Git 与 CHANGELOG。
- 新入口只有在真实官方 Runtime 返回原生 ID/事件后，才能标记为可用。
