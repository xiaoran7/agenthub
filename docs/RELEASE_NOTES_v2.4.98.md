## What's Changed in v2.4.98

### 🚀 Highlights & New Features

* **🎨 青空设计系统网关交互重构 (Aozora Gateway View Modernization)**:
  - **组件卡片化重构**：告别大段文字堆叠，全面重构为青空组件化架构。提供 4 组带有状态徽章与多色图标的看板卡片（网关服务探测、会话亲和保护、缓存命中预期、亲和调度协议键）。
  - **会话亲和与缓存演进拓扑图示 (Visual Topology Flow)**：直观对比“无亲和随机调度打穿 Prompt Cache (Cache Miss 80%+)”与“AgentHub 亲和粘滞 90%+ 缓存命中 (Cache Hit 90%+ ⚡)”，将技术优势具象化。
  - **快捷参数胶囊条**：支持一键复制网关 BASE URL、亲和 HEADER 以及探测来源。
  - **多环境代码切换器**：支持 Bash / Zsh、PowerShell (Windows)、Node.js / TS、Python (Anthropic SDK) 4 种语言即时切换与一键复制。
  - **场景接入卡片**：卡片化指引 Claude Code CLI、Codex / OpenCode 以及多智能体协同集群最佳实践。

* **⚙️ 顶栏设置齿轮按钮与系统设置弹窗 (Top Bar Settings Gear & Drawer)**:
  - **顶栏交互升级**：将顶栏关机按钮（⏻）替换为现代化设置齿轮按钮（⚙，快捷键 `Ctrl+,`）。
  - **全局设置弹窗**：提供 AgentHub Runner 核心版本信息、HTTP/SSE 监听端点、本地数据目录 (`~/.agenthub`) 展示与一键唤起系统文件资源管理器 (`/api/open-data-dir`)、明暗外观切换药丸、以及安全优雅的退出守护进程入口。

* **🖼️ 全尺寸高清原生系统托盘与任务栏图标 (Crisp Multi-Resolution High-DPI Icons)**:
  - **彻底根治模糊拉伸**：内置从 16×16 到 256×256（共 9 组分辨率）的标准多帧 ICO 图标。
  - **Win32 High-DPI 精准加载**：改用 `LoadImageW` 分别按 `SM_CXSMICON` 与 `SM_CXICON` 提取 1:1 像素级原生小图标，针对暗色托盘优化小尺寸高对比度透明电光蓝 Spoke。
  - **PWA 与桌面外壳高清适配**：新增 `icon-192.png` 与 `icon-512.png`，在 Edge PWA 与任务栏中呈现锐利画质。

* **🪄 现代 Fluent 原生上下文菜单 (Agent-Nexus Modern Tray Menu)**:
  - **Win32 现代托盘菜单**：加入运行中状态信息头、粗体控制台默认项、拓扑跳转、在系统浏览器中打开、数据目录快捷打开以及退出操作。

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.98-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.4.98-android-debug.apk` (Kotlin Compose mobile client)
* **Integrity**: `AgentHub-v2.4.98-SHA256.txt`
