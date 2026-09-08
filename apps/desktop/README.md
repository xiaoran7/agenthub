# AgentHub Desktop

Windows Tauri 客户端。Desktop 是 AgentHub 的本地主工作台，不是官方 Agent 的 UI 自动化层：它调用官方 Runtime 协议，并最终通过本地 `agenthubd` 管理 Session。

## 当前实现

当前存在两条尚未完全收敛的路径：

1. 默认 Dashboard 提供本机会话发现、历史正文、创建/续聊、模型/权限选择和流式输出；部分回合仍直接启动短时 CLI 子进程。
2. Remote Console 通过内嵌 Runner 的常驻 Codex app-server Host 连接 Gateway，支持创建、恢复、续聊、审批和中断。

二者都是真实功能，但还不是目标架构。不要把“切换到 Remote 页面”理解为进入另一类 Session。

## 目标形态

```text
Desktop UI ── Tauri IPC ──► agenthubd ──► Codex / Claude Code Runtime
                                  │
                                  └── optional Gateway Remote
```

下一阶段：

- 将 Codex `NativeSessionHost` 抽成单一 daemon 服务；
- Dashboard 的创建、恢复、回合、审批和中断全部改走本地 IPC；
- Remote 开关只控制 Gateway 暴露，不改变 Session 类型或 UI 模式；
- Desktop 自动启动、升级和重连 daemon，取消手动选择二进制/数据库的产品流程；
- 断网时本地会话完整可用，重连后继续同步同一 Registry；
- Claude Code 在官方长时控制与认证验证后接入同一 Host。

## 开发

```powershell
npm.cmd ci
npm.cmd run tauri --workspace @agent-pulse/desktop -- dev
```

只运行 Web 壳层：

```powershell
npm.cmd run dev --workspace @agent-pulse/desktop
```

生产构建：

```powershell
npm.cmd run tauri --workspace @agent-pulse/desktop -- build
```

默认服务地址为 `https://platform.tannerlab.xyz`。本地开发可设置：

```powershell
$env:AGENT_PULSE_SERVER_URL = 'http://127.0.0.1:3001'
```

非回环地址必须 HTTPS。Windows 会读取系统代理；`HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY` 和 `NO_PROXY` 仍优先于系统设置。

## 当前安全边界

- AgentHub 登录 Cookie 保存在 OS Keyring，前端 JavaScript 不接触。
- 设备私钥留在本机身份文件，不上传。
- 历史发现只读取白名单元数据；正文仅在用户打开会话时从受限 Provider 来源读取。
- managed Remote 的正文、工具事件和审批会进入用户授权的 Gateway 内容库；旧发现 Outbox 不上传正文。
- Provider 登录由官方 Runtime 管理。旧凭据读取命令是 Runner 的显式 opt-in 兼容能力，不接入新 Host。
- 授权工作区由本机选择，远端不能提交任意目录或 shell。
- 同一 Session 同时只允许一个受控回合；停止操作终止完整子进程树或调用原生 interrupt。
- 退出登录会删除 AgentHub Cookie 并停止当前内嵌 Remote Host；迁移到独立 daemon 后，退出登录与停止本地 Runtime 必须分开处理。

## 验证

```powershell
npm.cmd test --workspace @agent-pulse/desktop
npm.cmd run typecheck --workspace @agent-pulse/desktop
npm.cmd run build --workspace @agent-pulse/desktop
cargo test -p agent-pulse-desktop
```

Tauri fixture、真实 Codex、Windows 安装与登录后操作分别记录，不互相替代。
