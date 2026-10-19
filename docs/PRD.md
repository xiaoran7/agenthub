# AgentHub 产品需求文档 (PRD)

更新：2026-10-19。

## 1. 产品定义

**AgentHub = 官方 Agent 会话的统一远程继续入口（Remote-only）。**

AgentHub 不自建 Agent 推理循环，也不做通用本地工作台。它通过本机 `agenthubd`（`agenthub-runner`）托管 Claude Code、OpenAI Codex 等官方 Runtime 会话，让用户离开工位后通过自部署的 **Web 控制端（PC 浏览器与手机 PWA）** 实时查看输出、审批权限、继续多轮对话或一键急停。读取历史只服务于"判断并执行继续"。

## 2. 组件职责

| 组件                              | 角色           | 核心职责                                                                                                | 不承担                           | 状态         |
| :-------------------------------- | :------------- | :------------------------------------------------------------------------------------------------------ | :------------------------------- | :----------- |
| **agenthubd (`agenthub-runner`)** | 本机控制面     | 管理官方 Runtime、Session 所有权、本地事件流、命令幂等、内嵌本机控制台（`:4200`）、主动出站连接 Gateway | 自建推理循环                     | **主线核心** |
| **Gateway (`services/api`)**      | 自部署控制中枢 | 账户与设备鉴权、命令账本、状态路由、事件持久化与重放、SSE/WSS 分发                                      | 执行代码、持有 Provider 凭据     | **主线核心** |
| **Web (`apps/web`)**              | 统一远程控制端 | 工作区/会话管理、流式过程、Diff 审批、续聊、急停、移动端响应式 PWA                                      | 直读本机文件、保存 Provider 凭据 | **主线核心** |
| **Android (`apps/android`)**      | 原生移动客户端 | 移动端状态查看与快速干预                                                                                | 复杂环境配置                     | **暂时冻结** |

> 原 `apps/desktop`（Tauri）已移除，本机管理统一由 `agenthub-runner` 内嵌轻量 Web 控制台承接。

## 3. Session 语义

| 操作       | 含义                                   | 成功条件                                              |
| :--------- | :------------------------------------- | :---------------------------------------------------- |
| `create`   | 由 AgentHub 启动新的官方 Session       | Runtime 返回真实 `provider_session_id` 并建立控制通道 |
| `discover` | 只读发现官方历史或进程元数据           | 来源明确；仅作恢复候选，不代表可写                    |
| `resume`   | 用原生 ID 恢复上下文并建立控制通道     | Runtime 确认恢复该 ID                                 |
| `attach`   | 接管其他客户端正在持有的运行中 Session | Provider 提供可验证端点、事件订阅与写者所有权转移     |

- `hub_session_id` 是跨端稳定包装 ID，`provider_session_id` 是官方 Runtime 事实；跨 Provider 必须新建 Session。
- Session 分为 `managed` 与 `discovered`。未受控历史只展示有界元数据，不为非受控 Provider 提供全文浏览，也不因发现 PID 或 transcript 将其标为 `running`。

## 4. 范围边界

### 必须做

- Workspace-first 会话组织与 Provider 筛选；
- 官方 Runtime 的创建、恢复、连续多轮、工具事件、审批与中断；
- 设备绑定、授权目录白名单、命令幂等、断线回放与状态对齐；
- 运行时能力探测与明确的远程启停/设备撤销。

### 明确不做

- 自研 Agent 推理循环，或把普通 CLI 输出伪装成原生协议事件；
- 与"远程继续"无关的跨会话全文搜索或通用本地 IDE 工作台；
- 云端运行用户代码、代管 Provider Token，或从远端浏览任意未授权本机目录；
- 在 Provider 协议不支持时承诺无损 `attach`。

## 5. 验收标准

一个 Provider 只有完成以下闭环才标记为 Remote 可用：

1. 真实 Runtime 创建/恢复 Session，连续两轮保持同一 `provider_session_id`；
2. 本机与远端客户端读取同一 Session，正文、工具、审批、中断均由原生事件确认且只执行一次；
3. 断线重连与 Gateway 重启不丢历史、不重放已执行命令；Host 重启准确标记控制丢失并支持显式 `resume`；
4. 跨账户、撤销设备、重复命令、乱序帧与旧 Host generation 均被严格拒绝。
