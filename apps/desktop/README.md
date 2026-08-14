# AgentPulse Desktop

Windows Tauri 桌面客户端。用户通过现有 AgentPulse 邮箱账号注册或登录后，应用会自动：

1. 创建或加载本机 Ed25519 设备身份。
2. 使用当前登录会话完成设备登记，不展示一次性配对码。
3. 启动内嵌 Runner，发现 Claude Code 与 Codex Session 元数据。
4. 通过现有签名同步协议把白名单状态同步到当前账号。

仪表盘采用本地优先读模型：本机已发现的 Session 会立即显示，云端同步暂时不可用时也不会
错误显示为“未检测到”。来源卡分别显示扫描健康、会话数和待上传数量；账号面板显示当前
AgentPulse 账号与套餐。记忆区只统计 Claude memory 文件和 Codex memory 索引的数量与字节，
不读取或展示正文。

## 开发

```powershell
npm.cmd install
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

非回环地址必须使用 HTTPS。

Windows 版本会读取当前用户的系统代理配置；显式设置的 `HTTP_PROXY`、`HTTPS_PROXY`、
`ALL_PROXY` 和 `NO_PROXY` 仍由 HTTP 客户端优先处理。

## 本地数据与安全边界

- AgentPulse 的登录 Cookie 只保存在 OS Keyring，前端 JavaScript 不接触 Cookie。
- 设备私钥保存在应用数据目录的 Runner 身份文件中，不上传私钥。
- Claude 历史发现只读取 `CLAUDE_CONFIG_DIR` 或 `~/.claude/projects` 中的白名单元数据。
- Codex 优先使用 app-server；不可执行时只读 `CODEX_HOME` 最新状态库的 Thread 元数据。
- 桌面启动时先执行一次本地元数据扫描（每种来源最多 1000 条），随后进入后台自适应轮询。
- Claude/Codex memory 只读取文件或索引数量与字节大小，不读取记忆正文。
- 不读取或同步 Claude/Codex OAuth Token、API Key、消息正文、Prompt、工具参数、命令
  输出和 rollout 正文。
- 退出登录会停止当前同步进程并删除 OS Keyring 中的 AgentPulse Cookie；本地设备身份
  保留，以便同一台机器重新登录时继续使用稳定设备 ID。

## 验证

```powershell
npm.cmd test --workspace @agent-pulse/desktop
npm.cmd run typecheck --workspace @agent-pulse/desktop
npm.cmd run build --workspace @agent-pulse/desktop
cargo test -p agent-pulse-desktop
```
