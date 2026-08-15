# AgentPulse 产品需求文档

## 1. 产品概述

### 1.1 产品定位

AgentPulse 是一个支持网页端和移动端的统一平台，用于连接用户自己的
电脑，并集中查看电脑上已经安装、登录和运行的主流 Coding Agent。

首批支持：

- Claude Code
- OpenAI Codex
- Grok Build

后续可以扩展其他 Coding Agent。

平台不重新提供模型服务，也不要求用户在平台中重新登录 Claude 或 Codex。
它通过电脑端轻量级客户端读取用户本机已有 Agent 的 Session 信息，将必要的
元数据安全同步至云端，让用户可以跨设备、跨项目、跨 Agent 查看工作状态。

### 1.2 产品定义

一个跨 Claude Code、Codex 等本地 Coding Agent 的统一 Session 监控与远程
接入平台。用户只需在电脑上安装本地客户端，即可通过网页或手机统一查看多个
Agent、多个项目和多个设备的实时工作状态。

### 1.3 当前产品目标

产品从个人只读面板扩展为订阅制多租户 Agent 平台。监控仍保持默认只读，新增能力按
账户、订阅权益和设备授权逐层开放：

- 获取已登录电脑上的 Agent Session
- 准确识别并统一不同 Agent 的运行状态
- 低延迟同步必要的 Session 元数据
- 在网页端和移动端清晰展示设备、Agent 和 Session 状态
- 在默认配置下保护用户凭据、代码、对话和环境信息
- 支持注册、登录、订阅权益和按用户隔离的设备与 Session
- 支持邮箱验证、忘记密码、登录后改密和网页登录会话管理
- 支持个人/Team 工作区切换、成员邀请和 Owner/Admin/Member 角色权限
- 将 Claude Code、Codex 与 Grok Build 作为一级 Agent 来源分类
- 通过服务器可靠中转任务，由用户自己的 Runner 主动领取，并在本机确认后交给 Agent 执行

### 1.4 当前非目标与后续边界

第一阶段暂不实现：

- 在平台内托管 Claude 或 OpenAI 账号
- 重新提供或代理模型服务
- 手机端回复 Agent
- 批准或拒绝权限请求
- 中断或恢复任务
- 远程控制用户电脑
- 完整对话、项目代码或环境变量同步
- 未经设备侧授权的任意远程命令执行
- 转售或集中托管用户的 Claude/OpenAI 凭据与模型额度
- 首轮账户改造不直接绑定具体支付厂商；先稳定租户隔离、邮箱验证、会话安全和签名权益事件

### 1.5 订阅权益

| 方案 | 设备 |   历史 | 远程任务                 | 团队成员 |
| ---- | ---: | -----: | ------------------------ | -------: |
| Free |    1 |   7 天 | 不支持                   |        1 |
| Pro  |    5 |  90 天 | 支持，最多 3 个并发任务  |        1 |
| Team |   25 | 365 天 | 支持，最多 10 个并发任务 |       10 |

支付供应商只负责提供可验证的订阅事件，服务端将其转换成上述权益。前端支付完成页不得
直接修改权益；取消或欠费在已付周期结束前保留原权益，周期结束后按 Free 权益降级。

当前实现保持上述默认边界：未配置供应商时只提供签名事件入口；配置 Stripe 的密钥、Price
ID 和 HTTPS webhook 后，才开放 Checkout、Customer Portal 与订阅事件同步。无论支付页
或 webhook 是否可用，权益都只由服务端 Store 的已验签事件计算。

## 2. 用户痛点

- 不同 Coding Agent 相互独立，需要分别进入不同平台查看。
- 缺少统一界面展示 Claude Code、Codex 等 Agent 的 Session。
- 用户离开电脑后，难以快速了解 Agent 当前在做什么。
- 多台电脑、多个项目和多个 Session 缺少统一管理入口。
- 实时运行 Session 与历史 Session 不容易统一区分和检索。
- Agent 等待输入或权限批准时，用户可能无法及时发现。

## 3. 目标用户与核心场景

### 3.1 目标用户

- 同时使用 Claude Code 和 Codex 的开发者
- 在多台电脑上运行 Coding Agent 的用户
- 经常离开工作电脑但需要关注任务进度的用户
- 同时管理多个项目和并行 Session 的用户
- 需要多人共享设备、Session 和任务视图的小型开发团队

### 3.2 核心场景

1. 用户离开电脑后，通过手机查看某个 Agent 是否仍在工作。
2. 用户快速发现哪些 Session 正在等待输入或权限批准。
3. 用户在一个页面查看多台电脑上的 Claude Code 和 Codex Session。
4. 用户区分当前实时任务、最近完成任务和历史 Session。
5. 用户查看 Session 所属项目、运行时长、最后活动时间和当前任务摘要。
6. Team 成员在保留个人工作区的同时，切换到团队工作区共同查看设备、Session 和任务。

## 4. 第一阶段功能需求

### 4.1 设备管理

平台需要展示：

- 用户已绑定的电脑
- 设备名称和设备 ID
- 设备当前在线或离线状态
- 最后在线时间
- 每台设备已检测到的 Agent

本地客户端主动连接服务器，不要求用户开放公网端口。

### 4.2 Agent 检测

本地客户端需要：

- 检测电脑上是否安装 Claude Code、Codex 和 Grok Build
- 判断 Agent 是否存在可用的本地登录状态
- 使用现有登录状态调用官方 CLI、SDK 或本地接口
- 保留 Agent 类型、版本和可用性信息
- 支持后续通过适配器扩展新的 Agent

平台不得在未获得用户明确授权时读取 Agent 的 OAuth Token；用户开启本地凭据管理后，Runner
可以读取受支持的 auth 文件、OS Keychain/Keyring、环境变量和官方 CLI 的认证状态。
原始凭据只允许保存在本机受保护的凭据缓存中，默认不得上传到 Gateway、数据库或浏览器。

### 4.3 Session 采集

本地客户端需要读取并监听：

- Agent 类型
- Agent 原始 Session ID
- Session 所属设备
- 项目名称或工作目录的安全标识
- Session Presence（实时、持久化或未知）
- Agent 原始状态
- 平台统一活动状态
- 当前任务摘要（仅在能够安全获得时）
- 等待原因
- 开始时间
- 最后活动时间
- 最近状态变化

### 4.4 Session 展示

网页端和移动端需要支持查看：

- 当前在线设备
- 每台设备安装并登录的 Agent
- 正在运行的 Session
- 等待用户处理的 Session
- 最近完成的 Session
- 历史 Session
- Session 所属项目或工作目录
- Session 当前状态和最后活动时间

### 4.5 Session 详情

第一阶段详情页显示：

- Agent 名称
- 设备名称
- 项目名称
- 当前 Presence 与统一活动状态
- Agent 原始状态
- 工作时长
- 开始时间
- 最后活动时间
- 当前任务摘要
- 等待原因
- 最近状态变化
- 状态来源与置信度

### 4.6 Team 工作区

- 每个注册账户自动拥有个人工作区，加入团队不会改变个人数据归属。
- Team 所有者可邀请已注册账户为管理员或成员；邀请 72 小时有效，Token 只存摘要并通过
  URL fragment 传递，链接只在创建时展示。
- 已加入成员和待处理邀请共同占用 Team 的 10 人上限，接受邀请时必须重新校验当前套餐。
- 所有者可管理管理员与成员；管理员只能管理普通成员；所有者不能被移除或降级。
- 用户可在个人与已加入工作区之间切换。账户、改密和登录会话操作作用于当前登录人；
  设备、Session、任务、SSE 与 Push 作用于当前工作区。
- 成员被移除后，其已选择该团队的 Web 会话自动回到个人工作区。

## 5. 统一状态模型

不同 Agent 的原始状态需要映射为统一状态，同时必须保留原始状态及其附加信息，
避免在标准化过程中丢失细节。

“历史会话”与“正在工作、完成、失败”等活动状态不是同一维度。例如，一个
Session 的最近任务可以已经完成，但进程仍在线并可以继续；也可能只剩历史记录，
而最近一次任务是失败。因此平台分别记录 Presence 和 Activity Status。

### 5.1 Presence

| Presence | 含义                                          |
| -------- | --------------------------------------------- |
| 实时     | 最近收到可信实时信号，或确认 Agent 进程仍存活 |
| 持久化   | 仅存在持久化记录，当前没有可信实时信号        |
| 未知     | 无法确认是否仍有活动进程                      |

### 5.2 Activity Status

| 统一状态     | 含义                                        |
| ------------ | ------------------------------------------- |
| 正在工作     | Agent 正在执行任务、调用工具或生成结果      |
| 等待用户输入 | Agent 需要用户补充信息或回复                |
| 等待权限批准 | Agent 正在等待用户批准敏感操作或工具调用    |
| 空闲         | Session 仍然可用，但当前没有执行任务        |
| 已完成       | 最近一次任务正常完成                        |
| 执行失败     | 最近一次任务因错误而结束                    |
| 已停止       | 最近一次任务或 Session 被用户或系统主动终止 |
| 未知状态     | 无法可靠识别或尚未适配的状态                |

“历史 Session”通过 `presence = persisted` 筛选，而不是作为 Activity Status。

每次状态记录至少包含：

- `presence`：实时、持久化或未知
- `activity_status`：平台统一活动状态
- `raw_status`：Agent 原始状态
- `raw_status_detail`：经过白名单过滤的 Agent 特有状态信息
- `status_changed_at`：状态变化时间
- `waiting_reason`：等待原因（如适用）
- `source`：状态来源
- `confidence`：权威、观察、推断或未知

## 6. 核心数据对象

### 6.1 Device

- 用户 ID
- 设备 ID
- 设备名称
- 客户端版本
- 操作系统
- 在线状态
- 最后心跳时间
- 首次绑定时间

### 6.2 Agent Installation

- 设备 ID
- Agent 类型
- Agent 版本
- 安装状态
- 登录状态
- 最后检测时间

登录状态只表示本机 Agent 是否可用，不包含或上传登录凭据。

### 6.3 Session

- 平台 Session ID
- Agent 原始 Session ID
- 设备 ID
- Agent 类型
- 项目名称
- 工作目录的安全标识
- 当前 Presence
- 当前统一活动状态
- Agent 原始状态
- 等待原因
- 当前任务摘要
- 开始时间
- 最后活动时间
- 状态更新时间
- 状态来源与置信度

### 6.4 Session Status Event

- Session ID
- Presence
- 统一活动状态
- Agent 原始状态
- 等待原因
- 状态来源
- 状态置信度
- 事件时间

## 7. 电脑端客户端职责

本地客户端或 Runner 负责：

- 使用电脑上现有的 Claude Code 登录状态
- 使用电脑上现有的 Codex 登录状态
- 调用官方 CLI、SDK 或本地接口读取 Session
- 监听 Agent 状态变化
- 将必要的 Session 元数据同步到云端
- 主动建立并维持与云端的安全连接
- 网络中断后恢复同步

本地客户端默认不得：

- 读取或上传 OAuth Token
- 托管 Claude 或 OpenAI 账号
- 上传完整项目代码
- 上传完整对话
- 上传环境变量或 `.env` 文件
- 暴露用户电脑的远程控制权限

## 8. 云端平台职责

云端负责：

- 用户账号和设备绑定
- 多设备管理
- Session 元数据存储
- 实时状态同步
- 网页端和移动端数据展示
- 根据心跳判断设备离线
- 为后续消息通知能力提供基础

第一阶段云端仅保存实现监控所必需的 Session 元数据。

## 9. 信息架构

推荐主要页面：

1. 设备列表
2. 单设备 Agent 列表
3. 正在运行的 Session
4. 等待处理的 Session
5. 最近完成的 Session
6. 历史 Session
7. Session 详情

产品界面围绕“快速查看 Agent 状态”设计，不做成手机 IDE。

## 10. 安全与隐私原则

- 最小化采集：只采集实现状态监控所必需的数据。
- 默认不上传：代码、完整对话、Token、环境变量默认不离开用户电脑。
- 主动出站连接：本地客户端主动连接云端，不开放公网入站端口。
- 凭据隔离：云端不持有 Claude 或 OpenAI 的用户凭据。
- 明确授权：未来新增输出、Diff、回复或控制能力时，必须单独获得用户授权。
- 数据可见：用户应能查看平台采集了哪些字段。
- 设备可撤销：用户应能解除设备绑定并使该设备失去同步权限。

## 11. 非功能需求

### 11.1 准确性

- 统一活动状态必须可以追溯到 Agent 原始状态和状态来源。
- 推断状态必须显示置信度，不能伪装成 Agent 提供的权威状态。
- 不确定的状态应标记为“未知状态”，不得猜测为已完成或失败。
- 实时 Session 和历史 Session 必须明确区分。

### 11.2 实时性

- Agent 状态变化后应尽快同步到平台。
- 具体延迟指标在验证 Claude Code 和 Codex 的可用接口后确定。

### 11.3 可靠性

- 本地客户端短暂离线后应自动重连。
- 状态事件应能去重，避免重复展示。
- 云端应基于心跳和超时机制可靠判断设备离线。

### 11.4 可扩展性

- 每种 Agent 使用独立适配器。
- 统一数据模型不能依赖某个 Agent 的专有字段。
- 原始状态字段允许保存 Agent 特有信息。

## 12. MVP 验收标准

第一阶段完成时，至少满足：

1. 用户可以绑定一台电脑，并在网页端看到其在线或离线状态。
2. 本地客户端可以检测 Claude Code 和 Codex 的安装及可用状态。
3. 平台可以展示两个 Agent 的当前及历史 Session。
4. 每个 Session 至少展示设备、Agent、项目、Presence、统一活动状态和最后活动时间。
5. 平台能够区分正在运行、等待处理、完成、失败、停止和历史 Session。
6. 状态详情同时保留统一活动状态、Agent 原始状态、来源和置信度。
7. 设备断开连接后，平台能在规定超时时间内标记其离线。
8. 默认情况下不会上传 OAuth Token、完整代码、完整对话、环境变量或 `.env`。
9. 本地客户端无需用户开放公网端口。
10. 网页端和移动端均可完成核心只读查看流程。
11. Pro/Team 用户可向自己的已绑定设备提交 Claude Code 或 Codex 任务；Runner 必须
    使用本机项目指纹映射并在执行前要求本地确认，Free 与跨用户请求由服务端拒绝。
12. Team 所有者可邀请成员，成员接受后可切换共享工作区；角色权限、席位上限和移除后的
    会话回退均由服务端强制执行，切换工作区不得泄露所有者账户资料。
13. 用户可查看自己的账户安全/订阅审计和当前工作区的 Team 审计；成员移除后立即失去原
    工作区审计读取权，事件不得包含完整邮箱、密码、Token、IP、请求体或 Agent 内容。

## 13. 后续迭代

任务中转完成基础本地确认闭环后，可以逐步增加：

- 手机端回复 Agent
- 批准或拒绝权限请求
- 中断任务
- 恢复历史 Session
- 查看最近输出
- 查看代码 Diff
- 查看测试结果
- 消息通知
- 多 Agent 统一任务分配
- Claude 和 Codex 互相 Review
- 多 Agent 工作流编排
- 多电脑统一调度

## 14. 本地凭据、配额和认证管理（授权扩展）

本节覆盖原有“默认不读取 OAuth Token”约束的授权扩展。授权扩展只在用户明确打开
“本地凭据管理”后生效，默认关闭；关闭后 Runner 只能读取官方非敏感登录状态。

### 14.1 允许读取的本地来源

- Codex：`CODEX_HOME/auth.json`、`CODEX_HOME/.credentials.json`、Codex 官方
  App Server `account/read`，以及 Codex 配置的 OS Keychain/Keyring 后端。
- Claude Code：`CLAUDE_CONFIG_DIR/.credentials.json`、平台默认 credentials 文件、
  OS Keychain/Keyring，以及 `claude auth status --json`。
- 用户显式配置的 `OPENAI_API_KEY`、`CODEX_ACCESS_TOKEN`、
  `CLAUDE_CODE_OAUTH_TOKEN`、`CLAUDE_CODE_OAUTH_REFRESH_TOKEN` 等环境变量。

Runner 必须使用固定 provider allow-list 和字段 allow-list，只提取认证所需字段；不得
递归扫描用户目录、读取浏览器 Cookie、读取无关密码库或把完整文件复制到缓存。

### 14.2 Token 生命周期

- access token、refresh token、token expiry、scope、account id 和认证模式属于敏感字段。
- 原始 token 只进入本机加密缓存，CLI 的 `auth token --reveal` 才能在用户明确要求时
  输出，Web/UI/API 只收到 token 是否存在、脱敏账号、过期时间和状态。
- Codex 优先通过 App Server 的 `account/read`、`account/login/start`、`account/logout`
  完成刷新和重新认证，不自行复制 Codex 的 OAuth 客户端密钥或实现未公开的刷新端点。
- Claude 优先委托 `claude auth login/logout` 管理本地凭据；若用户提供 refresh token，
  通过 `CLAUDE_CODE_OAUTH_REFRESH_TOKEN` 和官方 CLI 完成重新认证，不在 Runner 中伪造
  Anthropic 的私有 OAuth 协议。
- 刷新失败必须转换为 `reauth_required` 或 `unknown`，不能把过期 token 继续标记为有效。

### 14.3 配额和用量

- Codex 使用 App Server `account/rateLimits/read` 获取窗口使用率、重置时间和可用
  reset credit；消费 reset credit 后必须再次刷新快照。
- Claude 只展示官方 CLI/状态行能够提供的 rate limit 字段；如果当前 CLI 没有机器可读
  配额接口，状态为 `unavailable`，不根据请求次数猜测配额。
- 配额快照缓存到本机并同步脱敏快照；原始 token、refresh token、完整响应和 Cookie
  永不同步。

### 14.4 认证操作

本阶段提供 Runner CLI 的 `auth status`、`auth refresh`、`auth login`、`auth logout`、
`auth quota` 和显式 `auth token --reveal`。所有会改变登录状态、消费 reset credit 或
输出原始 token 的命令必须由用户主动执行；后台轮询只能读取、刷新过期 token 和更新
缓存，不能自动登出、消费配额或发起交互式登录。

## 14. 待验证事项

进入技术设计前需要通过原型验证：

- Claude Code 可以通过哪些官方或稳定本地接口读取实时及历史 Session
- Codex 可以通过哪些官方或稳定本地接口读取实时及历史 Session
- 两种 Agent 能否可靠识别“等待输入”和“等待权限批准”
- 在不上传完整对话的前提下，如何安全生成当前任务摘要
- 项目路径在云端展示时采用原始路径、别名还是脱敏标识
- 独立原生 Android App 已确定，继续与响应式 Web/PWA 共享同一只读 API、Session
  分类语义和视觉系统；iOS 客户端形态仍待后续决定
- 第一阶段目标状态同步延迟和设备离线超时时间
