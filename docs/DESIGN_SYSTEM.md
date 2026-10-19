# AgentHub 设计系统 (DESIGN_SYSTEM)

## 1. 双表面架构

AgentHub 包含两个视觉表面，共享相同的性能与无障碍红线，但服务不同场景：

1. **远程跨端表面（`apps/web` & `apps/android`）**：服务 Workspace-First 的多设备远程控制（PC 浏览器、手机 PWA 与原生 Android）。
2. **本机 Runner 内嵌控制台（`crates/runner/resources/`）**：服务单窗口本机控制面（`http://127.0.0.1:4200`），直接采用上游 [magpie](https://github.com/yetone/magpie) 的设计系统与布局原语（46px 顶栏、分组固定侧栏、`.list`/`.row`/`.stats`/`.kpi`、独立 `#view-settings` 五分区设置页与 20 个原版厂商图标）。

## 2. 远程跨端核心令牌

| 语义        | Light     | Dark      | Web CSS 变量          | Android      |
| :---------- | :-------- | :-------- | :-------------------- | :----------- |
| Paper       | `#FBFAF7` | `#11110F` | `--bg`                | `Paper`      |
| Surface     | `#FFFFFF` | `#1B1B18` | `--surface`           | surface      |
| Ink         | `#111111` | `#F5F2EC` | `--text` / `--accent` | `Ink`        |
| Muted       | `#777570` | `#B9B5AD` | `--muted`             | `Muted`      |
| Line        | `#E2DFD9` | `#3A3933` | `--border`            | `Line`       |
| Pink Signal | `#EF7FA7` | `#EF7FA7` | `--signal`            | `PinkSignal` |

- **状态语义色**：工作中为绿（脉冲）、等待输入为琥珀、等待审批为紫、完成为蓝、失败为红。不能仅靠颜色表达状态，必须配状态文字。
- **组件与触控**：以项目工作区（Workspace）为第一容器；触控目标最小 `44px/dp`；桌面端采用概览 + 列表 + sticky 详情并列，手机端采用固定底栏 + 全屏详情。

## 3. 动效与 60 FPS 性能铁律

- **禁止全屏重绘陷阱**：严禁在 `body` 使用 `background-attachment: fixed`，严禁在滚动列表中大面积堆叠 `backdrop-filter: blur(...)`（模糊仅限模态遮罩层）。
- **GPU 合成加速**：高频动画仅驱动 `transform` / `opacity`（配合 `will-change: transform; transform: translateZ(0)`），不用 `width/height/top/left` 驱动位移；轮询刷新间隔不低于 500ms。
- **无障碍降级**：严格服从 `prefers-reduced-motion: reduce` 与 Android 系统 Animator duration scale（`0x` 时直接呈现终态，列表 stagger 封顶 270ms）；业务状态与焦点流转不得依赖动画回调。
