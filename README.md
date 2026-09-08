# AgentHub（智能体集成中心）

> 当前为原生 Remote 重构阶段：Codex 持久 Host、鉴权重放、Web/桌面/Android 入口已接入，实际验证范围见 [迁移状态](docs/README.md)。既有 v2 方案介绍不能作为完成证据；Claude、实际客户端安装和生产验收仍待完成。

多 Coding Agent 的原生远程会话入口：在电脑开始工作，在网页、桌面或手机继续同一个会话。执行留在本机 Runner，客户端共享原生上下文、实时输出、审批与中断。当前正在从监控与本地交互迁移，阶段状态见 [文档索引](docs/README.md)。

## 开发

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
