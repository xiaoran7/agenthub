# AgentHub Desktop

Windows Tauri 客户端。Desktop 是 AgentHub 的本机宿主客户端（Remote-only 定位），不是官方 Agent 的 UI 自动化层，也不是通用本地工作台：它通过当前用户 IPC 使用进程外 `agenthubd`，由 daemon 调用官方 Runtime 协议并管理 Session。

## 当前实现

Desktop 只有一个 Dashboard 和一套 managed Session 控制路径：

- Codex 创建、恢复、续聊、审批、中断和归档统一进入 daemon 的 `DurableHost`；
- Remote 只是同一 daemon 的 Gateway 暴露开关，不创建第二个 Host 或第二套 Session UI；
- 会话历史预览经 daemon 的 `GetSessionConversation` IPC 获取，transcript 解析只在 runner 单点实现；Desktop 不直接读取或解析 Provider 文件；
- Claude Code、Antigravity 与 Grok Build 仅展示发现元数据（恢复候选），无全文浏览，managed 写入口明确禁用；
- Desktop 先连接现有 daemon，仅在 IPC endpoint 不可用时以自身隐藏 `--agenthubd` 模式启动；
- Desktop 退出只断开本地订阅，退出登录和关闭 Remote 只停止 Gateway 暴露，本地 Runtime 继续运行；Remote 开启、关闭、退出登录与 profile 提交由同一状态锁串行化。

当前 daemon 实现仍由 `agent-pulse-runner` crate 提供，没有独立命名的二进制或 OS 服务安装包。工作区授权为运行期只增不减，daemon 运行时不能更换 Codex 二进制；UI 会锁定对应编辑项。

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
- 历史发现只读取白名单元数据；会话正文预览只在会话可能被远程继续时（当前为 Codex）经 daemon 读取，且响应有界。
- managed Remote 只把消息正文、工具完成状态和 opaque 审批描述符写入用户授权的 Gateway 内容库；完整 Provider 事件、收据与工具详情留在本机，旧发现 Outbox 不上传正文。
- Provider 登录由官方 Runtime 管理。旧凭据读取命令是 Runner 的显式 opt-in 兼容能力，不接入新 Host。
- 授权工作区由本机选择，远端不能提交任意目录或 shell。
- 同一 Session 同时只允许一个受控回合；停止操作调用原生 interrupt，并等待 Provider 终态事件。
- 退出 Desktop 或登录账号不会停止 daemon/Runtime；当前本地停止需显式结束 daemon 进程，独立产品控制待 OS 服务阶段补齐。

## 验证

```powershell
npm.cmd test --workspace @agent-pulse/desktop
npm.cmd run typecheck --workspace @agent-pulse/desktop
npm.cmd run build --workspace @agent-pulse/desktop
cargo test -p agent-pulse-desktop
```

Tauri fixture、真实 Codex、Windows 安装与登录后操作分别记录，不互相替代。
