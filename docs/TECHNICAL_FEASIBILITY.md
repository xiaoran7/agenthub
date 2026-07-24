# Agent-Platform 技术可行性报告

调研日期：2026-07-25

## 1. 结论

第一阶段的只读 Session 监控整体可行，但需要区分三种能力：

1. **历史发现**：发现本机已保存的 Session 及基础元数据。
2. **实时监控**：准确判断 Session 正在工作、等待输入、等待审批或已经结束。
3. **进程接管**：由 Runner 启动或承载 Agent，从而获得最完整的事件流。

建议 MVP 采用“官方查询接口 + 生命周期 Hook + 本地缓存”的组合：

- Claude Code：`claude agents --json` 为主，Hooks 为实时补充。
- Codex：`codex app-server` 为历史与 Runner 托管 Session 的主接口，Codex
  Hooks 用于观察普通 CLI、IDE 和桌面 Session。
- 本地 JSONL 文件只用于发现和兜底，不把内部格式当作稳定协议，也不默认读取
  或上传对话正文。

在用户安装 Runner 后新建或恢复的 Session，可以实现较准确、低延迟的状态监控。
安装 Runner 之前已经运行、且没有接入 Hook 或 Runner app-server 的 Codex
Session，只能做尽力识别，不能保证实时状态完全准确。

## 2. Claude Code 可用能力

### 2.1 当前 Session 查询

Claude Code 官方 CLI 提供：

```text
claude agents --json
claude agents --json --all
```

`claude agents --json` 可以输出活动 Session；`--all` 还包含已完成的后台
Session。官方字段包括：

- `cwd`
- `kind`：`interactive` 或 `background`
- `startedAt`
- `id`
- `sessionId`
- `name`
- `state`：`working`、`blocked`、`done`、`failed`、`stopped`
- `pid`
- `status`
- `waitingFor`

`waitingFor` 能进一步区分：

- `permission prompt`
- `input needed`
- `sandbox request`
- `worker request`
- `dialog open`

这已经覆盖 MVP 最重要的实时状态，不需要读取 Claude 登录 Token。

限制：

- CLI 版本较旧时不存在这些字段或命令。
- `--all` 主要覆盖 Agent View 管理的后台 Session，不等价于所有历史对话。
- 未识别的新增字段必须保留原始值，并映射为“未知”，不能猜测。

### 2.2 生命周期 Hooks

Claude Code Hooks 可在以下事件触发：

- `SessionStart`
- `UserPromptSubmit`
- `PreToolUse`
- `PermissionRequest`
- `PostToolUse`
- `PostToolUseFailure`
- `Notification`
- `Stop`
- `StopFailure`
- `SessionEnd`
- `Elicitation`

Hook 输入包含 `session_id`、`cwd`、`transcript_path` 和事件特有字段。
这允许 Runner 在不读取完整对话的情况下接收状态变化。

推荐只向本机 Runner 的环回地址或本地 IPC 发送白名单字段：

- Session ID
- 当前工作目录的脱敏标识
- Hook 事件名
- 工具类型
- 等待类型
- 错误类型
- 事件时间

不得直接转发 Hook 的完整输入，因为工具参数、Prompt 和路径中可能包含敏感信息。

### 2.3 历史 Session

Claude Code 官方文档说明 Session 持续保存在：

```text
~/.claude/projects/<project>/<session-id>.jsonl
```

但官方明确说明 JSONL 条目格式属于内部实现，可能在任何版本发生变化。MVP
可使用文件名、修改时间和 Hook 已建立的本地索引发现历史 Session，但不应依赖
解析完整 JSONL 作为主协议。

如果用户关闭 Session 持久化，Runner 必须接受历史信息不可用。

### 2.4 Agent SDK 不作为现有登录适配器

Claude Agent SDK 适合创建由应用自身运行的新 Agent，但官方文档说明第三方产品
不能默认复用用户的 claude.ai 登录或额度，通常需要 API Key。因此它不适合作为
“读取用户已经登录的 Claude Code”这一需求的主适配器。

### 2.5 Claude Remote Control

Claude Code 自带 Remote Control，能够通过 Anthropic 官方网页和移动应用连接
本机 Session。它验证了“本机主动出站连接、云端展示、无需开放端口”的方案。

但 Remote Control：

- 是 Anthropic 自有产品能力，不是第三方聚合平台的通用读取 API。
- 会把 Session transcript 存储到 Anthropic 服务以实现跨设备同步。
- 不能替代 Agent-Platform 对 Claude、Codex 和其他 Agent 的统一聚合。

因此它是产品对标和安全设计参考，不是 Agent-Platform 的数据源。

## 3. Codex 可用能力

### 3.1 App Server

Codex 官方提供 `codex app-server`，使用 JSON-RPC 2.0，并支持 JSONL stdio、
WebSocket 和 Unix socket。

与本项目直接相关的稳定能力包括：

- `thread/list`：分页查询持久化 Thread，支持目录、归档、来源和时间排序。
- `thread/read`：读取 Thread 元数据，可选择是否包含 Turn。
- `thread/loaded/list`：列出当前 app-server 进程中已加载的 Thread。
- `thread/status/changed`：接收 `active`、`idle`、`systemError`、`notLoaded`
  状态变化。
- `turn/started`：Turn 开始。
- `turn/completed`：Turn 以 `completed`、`interrupted` 或 `failed` 结束。
- `item/started` 与 `item/completed`：工具、命令和文件变更生命周期。
- `item/commandExecution/requestApproval`：命令执行等待审批。
- `item/fileChange/requestApproval`：文件修改等待审批。
- `item/permissions/requestApproval`：权限配置等待审批。
- `item/tool/requestUserInput`：等待用户回答。
- `mcpServer/elicitation/request`：MCP 服务等待用户输入。

App Server 直接使用用户本机 Codex 状态和登录环境，Runner 无需读取
`auth.json`。

### 3.2 App Server 的边界

Runner 启动的 app-server 可以可靠观察：

- 由该 app-server 新建或恢复的 Thread
- 该 app-server 已加载的 Thread
- 持久化 Thread 列表和元数据

它不能自动保证订阅另一个独立 Codex CLI、IDE 或桌面进程的内存事件。
`thread.status = notLoaded` 只表示 Thread 没有加载到当前 app-server，不能直接
解释为“历史会话”，因为它可能正在另一个进程中运行。

另外，App Server 的 WebSocket transport 仍标记为实验性/不受支持。MVP
应该由 Runner 在本机使用 stdio 启动 app-server，而不是在用户电脑上暴露
WebSocket 端口。

### 3.3 Codex Hooks

Codex 支持用户级、项目级和插件级 Hooks，关键事件包括：

- `SessionStart`
- `UserPromptSubmit`
- `PreToolUse`
- `PermissionRequest`
- `PostToolUse`
- `Stop`
- `SessionEnd`

Hook 输入包含：

- `session_id`
- `transcript_path`
- `cwd`
- `hook_event_name`
- `model`
- Turn 事件中的 `turn_id`
- 部分事件中的 `permission_mode`

因此，MVP 应安装一个可审查的 Agent-Platform Codex 插件或用户级 Hook，
用于向本机 Runner 推送普通 CLI、IDE 和桌面 Session 的生命周期事件。

限制：

- 非托管 Hooks 首次安装或内容变化后需要用户审查并信任。
- Hooks 不是完整安全边界，少数特殊工具路径可能不经过通用 Hook。
- 当前 Hook 事件不如 App Server 事件完整，失败原因和等待类型可能无法全部
  细分。
- 已经运行的 Session 是否加载新 Hook 不能作为稳定保证；MVP 应提示用户在
  安装后新建或恢复 Session。

### 3.4 本地持久化状态

Codex 在 `CODEX_HOME`（默认 `~/.codex`）保存配置、Session 和其他本地状态，
并提供 `thread/list` 对持久化记录进行查询和修复。

Runner 不应读取：

- `auth.json`
- OS Keychain/Keyring 中的 Codex 凭据
- 未经授权的完整 rollout/transcript

## 4. 能力对比

| 能力 | Claude Code | Codex |
| --- | --- | --- |
| 检测安装 | `claude --version` | `codex --version` |
| 检测登录可用性 | 非敏感 CLI 状态/命令退出码 | `codex login status` |
| 当前 Session 列表 | `claude agents --json` | app-server `thread/list` + Hooks |
| 当前运行状态 | 官方 `state/status/waitingFor` | app-server 状态/Turn 事件 + Hooks |
| 等待输入 | `blocked + waitingFor`、Notification/Elicitation Hook | `requestUserInput`、MCP elicitation |
| 等待权限 | `permission prompt`、PermissionRequest Hook | 多种 requestApproval |
| 成功/失败/停止 | `done/failed/stopped`、Stop/StopFailure | `turn/completed` |
| 历史 Session | 本地索引，完整枚举有限 | `thread/list` |
| 原始登录复用 | 是，不读取 Token | 是，不读取 Token |
| 完整对话非必需 | 是 | 是 |

## 5. 状态准确性分级

每个状态记录必须带 `confidence`：

| 等级 | 含义 |
| --- | --- |
| `authoritative` | 来自 Agent 官方状态字段或实时协议事件 |
| `observed` | 来自官方 Hook 生命周期事件 |
| `inferred` | 根据进程、文件时间或超时推断 |
| `unknown` | 无法可靠判断 |

优先级：

```text
官方实时事件 > 官方查询结果 > Hook > 进程探测 > 文件时间推断
```

低等级状态不能覆盖时间更新、更高等级的状态，除非高等级状态已经超过有效期。

## 6. MVP 必须接受的限制

1. 安装 Runner 之前已经运行的 Codex Session，实时状态可能不完整。
2. Session 安装或适配器版本过旧时，只能显示有限字段。
3. Claude Code 的全部历史交互式 Session 没有稳定的机器可读枚举 API。
4. 不读取完整 transcript 时，“当前任务摘要”可能为空。
5. Agent 关闭持久化后，历史 Session 无法完整恢复。
6. Hooks 被用户禁用、未信任或被企业策略禁止时，实时精度下降。
7. `notLoaded`、进程不存在或文件长时间未更新，都不能单独证明任务已完成。

UI 必须显示数据来源、最后观察时间和状态置信度，避免把推断结果伪装成事实。

## 7. 安全结论

该方案符合产品的基本安全边界：

- Runner 主动连接云端，不开放公网端口。
- 使用本机 Agent 的合法接口，不复制或上传 OAuth Token。
- 只同步白名单元数据。
- 原始工作目录默认转换为显示名和不可逆设备内指纹。
- transcript 只保留在本机，除非用户未来明确开启更高级功能。
- 远程控制、回复和审批能力与只读监控使用独立权限开关。

## 8. 官方资料

### Claude Code

- [Agent View 与 `claude agents --json`](https://code.claude.com/docs/en/agent-view.md)
- [Hooks 参考](https://code.claude.com/docs/en/hooks.md)
- [Session 管理与本地存储](https://code.claude.com/docs/en/sessions.md)
- [Remote Control](https://code.claude.com/docs/en/remote-control.md)
- [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview.md)

### OpenAI Codex

- [Codex App Server](https://developers.openai.com/codex/app-server/)
- [Codex Hooks](https://developers.openai.com/codex/hooks/)
- [Codex App Server 开源实现](https://github.com/openai/codex/tree/main/codex-rs/app-server)
