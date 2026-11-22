# AgentHub 产品需求文档 (PRD)

## 1. 产品定义

**AgentHub = 官方 AI 编程智能体会话的统一远程继续与跨端控制面。**

AgentHub 不自建推理循环，也不代管模型 API Key。它通过开发机上的 `agenthubd`（`agenthub-runner`）托管 Claude Code、OpenAI Codex、Google Antigravity（`agy`）及 ACP 兼容 Runtime，使开发者离开工位后可通过自部署 **Web 控制端（PC / 手机 PWA）** 或 **Android 原生客户端** 查看流式输出、审批 Diff 权限、回答提问、继续多轮对话或紧急中断。

## 2. 组件职责边界

| 组件 | 角色 | 核心职责 | 明确不承担 |
| :--- | :--- | :--- | :--- |
| **agenthubd (`crates/runner`)** | 本机唯一控制面 | 托管官方 Runtime、单写者所有权、本地 IPC v2、`:4200` 内嵌控制台与托盘、主动出站 WSS | 自建推理循环 |
| **Gateway (`services/api`)** | 自部署中继中枢 | 账户与 Ed25519 设备鉴权、幂等命令账本、`remote.db` 有界事件回放、SSE/WSS 分发 | 执行用户代码、存 Provider 凭据 |
| **Web (`apps/web`)** | 静态远程控制端 | Vite 静态多页 + nginx 同源反代：跨设备待办置顶、流式转录、Diff 审批、续聊、急停 | 直读本机物理路径或凭据 |
| **Android (`apps/android`)** | 原生移动控制端 | Kotlin + Compose Remote v3 客户端：Keystore 凭据保护、移动触控审批、附件与快捷终端行 | 复杂宿主机环境编排 |

## 3. Session 四态语义

| 操作 | 含义 | 成立条件 |
| :--- | :--- | :--- |
| `create` | 由 AgentHub 启动新的官方会话 | Runtime 返回真实 `provider_session_id` 并建立控制通道 |
| `discover` | 只读发现本机历史或活跃元数据 | 来源明确；仅作恢复候选，绝不直接推导为可写 |
| `resume` | 以既有原生会话 ID 恢复上下文与控制通道 | Runtime 确认恢复该 ID 并绑定当前 `host_generation` |
| `attach` | 接管运行中的原生会话控制权 | Provider 提供可验证端点与单写者所有权转移 |

- `hub_session_id` 为跨端稳定标识，`provider_session_id` 为官方 Runtime 事实；跨 Provider 必须新建会话。
- 未受控 `discovered` 历史仅暴露有界元数据，不因探测到进程或转录文件而谎报可控 `running`。

## 4. 验收红线

1. 真实 Runtime 完成 `create`/`resume` 且连续多轮保持同一 `provider_session_id`；
2. 正文增量、工具状态、Diff 审批、多选问答与中断均由原生事件驱动且满足单次幂等；
3. 网络闪断、Gateway 重启或 Host 换代不丢已确认历史、不重放副作用命令，旧 `host_generation` 审批严格拒绝；
4. 物理绝对路径、未白名单字段与 Provider 凭据零外泄。
