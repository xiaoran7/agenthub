# AgentPulse 技术可行性报告

调研日期：2026-07-25；2026-08-14 按当前官方文档与本机数据复核

## 1. 结论

第一阶段的只读 Session 监控整体可行，但需要区分三种能力：

1. **历史发现**：发现本机已保存的 Session 及基础元数据。
2. **实时监控**：准确判断 Session 正在工作、等待输入、等待审批或已经结束。
3. **进程接管**：由 Runner 启动或承载 Agent，从而获得最完整的事件流。

建议 MVP 采用“官方查询接口 + 生命周期 Hook + 本地缓存”的组合：

- Claude Code：本地 Session JSONL 用于历史发现，Hooks 用于实时状态补充。当前官方
  CLI 没有 `claude agents --json`，不得依赖该命令。
- Codex：`codex app-server` 为历史与 Runner 托管 Session 的主接口，Codex
  本地状态数据库作为 app-server 不可执行时的只读历史兜底。
- 本地 JSONL/SQLite 只读取白名单元数据，不把内部格式当作稳定协议，也不读取或
  上传对话正文、工具参数和供应商登录凭据。

在用户安装 Runner 后新建或恢复的 Session，可以实现较准确、低延迟的状态监控。
安装 Runner 之前已经运行、且没有接入 Hook 或 Runner app-server 的 Codex
Session，只能做尽力识别，不能保证实时状态完全准确。

## 2. Claude Code 可用能力

### 2.1 当前 Session 查询

截至 2026-08-14，Claude Code 官方 CLI 参考只提供 `--continue`、`--resume`、
`--print` 等会话操作，没有机器可读的全局 Session 枚举命令。早期设计记录中的
`claude agents --json` 不属于当前官方 CLI，现有 Runner 已停止调用它。

因此 Claude 的能力分两层：

- 历史发现：扫描 `CLAUDE_CONFIG_DIR` 或默认 `~/.claude/projects` 下的 Session
  JSONL，只从有界的头部记录提取 `sessionId`、`cwd` 和时间字段。
- 实时状态：通过 Claude Code Hooks 接收生命周期事件；没有 Hook 证据时，历史记录
  只标为 `PERSISTED/UNKNOWN`，不能根据文件存在猜测正在工作或已经完成。

该路径不需要 Claude 可执行文件，也不读取 Claude 登录 Token。未知字段直接忽略；
正文、Prompt 和工具载荷不会进入标准化事件。

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

Runner 只反序列化顶层白名单元数据，忽略未知字段，并限制每个文件的读取行数；
文件名和修改时间仍作为结构变化时的兜底。它不把 JSONL 条目当作稳定的完整对话协议。

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
- 不能替代 AgentPulse 对 Claude、Codex 和其他 Agent 的统一聚合。

因此它是产品对标和安全设计参考，不是 AgentPulse 的数据源。

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

因此，MVP 应安装一个可审查的 AgentPulse Codex 插件或用户级 Hook，
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

默认监控模式下 Runner 不应读取：

- `auth.json`
- OS Keychain/Keyring 中的 Codex 凭据
- 未经授权的完整 rollout/transcript

## 4. 能力对比

| 能力              | Claude Code                                  | Codex                                        |
| ----------------- | -------------------------------------------- | -------------------------------------------- |
| 检测安装          | `claude --version`                           | `codex --version`                            |
| 检测登录可用性    | 非敏感 CLI 状态/命令退出码                   | `codex login status`                         |
| 当前 Session 列表 | Hooks 建立的本地索引                         | app-server `thread/list` + Hooks             |
| 当前运行状态      | Hooks 生命周期事件                           | app-server 状态/Turn 事件 + Hooks            |
| 等待输入          | Notification/Elicitation Hook                | `requestUserInput`、MCP elicitation          |
| 等待权限          | PermissionRequest Hook                       | 多种 requestApproval                         |
| 成功/失败/停止    | Stop/StopFailure/SessionEnd Hook             | `turn/completed`                             |
| 历史 Session      | `~/.claude/projects` 元数据扫描              | `thread/list`；本地状态库兜底                |
| 原始登录复用      | 默认不读取 Token；授权模式可读取本机凭据缓存 | 默认不读取 Token；授权模式可读取本机凭据缓存 |
| 完整对话非必需    | 是                                           | 是                                           |

## 5. 状态准确性分级

每个状态记录必须带 `confidence`：

| 等级            | 含义                                  |
| --------------- | ------------------------------------- |
| `authoritative` | 来自 Agent 官方状态字段或实时协议事件 |
| `observed`      | 来自官方 Hook 生命周期事件            |
| `inferred`      | 根据进程、文件时间或超时推断          |
| `unknown`       | 无法可靠判断                          |

优先级：

```text
官方实时事件 > 官方查询结果 > Hook > 进程探测 > 文件时间推断
```

低等级状态不能覆盖时间更新、更高等级的状态，除非高等级状态已经超过有效期。

## 6. MVP 必须接受的限制

1. 安装 Runner 之前已经运行的 Codex Session，实时状态可能不完整。
2. Session 安装或适配器版本过旧时，只能显示有限字段。
3. Claude Code 没有稳定的机器可读枚举 API；历史扫描必须容忍 JSONL 结构演进。
4. 不读取完整 transcript 时，“当前任务摘要”可能为空。
5. Agent 关闭持久化后，历史 Session 无法完整恢复。
6. Hooks 被用户禁用、未信任或被企业策略禁止时，实时精度下降。
7. `notLoaded`、进程不存在或文件长时间未更新，都不能单独证明任务已完成。

UI 必须显示数据来源、最后观察时间和状态置信度，避免把推断结果伪装成事实。

## 7. 安全结论

该方案符合产品的基本安全边界：

- Runner 主动连接云端，不开放公网端口。
- 使用本机 Agent 的合法接口；授权模式可在本机读取凭据，但不复制或上传 OAuth Token。
- 只同步白名单元数据。
- 原始工作目录默认转换为显示名和不可逆设备内指纹。
- transcript 只保留在本机，除非用户未来明确开启更高级功能。
- 远程控制、回复和审批能力与只读监控使用独立权限开关。

授权模式补充：默认监控仍不读取敏感凭据；用户显式打开本地凭据管理后，Runner 可以
读取 `auth.json`、`.credentials.json`、OS Keychain/Keyring 和授权环境变量。原始值只
进入本机加密缓存，不进入同步协议。Codex 优先使用 App Server 的
`account/read`、`account/rateLimits/read`、`account/login/start` 与 `account/logout`；
Claude 优先委托 `claude auth status/login/logout`，不在 Runner 中复制私有 OAuth client
secret 或猜测未公开刷新端点。

## 8. 官方资料

### Claude Code

- [CLI 参考](https://docs.anthropic.com/en/docs/claude-code/cli-usage)
- [Hooks 参考](https://code.claude.com/docs/en/hooks.md)
- [Session 管理与本地存储](https://code.claude.com/docs/en/sessions.md)
- [Claude 配置目录](https://code.claude.com/docs/en/claude-directory.md)
- [Remote Control](https://code.claude.com/docs/en/remote-control.md)
- [Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview.md)

### OpenAI Codex

- [Codex App Server](https://developers.openai.com/codex/app-server/)
- [Codex Hooks](https://developers.openai.com/codex/hooks/)
- [Codex App Server 开源实现](https://github.com/openai/codex/tree/main/codex-rs/app-server)

### 开源实现参考

- [claude-code-trace](https://github.com/delexw/claude-code-trace)：扫描
  `~/.claude/projects` 的本地 Session。
- [claude-code-viewer](https://github.com/d-kimuson/claude-code-viewer)：按 Session
  JSONL 展示历史，验证无需解密凭据即可发现记录。
- [claude-code-history-viewer](https://github.com/jhlee0409/claude-code-history-viewer)：
  以只读方式挂载 `~/.claude` 与 `~/.codex`。
