# AgentHub 产品需求文档

更新：2026-10-10（方向调整：收敛聚焦 Gateway + Web 自部署，暂时冻结 Desktop 与 Android 原生端）。

## 1. 产品定义

**AgentHub = 官方 Agent 会话的统一远程继续入口（Remote-only）。**

AgentHub 不实现自己的 Agent 推理循环，也不做通用本地工作台。它经官方协议把 Codex、Claude Code 等官方 Runtime 的会话托管在本机 `agenthubd` 上，让用户离开电脑后通过自部署的 **Web 控制端**（PC 与手机浏览器自适应）继续同一个 Session。读取历史只有一个目的：判断并执行“继续”。

价值不是“替代官方客户端”，而是让用户：

- 在电脑前按需托管官方 Runtime（Codex / Claude Code）；
- 离开电脑后从自部署 Web 界面看到实时过程、续聊、审批、中断；
- 保留 Provider 的真实能力、身份和会话历史，不制造伪兼容。

## 2. 客户端与架构职责

> **路线调整说明**：为了聚焦核心闭环、避免分散工程精力，当前阶段**全力打通 Gateway + Web 自部署全功能体验**。Web 端采用响应式设计兼顾桌面与手机（PWA）。Desktop（Tauri）与原生 Android 暂时冻结。

| 组件 | 角色 / 场景 | 必须具备 | 不承担 | 状态 |
| --- | --- | --- | --- | --- |
| **Gateway** | 自部署控制中枢 | 账户与设备身份、命令账本、状态路由、事件持久化与重放、SSE/WebSocket 分发 | 执行 Agent、持有 Provider 凭据 | **主线核心** |
| **Web** | 统一远程控制端（PC & 手机） | 设备/工作区/Session 管理、流式过程查看、续聊、审批、急停中断、历史回放、移动端自适应 | 访问本机文件系统、保存 Provider 凭据 | **主线核心** |
| **agenthubd (Runner)** | 本机会话控制面 | 管理官方 Runtime、Session 所有权、事件流、命令幂等、主动出站连接 Gateway | 通用本地 GUI 渲染 | **主线核心** |
| **Desktop** | 本机宿主客户端（Tauri） | 本机 IPC 交互、工作区授权、本地查看 | 通用本地工作台 | **暂时冻结** |
| **Android** | 原生移动客户端（Kotlin） | 移动端快速干预 | 复杂环境配置 | **暂时冻结 (由 Web PWA 承接)** |

## 3. 核心用户旅程

### 本地开始

1. Desktop 自动启动或连接本机 `agenthubd`。
2. 用户选择已授权工作区和 Provider，创建一个官方 Session；或选择官方历史进行恢复。
3. `agenthubd` 启动官方 Runtime，取得真实 Provider Session ID 后才建立 Hub Session。
4. Desktop 经本地 IPC 消费事件并发送命令。Gateway 不可用时，本地体验继续工作。

### 远程继续

1. `agenthubd` 用设备身份主动连接 Gateway，不要求本机开放公网端口。
2. Web/Android 读取已授权设备、工作区和 Session，看到与 Desktop 相同的历史与运行状态。
3. 用户发送 Prompt、审批或中断；Gateway 持久记录命令并路由到 Session 的当前 Host。
4. 客户端明确区分“已排队”“Runtime 已接受”“已完成”和“结果未知”。

### 返回桌面

Desktop 从同一个 `agenthubd` 读取状态，不另起 Runtime 或复制 Session。远端断开不结束本地执行，关闭某个客户端也不改变 Session 所有权。

## 4. Session 语义

| 操作     | 含义                                         | 成功条件                                                |
| -------- | -------------------------------------------- | ------------------------------------------------------- |
| create   | 由 AgentHub 启动一个新的官方 Session         | Runtime 返回真实 Provider Session ID，Host 建立控制通道 |
| discover | 只读发现官方历史或进程元数据                 | 有明确来源；不代表可写                                  |
| resume   | 用原生 ID 恢复官方上下文并建立新的控制通道   | Runtime 确认恢复该 ID                                   |
| attach   | 连接另一个客户端当前正在持有的运行中 Session | Provider 提供可验证端点、事件订阅与写者所有权           |

`hub_session_id` 是跨端稳定包装 ID，`provider_session_id` 是 Provider 事实。两者在重连或换端时不变；跨 Provider 继续必须创建新 Session。

Session 分为 `managed` 与 `discovered`。没有控制通道的历史只展示有界元数据（标题、模型、时间等），作为恢复候选；不为浏览目的读取或展示全文。不能因为找到 PID、状态库或 transcript 就把它标为 running 或可控制。

## 5. 功能范围

### 必须做

- Workspace-first 的 Session 组织和 Provider 筛选；
- 官方 Runtime 的创建、恢复、连续多轮、工具事件、审批和中断；
- 本地单一 Session Host 与三端共享的读模型；
- 设备绑定、授权目录、命令幂等、事件持久化、断线回放与状态对齐；
- Provider 能力探测和差异化 UI；
- 明确的远程启停、设备撤销和内容同步说明。

### 明确不做

- 自研 Agent 大脑、替代模型推理或把普通 CLI 输出伪装成原生事件；
- 通用本地工作台、跨会话搜索、历史分析等与“远程继续”无关的读取功能；对暂不能远程控制的 Provider 不提供全文会话浏览；
- 云端运行用户代码或保存 Provider Token；
- 未经授权浏览整台电脑、从手机创建任意 SSH/WSL/Docker 连接；
- 在 Provider 不支持时承诺无损 attach、模型切换、权限更新或 Slash 命令；
- 把旧的通用“远程任务代执行”调度系统重新包装为 Session Remote。

## 6. 产品原则

- **本地优先**：Desktop 到 Host 走本地 IPC，Gateway 只扩展远程能力。
- **单一会话核心**：Desktop 与 Remote 不得各自启动同一 Session 的进程。
- **官方事实优先**：状态只由 Runtime 事件、协议响应或可验证的进程退出更新。
- **单写者优先**：同一原生 Session 默认只有一个 Host writer；无法安全共享时不抢占。
- **渐进兼容**：统一 UI 不等于抹平 Provider 差异，未知能力必须显示未知或不可用。
- **安全可撤销**：远程授权可显式停止、设备可撤销、旧连接必须立即失效。

## 7. 开发顺序

P0 已完成 Codex 常驻 Host 进程外置、Desktop 本地 IPC、单一 Session Registry/命令账本/事件日志，以及同一 daemon 的可选 Gateway Remote。后续顺序：

1. 将隐藏 daemon 进程升级为可安装、可升级、可诊断的用户级 OS 服务，并补独立停止控制。
2. Web/Android 完整消费统一能力报告、状态机和命令收据，补齐通知与离线体验。
3. 在官方协议和认证验证后接入 Claude Code 长时 Host；其他 Provider 按能力逐项开放。
4. 验证安全 attach/所有权转移；Provider 无可靠支持时保持只读或显式 resume。
5. 最后处理多实例 Gateway、安装升级和生产容量。

## 8. 验收标准

一个 Provider 只有完成以下闭环才标为 Remote 可用：

- 真实 Runtime 创建 Session，连续两轮保持原生 ID；
- Desktop 与至少一个远端客户端读取同一 Session；
- 正文、工具事件、审批和中断均由原生事件确认且只执行一次；
- 客户端/Gateway 断线期间 Host 继续运行，重连后无重复正文；
- Gateway 重启保留历史和命令收据；
- Host 重启准确标记控制丢失，并可显式恢复；
- 跨账户、撤销设备、重复命令、乱序帧和旧 Host generation 均被拒绝。

Fixture、回环网络、真实 CLI、桌面安装、Android 模拟器/真机和生产部署分别记录，不互相替代。
