# Agent-Platform

一个跨 Claude Code、OpenAI Codex 等本地 Coding Agent 的统一 Session
监控与远程接入平台。

用户在电脑上安装轻量级本地客户端后，可以通过网页端或移动端统一查看：

- 多台电脑的在线状态
- 每台电脑已观测到的 Coding Agent
- 多个项目和 Session 的实时状态
- 等待用户处理、最近完成及历史 Session

## 当前阶段

第一阶段聚焦安全、准确、低延迟的只读 Session 监控，不提供远程控制、
模型服务或完整代码与对话上传能力。

## 文档

- [产品需求文档](docs/PRD.md)
- [技术可行性报告](docs/TECHNICAL_FEASIBILITY.md)
- [MVP 系统架构](docs/ARCHITECTURE.md)
- [设备同步协议](docs/SYNC_PROTOCOL.md)
- [跨端设计系统](docs/DESIGN_SYSTEM.md)
- [Android 验收记录](docs/ANDROID_VALIDATION.md)
- [变更日志](docs/CHANGELOG.md)
- [设计取舍](docs/MISC.md)
- [踩坑集](docs/PITFALLS.md)

## 开发

- [Runner 开发说明](crates/runner/README.md)
- [Gateway API 开发说明](services/api/README.md)
- [共享同步契约](packages/contracts/README.md)

当前 Runner 已支持 Claude/Codex 一次性和持续采集、状态标准化、隐私脱敏、
SQLite Outbox、Adapter 退避与健康诊断，以及本地设备身份和签名同步批次。
常驻 `run` 配置 Gateway 后会在独立线程自动排空 Outbox；新事件立即唤醒同步，
网络失败按独立退避恢复，不阻塞 Agent 采集。
云端已具备受认证的一次性设备绑定、软撤销、签名批次验收、内存幂等写入与只读
Session 查询；绑定码和设备生命周期在 PostgreSQL 部署中持久化。
Runner 已可通过 HTTPS（本地开发允许回环 HTTP）完成设备绑定、批次上传和确认清理，
并支持为私有 Gateway 显式配置 CA 根证书而不关闭证书验证。
Gateway 已提供 PostgreSQL 迁移和事务 Store；未配置数据库时仍使用开发期内存存储。
Web 面板（`apps/web`）已提供设备列表、会话分类视图与状态时间线的只读展示，
并经 `GET /v1/stream`（SSE）实时刷新，轮询保留为兜底。会话进入等待输入或等待
权限时可通过 Web Push 推送通知（需在 Gateway 配置 VAPID 密钥）。
独立原生 Android 客户端（`apps/android`）已提供同一五类 Session 视图、待处理
入口、设备页、详情时间线和相邻会话巡检；它直接访问 Gateway 只读接口，每 5 秒
刷新，并用 Android Keystore 加密保存部署凭据。Web、桌面宽屏和 Android 共享
[跨端设计系统](docs/DESIGN_SYSTEM.md) 的颜色、圆角、排版与状态表达。
个人部署已支持服务端 Bearer 访问令牌：Gateway 只保留令牌摘要，Web 同源代理在
服务端注入凭据，令牌不进入浏览器存储。设置面板可签发一次性绑定码并软撤销设备。
联合登录、跨实例广播与心跳尚未接入；
Service Worker 只服务推送，不做离线缓存。
M4 安全与稳定性阶段已完成自动化基线；PostgreSQL 18.4 的 12 设备/3000 Session
并发工作负载、外部不可达后的 Outbox 保留/恢复，以及 Codex CLI 0.144.2 当前版本
只读采集也已现场通过；本机真实 TLS 握手和私有 CA 恢复已进入回归套件。真实 Agent
升级矩阵、部署网络证书故障/长时间离线和系统级 Web Push 呈现仍待环境验收，详见
[M4 验收记录](docs/M4_VALIDATION.md)。

### Web 面板

先启动 Gateway（见 [Gateway 说明](services/api/README.md)），再启动面板：

```powershell
npm.cmd run dev --workspace @agent-platform/web
```

开发服务器监听 `http://localhost:3000`，并把 `/v1` 请求代理到
`http://127.0.0.1:3001`（可用 `GATEWAY_ORIGIN` 环境变量覆盖）。个人部署还应在
Web 服务端设置与 Gateway 相同的 `GATEWAY_ACCESS_TOKEN`；同源代理会注入令牌，
浏览器无法读取或覆盖。面板右上角「设置」中的用户标识仅用于 Gateway 显式开启
开发认证时的本地调试。直连要求 HTTPS 或回环 HTTP，且不会获得服务端访问令牌，
令牌部署请保持网关地址为空并使用同源代理。

「设置」里的「推送通知」开关需要 Gateway 配置了 VAPID 密钥（见 Gateway 说明），
且页面运行在 https 或 localhost 上；开启后会话进入等待输入/等待权限时推送通知，
点击通知会聚焦已打开的面板并定位到该会话。

### 原生 Android App

Android 客户端是独立 Kotlin + Jetpack Compose 应用，不是 WebView 或 PWA 封装。
本机需要 JDK 17+ 与 Android SDK 36；仓库已包含 Gradle Wrapper：

```powershell
cd apps/android
.\gradlew.bat testDebugUnitTest assembleDebug
```

调试 APK 输出到 `apps/android/app/build/outputs/apk/debug/app-debug.apk`。连接本机
Gateway 时先执行：

```powershell
adb reverse tcp:3001 tcp:3001
```

然后在 App 设置中使用 `http://127.0.0.1:3001` 和本机开发用户。生产部署必须填写
HTTPS Gateway 和访问令牌；令牌经 Android Keystore 加密后写入不可备份的私有配置，
不会进入日志、URL 或仓库。当前原生版本只读并使用周期刷新；原生推送、发布签名与
实体设备矩阵仍属于后续交付边界，详见
[Android 验收记录](docs/ANDROID_VALIDATION.md)。

Rust 与 TypeScript 全量检查：

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
cd apps/android
.\gradlew.bat testDebugUnitTest lintDebug assembleDebug
```
