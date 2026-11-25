# Windows 一键接入包

将下面三个文件放在同一个目录：

- `agenthub-runner.exe`
- `Connect-AgentHub.cmd`
- `Connect-AgentHub.ps1`

双击 `Connect-AgentHub.cmd`，按提示输入自部署 AgentHub 服务器地址（例如 `https://agent.example.com` 或 `http://localhost:3000`）以及控制台页面生成的一次性设备连接码即可。脚本为兼容所有区域设置的 Windows PowerShell 5.1（含中文 GBK 环境）采用纯 ASCII 英文提示。

首次运行会自动创建设备身份、通过环境变量 `AGENT_PLATFORM_BINDING_CODE` 安全传递连接码完成绑定（不会暴露在命令行参数或进程审计日志中），随后以 Remote v3 模式（`remote-run`）启动本地控制面与内置 Web 控制台（默认 `http://127.0.0.1:4200`）。再次运行会复用本机身份，无需重复绑定。

如需指定授权工作区或本地控制台端口，可在 PowerShell 中传参运行：

```powershell
.\Connect-AgentHub.ps1 -ServerUrl https://agent.example.com -Workspace C:\Projects\demo -WebPort 4200
```

设备身份与本地状态默认保存在 `%LOCALAPPDATA%\AgentHub`，不会将平台访问令牌或一次性连接码写入脚本。连接码仅用于首次绑定，10 分钟后失效。

构建 Runner：

```powershell
cargo build --release -p agenthub-runner
```

构建完成后，二进制直接产出为 `target\release\agenthub-runner.exe`，复制到本目录即可作为 Windows 接入包分发。
