# AgentHub 设计系统 (DESIGN_SYSTEM)

## 1. 双表面架构
1. **远程跨端表面（`apps/web` & `apps/android`）**：面向多设备 Workspace-First 远程接管（桌面三栏并列、移动端固定底栏 + 全屏详情），顶部侧栏提供跨设备 `"要你处理"` 待审批置顶队列。
2. **本机内嵌表面（`crates/runner/resources/` & `tray_menu/`）**：`:4200` 控制台沿用上游 [magpie](https://github.com/yetone/magpie) 布局原语（46px 粘性顶栏、固定分组侧栏、五分区 `#view-settings` 与 20 枚厂商图标）；Windows 托盘采用纯 Win32 GDI 分层圆角卡片（支持 DPI 缩放与全键盘焦点环）。

## 2. 核心色彩与对比度令牌（Qoder Forest / Mint 视觉体系）
| 语义 | Light（青岚 Mint Light） | Dark（森墨 Forest Dark） | Web CSS 变量 | Android 令牌 |
| :--- | :--- | :--- | :--- | :--- |
| 背景画布 | `#F7FAF8` | `#0F1110` | `--remote-bg` | `MINT_LIGHT` / `FOREST_DARK`（保留 `PAPER` / `GRAPHITE` / `MIDNIGHT`） |
| 容器表面 | `#FFFFFF` | `#171A18` | `--remote-surface` | `bgPanel` / `bgRaised` |
| 主文本 | `#16221C` | `#F4F5F3` | `--remote-ink` | `textPrimary` |
| 次要文本 | `#5C6E64` | `#858E88` | `--remote-muted` | `textMuted` |
| 边框分隔 | `#DCE7E2` | `#272E2A` | `--remote-line` | `borderSubtle` |
| 森绿 / 薄荷强调文字 | `#1F6A57`（6.2:1） | `#8EE5A1`（13.2:1） | `--cds-clay-ink` | `accentBlue` / `surfaceBright`（WCAG AA `>=4.5:1`） |

- **交互与无障碍**：状态必须同时具备颜色与文字标签，禁止原生装饰性 Emoji；移动端触控热区不小于 `44px/dp`。

## 3. 60 FPS 渲染红线
- 严禁在 `body` 使用 `background-attachment: fixed` 或在滚动列表堆叠 `backdrop-filter: blur(...)`；
- 动效仅驱动 `transform` / `opacity`，严格服从 `prefers-reduced-motion: reduce` 与 Android Animator scale；
- 流式增量使用内联状态条与尾部增量折叠（`foldWithCache` / `IncrementalTranscriptFold`），避免整表重排与巨型闪烁骨架。


### 移动端 Qoder 导航与拇指区人机工程（Web Mobile & Android）

1. **右下角悬浮胶囊主操作（Thumb-Zone Primary CTA）**：
   - Android 首页（`SessionsScreen`）与 Web 移动端空态页将"+ 新任务"下沉至右下角 `48–50dp/px` 翡翠胶囊 FAB，释放顶栏视觉干扰，单手大拇指一步可达；自动继承当前筛选的 Agent 引擎。
2. **对话优先的新建页与可视化 Agent 卡片**：
   - Android（`LaunchScreen.kt` + `LaunchPickerComponents.kt`）与 Web（`RemoteLaunchModal.tsx`）不再用深层表单堆砌配置，而是居中呈现 2 列带单色原生图标的"选择执行 Agent"网格（支持原生引擎与已启用 ACP 插件），配合快捷开场指令与默认折叠的高级参数（思考强度 / Codex 沙箱 / 原生会话 ID）。
   - Android 新建页底部固定 24dp 悬浮输入胶囊与 4 枚单手弹出 `ModalBottomSheet` 芯片（**电脑 · 目录 · 模型 · 权限**），无需退出页面即可跨设备切换在线电脑与工作区。
3. **运行中紧急停止下沉底栏**：
   - Android `RemoteBottomConsoleDock` 与 Web `remote-composer-footer` 在 Agent 执行中直接于底部拇指区提供醒目"停止"胶囊，避免单手够顶栏误触。
4. **底栏柔和胶囊高亮**：
   - Android `BottomBar` 激活 Tab 采用薄荷圆角高亮胶囊（14% alpha），与森墨 / 青岚视觉系统统一。
