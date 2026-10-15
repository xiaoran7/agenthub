# AgentHub 文档索引

实现描述按源码核对；设备与生产结论只引用带日期的验收证据。

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

> **当前战略重点（2026-10）**：集中精力打穿 **Gateway + Web** 自部署链路；Desktop（Tauri）与原生 Android 暂时冻结。

| 范围          | 当前事实                                                                 | 下一步                                      |
| ------------- | ------------------------------------------------------------------------ | ------------------------------------------- |
| Gateway       | v3 单实例 SQLite 内容库、设备签名、命令账本、回放                       | 强化直连出站通道管理、命令路由与稳定性      |
| Web (主力端)  | v3 Remote：Session 列表、Workspace 选择、创建/恢复/续聊、审批与中断；移动端响应式支持 | 完善全功能 Web 控制面板与手机端 PWA 交互体验 |
| Codex Host    | 进程外 daemon + 常驻 app-server；创建、恢复、多轮、审批、中断可用       | 强化状态对齐和版本兼容                      |
| Claude Code   | 源码已包含 Claude 流式续聊分支及取消/事件队列；已升级双向会话驱动        | 核验真实 CLI 协议、审批、取消与多轮继续     |
| Desktop       | 本机宿主客户端（Tauri），经当前用户 IPC 使用 daemon                      | **暂时冻结**：暂停新功能开发，保留代码资产 |
| Android       | 原生移动端应用（Kotlin）                                                 | **暂时冻结**：由 Web 响应式与 PWA 承接     |
| 其他 Provider | 发现仅作恢复候选；不声称可写 attach                                      | 仅在 Provider 提供可靠所有权/订阅机制后开放 |

`agenthub-runner` 是当前可执行文件名与 Crate 名，`agenthubd` 是已经落地的进程职责；旧 v1 元数据同步仍保留，但不得承载新的会话正文或被当成 Remote 完成证据。

## 文档规则

- PRD 只写稳定产品决定；架构写目标结构并明确当前差距；组件 README 写真实命令和接口；验收记录写证据。
- 不再维护重复的能力清单、可行性摘要和逐轮设计日记。Provider 能力证据统一放在架构文档，变化历史留在 Git 与 CHANGELOG。
- 新入口只有在真实官方 Runtime 返回原生 ID/事件后，才能标记为可用。
