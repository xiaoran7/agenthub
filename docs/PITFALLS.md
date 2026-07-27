# 踩坑集

> 记录调试耗时、失败尝试和环境陷阱。每条包含现象、根因、解决方式和规避方法。

## 2026-07-27 嵌入式浏览器的 `Notification.permission` 恒为 denied，推送无法本机验证

- **现象**：Claude Code 浏览器面板里 `window.isSecureContext`、
  `'serviceWorker' in navigator`、`'PushManager' in window` 全为真，但
  `Notification.permission` 直接是 `denied`，`requestPermission()` 不会弹窗。
  面板因此只能显示"浏览器已拒绝本站通知权限"。
- **根因**：嵌入式 WebView 没有可用的权限提示 UI，实现直接返回拒绝；这与
  代码是否正确无关。
- **解决方式**：把能在该环境验证的部分验证到位——Service Worker 注册与激活
  （root scope）、`/v1/push/key` 在未配置 VAPID 时返回 404、开关渲染出对应的
  禁用态与原因文案；订阅与真实推送投递如实标为未验证，不当作已通过。
- **如何避免**：任何依赖浏览器权限提示的功能都要提前确认验证环境能否弹窗，
  并把"环境能测的"与"必须真人真机测的"在验收清单里分开写。

## 2026-07-27 Next.js rewrites 会缓冲响应体，SSE 经它转发一帧都收不到

- **现象**：网关 `GET /v1/stream` 在浏览器里 `fetch` 到 `200 text/event-stream`
  响应头，但 `body.getReader()` 读了 38 秒（期间已过两次 20 秒心跳）一帧没有；
  同一时刻 Node 直连 `127.0.0.1:3001` 首帧 47ms 到达，证明网关侧写入与 flush
  都正常。
- **根因**：`next.config.mjs` 里 `rewrites()` 的代理会缓冲上游响应体，流式响应
  被攒在代理里，只有连接结束才可能吐出。响应头先到很有迷惑性——它让人以为链路
  已经打通，问题出在客户端解析。
- **解决方式**：SSE 单独走 Route Handler（`app/v1/stream/route.ts`），用
  `fetch` 拿到上游 `Response` 后原样 `new Response(upstream.body, …)` 交还，
  并透传 `request.signal` 让关标签页能连带关掉网关侧的流。改完首帧 272ms。
- **如何避免**：任何长连接/流式接口都不要假设反向代理透明，先用"直连 vs 经代理"
  两路对比首帧时间；只验证响应头到达等于没验证。同理生产环境的 nginx 需要
  `proxy_buffering off`，网关已经在响应里带 `X-Accel-Buffering: no`。

## 2026-07-27 npm overrides 对已存在的依赖树不生效，只有全量重装才可靠

- **现象**：为修 Next 依赖链的 postcss/sharp 漏洞在根 package.json 加
  `overrides` 后，`npm install`、`npm update postcss sharp`、甚至删除
  package-lock.json 再 install 都显示 "up to date"，`npm ls` 里仍是旧版本；
  定点删除 `node_modules/postcss` 等目录再装，npm 反而把包删没导致树不一致。
- **根因**：npm 会从现存 node_modules 反推 lockfile，物理目录里的旧版本会被
  当作现状保留；overrides 只在真正重新解析依赖树时参与。定点删目录则触发了
  "从残缺现状重建" 的另一个坑。
- **解决方式**：删除整个 node_modules 加 package-lock.json 后全量
  `npm install`，overrides 立即生效且 `npm audit` 归零。
- **如何避免**：改动 `overrides` 后一律全量重装验证；`npm ls` 之后会对被
  override 的包永远报 `invalid`（override 越过上游声明范围的预期表现，并使
  命令以非零退出），不要把 `npm ls` 的退出码当健康检查。另外重装时若开发
  服务器仍在运行，会因占用 esbuild.exe 等二进制报 EPERM 并留下无害的
  `.package-aKtoqp3V` 残目录，先停服务再重装可避免。

## 2026-07-27 Next.js 15 与 TypeScript 7 不兼容；16 需要显式启用 CLI 模式

- **现象**：`next dev`（15.5.22）启动即失败："TypeScript 7.0.2 is not
  supported by this version of Next.js"；升级 next@16.2.12 后启动仍抛
  Unhandled Rejection，提示启用 `experimental.useTypeScriptCli`。
- **根因**：TS7 原生编译器不再提供 Next 依赖的进程内 JS 编译器 API；Next
  16.2.11+ 提供 "shell 出去调 TS CLI" 的兼容路径，但默认关闭。
- **解决方式**：升级 next@16 并在 next.config.mjs 设
  `experimental: { useTypeScriptCli: true }`。仓库统一 TS7 是拍板决策，
  不为 Next 降级 TS6。
- **如何避免**：Next 16 首次运行会重写 next-env.d.ts 引用
  `.next/dev/types/routes.d.ts`，新检出仓库直接 `tsc --noEmit` 会因文件
  不存在而失败——web 的 typecheck 脚本必须是 `next typegen && tsc --noEmit`。

## 2026-07-27 嵌入式 WebView 的 document.hidden 恒为 true，可见性暂停会饿死首屏

- **现象**：面板在 Claude Code 浏览器面板中始终显示"连接网关中…"，
  read_network_requests 里一个请求都没有；同一页面在普通浏览器正常。
- **根因**：轮询为省流量在 `document.hidden` 时跳过，而该 WebView 未合成
  帧（viewport 0x0）时 visibilityState 恒为 hidden，visibilitychange 也
  永远不触发，连首次拉取都被跳过。
- **解决方式**：首次拉取与配置变更后的拉取无条件执行，只有周期轮询检查
  `document.hidden`。
- **如何避免**：任何"可见才刷新"的优化都要保证首屏数据路径不依赖可见性；
  在无头/嵌入环境验证 UI 时优先看 DOM/网络请求而不是截图。

## 2026-07-27 `localeCompare` 不是字节序：跨语言 canonical JSON 的潜伏炸弹

- **现象**：代码审查发现 Node 侧 canonical JSON 用 `keys.sort(localeCompare)`
  排序 `raw_status_detail`，Rust 侧是 `BTreeMap`（UTF-8 字节序）。当前所有键
  恰为同大小写的小写 ASCII，两种顺序碰巧一致，测试全绿。
- **根因**：`localeCompare` 是 ICU 排序且依赖进程 locale——`['B','a']` 在
  locale 排序下是 `['a','B']`，字节序是 `['B','a']`；甚至纯小写键在丹麦语等
  locale 下顺序也会翻转。签名字节因此依赖部署机器的 locale。
- **解决方式**：Node 改为码元比较（`<`/`>`），同时把 detail 键收紧到
  `^[a-z][a-z0-9_]{0,63}$`——该字符集内 UTF-16 码元序、Unicode 码点序、UTF-8
  字节序三者等价；fixture 增加多键 detail 锁定两侧字节一致。
- **如何避免**：任何"排序后签名"的跨语言协议禁止使用 locale 相关比较；协议文档
  必须写明比较的是码点还是码元还是字节，并用包含混排键的向量测试钉死。

## 2026-07-27 rusqlite 默认 busy_timeout 为 0，双进程写同库时守护进程会静默死亡

- **现象**：审查发现 `run` 守护进程与 hook 触发的 `ingest-hook` 进程写同一个
  `runner.db`，写锁重叠时 rusqlite 立即返回 SQLITE_BUSY；`run` 循环把
  OutboxError 当致命错误 `?` 传播，监控进程直接退出且无人重启。
- **根因**：SQLite 默认不等待写锁（busy handler 为空）；WAL 只消解读写冲突，
  写-写冲突仍即时失败。架构上并发写是设计内行为，却没有配等待与容错。
- **解决方式**：连接统一设置 5 秒 `busy_timeout`；`run` 循环把数据库错误降级为
  与采集失败相同的退避重试并输出稳定错误码。
- **如何避免**：任何"多进程共享 SQLite"的设计必须显式设置 busy_timeout 并测试
  写-写并发；守护进程主循环里区分"该崩"与"该退避"的错误类别。

## 2026-07-27 子进程成功退出不代表管道已关：join 前必须先杀进程树

- **现象**：审查发现 Claude 采集**成功**路径在 `reader.join()` 之后才 drop Job
  Object；后代进程继承 stdout 写端时读线程等不到 EOF，`join` 永久阻塞——超时
  失败路径当初特意先 `drop(job)` 再 join，成功路径漏了同样的处理。
- **根因**：EOF 由"最后一个写端关闭"决定，与直接子进程退出无关；PITFALLS 里
  2026-07-25 的 Job Object 条目只修了超时分支。
- **解决方式**：`wait_for_child` 返回后、join 之前统一先 drop Job Object。
- **如何避免**：所有"进程树 + 管道 + 读线程"的清理顺序固定为：拿到退出状态 →
  杀完整进程树 → join 读线程；两条路径（成功/超时）用同一段顺序，不允许分叉。

## 2026-07-27 迁移校验和对行尾敏感：Windows 开发 + Linux 部署会自毁

- **现象**：迁移文件 SHA-256 直接对检出文本计算；`core.autocrlf` 在 Windows 把
  `.sql` 变 CRLF，Linux 检出是 LF，同一文件两个校验和。数据库先在 Linux 迁移、
  再从 Windows 连接时会报 checksum mismatch 拒绝启动。
- **根因**：校验和的输入是"工作区字节"而不是"仓库内容"，而工作区字节受 git
  行尾转换影响。
- **解决方式**：计算前把 `\r\n` 归一化为 `\n`，并加 `.gitattributes` 强制
  `*.sql text eol=lf` 双保险。
- **如何避免**：凡是对仓库文件做内容寻址（哈希、签名、缓存键）都先归一化行尾，
  或在 `.gitattributes` 固定该类文件的检出形态。

## 2026-07-27 本机验证 Unix 代码路径的两条弯路

- **现象**：为验证新增的 cfg(unix) 进程组代码，`rustup target add
  x86_64-unknown-linux-gnu` 下载 rust-std 被网络重置（可断点续传重试）；改用
  本机已有的 `x86_64-linux-android` 目标又因 rusqlite bundled 需要 NDK clang
  编译 SQLite C 源码而失败。
- **根因**：cargo check 也会执行依赖的 build.rs（含 cc 编译），纯 Rust 交叉
  检查被 C 依赖绑架；Android 目标虽属 unix family 但需要完整交叉 C 工具链。
- **解决方式**：重试下载 rust-std 成功后，把 cfg(unix) 片段抽成无 C 依赖的
  最小 crate 单独 `cargo check --target x86_64-unknown-linux-gnu`（只需
  rust-std，不需链接器与 cc），编译通过。注意这只验证了编译，未在真实 Unix 上
  运行过进程组终止路径。
- **如何避免**：验证平台特定代码的编译性时优先构造无 build.rs 依赖的最小
  再现；`cargo check` ≠ 不跑构建脚本。

## 2026-07-25 WindowsApps 中的 Codex 可发现但不可执行

- **现象**：`Get-Command codex` 能返回
  `C:\Program Files\WindowsApps\...\codex.exe`，但执行 `codex --version`
  返回 `Access is denied`。
- **根因**：Windows App 安装目录中的资源可被命令解析器发现，不代表当前 Shell
  拥有直接执行该内部二进制的权限。
- **解决方式**：代码侧允许 `--binary` 显式指定独立安装的 Codex CLI；真实协议验证
  等可执行 CLI 安装后再运行，不能把“能找到路径”当作“CLI 可用”。
- **如何避免**：环境探测必须实际执行无副作用的 `--version`，不能只使用
  `Get-Command` 或文件存在性判断。

## 2026-07-25 PowerShell 默认编码导致中文 README 显示乱码

- **现象**：`Get-Content -Raw` 输出 UTF-8 中文 README 时显示为乱码，但文件内容并未
  损坏。
- **根因**：当前 PowerShell 的默认文本编码与无 BOM UTF-8 文件不一致。
- **解决方式**：读取仓库中文文档时显式使用 `Get-Content -Encoding UTF8`。
- **如何避免**：看到乱码先核对读取编码，不要在未确认文件实际字节前重写文件。

## 2026-07-25 `git diff --check` 拦截文件尾多余空行

- **现象**：初始提交前质量检查返回多处 `new blank line at EOF`，同时输出 Windows
  的 LF/CRLF 转换提示。
- **根因**：部分文件结尾是两个换行，形成额外空白行；CRLF 提示只是工作区换行约定，
  与真正导致检查失败的空白行不是同一个问题。
- **解决方式**：删除文件尾额外空白行后重新暂存，再执行 `git diff --cached --check`。
- **如何避免**：把 `git diff --cached --check` 固定放在 commit 前，并分别处理
  whitespace error 与行尾转换 warning。

## 2026-07-25 PowerShell 将正常 stderr 日志包装成 `NativeCommandError`

- **现象**：持续 Runner 烟雾测试把 `2>&1` 赋值给变量时，JSON 轮询日志前出现
  `cargo : ... NativeCommandError`，但进程退出码仍是 0，健康状态也正确落库。
- **根因**：Runner 按守护进程惯例把运行日志写到 stderr；Windows PowerShell 在捕获
  原生命令 stderr 时会把每行包装为 ErrorRecord，这不表示子进程执行失败。
- **解决方式**：自动化同时检查 `$LASTEXITCODE` 和 JSON 内容，不用 ErrorRecord 的
  外观替代退出码判断。
- **如何避免**：命令级测试若需要机器读取日志，优先分别捕获 stdout/stderr；以后接入
  正式日志协议时为诊断输出提供明确的 JSONL 文件或结构化日志目标。

## 2026-07-25 只杀直接子进程无法解除 Windows 管道阻塞

- **现象**：Claude 超时测试虽然在截止时间杀掉了 `.cmd`，采集函数仍等待约 5 秒，
  直到脚本启动的 PowerShell 后代自然退出。
- **根因**：后代进程继承了 stdout 管道写端；直接子进程退出后，读取线程仍看不到
  EOF，所以 `join` 会继续阻塞。
- **解决方式**：为每个采集进程创建 Windows Job Object，设置
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，把直接子进程加入 Job；超时和连接关闭时终止
  Job 中的完整进程树，再等待进程和读取线程回收。
- **如何避免**：所有带管道和超时的外部命令测试都要让脚本再派生一个持有管道的后代，
  不能只测试单进程 `sleep`；Unix 实现时也应使用进程组而不是只发送单 PID 信号。

## 2026-07-25 PowerShell 执行策略拦截 `npm.ps1`

- **现象**：Node 可正常执行，但直接运行 `npm --version` 报告 `npm.ps1 cannot be
  loaded because running scripts is disabled`。
- **根因**：PowerShell 命令解析优先选中了 Node 安装目录中的 `npm.ps1` 包装脚本，
  当前系统执行策略禁止该脚本；npm 本身和对应的 Windows 命令文件并未损坏。
- **解决方式**：仓库命令和自动化在 Windows 上显式调用 `npm.cmd`。
- **如何避免**：工具链探测要分别检查 `node --version` 与 `npm.cmd --version`；
  不要为了运行 npm 全局放宽 PowerShell 执行策略。

## 2026-07-25 本机缺少 PostgreSQL 与容器运行时

- **现象**：PostgreSQL Store 的类型检查和内存契约测试可执行，但找不到 `psql`、
  PostgreSQL Windows 服务、Docker 或 Podman，无法在本机启动真实数据库。
- **处理方式**：集成测试要求显式设置 `AGENT_PLATFORM_TEST_DATABASE_URL`，并在
  专用数据库下创建随机 Schema；没有连接串时报告 `SKIP`，不把跳过写成数据库通过。
- **如何避免**：CI 增加 PostgreSQL Service Container，并把该环境变量指向专用测试
  数据库；开发机不要为了单次验证静默安装系统服务或把测试指向生产数据库。
