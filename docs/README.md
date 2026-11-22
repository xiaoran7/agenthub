# AgentHub 文档索引

实现细节以源码与单测为准；生产与设备结论只陈述可验证证据。

## 权威文档导航

| 文档 | 职责 |
| :--- | :--- |
| [PRD](PRD.md) | 产品定义、组件边界、Session 四态语义与验收标准 |
| [ARCHITECTURE](ARCHITECTURE.md) | `agenthubd`、内嵌控制台、多智能体 Adapter、Gateway、Vite/Nginx Web 与 Android 拓扑 |
| [SYNC_PROTOCOL](SYNC_PROTOCOL.md) | 本地 IPC v2、Remote v3 WSS、代际隔离、幂等命令与脱敏事件投影 |
| [DESIGN_SYSTEM](DESIGN_SYSTEM.md) | 远程跨端令牌（Paper/Graphite/Midnight）、本机控制台（magpie）与 60 FPS 约束 |
| [DEPLOYMENT](DEPLOYMENT.md) | Docker 自部署 / Host 生产模式 / Systemd 裸机 / HTTPS 反代与 Windows 自启 |
| [PITFALLS](PITFALLS.md) | 协议、隐私、SQLite/Nginx 反代与跨平台进程树避坑检查表 |
| [M4_VALIDATION](M4_VALIDATION.md) / [ANDROID_VALIDATION](ANDROID_VALIDATION.md) | 安全脱敏、断网重放与移动端历史实测基线 |
| [OPEN_SOURCE_GUIDE](OPEN_SOURCE_GUIDE.md) | 仓库元数据、[收录指南](COMMUNITY_SUBMISSIONS.md)、[问答种子](DISCUSSIONS_QA_SEEDS.md) 与 [宣发物料](LAUNCH_COPYWRITING.md) |
| [CHANGELOG](CHANGELOG.md) | 全量历史版本演进记录 |

## 当前各端实现状态（v2.7.4）

| 模块 | 当前实现事实 |
| :--- | :--- |
| **Runner (`crates/runner`)** | 纯 Rust 单二进制控制面（稳态 `~13.5 MB` RSS，全仓单文件 `<=800` 行），含 `:4200` 内嵌控制台、按需隔离 `ui` WebView2 子进程、原生 Win32 托盘与 `UsageIndexDb` 冷热分层用量账本 |
| **Runtime Adapters** | 原生双向支持 Codex `app-server`、Claude Code、Google Antigravity（`agy` `stream-json` + 实时模型探测）与通用 ACP stdio；ZCode/Grok/Gemini CLI 支持受控发现与用量归集 |
| **Gateway (`services/api`)** | Fastify v3 Remote 中枢：Ed25519 设备验签、离线防抖宽限（`offlineGraceMs`）、`storableFrame()` 瘦身事件库（`20,000`/设备上限）与按需 `/hello` 端点 |
| **Web (`apps/web`)** | Vite 8 静态多页 + `nginx:1.29-alpine` 同源反代（镜像 `96.7 MB`、常驻 `~8.4 MiB`），含跨机 `"要你处理"` 置顶审批条与乐观消息确认 |
| **Android (`apps/android`)** | Kotlin + Jetpack Compose 原生 Remote v3 客户端（v2.6.0 重写解冻），支持四级会话树、增量折叠转录、Diff 审批、附件与三主题切换 |
