# AgentHub Desktop

Windows Tauri 桌面客户端。用户通过现有 AgentHub 邮箱账号注册或登录后，应用会自动：

默认入口已改为 Remote，会话 UI 与 Web 共用；通过 Tauri IPC 使用安全存储中的登录 Cookie 请求 Gateway，不把 Cookie 交给前端。新入口支持同一原生会话的输入、审批、中断与恢复。启用 Gateway v3 后，点击“连接本机”，选择 Codex 原生可执行文件与授权目录即可启动内嵌 Remote Host；也可使用独立 `remote-run`。同一设备不能同时启动两份 Host。配置与内容库按设备/Gateway 隔离；关闭会话页面不停止 Host，退出登录或退出桌面应用会停止。设备与账户按钮保留下面描述的旧管理界面。

1. 创建或加载本机 Ed25519 设备身份。
2. 使用当前登录会话完成设备登记，不展示一次性配对码。
3. 启动内嵌 Runner，发现 Claude Code、Codex、Antigravity 与 Grok Build Session 元数据。
4. 通过现有签名同步协议把白名单状态同步到当前账号。

仪表盘采用本地优先读模型：本机已发现的 Session 会立即显示，云端同步暂时不可用时也不会
错误显示为“未检测到”。来源卡分别显示扫描健康、会话数和待上传数量；账号面板显示当前
AgentHub 账号与套餐。记忆区只统计 Claude memory 文件、Codex memory 索引和 Grok
`summary.json` 的数量与字节，不读取或展示正文。

会话列表现在也是本机控制台：先用全局选择器确定 Agent，再按脱敏工作目录分组并显示显式会话名；会话支持置顶、目录折叠和长列表展开/收起。点击会话可查看
模型、推理强度、权限/审批策略、沙箱、上下文窗口与 Token 用量，并直接继续 Claude Code、
Codex、Antigravity 或 Grok Build 会话、按设备筛选账号下的 Session、查看当前账号范围内的状态时间线、在当前筛选结果中用上一条/下一条按钮或左右方向键巡检会话、清空本次打开期间的显示内容，以及归档/恢复会话。状态时间线
只显示序列、观察时间、接收时间、项目名和状态，不读取对话正文；尚未同步到云端的本机会话会明确显示暂无记录。设置面板
可配置 2–60 秒刷新周期（默认 4 秒）和三个 CLI 的可执行文件路径。桌面 Host 会使用登录
Cookie 建立 Gateway SSE 出站连接，云端变化到达即刷新；周期轮询保留为断流兜底。连接状态
会显示“`SSE 实时连接`”或当前兜底刷新周期。

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

- AgentHub 的登录 Cookie 只保存在 OS Keyring，前端 JavaScript 不接触 Cookie。
- 设备私钥保存在应用数据目录的 Runner 身份文件中，不上传私钥。
- Claude 历史发现只读取 `CLAUDE_CONFIG_DIR` 或 `~/.claude/projects` 中的白名单元数据。
- Codex 优先使用 app-server；不可执行时只读 `CODEX_HOME` 最新状态库的 Thread 元数据。
- 桌面启动时先执行一次本地元数据扫描（每种来源最多 1000 条），随后进入后台自适应轮询。
- 用户打开会话时，桌面端会从各 Provider 原生目录按 Session ID 读取本地 JSONL 正文；支持 Claude Code、Codex、Antigravity 和 Grok Build，并兼容 UTF-8 BOM 与 UTF-16 LE/BE（含无 BOM 启发式识别）。读取结果会规范化为消息、工具调用、命令和状态；普通消息使用安全的 CommonMark/GFM 渲染，Provider 控制标签不会作为正文显示。正文只进入当前 WebView，不进入 Outbox、Gateway、配置或日志。
- Claude/Codex memory 只读取文件或索引数量与字节大小，不读取记忆正文。
- 日常发现与同步不读取 Claude/Codex OAuth Token、API Key、消息正文、Prompt、工具参数或
  命令输出。桌面运行上下文是独立的本机增强层：Claude 只扫描有界 JSONL 头尾的供应商标题事件、
  权限模式、模型和用量字段；Codex 只读状态库白名单列和受 `CODEX_HOME` 路径约束的 rollout
  最多 2 MiB 尾部，并只接受 `turn_context`/`token_count`。结果缓存 15 秒，不进入同步协议。
- 会话标题优先使用 Claude `/rename`/`custom-title`、Claude 自带 `ai-title`、Codex 本地状态库或其他 Provider 保存的标题；没有显式标题时，会在本机从首条有效用户请求生成有界摘要，并过滤命令提示、后台通知等控制内容。标题不进入同步协议，远端设备未提供本机上下文时使用项目名回退。
- 用户在会话控制台显式发送时，Prompt 仅通过 Tauri IPC 和 stdin 交给本机 CLI；回复只保留
  在当前前端内存中，不进入桌面配置、SQLite Outbox、Gateway 或日志。单次输入最多 16000
  字符，输出最多 2 MiB，执行最长 15 分钟；stderr 只被排空，不回传到界面。
- Claude 回复通过 `stream-json` partial event 增量显示；Codex JSONL 会实时显示安全活动标签，
  并在每条助手消息完成时更新内容。thinking、命令正文、工具参数和 stderr 不进入 WebView。
  同一会话同时只允许一个本机回合；“停止回合”会终止完整子进程树。运行期间不能关闭会话
  抽屉，避免丢失仅存在前端内存中的增量内容。
- SSE Cookie 仍只由 Rust Host 从 OS Keyring 读取；WebView 仅收到连接布尔值和变化信号。
  SSE 单行上限 64 KiB，断线后指数退避重连，连接不可用不影响周期刷新。
- Codex 的归档/恢复会同步到本机 Codex app-server。Claude 归档仅是 AgentHub 本机视图
  标记，不删除或改写 Claude JSONL；归档记录只含 Agent 类型、Session ID、项目显示名/
  指纹和观察时间。
- 退出登录会停止当前同步进程并删除 OS Keyring 中的 AgentHub Cookie；本地设备身份
  保留，以便同一台机器重新登录时继续使用稳定设备 ID。

## 验证

```powershell
npm.cmd test --workspace @agent-pulse/desktop
npm.cmd run typecheck --workspace @agent-pulse/desktop
npm.cmd run build --workspace @agent-pulse/desktop
cargo test -p agent-pulse-desktop
```
