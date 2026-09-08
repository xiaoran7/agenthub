# AgentHub 文档与迁移状态

更新：2026-09-08。

> **核心宗旨与唯一标准**：
> **AgentHub = 多种本地 AI Agent 的统一远程控制入口。**
> 核心只做三件事：**发现**本机运行的 Codex/Claude 等官方 Agent、**接管**现有 Session、**暴露**给手机和 Web 离开电脑后继续使用。
> **三不原则**：不替代官方 Agent，不自己再造 Agent，不改变用户本地使用习惯。
> **唯一开发判断标准**：“这个功能是不是在帮助用户‘远程继续使用本机已经运行的 Agent’？不是就不做。”

规范描述目标，下面记录实际阶段，两者不可混用。

## 权威入口

- [PRD](PRD.md)：产品体验、发现/恢复/连接语义、验收标准。
- [架构](ARCHITECTURE.md)：Runner、Host、Gateway、客户端和迁移边界。
- [同步协议](SYNC_PROTOCOL.md)：身份、命令账本、ACK、回放与多端。
- [能力与接入](CAPABILITY_AND_LAUNCH_SPEC.md)：真实能力证据、会话配置与 UI 动作。
- [可行性](TECHNICAL_FEASIBILITY.md)：官方来源、适配选择和待验证事项。
- [设计系统](DESIGN_SYSTEM.md)、[部署](DEPLOYMENT.md)、[坑点](PITFALLS.md)、[历史变更](CHANGELOG.md)。
- [M4 验收](M4_VALIDATION.md)、[Android 验收](ANDROID_VALIDATION.md) 按日期区分旧阶段与新 Remote 证据，不把模拟器/回环网络扩大为真机或生产验收。

## 迁移阶段

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| R0 文档定界 | 原生 Remote 目标、接入语义、协议与旧边界替换 | 已替换并同步组件入口 |
| R1 原生 Host | Codex 长时 app-server、真实 ID、恢复、审批、中断、命令去重 | 已实现；原生两轮交互、协议 fixture 验证通过 |
| R2 可靠远程链路 | 设备鉴权、持久 Registry/账本、双向日志、内容存储、重连对齐 | 单实例 SQLite 链路已实现；真实 Gateway 重启重放验证通过；多实例/压力验收待做 |
| R3 多端统一 | Web/桌面/Android 消费同一会话，Workspace-first | Web 创建/两轮续聊/刷新/Host 重启恢复/离线禁控及 390px、1440px 布局验证通过；桌面共享 UI、目录选择、内嵌 Host 启停已接入，Tauri fixture UI 与真实 Codex 嵌入式停止验证通过；v2.3.2 桌面实际安装及登录页检查通过（登录后操作待手动认证）；Android 实际 APK 在模拟器完成创建/两轮续聊/审批/中断/重启恢复，详见验收文档 |
| R4 Provider 扩展与验收 | Claude 长时控制，其余按证据开放，真机/生产 | 待实施 |

## 可复用现状

已有账户、设备绑定与签名、v1 元数据 Outbox/数据库、发现适配器、桌面本地正文渲染与真实 CLI 启动。旧模拟 Router 与硬编码能力表已移除，v2 内存原型仅允许测试显式注入，生产默认入口不注册。当前原生 Remote 使用独立 v3 链路。

本轮开工前 README、AGENTS 和 CHANGELOG 有其他会话未提交的文档精简；本轮保留这些改动，提交只纳入本轮所属内容。部署/发版状态留在各自证据中。
