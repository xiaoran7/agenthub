## What's Changed in v2.4.99

### 🚀 Highlights & New Features

* **🧩 智能体扩展生态纳管架构 (Federated Multi-Agent Library Architecture)**:
  - **吸收 Magpie 统一资料库设计**：参考上游开源项目 magpie（`internal/library/`）架构，在 AgentHub 中全面打通并纳管多智能体生态扩展资产（Skill、MCP Server、Hook）。
  - **Skill 扩展技能纳管**：深度扫描 `~/.claude/skills`、当前工作区 `.claude/skills` 及 Codex 技能目录，自动提取 Markdown YAML Frontmatter、技能名称、首行功能摘要以及自动化脚本支持标识（`has_scripts`）。
  - **MCP Server 上下文协议服务**：集中解析 Claude（`~/.claude.json`）、Codex（`~/.codex/config.toml`）、工作区（`.mcp.json`）以及 AgentHub 自定义（`~/.agenthub/mcp.json`），自动归一化为 `stdio` 本地进程与 `sse`/`http` 远程端点，保留启动命令、参数、环境变量与自定义 HTTP Headers。
  - **Hook 生命周期钩子**：深度解析 Claude `settings.json` 与 Codex hooks，精准识别捕获 `PreToolUse`、`PostToolUse`、`SessionStart`、`PermissionRequest` 等自动化执行事件。
  - **零额外依赖与安全健壮性**：原生编写高效可靠的 TOML 流式状态机与反斜杠字符还原器（`parse_mcp_from_codex_toml` / `unescape_toml_str`），支持完全离线编译与严苛的 Windows 路径解析。
  - **多源数据去重与智能体归集合并**：同名能力在多个智能体或工作区同时存在时自动聚合并保留所有所属智能体标签，并在卡片底部渲染专属微型矢量胶囊徽章。

* **🎨 青空设计系统生态能力看板 (Aozora Capabilities Console View)**:
  - **全新能力控制台**：在内嵌 Web 控制台开辟独立的“能力 (Capabilities)”视图，摒弃大段纯文字，全面践行青空规范的“图标 + 卡片 + 组件结合”。
  - **4 组核心指标看板**：技能数（Skills / Indigo）、MCP 服务（MCP Servers / Green）、钩子（Hooks / Amber）、受管智能体（Agents / Blue）。
  - **多维过滤条与全局即时检索**：支持全部 / 技能 / MCP 服务 / 钩子 4 类维度即时过滤，并提供全局关键词模糊匹配。
  - **精美卡片交互**：
    - Skill 卡片：提供参数模板展示与自动化脚本指示；
    - MCP 服务卡片：等宽终端命令行预览与一键复制命令胶囊，远程端点展示与 Headers 计数；
    - Hook 卡片：事件类型彩色胶囊徽章、执行超时指示与指令一键复制。
  - **交互式 MCP 服务注册与热探测**：新增“+ 注册 MCP 服务”青空弹窗组件与“重新探测”即时同步按钮。
  - **60 FPS 性能铁律保障**：严格硬件加速标注，杜绝滚动掉帧。

* **📊 移动端上下文容量表、多选问答与增量 Markdown 渲染 (Context Capacity, Multi-Select Questions & Incremental Markdown)**:
  - **上下文用量终于看得见**：Runner 把 Claude `result`、Codex `thread/tokenUsage/updated` 与 ACP `usage_update` 统一归一化为 `{used, window}`，手机会话页标题栏新增容量环，点按展开 `64k / 200k · 32%` 内联容量条；容量未上报时明确写“容量未上报”，不编造百分比。
  - **多选问答**：问答卡片识别 `multi_select`，显示勾选框与“可多选”标识，可勾选多项后一次提交，命令体为字符串数组；未选择时提交按钮禁用，单选卡片行为不变。
  - **流式渲染更省**：新增增量 Markdown 解析缓存，追加内容只解析新增尾部，代码块闭合后不再重建；围栏被拆开或整段替换时回退全量解析，并有单测直接断言“增量结果 == 全量解析”。

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.99-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.4.99-android-debug.apk` (Kotlin Compose mobile client)
* **Integrity**: `AgentHub-v2.4.99-SHA256.txt`
