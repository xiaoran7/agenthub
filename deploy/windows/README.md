# Windows 一键接入包

将下面三个文件放在同一个目录：

- `agent-pulse-runner.exe`
- `Connect-AgentPulse.cmd`
- `Connect-AgentPulse.ps1`

双击 `Connect-AgentPulse.cmd`，输入 Agent 平台页面生成的一次性连接码即可。首次运行会自动创建设备身份、完成绑定并启动持续同步；以后再次双击会复用这台电脑的身份，不需要重复绑定。

设备身份和本地 Outbox 默认保存在 `%LOCALAPPDATA%\AgentPulse`，不会把平台访问令牌写入脚本。连接码只用于首次绑定，10 分钟后失效。

构建 Runner：

```powershell
cargo build --release -p agent-pulse-runner
```

构建完成后，把 `target\release\agent-pulse-runner.exe` 复制到本目录，再把整个目录作为 Windows 接入包分发。
