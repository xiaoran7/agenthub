# Changelog

## [Unreleased]

- **设置齿轮按钮靠最右对齐与资源缓存穿透（`crates/runner/resources/dashboard.css` / `src/web_server.rs`）**：
  - **根因分析**：顶栏 `.top` 原先采用 3 列 CSS Grid（`grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);`）。在侧边栏展开状态下（`body.side-nav`），中间槽位 `.top-slot` 被设为 `display: none;` 从文档流中移除，导致 CSS Grid 自动放置算法将 `.actions` 顺延填充至第 2 列居中偏左。同时，内嵌服务器此前对静态资源下发了 `immutable` 强缓存响应头，导致 Edge `--app` 模式在热重载或重启时继续使用本地磁盘上的旧版 CSS；
  - **修复**：将 `.top` 改为坚固的 Flex 布局（`display: flex; justify-content: space-between; align-items: center;`），并将右侧操作区显式声明为 `.actions { margin-left: auto !important; flex: none; }`，无论中间槽位是否显示，齿轮均 100% 绝对贴靠在顶栏最右侧边缘；同时将资源版本参数递增为 `v=2.5.3.1`，缓存响应策略由 `immutable` 调整为 `no-cache, must-revalidate`，确保客户端加载最新编译的样式。

- **解决会话加载初期误判为空白状态及加载动画缺失问题（`crates/runner/resources/dashboard.html` / `dashboard.js` / `src/web_server.rs`）**：
  - **根因分析**：
    1. Runner 启动初期后台 `NativeSessionHost` 正在进行全盘转录初次扫描（`control.ready` 为 `false`），调用 `Discover` 返回 `DaemonResponse::Error { code: "daemon_starting" }`；
    2. `build_dashboard_summary` 原先无条件将首个 0 会话响应以 12 秒 TTL 写入 `dashboard_summary_cache()`，且未向上层返回 `ready` / `status` 状态；
    3. 前端在收到 0 会话响应时将空状态打击计数 `consecutiveEmptyResponses` 累加，且 `dashboard.html` 静态标签初始写死为 `0 个历史会话`，造成在真实会话完成解析前过早闪烁“还没有可读取的会话”空白提示；
  - **修复**：
    - 后端（`web_server.rs`）：在返回结果中注入 `ready` 和 `status` 字段；当会话数为 0 且缓存中有先前有效会话时自动复用；针对非空缓存将 TTL 降为 4 秒，空/启动状态仅允许 500ms 重查，且未完成初始化时不污染有效缓存；
    - 前端（`dashboard.html` & `dashboard.js`）：将初始徽标文本改为 `正在加载…`；初次启动提供 5 次（间隔 800ms）的主动重试；当后端报告 `ready: false` 或 `status: "starting"` 时，保留骨架屏加载动画（Shimmer Skeleton），不将未完成状态计入空状态打击，并在后台完成后无缝呈现真实会话。

- **支持会话子代理（Subagents）折叠与展开交互，且默认处于收起状态（`crates/runner/resources/dashboard.js` / `dashboard.css`）**：
  - **交互设计**：
    - 针对包含子代理的会话，默认处于折叠收起状态，避免大量子任务平铺冲淡主会话列表可读性；
    - 将原本静态的 `<span>N 个子代理</span>` 升级为交互式按钮 `<button type="button" class="tag sub-count sm-toggle-subs">`，内置动态旋转指示箭头（SVG Chevron）；
    - 支持两种便捷收起/展开方式：点击“N 个子代理”按钮，或直接点击会话行本身（自动规避复选框、删除按钮等操作区）；
    - 展开状态下子代理列表清晰展示，收起状态下 `.subs.folded` 隐去列表，指示箭头平滑逆时针旋转 90 度呈收起状态；
    - 展开偏好持久化：通过 `agenthub_expanded_subagents` 键在 `localStorage` 中记录用户主动展开的会话 ID，跨刷新与重绘保持记忆。

- **修复设备绑定成功后 Web 控制台未显示设备的通讯链路缺陷（`crates/runner` / `services/api` / `apps/web`）**：
  - **根因分析**：
    1. **Runner 配对后未即时切换模式**：本机 Runner 接收 `/api/device/bind` 后仅写入 `~/.agenthub/binding.json`，未向正在运行的后台 `daemon` 调度 `DaemonRequest::SetRemoteEnabled { server_url: Some(gateway_url) }`，导致守护进程仍停留在 Local-only 模式，未建立到网关 `/v3/remote/ws` 的出站 WebSocket 连接；
    2. **Runner 重启未继承持久化配置**：`main.rs` 在启动 `Command::Daemon` 和 `Command::Web` 时未解析持久化绑定文件，导致 Runner 重启后网关地址丢失；
    3. **网关注册与会话映射割裂**：网关在收到设备注册后写入 `devices` 表，但 `/v3/remote/sessions` 仅单向查询 `remote.db` 中的 `content.runners(owner)`，在客户端完成首次 WebSocket Hello 握手前该设备在会话快照中不可见；
    4. **Web 设置页缺乏配对感知轮询**：用户在 Web 控制台生成绑定码并在本地客户端配对成功后，Web 端未在绑定弹窗打开期间进行高频设备轮询，弹窗不会自动关闭且设备列表未实时刷新。
  - **全链路修复**：
    - **Runner 即时连网与解绑支持（`crates/runner/src/web_server.rs`）**：在 `/api/device/bind` 成功后立即触发 `handler.handle(DaemonRequest::SetRemoteEnabled { server_url: Some(gateway_url) })` 并清空看板缓存，同时补齐网关地址自动规范化（缺失前缀时默认补齐 `https://`）与 `POST /api/device/unbind` 解除绑定接口；
    - **Runner 自启动持久化绑定恢复（`crates/runner/src/main.rs`）**：新增 `resolve_bound_gateway_server()` 解析 `~/.agenthub/binding.json`，在自启、托盘或无参数启动时自动继承已绑定的云端网关；
    - **网关已注册设备投影（`services/api/src/remote-routes.ts`）**：`GET /v3/remote/sessions` 统一将已注册且未吊销的设备预先注册至 `content.register(owner, device_id)`，并将离线但已绑定的设备投影至 `runners` 返回结果，确保 Web 端立即呈现；
    - **Web 端主动感知轮询（`apps/web/src/components/SettingsPage.tsx`）**：在绑定弹窗开启时以 2 秒间隔自动轮询设备列表，检测到新设备绑定成功后自动关闭弹窗并刷新设备列表；切换至设备标签页时自动刷新。

- **用量页的 Token 总量改为全机口径（`crates/runner/src/usage_ledger.rs`）**：
  - **根因**：账本此前只读 Coding Assistant 的转录（实测 11.67B），而 magpie 的 25B 还含 ZCode 与 Codex，两个数字自然对不上；现按"对齐 magpie 全量"把本机其他智能体的用量并入同一账本，实测总量 **28.30B / 203,464 次请求 / $11,117.91**，其中 ZCode 14.27B、Claude Desktop 11.67B、Codex 2.37B、Coding Assistant 0.0002B；
  - **ZCode**：`model_usage` 一行一次调用，字段已分好 prompt / cache / completion 与两端时间，直接取用；早于该表的会话退回读 assistant 消息自带的 `tokens` 块。两条路径的 `input` 语义相反——`model_usage.input_tokens` **不含**缓存，消息的 `input` **包含**缓存（其 `total` 即 `input + output`）——所以减法只在消息路径做，否则 `model_usage` 会被压掉 2.44B；
  - **Codex**：每次会话一个 zstd 压缩的 rollout，末尾 `token_count` 的 `total_token_usage` 即累计值，只取最后一次以免把累计当增量重复相加；`input_tokens` 本就含 `cached_input_tokens`，故只记 input 与 output，不再加回缓存。早于 `provenance` 的 rollout 从最后一次 `turn_context.model` 取名（原先 1.19B 落在空模型名桶里），一分未花的 rollout 直接跳过，排名不再出现零值行；
  - **轮询成本**：rollout 是整段压缩会话（本机 200 个共 525 MB），新增按（长度, 修改时间）记忆的 `LedgerCache.codex`，未变化的 rollout 不再解压；新增 `a_codex_rollout_is_read_once_and_reread_when_it_grows` 断言"读过的不重读、变长的会重读"。实测冷启动 12.5 s、账本 TTL 内 1.25 s；
  - **语义**：失败（`error`/`rate_limit`）的调用计入总量但不进模型排行，与转录路径一致。

- **托盘菜单改为紧凑的条目列表，右键不再转圈（`crates/runner/src/tray_menu.rs` / `tray.rs`）**：
  - **根因（光标）**：弹窗窗口类注册时 `hCursor` 为 0，Windows 于是沿用线程当前光标；托盘线程刚处理完右键，带的还是"忙碌"光标，所以菜单一出现指针就转。现注册 `IDC_ARROW` 并在 `WM_SETCURSOR` 里无条件 `SetCursor`，指针一进菜单即恢复箭头；
  - **版式**：卡宽 292→248、行高 43→36、圆角 17→10、投影留白 16→10，100% DPI 下条目区卡面 248×250、整窗 268×270（原为 292×320 / 324×352）；头部由"品牌标+标题+状态两行"压成单行 `● AgentHub（就绪） … v2.5.3`，去掉自绘六边形标记；条目字号 16→14px、标题 15.5→13.5px、行内左边距 45→14；
  - **悬停**：高亮条此前按窗口坐标定位，左侧贴着边框、右侧短 16px；改为按卡片内缩 5px 对齐，圆角 7px，进出两行仍只重绘各自行带；
  - 首项文案与托盘一致改为"显示协同控制台"。

- **全平台图标高清化与三维 AI 聚合魔方重构（`crates/runner` / `apps/web`）**：
  - **视觉设计与品牌重构**：
    - 全新引入三维等距（Isometric 3D）**AI 聚合立方魔方**（AI Aggregation Cube），取代先前的抽象几何棱镜：
      - **顶立面（Top Face）**：深邃机甲哑光底板嵌 Claude Terracotta 赤陶太阳芒矢量徽记；
      - **左立面（Left Face）**：银灰金属质感切面嵌 DeepSeek 彩虹浪涌动感徽记；
      - **右立面（Right Face）**：玄武岩深灰切面嵌 OpenAI ChatGPT 经典六瓣花环徽记；
      - **外骨骼机甲框架（Chassis）**：四角高精度倒角内凹机身与精密切削导角，象征 AgentHub 将 Claude、DeepSeek、OpenAI 等顶级 AI 智能体跨设备无缝协同汇聚。
  - **极致纯净透明背景与射线轮廓追踪抠图（Radial Ray-Casting Silhouette Extraction）**：
    - 针对原设计暗色背景与立方体左侧暗面凹槽微弱色差（暗槽灰度 ~25，外景 ~22），摒弃易泛洪漏渗的传统算法，采用自立方体几何中心 `(512, 512)` 呈 3600 根径向射线（0.1° 角分辨率）向外步进的反向轮廓扫描算法，精准锁定导角机甲外轮廓（灰度阈值 ≥38），完整保留机身卡槽与倒角细节；
    - 结合 3×3 高斯亚像素边缘平滑（$\sigma = 0.5$）与外沿色溢消除算法，在纯白（`#ffffff`）、暗黑（`#181820`）及高饱和亮青（`#06b6d4`）背景下均无黑边（Zero Dark Halo）或背景杂色溢出，实现 100% 纯透明无边框坐落。
  - **消除 Windows 11 高分屏缩放模糊与全链路高阶光栅化（任务栏图标发糊的真正根因）**：
    - **根因定位（实测而非推断）**：本项目是 **Rust** 写的 `agenthub-runner`，它用 **Microsoft Edge 的 `--app` 模式**承载桌面控制台，并非 Electron/Tauri/Qt，因此 `icon:` 配置项、`npx @tauri-apps/cli icon`、`setWindowIcon` 等框架做法一概不适用。任务栏按钮图标最终由 **Windows 从 Chromium 上报的 favicon 位图**绘制，而 Chromium 的 `Favicons` SQLite 缓存**只会保存 16×16 与 32×32 两帧**（实测 `favicons` 表仅 1 行：`http://127.0.0.1:4200/icon-32.png`，`favicon_bitmaps` 表仅 16/32 两行），无论页面声明了多少尺寸。AgentHub 窗口实际落在 **1920×1080 @ 100% DPI** 主屏上（`GetDpiForWindow` = 96），任务栏槽位实测恰为 **24×24 物理像素**——于是系统只能把 32px 位图缩到 24px，插值把清晰的像素抹糊；
    - **实测任务栏绘制规则**：用纯色探针图标（红/绿/蓝）分别写入 `WM_SETICON` 的 `ICON_BIG` 与 `ICON_SMALL` 槽位并 `PrintWindow` 抓取真实任务栏，确认任务栏**只读 `ICON_BIG`**（`WM_GETICON` wParam=1），完全忽略 `ICON_SMALL`；且 `ICON_BIG` 的槽位物理尺寸恒为 `24 × dpi / 96`（100% → 24、175% → 42）。进一步用 16/24/32/48px 探针复测：Windows **仅在图标自身像素尺寸恰好等于槽位尺寸时才 1:1 直绘**，其余尺寸一律重采样——这正是此前"多帧 ICO 仍然发糊"的原因，ICO 里多存几帧并不能让系统挑中它；
    - **绕开 favicon，直接接管窗口图标（新增 `crates/runner/src/window_icon.rs`）**：Runner 在拉起 Edge 后启动一个看门狗线程，定位标题以 `AgentHub Runner` 开头的 `Chrome_WidgetWin_1` 窗口，按窗口当前所在显示器的 DPI 算出任务栏槽位尺寸（`24 * dpi / 96`），从自身内嵌 ICO 资源里 `LoadImageW` 出**与槽位等大的原生帧**（新增 24/30/36/42/48 五个槽位尺寸，分别对应 100%/125%/150%/175%/200% 缩放），再以 `WM_SETICON` 写回 `ICON_BIG`；看门狗每 400ms 比对窗口当前持有的句柄，一旦 Chromium 用自己的 favicon 抢回图标就再次覆写，因此不与 Chromium 抢写、也不会被其覆盖；
    - **原生帧补齐（14 帧 ICO）**：ICO 扩充为 16、20、24、28、**30**、32、**36**、40、**42**、48、**56**、64、128、256 共 14 帧原生位图（32bpp BGRA DIB，256px 用 PNG 压缩），并新增同名 `icon-30.png` / `icon-36.png` 供浏览器按需点阵直取；`web_server.rs` 改为表驱动一次性下发全部尺寸路由；
    - **Chromium 侧同样受益**：`dashboard.html` 显式声明 17 个 `<link rel="icon" sizes="…">`（含 28/30/36/42/56/72/96），Web App Manifest 与 `apps/web/app/layout.tsx` 的 `metadata.icons` 同步补齐全尺寸阶梯，浏览器按原生点阵网格取图，不再插值拉伸；
    - **留白压缩与边缘去色溢**：旧版四周留白高达 9.4%（256px 下左右各 24px），魔方被压到画布 81%；现按内容包围盒重新裁切并只留 2.5% 透明边距，小尺寸下魔方占幅提升到 93%~100%，28px 档三个品牌徽记全部可辨；
    - **边缘色溢消除（Zero Dark Halo）**：抠图残留的暗色源背景会在半透明边缘形成黑边。改用最近实心像素回填（distance-transform 最近邻）把前景本色扩散到边缘，且在**每一次重采样之后**再做一次回填——否则 Lanczos 振铃会把半透明像素的颜色压到 0（黑）而重新长出黑边。以「半透明像素与其最近不透明像素的颜色差 > 40」为判据，16/20/24/28/30/32/36/40/42/48/56/64/72/96/128/256/512 各档实测**色溢像素数均为 0**；
    - **Lanczos 采样 + 微对比度非锐化掩模（Unsharp Masking）**：针对 ≤48px 的超小尺寸图标，在 RGB 通道施加微对比度锐化（`radius=0.8, percent=120, threshold=2`），确保缩到任务栏后三个品牌图形与导角机身依旧清晰；
  - **启动层缓存主动淘汰与全链路更新**：`tray.rs` 启动 Edge 前主动清理 `Default/Favicons*` 与 `HubApps Icons*`，`web_server.rs` 注入 `no-cache, must-revalidate` 头与版本校验后缀 `v=2.5.7`。重新编译发布 `agenthub-runner.exe`（13,469,696 字节，内嵌 14 帧原生 ICO 资源）与 Windows 发行包 `AgentHub-Runner-v2.5.3-windows-x64.zip`（5,743,543 字节，SHA-256: `9a8775191adc5aba11f54a22a54eb6e68ed76c1cb0c760c5854d5a934cc7d12c`）。实机验证：修复前 100% 主屏任务栏图标对 24px 原生帧的匹配 MAE 为 5.05~16.20（视候选源而定，整体明显发糊），修复后 `WM_GETICON(ICON_BIG)` 持有的位图经 `GetIconInfo` + `GetDIBits` 读回，与 `icon-24.png` 的 **RGB 与 alpha 通道 MAE 均为 0.0000**；`PrintWindow` 抓取真实任务栏像素与之比对 **MAE = 0.07**，切到 175% 副屏后 `ICON_BIG` 自动切换为 42×42 原生帧、该屏任务栏匹配 **MAE = 0.00**；连续 12 秒以 100ms 采样 `WM_GETICON`，句柄变更 0 次（无图标抖动回退）。

- **代码审查全量修复（删除、鉴权、缓存与重绘）**：
  - **会话删除不会再半途而废（`collectors/mod.rs`）**：`file-history/<id>`、`session-env/<id>` 与会话目录 `<id>` 同名，原先统一平移到 `trash/<basename>`，第二次 `rename` 撞上已存在的目录（os error 145），跨卷回退又对目录调用 `remove_file`（os error 5），结果是转录已挪走、接口却报失败。现由 `trash_target()` 在重名时带上来源目录名（`file-history-<id>`、`session-env-<id>`），`move_into()` 按类型复制/删除并在删除失败时撤回副本；移动循环抽成 `move_all_into()`，任一项失败即把已移动的项按逆序移回原位，并在错误里说明"已移动 N 个文件后，某文件失败，已全部放回"。新增 `deleting_a_session_that_has_two_folders_named_after_it` 与 `a_delete_that_fails_part_way_puts_the_session_back` 两个测试；
  - **防跨站驱动（`web_server.rs`）**：所有响应不再下发 `Access-Control-Allow-Origin: *` 与预检 `Max-Age`，新增 `origin_allowed()` 严格同源校验（`Origin` 必须等于 `http://{Host}`），在任何请求体被读取之前 403；无 `Origin` 的 CLI/托盘请求照常放行。`OPTIONS` 只回裸 `204`，不授予任何跨源权限。除 `only_the_consoles_own_origin_may_drive_the_daemon` 单测外，新增 `a_foreign_origin_cannot_post_to_the_daemon` 真正起服务并用 HTTP 断言：外来 `Origin` 与 `"null"` 均 403，控制台自身源与无 `Origin` 请求 200，且响应不含任何 CORS 授权头；
  - **删除后缓存立即失效（`web_server.rs` / `usage_ledger.rs` / `collectors/mod.rs`）**：删除成功时清空 12 秒的 `dashboard_summary_cache`、`usage_ledger::forget_transcripts()` 清空账本缓存，并新增 `forget_subagents()` 丢弃子代理记忆，避免页面刷新后已删会话仍在、二次删除被告知"no such session"；
  - **子代理读取加速（`collectors/mod.rs`）**：`read_claude_subagent_cached()` 以（文件长度, 最后写入）为键做进程内记忆，已完成的子代理转录不再每次轮询重读全部字节；超过 500 个时按修改时间保留**最新**的，而不是原先按路径字典序保留最早的。新增 `a_finished_subagent_is_read_once` 用固定 mtime 断言"未变化的文件不重读、变长的文件会重读"；
  - **整轮下限的单测理由与门槛对齐（`usage_ledger.rs`）**：`a_transcript_tells_only_the_whole_turn` 里的 17k token / 77 ms 用例实际是被 1,000 ms 时长门槛挡下，原注释却称它证明了"首行不是首 token"。现改为显式断言该用例因时长被拒，并新增一条 17,145 token / 1.5 s（11,430 tok/s）用例真正触发 10,000 tok/s 上限，再用 14,000 token / 1.5 s（9,333 tok/s）证明上限之下照常给出下限；
  - **写入守卫不再跟随链接（`collectors/mod.rs`）**：`written_within()` 改用 `symlink_metadata` 并加 16 层深度上限，指向上级目录的 junction 不会再把守护进程递归到爆栈；
  - **用量账本不再解析巨型行（`usage_ledger.rs`）**：用户/tool-result 行只按字节扫描 `"timestamp":"…"` 取时刻，不再把数百 MB 的 base64 工具输出交给 JSON 解析器；
  - **托盘菜单悬停只重绘一行（`tray_menu.rs`）**：卡片距离场、阴影、字型与 DIB 在菜单存活期间缓存（`Frame`），悬停切换只重算进入/离开的两条行带并只回传这些行；原先每次 `WM_MOUSEMOVE` 都重建整窗、逐像素求两次 SDF 并新建 DIB。新增三个单测覆盖"只动一行""离开即还原""换菜单才重绘"；
  - **删除交互不再吞掉选择（`dashboard.js`）**：空列表渲染时清空已选并同步工具条；`POST /api/sessions/delete` 返回非 2xx 时提示 HTTP 状态、保留勾选与确认弹窗，不再静默关闭；
  - **复制按钮补齐样式（`apps/web/src/components/remote.css`）**：`remote-copy-badge` / `remote-copy-action` / `remote-copied-badge` 此前无任何样式定义，图标与文字紧贴、已复制状态无颜色变化；现补齐 `inline-flex` 间距与翠绿加粗的已复制态。

- **修复能力发现单测在干净 CI 环境断言失败（`crates/runner/src/collectors/capabilities.rs`）**：
  - 根因：`test_discover_capabilities_smoke` 此前硬编码断言 `caps.total_skills > 0 && caps.total_hooks > 0`，在未配置全局 `~/.claude` 或 `~/.agenthub` 的纯净 Linux/CI 容器中必定断言失败（`assertion failed: caps.total_skills > 0`）；
  - 修复：将烟雾测试调整为验证总数与列表集合一致性，并新增独立单元测试 `test_discover_capabilities_in_workspace`，基于隔离的 `tempdir` 构造 Mock 工作区并验证 Skills 与 MCP 服务发现逻辑闭环。

- **Web 控制台 UI/UX 重构与交互精简（`/ui-design`）**：全站移除 Emoji，改用 2.2px 线宽内联 SVG 图标；拔除 `.remote-session-tree` 顶部渐变遮罩与 `.settings-page` 硬编码暗色变量，恢复浅/暗主题自然继承；输入框 42~46px、按钮 38~48px，补齐 GPU 加速微动效与 WCAG AA 对比度，统一 Claude Terracotta 品牌色；侧边栏外观主题切换收拢至设置页（外观与主题，带 SVG 矢量图标预览与卡片微动效），彻底移除侧栏页脚堆叠控件；顶部面包屑仅在进入子层级（`selectedSessionPath.length > 0`）时动态呈现，会话过滤胶囊行仅在存在多厂商（`availableProviders.size > 1`）时呈现，彻底根除根视图下"全部会话"与单厂商"全部"垂直堆叠的重复按钮问题。
- **Remote 控台的转场与路由修复（`apps/web`）**：面包屑从侧栏移到主区头部并删除侧栏重复项；移除客户端路由段的多余导出，修复 Next.js 构建；会话切换、欢迎页、设置弹窗与标签分区、新建会话对话框、消息卡片与审批/提问卡片统一加入进场动画，欢迎页快捷胶囊补齐悬停抬升与点击反馈，全部在 `prefers-reduced-motion` 下自动降级。
- **文档无损精简与事实对齐**：精简 `AGENTS.md`、`README*.md`、`llms*.txt` 与 `docs/*.md`（含验收记录、宣发模板与 `RELEASE_NOTES_*.md` 原位瘦身），`docs/CHANGELOG.md` 100% 保留全部历史版本头；统一 UTF-8 无 BOM 与纯 LF，清理过期测试计数、旧入口命令与相对时间词。
- **输出速度只报整轮下限，不再出现 3,000+ tok/s 的假数字（`crates/runner/src/usage_ledger.rs`）**：
  - 根因：Claude 转录里同一条消息的每个内容块都会重复写一次最终 usage，消息的"第一行"是第一个内容块而不是第一个 token；按首行→末行计时读到过 8,300 tok/s，属于把"整块落地"当成"逐 token 流式写出"；
  - 对齐上游 magpie 的 `Row.Floor`：转录来源的调用**只**报整轮下限（从"被提问"到最后一行），要求输出 ≥1,000 token、时长 ≥1,000 ms、且不高于 10,000 tok/s，否则不计；
  - `Timing` 删掉 `decode_*` 三个字段，`write_ms()`/`decode_window()` 移除，`LedgerRecord.t_start` 一并删除；线上实测从 1,170 tok/s 降到 **149.9 tok/s / 1,287 个整轮回复**；
  - 前端 `ledSpeed()` 改为 `ledFloor()`，KPI、趋势图、排行、tooltip 一律显示 `≥N tok/s`，速度轴改为按"每个模型自己的下限刻度"绘制，不再把速度当成可叠加份额。
- **用量趋势的指标/分组按钮全部生效（`resources/dashboard.js`）**：三组按钮原来只写 localStorage 与 `.on` 状态、不触发 `pickUsageSegs()` 重绘，现改为切换后立即重算分组与图表；实测 Token/费用/请求/速度与模型/模型·智能体/智能体六个按钮均会切 `.on` 并重绘。
- **会话页对标 magpie：子代理展开 + 多选删除（`collectors/mod.rs` / `remote_host.rs` / `web_server.rs` / `dashboard.js`）**：
  - 新增 `ClaudeSubagentEntry` 与 `collect_claude_subagents()`，读取 `<session>/subagents/agent-*.jsonl` 与 `workflows/<run>/agent-*.jsonl`，从 `.meta.json` 取 `agentType`、`description`、`spawnDepth`、`parentAgentId`，按 `message.id` 聚合请求数与 token；子代理缩进挂在父会话下方，显示类型、任务说明、模型、请求数与 Token；
  - 新增 `POST /api/sessions/delete` 与 `delete_claude_session()`：把会话转录、子代理目录、`file-history/`、`session-env/`、`todos/` 一并移入 `~/.agenthub/trash/sessions/<时间戳>-<id>/`（跨卷时复制再删除），最近 60 秒内仍在写入的会话拒绝删除；
  - 前端补复选框、全选（含 indeterminate）、`N 个已选`、行内悬停删除与二次确认弹窗；
  - 顺带修复会话行读错 id 的缺陷：守护进程下发的是 `provider_session_id`，页面只读 `session_id || id`，导致每行 `data-id` 为空、勾选与删除实际作用在空字符串上。
- **能力生态页改回控制台统一行式排版（`dashboard.css` / `dashboard.js` / `dashboard.html`）**：删除 emoji 分区头、彩色徽章、悬停上浮、两行截断与 `word-break` 卡片体系，改为 `.cap-row` 平铺行（名称 + 标签 + 一行说明 + 省略号路径 + 右侧智能体图标与元信息）；实测 49 行无横向溢出、无内容超出组件，`@container` 窄屏自动换行。
- **设置页标签行吸顶（`dashboard.css` / `dashboard.js`）**：`.set-head` 改为 `position: sticky; top: -16px` 并负边距铺满视图宽度、自带背景遮住滚动内容，滚动超过 2px 才淡入底部发丝线；实测滚动 900px 后标签行仍固定在页面顶端。
- **构建与发版**：因上述修复晚于原 2.5.2 产物，按"发版必自增版本"升到 **2.5.3** 并重新编译 `cargo build --release -p agenthub-runner`，打包 `AgentHub-Runner-v2.5.3-windows-x64.zip`（5,736,204 字节，含 13,458,944 字节的可执行文件、`README.md`、`Connect-AgentHub.cmd`、`Connect-AgentHub.ps1`）与 `AgentHub-v2.5.3-SHA256.txt`（`745ceefa…1fea`）；同步提升 `Cargo.toml`、根 `package.json`、`apps/web`、`packages/contracts`、`services/api` 与托盘菜单测试夹具的版本号；2.5.2 的 zip 与校验清单已按"打新包必删旧包"移除，产物目录只保留最新一版。验证：`cargo test --workspace` 277 项全绿（217 库 + 60 集成），`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all --check` 均通过；并用隔离的 `CLAUDE_CONFIG_DIR` 起真实守护进程做端到端复核：外来 `Origin` 的 `POST /api/sessions/delete` 与 `OPTIONS` 均返回 403，同源与无 `Origin` 请求 200；造出同时含转录、会话目录、`file-history/`、`session-env/` 与 `todos/` 的会话后一次删除，六个文件全部落入 `~/.agenthub/trash/sessions/<stamp>-<id>/` 且各占独立名字，源目录清空，二次删除返回 `no such session` 而不是"删一半"。

## [2.5.3] - 2026-10-20

- **会话删除不再"删一半"（`crates/runner/src/collectors/mod.rs`）**：`file-history/<id>`、`session-env/<id>` 与会话目录 `<id>` 同名，多个来源平移进同一个 `trash/<basename>` 时第二次 `rename` 撞上已存在的非空目录（os error 145），跨卷回退又对目录调用 `remove_file`（os error 5），导致转录已被挪走、接口却报失败。现按来源限定重名目标，移动循环抽成 `move_all_into()` 并在任一项失败时按逆序把已移动的项放回，成功后才丢弃子代理、账本与仪表盘缓存。
- **防跨站驱动（`crates/runner/src/web_server.rs`）**：响应不再下发 `Access-Control-Allow-Origin: *`，`origin_allowed()` 在读取请求体之前要求 `Origin` 等于 `http://{Host}`，其余一律 403；无 `Origin` 的托盘与 CLI 请求照常放行，`OPTIONS` 只回裸 `204`。
- **子代理读取按（长度, 修改时间）记忆（`crates/runner/src/collectors/mod.rs`）**：已完成的子代理转录不再每 5 秒轮询重读约 81 MB，超过 500 个时保留最新的一批。
- **整轮下限单测与实现门槛对齐（`crates/runner/src/usage_ledger.rs`）**：修正 17k token / 77 ms 用例的断言理由（实为时长门槛所拒），并补一条真正触发 10,000 tok/s 上限的用例。

## [2.5.2] - 2026-10-19

- **托盘图标回到可辨识的浅色六边形（`crates/runner/resources/icon.svg`）**：撤回 2.4.98 的深蓝渐变底白线版本（16px 下糊成一团），恢复 `#f8fafc` 圆角白底 + `#0284c7` 六边形与辐条的原始构图，并另做 16/20/24px 加粗变体（线宽与节点放大一档）；`icon.ico` 重建为 16/20/24/32/40/48/64/128/256 九帧（135,162 字节），`icon.png`、`icon-192.png`、`icon-512.png` 与 `apps/web/public/` 下同名资源同步重渲染且 MD5 一一对应。
- **右键菜单改为自绘亮色卡片（`crates/runner/src/tray_menu.rs`）**：
  - 移除 `TrackPopupMenu` 原生菜单与 `SetPreferredAppMode(2)` 调用（ordinal 135 的 `2` 实为 ForceDark，正是菜单发暗的根因，原注释误标为 AllowDark）；
  - 新增分层窗口菜单：`WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST` + `UpdateLayeredWindow` 预乘 BGRA，纯 Rust 圆角 SDF 抗锯齿与两遍盒式模糊投影，白卡 `#ffffff`、`#dcdce4` 细边框、`#eeeef2` 发丝分隔线；
  - 按 "控制间距、字体更大更圆润" 重排版式：卡宽 292、行高 43、左右内边距 15、圆角 17、投影留白 16；菜单项 `Microsoft YaHei UI` 16px，标题 15.5px 半粗，版本/状态 12.5px，行距与分隔留白拉开后不再拥挤；
  - 头部固定展示品牌标、版本、绿点运行状态与本机地址；五项动作分三组（打开协同控制台 / 查看演进拓扑（网关）/ 在系统浏览器中打开 ‖ 打开数据存储目录 ‖ 退出 AgentHub 红字），悬停按亮色 `#f2f2f7` 高亮；
  - 关闭与焦点：托盘浮出层在按下图标后会抢前台，故设 400ms `SETTLE` 窗口忽略早期 `WA_INACTIVE`，并挂 120ms 线程定时器 `hide_if_clicked_away()` 按光标位置收起；`closed_within(250ms)` 让"点图标收起菜单"不会顺带弹出控制台窗口；
  - 按 DPI 缩放（`GetDpiForMonitor`）并把卡片夹进光标所在显示器工作区，放不下时翻到光标下方。
- **网关入口直达设置分区（`tray.rs` / `resources/dashboard.js`）**：菜单项改为打开 `#settings/gateway`；控制台新增 `applySettingsHash()` 与 `hashchange` 监听，取走 hash 后以 `replaceState` 清除，刷新仍回到读者上次停留的页面。
- **开发工具与依赖**：新增 `crates/runner/examples/tray_menu_preview.rs`，`cargo run -p agenthub-runner --example tray_menu_preview` 直接在光标处画出菜单，便于脱离托盘单独验收版式；`windows-sys` 增开 `Win32_UI_Controls`、`Win32_UI_HiDpi`、`Win32_UI_Input_KeyboardAndMouse`。
- **构建与发版**：全工作区版本递增至 `2.5.2`，归档 `AgentHub-Runner-v2.5.2-windows-x64.zip` 与 `AgentHub-v2.5.2-SHA256.txt`，按"打新包必删旧包"移除 2.5.1 的 zip 与校验清单。

## [2.5.1] - 2026-10-19

- **内嵌控制台主视图收敛（4 页 + 统一设置视图）**：删除 `#view-models` 与 `#view-gateway`（上游路由与拓扑统一交由 CPA 管理），主导航收敛为 **会话 / 智能体 / 用量 / 工作区**；原能力视图与设备配对分别并入设置页 **能力生态** 与 **设备与网关**（`setPage-gateway`）分区，移除页脚冗余端口显示。
- **逐请求用量账本与对标 magpie 的用量页（`GET /api/usage/ledger`）**：在 `crates/runner/src/usage_ledger.rs` 按 `message.id` + `requestId` 聚合 Claude、Codex 与 ZCode 逐次调用，10 分钟窗口内自动配对失败后重试成功并计算输出速度（tokens/s）；前端提供概览/请求/会话/上下文四段标签、5 格 KPI、堆叠趋势图、53 周热力图与 UTF-8 BOM CSV 导出。
- **magpie 厂商图标与五款官方智能体卡片**：从上游 `magpie/internal/gui/assets/icons/` 逐字节引入 19 个 SVG 与 1 个 PNG 至 `crates/runner/resources/icons/`（`/icons/<name>.<ext>`）；智能体卡片严格限定为 `claude_code`、`claude_desktop`、`codex`、`grok`、`gemini` 五款产品（258 项 Rust 测试全绿）。

## [2.5.0] - 2026-10-19

- **设置弹窗重构为独立视图（`#view-settings`）**：对齐上游 magpie 页面模型，点击顶栏齿轮（`Ctrl+,` / `Esc`）原位切换 `.view-slot` 为五分区设置页（通用 / 外观 / 设备与配对 / 网关 / 关于），采用 `.list.prefs` + `.row.pref` 结构并支持跨分区关键词实时过滤，收拢顶栏裸露的刷新、主题与轮询控件。
- **开发构建静态资源热重载（Debug Hot Reload）**：`cfg!(debug_assertions)` 下每次请求直接读取 `crates/runner/resources/` 磁盘文件并以文件 mtime 注入 `?v=2.5.0-<nanos>` 防缓存查询串，配合 `GET /api/dev/wait?since=<stamp>` 长轮询实现保存即刷新；Release 与测试仍走单文件内嵌资源。
- **视图切换竞态与后台挂起修复**：为 `startViewTransition` 引入单调递增序号防止快速连点旧回调覆写新视图，并在隐藏窗口或 300ms 超时未就绪时直接完成 DOM 切换；修复 `AppLayout.storage` 缺失 `motion: agenthub_motion` 键与 `#refreshBtn` 旋转态问题。

## [2.4.99] - 2026-10-19

- **Runner 事件队列可重入死锁根治（`crates/runner/src/remote_host.rs`）**：修复 `events_queue.lock()` 持有期间二次加锁导致的守护进程挂起，确保高频流式输出与历史快照并发入队零阻塞。
- **多智能体扩展生态纳管（`crates/runner/src/library.rs`）**：扫描纳管 Skills（`~/.claude/skills`、`.claude/skills`、Codex 技能目录及 YAML Frontmatter）、MCP Servers（`~/.claude.json`、`~/.codex/config.toml`、`.mcp.json`、`~/.agenthub/mcp.json`，内置零依赖 `parse_mcp_from_codex_toml`）与 Hooks（`PreToolUse`/`PostToolUse`/`SessionStart`/`PermissionRequest`），支持跨智能体去重、注册（`POST /api/capabilities/mcp`）与热重扫（`POST /api/capabilities/rescan`）。
- **上下文容量环、多选问答与增量 Markdown 解析**：Runner 将 Claude `result`、Codex `thread/tokenUsage/updated` 与 ACP `usage_update` 归一化为 `{used, window}` 并在移动端渲染容量环；支持 `multi_select` 问答卡片数组提交（`string[]`）与流式增量 Markdown 尾部缓存。

## [2.4.98] - 2026-10-19

- **内嵌控制台网关看板与顶栏设置入口**：网关视图重构为 4 组状态卡片、会话亲和缓存命中对比拓扑、快捷参数复制胶囊与 4 语言（Bash / PowerShell / Node.js / Python）接入代码切换器；顶栏关机键替换为设置齿轮（`Ctrl+,`）并新增 `/api/open-data-dir` 一键打开数据目录。
- **Win32 多分辨率高清托盘与现代上下文菜单**：内置 16×16 至 256×256 共 9 帧 ICO 图标，使用 `LoadImageW` 按 `SM_CXSMICON` / `SM_CXICON` 1:1 像素加载，新增 PWA 高清图标（`icon-192.png`/`icon-512.png`）与原生 Win32 托盘菜单。
- **移动端流式转录、历史快照与工具卡片归一化**：`history/snapshot` 的 `content` 在投影层归一化为 `text`，为 `item/started` 开放窄字段白名单；合并 `item/started` 与 `item/completed` 为可展开工具卡片并支持补丁着色、连续工具折叠与 `<think>` 思考过程手风琴。

## [2.4.97] - 2026-10-19

- **多智能体 Token 与缓存吞吐量对齐 magpie**：修复 ZCode SQLite `LIMIT 1` 子查询漏计缺陷，通过 `parent_map` 递归汇总 `sess_subagent_...` 子会话，并使用 `WHERE data LIKE '%"role":"assistant"%'` 与 5s 内存缓存将 9.8 万行聚合耗时从 5.13s 降至 0.39s；接入 `~/.cache/magpie/sessions.json` 与 `thread_history_1.sqlite` 补齐 Codex 缓存读取与调用次数。
- **控制台骨架屏与粘性顶栏优化**：新增 5 行 Shimmer 骨架屏占位消除首屏空白闪烁，顶栏导航药丸增至 32px 高并固定 `position: sticky; top: 0; z-index: 20` 保证 `#sideToggle` 随时可达。
- **Android Orca 视觉令牌与列表收敛**：统一 `OrcaSpace`/`OrcaRadius`/`OrcaSize`/`OrcaType` 石墨深色单主题令牌，使用 `OrcaRow`/`OrcaListGroup`/`OrcaStatStrip` 替代碎片化卡片栈并实现 60 FPS 生命周期感知轮询。

## [2.4.96] - 2026-10-19

- **审批卡片可折叠 Diff 预览与本机路径脱敏**：自动提取 Claude Code `Edit`/`Write`、Codex 与 ACP `item/fileChange/requestApproval` 的结构化代码变更，本机绝对路径脱敏为 `[local_path_redacted]` 并执行 64 KiB 帧上限，前端渲染 `+add -del` 统计徽章与双栏着色对比块。
- **通用 ACP（Agent Client Protocol）stdio 适配器（`crates/runner/src/collectors/acp_app_server.rs`）**：实现标准 JSON-RPC 2.0 stdio 客户端（兼容 `claude-agent-acp`、`codex-acp`），通过 `ChildJob`（Windows Job Object `KILL_ON_JOB_CLOSE` / Unix 进程组）隔离进程树，异步桥接长耗时 `session/prompt` 与非阻塞 `turn/start`，并在 `MultiProviderTransport` 支持 `acp_` 前缀与 `agentType == "acp"` 动态路由。

## [2.4.95] - 2026-10-19

- 内嵌 Web 控制台引入 Shimmer 骨架屏、3 次轮询/15s 空状态宽限期与 12s 摘要缓存；Claude 转录改为全文件扫描并计入 `cache_read_input_tokens`，本地模式启动超时缩至 3s；Android 抽离 `OrcaComponents.kt`。

## [2.4.94] - 2026-10-18

- Android 引入对标 `stablyai/orca` 的多设备控制舱与虚拟终端按键行（`Esc`/`Tab`/`Ctrl-C`/方向键）；内嵌控制台补齐 `request_count`/`calls` 统计；完善 GitHub Actions CI、社区健康文件与 `llms.txt` / `llms-full.txt`。

## [2.4.93] - 2026-10-18

- 完成开源工程治理、仓库社区标准文件、大模型检索索引（`llms.txt` / `llms-full.txt`）与宣发文档体系建设，全工作区自动化测试通过。

## [2.4.92] - 2026-10-18

- 将 `crates/runner/resources/dashboard.html` 内联的 1700 行脚本拆分为独立 `app.js` 并纳入语法检查，统一模型费率计算表消除三处重复计费逻辑。

## [2.4.91] - 2026-10-18

- 对标 magpie 桌面极简规范，移除内嵌控制台顶栏冗余应用图标、装饰性 Emoji 与外层大白框容器，统一页面呼吸感与留白。

## [2.4.90] - 2026-10-18

- 修复内嵌控制台 `#sideToggle` 侧边栏开关被外层容器遮挡失效的问题，合并双层顶栏并移除右上角重复关闭按钮。

## [2.4.89] - 2026-10-18

- 为内嵌控制台引入 View Transitions 页面切换、分组折叠缓动、弹窗进出场过渡、数字插值滚动与悬停微交互，同时支持 `prefers-reduced-motion` 降级。

## [2.4.88] - 2026-10-17

- 移除 Edge App 模式下的异色系统标题栏割裂感，统一内嵌控制台顶栏与主画布背景色，对齐 magpie 原生窗口视觉一体性。

## [2.4.87] - 2026-10-17

- 删除内嵌控制台顶栏与侧栏冗余分组小标题（工作台/洞察/系统），统一顶栏与内容区底色。

## [2.4.86] - 2026-10-17

- 内嵌控制台默认窗口尺寸收敛至 820×600（最小 640×460），修复侧边栏展开后切换按钮消失与图标方向错误，精简顶栏元素并放大字号。

## [2.4.85] - 2026-10-17

- 内嵌控制台默认窗口尺寸由 1240×820 收敛至 980×720（最小 720×520），并确保窗口首次启动时在屏幕正中央居中呈现。

## [2.4.84] - 2026-10-17

- 修复内嵌控制台侧边栏拖拽宽度（`--side-w-override`）与响应式断点冲突导致的分界线错位问题，实现主内容区自适应伸缩。

## [2.4.83] - 2026-10-17

- 修复内嵌控制台 `#sideToggle` 窄屏切换失效，优化顶栏排版、全局字号、卡片圆角与窗口默认尺寸调节。

## [2.4.82] - 2026-10-17

- 修复 `crates/runner/src/remote_host.rs` `discover()` 扫描超时或瞬态空结果覆盖既有非空会话缓存导致偶发显示 0 条历史会话的缺陷，并精修控制台界面排版。

## [2.4.81] - 2026-10-17

- 内部过渡版本号，扫描侧空洞修复直接并入 v2.4.82 发布。

## [2.4.80] - 2026-10-17

- `apps/web` 对齐 `agenthub-runner` 最新模型目录探测、多 Provider 分页与 Token 用量聚合契约，优化新建会话模型名称展示。

## [2.4.79] - 2026-10-17

- 修复 `crates/runner/src/remote_host.rs` `thread/list` 跨 Provider 分页边界截断与重复问题，确保多智能体会话合流排序与游标分页稳定。

## [2.4.78] - 2026-10-16

- 根治桌面工作目录残留 `agenthub.db` 与启动控制台黑框闪烁；将本机控制台重构为对标 magpie 的独立桌面窗口体验，统一顶栏底色与居中尺寸。

## [2.4.77] - 2026-10-15

- 对标 `yetone/magpie` 重构 `crates/runner/src/collectors/workspace.rs` 全局工作区深度发现、路径还原与模型真实用量聚合引擎。

## [2.4.76] - 2026-10-14

- 移除多租户商业化遗留组件（Telegram OAuth / 设备码流 / 团队账单），在 `services/api/src/sqlite-account-store.ts` 实现单机 SQLite 账户与会话持久化闭环。

## [2.4.73] - 2026-10-12

- 收敛本地开发辅助资产与 `.gitignore` 规则，解除本地索引与私有网络配置追踪，确保远端开源仓库零本地环境残留。

## [2.4.72] - 2026-10-10

- 跑通 Gateway + Web + Runner + Codex 真实自部署全链路（注册、绑定码、Ed25519 绑定、同源反代 `connect`/`create`/`send_input`/`interrupt`），修复联调发现的边界问题。

## [2.4.71] - 2026-10-10

- 重构 `apps/web/src/components/remote.css` 运行中工具卡片 Shimmer 与滚动容器，全面迁移至 `transform`/`opacity` GPU 合成层以保障 60 FPS 渲染。

## [2.4.70] - 2026-10-10

- 增强 `apps/web/src/server/gateway-proxy.ts` `isLoopbackOrContainerHost` 判定以放行 Docker Compose 容器内网反代，补齐 `deploy/` 容器化编排与健康检查。

## [2.4.69] - 2026-10-10

- 战略重心收敛至 Runner + Gateway + Web 自部署核心链路，移除重型 `apps/desktop`（Tauri）客户端并由 Runner 内嵌轻量控制台与 Web 端承接。

## [2.4.68] - 2026-10-08

- 根目录 `package.json`、`package-lock.json` 与部署编排配置统一由 `agent-pulse` 规范命名为 `agenthub`。

## [2.4.68] - 2026-10-07

- 新增 `ClaudeProcessSession` 将 Claude Code 驱动方式由单次 `--resume` 拉起升级为本地常驻 stdin/stdout 流式双向进程控制。

## [2.4.67] - 2026-10-06

- 清理 Web 端废弃的 17 个旧版监控工作台组件与冗余动画库依赖（`gsap`），收敛未引用导出并完成全端按钮交互审查。

## [2.4.66] - 2026-10-05

- 实现对话轮次中间思考与工具执行过程统一折叠收敛（Turn Process Grouping），突出展示每轮最终回复结论。

## [2.4.65] - 2026-10-05

- 实现连续工具调用分组折叠与分段展开（默认展示首屏 6 条，支持增量展开与一键回缩），防止海量工具调用撑爆会话视图。

## [2.4.64] - 2026-10-04

- 在 Rust 转录解析层过滤 `isApiErrorMessage` 与 `<synthetic>` 合成 429 报错气泡，修复连续用户重试输入被吞缺陷并优化系统错误警示条样式。

## [2.4.63] - 2026-10-04

- 补全 Claude Code 深度思考与上下文压缩流式状态遥测，实现发送期间输入框全生命周期锁定与回合碰撞/网络异常时的乐观消息自动回滚。

## [2.4.62] - 2026-10-04

- 实现同工作区会话状态吸收合并与 `realSessionId` 动态更新，修复侧边栏幽灵执行中会话分身及上游 429 配额耗尽导致的回合死锁。

## [2.4.61] - 2026-10-04

- 规范化 `NativeSessionHost` 会话内部前缀（`claude_` / `antigravity_` / `grok_` / `sess_`），根治宿主内存状态与磁盘观测前缀失配导致的同名会话分身。

## [2.4.60] - 2026-10-04

- 打通 CC Switch 活跃数据库路由与上游 `/v1/models` 实时模型探测，消除静态模型缓存阻断并在下拉展开或窗口切回时自动刷新。

## [2.4.59] - 2026-10-04

- 实现会话首条指令自适应智能命名、前缀规范化交叉索引、无转储残留僵尸会话淘汰与 LocalStorage 孤儿缓存自动清洗。

## [2.4.58] - 2026-10-02

- 修复已有会话续聊模型参数丢失与第三方网关 429 递归重试风暴，支持流式重试与配额耗尽状态在 UI 透明反馈。

## [2.4.57] - 2026-10-01

- 修复新建会话异步拉起瞬态路由假死、成功回合静默退出误报失败与多 Provider 绑定识别偏差，整合官方真实模型动态目录。

## [2.4.56] - 2026-10-01

- 新建会话跳过磁盘探测实现零延迟启动；补齐 ACP 工具特化渲染（提问交互、Web 搜索/抓取卡片、超长输出折叠）与通透白底品牌图标重绘。

## [2.4.55] - 2026-10-01

- 修复新建 Claude 会话误带未初始化随机 UUID 执行 `--resume` 导致的报错，支持无效参数自愈回退与侧边栏精简。

## [2.4.54] - 2026-09-29

- 新增 `WindowControls.tsx` 优化无边框窗口拖拽与标题栏控件，打通 Codex 动态模型发现并重构 Remote 工作区管理。

## [2.4.53] - 2026-09-29

- 移动端 UI 全面去 Demo 化并迁移至暖中性色视觉体系，完成多底色对比度排查与无障碍触控修复。

## [2.4.52] - 2026-09-28

- 对标 Claude Desktop 重构顶栏工具条（应用主菜单 `≡`、侧边栏折叠 `◫`、前进/后退导航）并升级进行中会话实时状态反馈。

## [2.4.51] - 2026-09-28

- 优化 `services/api/src/remote-store.ts` `commandHistory` 按会话列走复合索引查询，消除长时运行后的全表扫描放大点。

## [2.4.50] - 2026-09-28

- 过滤 `~/.claude/projects/**/*.jsonl` 中无实质消息的空会话文件，完成投产前性能优化与真实会话恢复边界修复。

## [2.4.49] - 2026-09-28

- 修正 `turn_state` 与 `execution_state` 对字符串与 JSON 状态对象的双格式解析，根治误报回合正在运行导致无法继续输入的问题。

## [2.4.48] - 2026-09-28

- 新增 `crates/runner/src/claude_gateway.rs` 自动探测 CC Switch / Claude-3p 本地网关配置与路由模型映射，支持开启路由时无缝接管会话。

## [2.4.47] - 2026-09-27

- 新增 `classifyTool` 多维工具分类（`terminal`/`file_edit`/`file_read`/`interactive`）、差异比对展示、交互式提问卡片与富上下文审批卡片。

## [2.4.46] - 2026-09-27

- 重构 `discover_claude_binary()` 为候选收集、版本探测与择优回退机制，避免使用旧版 CLI 恢复新版创建的会话。

## [2.4.45] - 2026-09-26

- 重写 `ContextCapacityPopover` 仅展示 Provider 实测上报的分类 Token 计数，新增思考强度控制与认证失败错误归类。

## [2.4.44] - 2026-09-26

- 移除 `sessionDisplayTitle` 将缺失标题回退为工作区目录名的冗余逻辑（统一回退为短 ID），并支持会话自定义重命名。

## [2.4.43] - 2026-09-26

- 修正远程连接状态指示灯逻辑：当 `snapshot.realtime_connected` 为真时正常亮绿灯（`online`），仅在降级轮询或断连时提示警告。

## [2.4.42] - 2026-09-26

- 侧边栏工作区目录优先按所含有效会话数降序排列（`b.totalCount - a.totalCount`），同数量按字母序平局决胜。

## [2.4.41] - 2026-09-26

- 自动过滤集成测试通过 `os.tmpdir()` 产生的临时工作区（`agenthub-native-*`、`agenthub-remote-*`），防止测试目录污染侧边栏。

## [2.4.40] - 2026-09-26

- 侧边栏工作区目录默认保持折叠（`collapsed`）并记忆展开状态（搜索时自动展开命中项），修复启动加载光圈样式缺陷。

## [2.4.39] - 2026-09-26

- 默认主题统一对齐为清爽亮色模式（`[data-theme="light"]`），修复读取中与会话空状态在浅色背景下的对比度问题。

## [2.4.38] - 2026-09-26

- 树状侧边栏引入会话级（默认前 8 条，单次增量 +16 条）与工作区级渐进分页加载，避免海量历史会话一次性渲染卡顿。

## [2.4.37] - 2026-09-26

- 清理测试遗留伪造设备名并将本机历史碎片设备 ID 统一归集至唯一本机节点，静默隐藏无会话空设备。

## [2.4.36] - 2026-09-26

- 侧边栏重构为设备 -> Agent -> 工作区 -> 会话 4 级文件夹层级树，支持各层级折叠记忆与设备在线状态指示。

## [2.4.35] - 2026-09-25

- 抽离纯函数 `session-tree.ts`（含 12 项单测）构建四层会话树与顶部面包屑导航，支持按发现元数据判定 Provider 与工作区分组。

## [2.4.34] - 2026-09-25

- Web 端在 Host 换代（`host_generation` 变更）后自动按新代次重连已选会话并触发有界历史回放。

## [2.4.33] - 2026-09-25

- Daemon 为缺少原生历史回放的 Provider 生成有界会话快照并沿远程事件链路下发，修复受控会话恢复后网页空白问题。

## [2.4.32] - 2026-09-25

- 加宽 Web 远程会话阅读区与输入框，输入框新增实测上下文容量入口，连续工具事件按组折叠展示并强化本机路径脱敏提示。

## [2.4.31] - 2026-09-25

- Web 登录页与远程续聊界面视觉对齐，侧栏采用设备/工作区/会话层级；修复账户测试固定过期日期断言失败问题。

## [2.4.30] - 2026-09-25

- 完成 Web Console 响应式抽屉与 Android Jetpack Compose 原生对话界面双端移动端 UI 重构与全端版本同步。

## [2.4.19] - 2026-09-19

- 修复 `load_session_conversation` 传入前缀化会话 ID（如 `claude_<uuid>`）未剥离前缀导致 Claude Code 历史会话解析为空的缺陷。

## [2.4.18] - 2026-09-19

- 将 Google Antigravity 与 xAI Grok Build 接入后端统一 Remote 控制流，支持真实全双工远程接管与流式事件同步。

## [2.4.17] - 2026-09-18

- 设置面板新增插件商城（Plugin Marketplace），将实验性 Agent 驱动收敛至插件中心按需启用。

## [2.4.16] - 2026-09-18

- 修复 ZCode app-server 无头模式启动时内存模型目录为空导致自定义模型历史会话恢复报错本机会话不可用的缺陷。

## [2.4.15] - 2026-09-17

- 完成三套主题语义化配色重构（37 个语义令牌 × 3 主题），消除侧边栏徽章、表格与代码块二重边框对比度问题。

## [2.4.1] - 2026-09-15

- 新增 `ZCodeAppServer`（`crates/runner/src/collectors.rs`）直驱 `zcode.cjs app-server`，实现 ZCode 全双工远程控制与流式输出。

## [2.3.6] - 2026-09-14

- Web 端根路径 `/` 移除旧版 `LegacyPanelPage` 监控大屏，默认直出 Remote Console 并接入 Runner 在线指示灯与设备绑定入口。

## [2.3.5] - 2026-09-12

- Web/Android 对齐 Remote 创建状态机（HTTP 202 `queued` 显示为等待绑定，待真实 binding 或 receipt 终态后再收敛），统一将 `ws-*` 作为不透明工作区 ID 展示。

## [2.3.4] - 2026-09-10

- 对齐 Web/Android v3 Remote 创建、恢复、续聊、审批与中断契约文档，明确 `queued -> accepted -> session-bound` 状态流转与 `ws-*` 隐私边界。

## [2.2.0] - 2026-09-02

- 新增 `list_agent_models` 从本机 Agent 实测会话聚合可用模型列表（去硬编码），并在会话 Composer 支持模型、权限与上下文附件配置。

## [2.1.0] - 2026-09-01

- 支持多 Agent 模型动态切换与记忆持久化，新增暗色 `SelectDropdown` 组件与会话右键快捷菜单。

## [2.0.8] - 2026-09-01

- 发布 AgentHub 2.0.8 Windows x64 安装包（MSI 与 NSIS Setup）。

## [2.0.7] - 2026-09-01

- 发布 AgentHub 2.0.7 Windows x64 安装包（MSI 与 NSIS Setup）。

## [2.0.6] - 2026-09-01

- 重构双栏低噪音工作台 UI，引入暖纸色主舞台、深色导航与 Provider 信号色体系，并发布 2.0.6 Windows x64 安装包。

## [2.0.5] - 2026-09-01

- 侧边栏按 Provider 供应商分组组织会话，支持快捷搜索、前后导航与工作台模式切换。

## [2.0.4] - 2026-09-01

- 登录界面新增重新打开授权网页与取消重登控制面板，解决误关浏览器授权页后客户端阻塞问题。

## [2.0.3] - 2026-09-01

- Windows 端改用原生 `ShellExecuteW` API（`shell32.dll`）直调默认浏览器，消除 `cmd`/`powershell` 子进程黑框闪烁。

## [2.0.2] - 2026-09-01

- 修复 Windows 下 `cmd /c start` 调起浏览器时 `&` 字符截断 OAuth URL 查询参数的缺陷。

## [2.0.1] - 2026-09-01

- 桌面端 Rust 实现 RFC 8252 §7.3 本地临时回环端口（`http://127.0.0.1:<port>/callback`）PKCE 静默登录，修复 `TgOAuthClient` 空环境变量回退。

## [2.0.0] - 2026-09-01

- 落地 AgentHub v2.0 统一多 Agent 远程控制平面全栈基线，在 `packages/contracts` 实现 v2 Frame Envelope（`seq`/`ack_seq`/`hub_session_id`/`cmd_id`/`is_replay`）与端到端同步链路。
