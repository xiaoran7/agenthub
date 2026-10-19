## What's Changed in v2.5.0

### 🚀 Highlights & New Features

* **⚙️ 设置从弹窗变成独立页面 (Settings as a View, after magpie)**:
  - **对齐上游 magpie 的页面模型**：顶栏齿轮不再是“盖在当前页上的抽屉”，而是像 magpie 的 `#view-settings` 一样，把整个页面切到设置页——顶栏、侧栏、页脚原地不动，只有 `.view-slot` 里的 DOM 换掉。
  - **五个分区标签**：通用 / 外观 / 设备与配对 / 网关 / 关于，每个分区都是 magpie 那种 `.list.prefs` + `.row.pref` 设置行：左边写清楚“是什么、为什么”，右边是控件，绝不出现没有标签的裸控件。
  - **跨分区搜索**：“查找设置…”输入关键词会同时过滤五个分区，命中的行按分区名分组显示，没有命中会明确提示，不会给一个空白页。
  - **开关语义的齿轮**：再按一次齿轮、按 `Esc`，都回到进入设置前的那个页面；`Ctrl+,` 直接打开；打开时齿轮高亮；方向键左右在两个标签间切换。
* **🧹 裸露的操作收进设置 (Bare Controls Moved into Settings)**:
  - 顶栏删掉单独的“刷新”和“明暗主题”图标，只留齿轮；
  - 主题、动效（完整/精简）、自动探测间隔（2/4/10 秒）、侧边栏显隐、立即重新探测、上游模型同步、能力生态同步、设备 ID 与硬件公钥、配对表单、核心版本、监听端点、数据目录、热重载状态、退出守护进程，全部成为设置页里的设置行；
  - 侧栏底部“未绑定 Web 控制台”和会话页的“云端 Web 绑定”磁贴，现在直接跳到“设备与配对”分区。
* **🔥 开发构建热重载 (Hot Reload for Debug Builds)**:
  - `cargo run` 启动的调试构建每次请求直接读 `resources/` 下的页面、样式与脚本，改完保存，窗口自己刷新；release 构建与测试仍走二进制内嵌副本，测试断言的就是真正发出去的内容；
  - 静态资源 URL 带上磁盘最新 mtime 当版本号（`?v=2.5.0-<nanos>`），不会再出现“文件改了但浏览器还在跑旧代码”；
  - 新增长轮询 `GET /api/dev/wait?since=<stamp>`，“关于”分区会显示当前是“开发构建 · 已开启”还是“发布构建”。

### 🐛 Fixes

* **快速连点页面切换的状态错乱**：`startViewTransition` 的绘制回调会在之后的帧才跑，两次快速切换时旧回调会把旧页面画回来。现在每次切换带递增序号，只有最新一次允许绘制。
* **窗口不绘制时切换挂起**：窗口被遮挡或在后台标签页时 view transition 一直不 ready，页面卡住不动，表现为“齿轮点了没反应”。现在隐藏窗口直接切，另有 300ms 兜底，`transition.ready` 不来就手动交页。
* **手动刷新的忙状态**：会话工具条的“刷新”按钮补回 `id="refreshBtn"`，重新探测时按钮会转。
* **动效设置读不到**：`AppLayout.storage` 与 `app_layout_json` 补齐 `motion: agenthub_motion`，`calmMotion()` 不再对 `undefined` 键做判断。

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.5.0-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.5.0-android-debug.apk` (Kotlin Compose mobile client, versionCode 100)
* **Integrity**: `AgentHub-v2.5.0-SHA256.txt`
