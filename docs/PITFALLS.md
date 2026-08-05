# 踩坑集

> 记录调试耗时、失败尝试和环境陷阱。每条包含现象、根因、解决方式和规避方法。

## 2026-07-29 根 Prettier 会递归进入 Android 构建报告

- **现象**：新增 `apps/android` 后，`npm.cmd run format` 开始格式化 Gradle
  `build/intermediates` JSON，并在 Gradle 生成的测试 HTML 上报语法错误。
- **根因**：根脚本把整个 `apps` 目录交给 Prettier，而原 `.prettierignore` 只排除
  `node_modules` 和 `dist`，不了解 Gradle 的模块级 `build` 输出。
- **解决方式**：根 `.prettierignore` 增加 `apps/android/.gradle`、
  `apps/android/**/build` 和本机 `local.properties`。
- **如何规避**：任何新工具链加入 npm 扫描目录时，同时登记其生成目录；生成报告
  由生成器维护，不应被跨语言格式化器读取或改写。

## 2026-07-29 PowerShell `>` 会污染 adb 二进制截图

- **现象**：`adb exec-out screencap -p > screenshot.png` 返回成功且文件存在，但
  图片工具无法解析；设备上应用实际运行正常。
- **根因**：Windows PowerShell 把原生 stdout 当文本重新编码，PNG 字节被改变；
  这与此前 Fixture stdin 被管道改写是同一类问题。
- **解决方式**：先在设备执行 `screencap -p /sdcard/name.png`，再用二进制安全的
  `adb pull` 拉到主机，最后删除设备临时文件。
- **如何规避**：签名输入、压缩包、图片等二进制数据不要经过 PowerShell 文本
  管道或重定向；使用工具自身的文件参数或二进制安全复制命令。

## 2026-07-29 移动规则写在基础样式前可能被同权重选择器覆盖

- **现象**：手机详情已经命中 `max-width: 919px`，上一条/下一条却被挤成两列竖排；
  DOM 与媒体查询均正确，视觉仍像 Flex 而不是设计的 Grid。
- **根因**：移动媒体块里的 `.detail-header { display: grid }` 写在后面的基础
  `.detail-header { display: flex }` 之前，两者 specificity 相同，后出现的基础规则
  覆盖移动规则。
- **解决方式**：移动规则改为 `.detail .detail-header`，明确提高局部覆盖优先级。
  真实 Chrome 复验 computed `display=grid`，390px 下两个按钮各 145px。
- **如何规避**：媒体规则若因文件结构必须早于基础规则，检查 specificity 和最终
  computed style，不能只看 DevTools 是否显示“媒体查询已命中”。

## 2026-07-29 导航按钮在状态切换后 disabled 会把焦点退到 body

- **现象**：详情从第 1 条切到第 2 条成功，但原“下一条”同时变为 disabled，Chrome
  将焦点移出全屏 dialog 到 body，破坏模态焦点边界。
- **根因**：浏览器不会把焦点自动转给同组其他按钮；仅监听后续 Tab 只能补救下一次
  键盘操作，不能消除切换完成后的非法焦点状态。
- **解决方式**：记录导航方向，在新 Session DOM 提交后的 effect 中优先聚焦同方向
  可用按钮，边界时回退到反方向按钮或返回按钮。
- **如何规避**：任何“点击后自身可能 disabled/卸载”的控件都要验收提交后的
  `document.activeElement`，不能只验证业务状态已经变化。

## 2026-07-29 移动断点只看宽度会漏掉手机横屏

- **现象**：390px 宽的手机竖屏显示固定底栏，旋转到 844×390 后宽度超过 760px，
  突然恢复桌面控制面并失去单手导航；仅测常见竖屏看不出问题。
- **根因**：设备类型不能由 CSS 宽度单独推断，现代手机横屏宽度可以超过小平板断点，
  但高度和交互空间仍明显属于移动场景。
- **解决方式**：保留 `max-width: 760px` 竖屏规则，同时为不超过 920×480 的横屏
  视口启用同一移动壳；横屏单独把概览压成四列。真实 Chrome 在 844×390 验证底栏
  存在、页面 `scrollWidth === innerWidth`。
- **如何规避**：移动响应式验收至少覆盖一个超窄竖屏和一个现代手机横屏，并同时
  检查导航形态、横向溢出和首屏信息密度，不能只缩放桌面窗口。

## 2026-07-29 开发认证开启后仍必须提供固定 owner

- **现象**：设置 `AGENT_PLATFORM_ALLOW_DEVELOPMENT_AUTH=true` 后启动本地 Gateway
  仍立即退出，错误为 `AGENT_PLATFORM_OWNER_ID is required`；Web 能启动但只能等待
  不存在的 API。
- **根因**：开发认证只是允许回环请求通过 `x-user-id` 标识调用者，不替代服务端
  固定 owner 配置。Gateway 的进程级数据归属和认证入口是两个独立约束。
- **解决方式**：本地 QA 进程同时设置开发认证开关与
  `AGENT_PLATFORM_OWNER_ID=local-user`，确认 3001 端口监听后再执行绑定、签名同步和
  浏览器验收；变量只注入临时子进程。
- **如何规避**：启动脚本必须先读 `server.ts` 的完整必填环境校验，并在进入数据准备
  前检查健康端口和错误日志，不能用 Web 3000 已监听推断 Gateway 也已就绪。

## 2026-07-28 PowerShell 不会因原生命令非零退出自动停止

- **现象**：移动端真实数据 Fixture 脚本继续执行并启动 Web，但 Runner 设备绑定实际
  已失败；后续签名同步返回 403，面板只显示空引导页。
- **根因**：PowerShell 的 `$ErrorActionPreference = "Stop"` 不会把原生可执行文件
  的非零退出码转成 terminating error；隐藏标准输出也不会改变这一行为。
- **解决方式**：设备绑定、同步等关键原生命令后立即检查 `$LASTEXITCODE`，失败时
  显式 `throw`；同时读取 Gateway 日志确认 register/sync 的真实 HTTP 结果。本轮重新
  签发绑定码后完成设备注册，38 条 Session 获得有效 ACK 并进入浏览器验收。
- **如何规避**：PowerShell 编排任何依赖前一步的原生命令时都封装退出码检查，不能
  只看脚本是否跑到末尾或 UI 是否启动；带签名链路还要核对服务端注册与 ACK 证据。

## 2026-07-28 移动端固定底栏不能只加 `position: fixed`

- **现象**：底部导航视觉上固定成功，但最后几张 Session 卡、空状态按钮或设置操作栏
  会被它遮住；打开全屏详情后底栏还可能浮在详情上方。
- **根因**：固定元素脱离文档流，页面不会自动预留其高度；手机还存在底部安全区和
  地址栏伸缩，单独使用固定像素或 `100vh` 无法覆盖这些变量。
- **解决方式**：主面板底部 padding 同时包含导航高度和
  `env(safe-area-inset-bottom)`；设置使用 `dvh` 并在固定 footer 上补安全区；
  Session 全屏详情的 z-index 明确高于导航。390×844 视口下实际检查元素矩形、页面
  横向溢出和底栏覆盖关系。
- **如何规避**：新增任何移动端固定层时一起验收内容末端、safe-area、软键盘/动态
  视口和层级关系，不能只截一张首屏图就认定适配完成。

## 2026-07-28 Web Push 服务端已接受不等于系统通知已展示

- **现象**：Runner 的签名批次已 ACK、Outbox 已归零，但 Chrome/Windows 没有可
  观察的活动通知；只看同步结果无法判断是 FCM 拒绝、浏览器接收失败还是系统展示
  被抑制。
- **根因**：Web Push 故意在同步请求路径之外执行，原实现没有记录
  `PushOutcome`，把第三方推送失败与系统级展示问题混在同一个黑盒里。
- **解决方式**：每次等待状态投递记录 `delivered / gone / failed` 聚合计数，
  不记录 endpoint、owner、Session ID 或 payload。`delivered` 仅表示推送服务
  HTTP 成功接受；Service Worker 再在 `showNotification()` 成功后向已打开面板
  发无元数据时间戳回执，用于证明浏览器展示 API 已接受。两者仍不能替代真实系统
  通知中心验收。
- **如何规避**：现场验收必须分别记录 Runner ACK、推送服务结果和浏览器/系统
  展示证据，不能用上游成功替代下游成功。

## 2026-07-28 ureq/rustls 的证书错误可能包装成 `io::ErrorKind::InvalidData`

- **现象**：真实 TLS 回归连接自签服务端时，rustls 明确给出
  `InvalidCertificate(UnknownIssuer)`，Runner 却把它归类为 `gateway_unreachable`
  而不是 `gateway_tls_failed`。
- **根因**：ureq 3.3 的这条握手失败路径使用 `ureq::Error::Io` 包装 rustls 错误，
  内层 `io::ErrorKind` 为 `InvalidData`；只匹配顶层 `Tls`/`Rustls` 变体会漏掉。
- **解决方式**：传输分类同时把 `Io(InvalidData)` 识别为 TLS 失败，并用真实
  rustls 服务端钉死“未知 CA 拒绝、配置 CA 成功”的两次握手。
- **如何规避**：网络库错误分类要用真实握手覆盖，不能只构造枚举变体；升级 ureq/
  rustls 后保留该测试，若包装方式变化应更新分类但不得回显底层证书或系统错误。

## 2026-07-28 `next build` 与 `next typegen` 并行会争用 `.next` 并触发 EPIPE

- **现象**：全量门禁把 Web 生产构建与根级 `typecheck` 并行执行时，类型检查成功，
  `next build` 却在没有编译错误的情况下以 `EPIPE: broken pipe, write` 退出；随后
  单独执行同一构建完整通过。
- **根因**：Web 的 typecheck 会先运行 `next typegen`，它与 `next build` 同时读写
  `.next` 生成目录；两个 Next 进程不支持共享该目录并发生成。
- **解决方式**：先完成 typecheck，再单独运行生产构建；不要把这两个命令放进同一
  并行门禁批次。
- **如何规避**：可以并行执行不同工具链，但凡两个 Next 命令都写 `.next` 就必须
  串行。看到 EPIPE 时先排除生成目录争用，再判断为应用构建故障。

## 2026-07-28 PostgreSQL 服务存在但二进制不在 PATH，`Get-Command psql` 会误判环境

- **现象**：`Get-Command psql` 和常见安装目录探测都找不到客户端，早期验收因此
  记录“本机没有 PostgreSQL”；但 `Get-Service` 显示 `postgresql-x64-18` 正在
  运行，服务实际从 `D:\PostgresSQL\bin` 启动。
- **根因**：安装位置是非默认目录且没有把 `bin` 加入 PATH；只探测命令和
  `C:\Program Files\PostgreSQL` 不能代表 Windows 服务不存在。
- **解决方式**：读取 `Win32_Service.PathName` 找到 `pg_ctl.exe` 与数据目录，
  再用同目录的 `pg_isready.exe`/`psql.exe` 验证服务端 18.4 和连接凭据。
- **如何规避**：数据库环境探测至少同时检查 PATH、Windows 服务及服务命令行；
  只有三者都没有证据时才写“本机未安装”。连接密码只注入当前测试进程，不写入
  仓库、日志或持久环境变量。

## 2026-07-28 PowerShell 管道会改变原始 Fixture 字节，原生 stdin 验收应重定向文件

- **现象**：`Get-Content -Raw fixture.json | runner ingest-hook` 即使显式设置 UTF-8，
  Runner 仍在第 1 字节报告 JSON 为空或非法；同一个文件由测试直接读取时正常。
- **根因**：Windows PowerShell 把对象管道重新编码后再写给原生进程 stdin，
  `$OutputEncoding` 与无 BOM UTF-8 的组合并不可靠，输入已经不是磁盘原始字节。
- **解决方式**：用 `Start-Process -RedirectStandardInput fixture.json` 把文件字节
  原样接到 Runner stdin，Hook 随即正常入队并同步。
- **如何规避**：涉及签名、编码或原始协议字节的 CLI 验收不要经过 PowerShell
  文本对象管道；优先使用进程级 stdin 文件重定向。

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

## 2026-08-03 筛选器自动选中逻辑造成 React 状态循环

- **现象**：来源筛选排除当前检查器会话后，开发浏览器持续报告
  `Maximum update depth exceeded`，页面不断清空并重新选中检查器。
- **根因**：筛选清理 effect 从 `baseSessions` 清除被排除的会话，但预览模式的自动选中
  effect 仍从未过滤的完整 `sessions` 重新选择它；两个 effect 相互触发。
- **解决方式**：自动选中只从当前过滤后的 `baseSessions` 取第一条；没有匹配项时保持
  空选择。补充浏览器移动端导航、活动筛选和来源切换回归。
- **如何避免**：任何“清理选择”的 effect 都必须与“默认选择”的候选集合使用同一过滤
  结果，并在开发浏览器控制台检查更新深度错误，而不是只看页面是否暂时渲染。
