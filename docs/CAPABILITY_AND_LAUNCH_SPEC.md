# 能力发现与会话接入契约

更新：2026-09-07。产品语义见 [PRD](PRD.md)，协议见 [SYNC_PROTOCOL](SYNC_PROTOCOL.md)。

## 能力来自证据

CapabilityReport 记录 provider、CLI 版本、协议版本、探测时间、来源及可用性。分别声明 discover/read_history/create/resume/attach_running/send_prompt/interrupt/approve/change_permissions/slash。字段未知就是 unknown，不把 PATH 存在或历史记录存在当作 supported。

模型来自本机原生 model/list 或已验证等价接口；历史中用过的模型只能标“历史模型”，不能证明当前账号可用。版本、模型、Slash 列表不内置为假探测结果。探测失败返回 unavailable/unknown 和有界错误码，不输出凭据或原始配置。

## SessionConnectProfile

- target_runner_id：已绑定设备；服务端校验归属。
- workspace：来自本机授权工作区，Runner 再核验真实路径。
- provider：固定于会话，禁止恢复时跨 Provider 替换。
- mode：create / resume / attach；resume、attach 必填 provider_session_id，create 禁止伪造原生 ID。
- model 与 reasoning：仅发送原生支持的配置；省略表示沿用 Provider 默认/恢复配置，不能硬编码过时模型。
- permissions：Provider 原生会话级配置。只允许映射已验证的项，不支持的组合拒绝，不能静默升级为完全访问。
- initial_prompt：可选；先确认会话绑定，再发送独立可追踪的 turn 命令。

恢复同一原生 ID 时复用既有 hub_session_id。外部已运行而无可用 attach 通道时只显示只读，不自动 resume。用户显式恢复前提示可能存在的其他执行者；Runner 自身维护单写者绑定。

## UI 动作

| 接入状态 | 显示 | 允许动作 |
| --- | --- | --- |
| discovered | 本机历史，尚未连接 | 查看；能力允许时显式恢复 |
| connecting | 正在连接 | 等待真实结果 |
| connected | 已连接 | 根据会话能力输入、审批、中断 |
| lost | 会话连接已断开 | 查看历史、核对状态、显式恢复 |
| runner offline | 电脑离线 | 查看已同步内容；不假装控制成功 |

运行态单独显示。运行中不支持追加/steer 时输入返回 busy，不能偷偷创建新 turn 或新 session。

## Slash

客户端可以提供 Hub 自己的导航命令。原生命令必须映射到已验证的 Provider 控制操作；例如中断对应 interrupt，不把任意 /compact 当普通 Prompt 发送。未知 Slash 明确说明不支持，用户可自行改写为普通文本。运行时权限变更只在原生协议确认后更新界面。

## 首个切片

Codex app-server 先实现 create/resume、持续 turn、原生审批/中断和事件读循环。attach_running 默认不支持，直到对外部端点订阅与所有权完成验证。Claude、Antigravity、Grok 的每个能力分别验收，不沿用 Codex 的能力声明。