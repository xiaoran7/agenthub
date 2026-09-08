# 原生 Remote 接入可行性

核对日期：2026-09-07。以下区分官方协议事实、工程选择和待验证能力。

## 官方依据

- [Codex App Server](https://learn.chatgpt.com/docs/app-server)：提供长时 JSON-RPC 控制通道、thread/start、thread/resume、turn/start、turn/interrupt、事件通知和服务端审批请求。thread/read 读取历史不加载会话；resume 延续指定 thread。
- [Claude Remote Control](https://code.claude.com/docs/en/remote-control)：本机执行、跨设备继续的产品对标。它不自动构成 AgentHub 可调用的第三方 Remote API。
- [Claude 流式输入](https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode)：长时输入模式与单次调用不同，适合连续交互。具体 SDK 登录适用范围需独立核对，不能从已安装 Claude 推导第三方 SDK 获得同样授权。

本机 Codex CLI 于此次核对返回 0.122.0；已用其 generate-ts 生成本地协议定义核验字段。版本是此次证据，不是代码应硬编码的支持列表。

随后用隔离安装的 Codex 0.153.4 验证真实两轮会话与 Gateway 重启重放，原生 ID 保持一致。全局 0.122.0 能握手，但无法解析当前本机配置的 `service_tier = default`；未修改用户配置。新 Host 不传 `persistExtendedHistory`，因为部分版本即使传 false 也会触发实验能力门禁。这些版本差异必须通过原生错误反映，不能假报创建成功。

## 实施选择

优先复用 Rust Runner 中的 Codex app-server 进程传输与进程树管理，增加交互模式，使它不再过滤正文/工具通知。使用真实 thread ID、原始审批 request ID 和原生结束事件，不沿用模拟 CommandRouter 的状态赋值。

首个验收只承诺 Runner 自己创建/恢复的会话。对任意其他桌面/TUI 进程的无损 attach 尚未建立证据，默认不可用。恢复历史与接管活进程始终是不同操作。

Claude 先保留已验证的本机历史和 CLI 能力，长时控制在协议与认证均确认后接入。Antigravity/Grok 暂无本轮验证的等价控制协议，不能因存在历史解析器就宣布 Remote 支持。

## 工程风险与验证

- 传输活着不代表 Agent 活着：单独验证进程退出、卡死、原生错误及审批阻塞。
- JSON-RPC 请求/响应与服务端请求 ID 空间可能重叠；必须结合 method 字段分类。
- 本机已有 Codex 采集器禁用了多类内容通知，交互 Host 必须使用不同初始化能力。
- Provider 请求超时可能已经执行，不能无条件重试；落盘命令意图并标记未知结果。
- 磁盘/网络压力用背压和可恢复日志处理；固定 2000 帧内存数组并不提供持久可靠性。
- 原生最终消息与增量可能重叠，聚合必须按 item 身份处理，不能重复拼接。

以上是实现约束，不是测试通过记录。阶段状态和验收入口见 [文档索引](README.md)。
