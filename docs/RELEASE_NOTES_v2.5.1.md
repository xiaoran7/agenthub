## What's Changed in v2.5.1

### 🚀 Highlights & New Features

* **🧭 主界面收敛成四页 (Four Pages, One Settings)**:
  - 主界面只保留 **会话 / 智能体 / 用量 / 工作区** 四页；"模型"与"网关"两个视图删除，路由拓扑本来就该由上游 CPA（cliproxyapi）管，本机控制台不再画任何拓扑图；
  - "能力"从主视图搬进设置页，成为 **能力生态** 分区：全部 / 技能 / MCP / 钩子四个子筛选、跨分区搜索、注册 MCP 服务、重新探测；
  - "设备与配对"并入设置页 **设备与网关** 分区，设备 ID、硬件公钥、绑定状态、配对表单、上游网关 BASE URL、亲和标头、上游模型同步同处一页；
  - 页脚常驻的端口/端点显示删除，这些信息"关于"分区里都有。

* **📊 用量页按 magpie 的用法视图重做 (Usage Page, after magpie)**:
  - 顶部 **概览 / 请求 / 会话 / 上下文** 四段标签，周期 **今天 / 7 天 / 30 天 / 全部**，刷新频率手动或 10 秒 / 30 秒 / 1 分钟，选择全部持久化；
  - 概览：五格 KPI（请求、Token、预估花费、缓存命中、输出速度，含失败/重试与缓存命中率条）、堆叠柱状趋势图（Token / 费用 / 请求 / 速度，按模型 / 模型·智能体 / 智能体分组）、右侧排行条、53 周热力图（Token / 请求 / 费用，周一起，悬停看日详情）；
  - 请求表列出每次调用的时间、智能体、模型、输入/输出/缓存、费用与状态，支持"全部 / 失败 / 重试后成功"筛选、搜索与 CSV 导出（UTF-8 BOM）；
  - 数据来自新增的逐请求账本 `GET /api/usage/ledger`：按 `message.id` + `requestId` 去重，API 错误行计为失败，同一会话 10 分钟内的失败后成功配对为"重试后成功"，回复速度由同一条消息首末行时间戳计算。

* **🎨 模型图标逐字节取自 magpie (Vendor Marks from magpie)**:
  - 19 个 SVG 与 1 个 PNG 从 magpie `internal/gui/assets/icons/` 原样复制进 `crates/runner/resources/icons/`，经 `/icons/<name>.<ext>` 提供，按 magpie 的前缀表给模型配图；
  - 删掉页面里手绘的近似 logo，未知模型显示中性空位；单色图标在深色主题下自动反相。

* **🤖 智能体只留五个产品 (Five Agent Products)**:
  - Coding Assistant（`claude_code`）、Claude Desktop（`claude_desktop`）、Codex、Grok、Gemini CLI；Claude Desktop 与 Coding Assistant 是两款独立产品，按数据目录与 `entrypoint` 严格区分；
  - Antigravity / ZCode / Cursor / Cline 等只作为会话来源统计，不再出现在智能体卡片里。

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.5.1-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.5.0-android-debug.apk` (frozen per AGENTS.md; unchanged from v2.5.0)
* **Integrity**: `AgentHub-v2.5.1-SHA256.txt`
