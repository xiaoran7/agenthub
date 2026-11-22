# AgentHub 设计系统 (DESIGN_SYSTEM)

## 1. 双表面架构
1. **远程跨端表面（`apps/web` & `apps/android`）**：面向多设备 Workspace-First 远程接管（桌面三栏并列、移动端固定底栏 + 全屏详情），顶部侧栏提供跨设备 `"要你处理"` 待审批置顶队列。
2. **本机内嵌表面（`crates/runner/resources/` & `tray_menu/`）**：`:4200` 控制台沿用上游 [magpie](https://github.com/yetone/magpie) 布局原语（46px 粘性顶栏、固定分组侧栏、五分区 `#view-settings` 与 20 枚厂商图标）；Windows 托盘采用纯 Win32 GDI 分层圆角卡片（支持 DPI 缩放与全键盘焦点环）。

## 2. 核心色彩与对比度令牌
| 语义 | Light (Paper) | Dark | Web CSS 变量 | Android 令牌 |
| :--- | :--- | :--- | :--- | :--- |
| 背景画布 | `#FBFAF7` | `#11110F` | `--bg` | `Paper` / `Graphite` / `Midnight` |
| 容器表面 | `#FFFFFF` | `#1B1B18` | `--surface` | `surface` |
| 主文本 | `#111111` | `#F5F2EC` | `--text` | `Ink` |
| 次要文本 | `#777570` | `#B9B5AD` | `--muted` | `Muted` |
| 边框分隔 | `#E2DFD9` | `#3A3933` | `--border` | `Line` |
| 强调文字陶土色 | `#A9481F` | `#E08264` | `--cds-clay-ink` | 独立高对比强调色（WCAG AA `>=4.5:1`） |

- **交互与无障碍**：状态必须同时具备颜色与文字标签，禁止原生装饰性 Emoji；移动端触控热区不小于 `44px/dp`。

## 3. 60 FPS 渲染红线
- 严禁在 `body` 使用 `background-attachment: fixed` 或在滚动列表堆叠 `backdrop-filter: blur(...)`；
- 动效仅驱动 `transform` / `opacity`，严格服从 `prefers-reduced-motion: reduce` 与 Android Animator scale；
- 流式增量使用内联状态条与尾部增量折叠（`foldWithCache` / `IncrementalTranscriptFold`），避免整表重排与巨型闪烁骨架。
