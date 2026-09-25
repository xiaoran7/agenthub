# AgentHub 实现检查表

更新：2026-09-08。这里只保留会重复踩、会影响架构或验收结论的问题；阶段性讨论与已失效方案从主文档移除，历史可查 Git。

## 1. 证据与完成声明

- Fixture 只证明解析或协议分支，不能证明真实 CLI、网络、安装包、真机或生产。
- 真实 WebSocket + 模拟 Provider 只证明传输；Provider Remote 必须用真实 Runtime 完成多轮、审批、中断和恢复。
- “构建成功”不等于“安装可用”。Windows 安装包必须实际安装/覆盖安装并检查登录页和登录后关键入口。
- 模拟器不等于 Android 真机；回环 Gateway 不等于 HTTPS 生产。
- 文档中的“支持”必须能指向当前版本、命令和证据；规划写“目标/待实现”。

## 2. Session 与 Provider

- `discover`、`read_history`、`resume`、`attach` 是不同能力。发现 PID、JSONL 或状态库不能推导可写控制。
- Codex app-server 无法启动时仍可从本机状态库发现无正文的会话元数据，但必须标明不可远程恢复；恢复后的旧消息只由 daemon 的单点解析器生成有界快照，不在 Web/Gateway 解析 Provider 文件。
- 同一 Provider Session 默认单写者。未经协议支持的“接管”容易产生两个 Runtime 或两个 writer。
- 每回合启动一次 CLI 会破坏长期事件、审批和所有权语义；Desktop 与 Remote 必须共用常驻 Host。
- Runtime 返回真实 Session ID 之前不能创建成功绑定；跨 Provider 不能复用原生 ID。
- Codex app-server 的请求响应、通知和 server request 要按 method/ID 语义分类，不能把所有 JSON-RPC ID 当成同一空间。
- Codex 版本和用户配置会改变初始化能力；实验字段即使传 `false` 也可能触发能力门禁。错误应原样降级，不能假报成功。
- 原生最终消息可能与增量重叠，聚合按 turn/item/block 去重，不按数组尾部盲加。
- Claude Session 发现不能依赖不存在或版本不稳定的 `claude agents --json`；使用受限历史来源时保持只读边界。
- Grok Build 采集只读 `summary.json` / `active_sessions.json` 白名单，不读对话日志或 `auth.json`。
- 模型、权限、Slash 与上下文能力来自运行时探测；历史中出现过不等于当前账号仍支持。

## 3. 命令、审批与重连

- HTTP 202、写入 socket、Provider accepted、turn completed 是四个状态。
- 超时命令可能已经执行。可能修改文件的命令不能自动重试，先 reconcile，无法确认则标 `indeterminate`。
- `client_cmd_id` 的目标和 payload 都参与幂等；同 ID 异负载必须冲突。
- 审批绑定 Session、turn、Host generation 和原始 request ID；Host 重启后拒绝旧审批。
- interrupt 请求成功不等于进程已停止或状态已 idle，等待原生事件或可验证退出。
- ACK 只能推进到连续持久水位，不能取“见过的最大 seq”；窗口越界要求快照，不允许静默丢帧。
- Gateway/daemon 重连必须做 generation fencing，旧连接的 close 回调不能删除新连接。
- 零客户端不停止 Runtime；慢客户端不能阻塞 Provider 读循环。

## 4. 数据与安全

- 历史发现默认仅元数据，managed Remote 正文同步需要明确授权；两种数据通道不要混用。
- Provider Token、Cookie、环境变量和完整本机路径不得进入 Gateway、浏览器或诊断日志。
- 远端只能提交 daemon 已登记的 workspace ID，daemon 再解析本机路径；不能接受任意路径或 shell。
- 设备私钥只在本机；owner 必须由认证上下文解析，不能信任请求体中的 owner/runner。
- 登录身份与共享工作区 owner 分开。账户操作按登录人，设备和 Session 数据按当前工作区授权。
- 设备心跳使用独立时间水位，不能借事件序列刷新在线状态。
- 审计写入必须与业务状态同事务完成，不能在成功响应后异步补记。
- Stripe webhook 验签必须保留原始请求体；邮件适配器配置需互斥；秘密链接使用 fragment 时要处理同页重复导航。
- 私有 CA 通过显式证书信任接入，不能关闭 TLS 验证。

## 5. Windows 与进程

- WindowsApps 中的可执行文件可能能发现却不能直接执行；启动前做真实握手并允许用户指定路径。
- GUI 进程不会继承开发终端临时代理变量；显式环境变量和系统代理需要分别处理。
- 停止 Host/turn 时终止完整进程树。只杀直接子进程可能留下孙进程和未关闭管道。
- 子进程退出不代表 stdout/stderr 管道已排空；关闭/取消顺序要有超时，避免 join 永久阻塞。
- SQLite 多进程写需要 busy timeout 和单实例锁；daemon 数据库不能被 Desktop 第二份进程直接写。
- PowerShell 不因原生命令非零自动停止；验证脚本显式检查 `$LASTEXITCODE`。
- 使用 `npm.cmd` 避免执行策略拦截 `npm.ps1`；读取含中文文件时明确 UTF-8。
- 不用 PowerShell 文本重定向生成二进制 fixture/截图，避免编码和字节转换。

## 6. Web、Desktop 与 Android

- Desktop 本地发现列表不能用云端列表替代；Gateway 离线时本机 Session 仍应出现。
- 嵌入式 WebView 的 `document.hidden`、Notification 权限与普通浏览器可能不同，必须在真实壳层验证。
- Next.js rewrite 可能缓冲 SSE；实时流使用明确的流式代理或直连，保留轮询兜底。
- 快照和订阅必须共享 cursor；先订阅后快照或相反都可能在边界丢事件。
- 状态切换时禁用当前聚焦按钮会把焦点退回 body；审批/中断后要显式管理焦点。
- 移动布局同时检查窄宽、横屏和短高度；固定底栏还要处理安全区、内容 padding 与键盘。
- 动画服从 reduced motion / Android 动画缩放，不能承载唯一状态信息。
- React 自动选择器应由数据变化驱动，避免 effect 同时读写筛选状态造成循环。

## 7. 构建、数据库与部署

- 不并行运行会争用 `.next` 的 `next build` 与 `next typegen`。
- 根 Prettier 排除 Android/构建产物，避免扫描生成报告。
- PostgreSQL 服务存在不代表 `psql` 在 PATH；环境探测分别检查服务、客户端和连接。
- PostgreSQL `LISTEN/NOTIFY` 会回送发布实例；通知处理要幂等且不能携带 Session 正文。
- 数据库迁移校验和对行尾敏感；提交前保持迁移文件字节稳定，禁止修改已发布迁移。
- 时间相关集成测试使用相对当前时间或测试时钟，不使用会过期的固定日期。
- `git diff --check`、Markdown 链接检查和文档残留词扫描是文档批次最低验证。
