# AgentHub（多种本地 AI Agent 的统一远程控制入口）

> **AgentHub 的核心方向与唯一判断标准：**
> **AgentHub = 多种本地 AI Agent 的统一远程控制入口。**
> 
> 具体只做三件事：
> 1. **发现**：发现本机正在运行的 Codex / Claude 等官方 Agent；
> 2. **接管**：接管它们现有的 Session；
> 3. **暴露**：把这些 Session 安全暴露给手机端和 Web 端继续使用。
> 
> **核心边界铁律：**
> - **不替代官方 Agent，不自己再造 Agent，不改变用户本地使用习惯。**
> - 本地电脑依然正常使用官方 Codex CLI、Claude Code 等工具；用户离开电脑后，统一从 AgentHub 手机端或 Web 端继续查看进度、继续对话、处理工具审批、停止任务和查看运行结果。
> - **功能判断红线**：以后所有功能都围绕一个唯一标准——**“这个功能是不是在帮助用户‘远程继续使用本机已经运行的 Agent’？”**，如果不是，坚决不做！

---

## 核心工作拓扑

```
【本地环境】                            【云端中转】                 【远端接入】
用户原生使用的 Codex / Claude 会话
        │ (发现与接管)
AgentHub 本机 Runner / 守护 Host ───(出站安全通道)───► AgentHub Gateway ◄───► AgentHub 手机端 (Android)
        │                                                                ◄───► AgentHub Web 端
本地桌面继续正常工作                                                      (离开电脑后随时随地继续看、聊、审批、停止)
```

## 开发入口

在仓库根目录安装 Node 依赖；Rust、Gateway 与各客户端环境按下方组件说明准备。

```powershell
npm.cmd ci
# Windows 桌面
npm.cmd run tauri --workspace @agent-pulse/desktop -- dev
# Web：先按 services/api/README.md 启动 Gateway
npm.cmd run dev --workspace @agent-pulse/web
```

Web 开发入口为 `http://localhost:3000`，默认代理 Gateway `http://127.0.0.1:3001`，可用 `GATEWAY_ORIGIN` 覆盖。桌面开发可用 `AGENT_PULSE_SERVER_URL` 指向回环服务。

Android 使用 JDK 17+、SDK 36 和 `apps/android/gradlew.bat`；设备连接本机 Gateway 可用 `adb reverse tcp:3001 tcp:3001`。安装与打包前按 [AGENTS.md](AGENTS.md) 同步版本。

## 验证

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm.cmd test
npm.cmd run typecheck
npm.cmd run format:check
```

Android 单独运行 `apps/android` 下的 `testDebugUnitTest` 与 `lintDebug`；打包命令与产物位置见客户端说明。仅文档调整不运行构建。

## 边界与文档

目标和实现阶段分开维护：发现历史、恢复上下文、连接运行中会话是三种能力。新 Remote 只在原生协议确认后报告可控制；现有只读采集、元数据同步与桌面本地执行在迁移期间保留，不能作为新 Remote 已完成的证据。

- 新 Remote 链路沿用原生 Agent 登录，不读取、缓存或刷新 Provider 凭据。已接入远程控制的内容同步到授权 Gateway；历史扫描不自动上传全文。
- Runner 主动出站；设备绑定、签名批次、Outbox 与断线恢复的实际合同见同步协议。
- 生产使用 HTTPS；私有 CA 配置不能关闭证书验证。个人兼容令牌通过 Web 同源服务端代理注入，不暴露给浏览器。
- 邮件、支付、Web Push、真机和实际 Agent CLI 的可用性需要对应环境验收，不能从自动化基线推定。

| 内容 | 权威入口 |
|---|---|
| 工程约束 | [AGENTS.md](AGENTS.md) |
| 产品与架构 | [PRD](docs/PRD.md)、[ARCHITECTURE](docs/ARCHITECTURE.md) |
| 同步与能力发现 | [SYNC_PROTOCOL](docs/SYNC_PROTOCOL.md)、[Launch Profile](docs/CAPABILITY_AND_LAUNCH_SPEC.md) |
| 组件开发 | [Runner](crates/runner/README.md)、[Gateway](services/api/README.md)、[桌面](apps/desktop/README.md)、[Android](apps/android/README.md)、[契约](packages/contracts/README.md) |
| 部署与设计 | [DEPLOYMENT](docs/DEPLOYMENT.md)、[DESIGN_SYSTEM](docs/DESIGN_SYSTEM.md) |
| 验收证据 | [M4](docs/M4_VALIDATION.md)、[Android](docs/ANDROID_VALIDATION.md) |
| 经验与历史 | [PITFALLS](docs/PITFALLS.md)、[MISC](docs/MISC.md)、[CHANGELOG](docs/CHANGELOG.md) |

其他专题从 [文档索引](docs/README.md) 按需阅读。
