# AgentHub 跨端设计系统

## 1. 目标

2026-09-07：产品交互以 PRD 的原生 Remote 接续为准。首页先按工作区组织，不默认筛掉其他 Provider；会话同时表达运行态与控制可用性。发现历史不显示“已连接”，提交命令不显示“执行完成”。跨端消费同一会话契约，以下颜色和动效仅为现有视觉参考。

Web、桌面宽屏与原生 Android 使用同一套产品语言：安静的纸张背景、高对比黑白信息
层级、细描边大圆角卡片，以及只用于提醒和品牌识别的粉色信号点。设计系统服务于
AgentHub 的 Workspace-First（工作区优先）会话组织、多 Agent 远程控制与状态感知。

## 2. 核心令牌

| 语义 | Light | Dark | Web | Android |
| --- | --- | --- | --- | --- |
| Paper | `#FBFAF7` | `#11110F` | `--bg` | `Paper` |
| Surface | `#FFFFFF` | `#1B1B18` | `--surface` | color scheme surface |
| Ink | `#111111` | `#F5F2EC` | `--text` / `--accent` | `Ink` |
| Muted | `#777570` | `#B9B5AD` | `--muted` | `Muted` |
| Line | `#E2DFD9` | `#3A3933` | `--border` | `Line` |
| Pink Signal | `#EF7FA7` | `#EF7FA7` | `--signal` | `PinkSignal` |

状态语义色仍独立：工作中为绿（脉冲）、等待输入为琥珀、等待审批为紫（高亮）、完成为蓝、失败为红。Pink Signal 只用于
品牌标记、数量提醒和视觉锚点，不能代替状态文字。

## 3. 组件规则

- 页面标题使用 34–44px/sp 超粗字重和紧凑字距；正文保持 14–16px/sp。
- 主卡圆角 24–32px/dp，控件圆角 16–20px/dp，筛选与导航使用完整胶囊。
- 卡片以 1px/dp `Line` 描边为主，阴影只用于悬浮导航和层级切换。
- **工作区优先（Workspace-First）**：以项目工作区卡片为第一视觉容器，工作区内展示所属 Session 列表与状态（Provider 徽标、模型名、思考强度、权限模式）。
- 移动底栏使用四个 AgentHub 业务入口：工作区、待处理、设备、设置。
- 点击目标至少 44px/dp；不能只靠颜色表达等待、在线或选中。


## 4. 响应式对应

- 桌面 Web：全局概览、筛选、列表和 sticky 详情并列。
- 手机 Web/PWA：相同读模型，固定底栏、筛选抽屉和全屏详情。
- 原生 Android：相同读 API 与分类，Compose 页面栈、系统返回和 Keystore 配置。

跨端一致指视觉令牌、信息层级和业务语义一致，不要求三种布局逐像素复制。

## 5. 动效语言

- Web/桌面端使用 GSAP 编排首屏、筛选结果、详情和面板层级；React 生命周期统一通过
  `useGSAP` 和局部 scope 管理，组件卸载或依赖变化时自动回收。
- 原生 Android 使用 Compose `AnimatedContent`、`AnimatedVisibility`、
  `animate*AsState` 与 `InfiniteTransition` 对应同一动效语义，不嵌入 GSAP 或
  WebView；页面方向、列表 stagger、指标和品牌反馈由原生渲染。
- 页面入场遵循“品牌与连接 → 概览 → 控制面 → 内容”的阅读顺序；列表用短距离
  stagger 表达结果重排，详情在桌面从右侧、手机从底部进入。
- 数据数字可以平滑计数，实时连接点与 C 形品牌标记可以持续低速反馈；等待/完成等
  业务状态仍以文字和状态色表达，动画不能创造新状态。
- 高频动效只使用 `x/y/scale/rotation/opacity`，指针视差使用 `quickTo` 复用 tween；
  不用 width、height、top、left 驱动移动，不为未动画元素长期声明 `will-change`。
- `prefers-reduced-motion: reduce` 时停用位移、stagger、视差和循环动画，内容直接呈现
  最终状态；焦点、筛选、详情导航和数据刷新不能依赖动画完成回调。
- Android 动效服从系统 Animator duration scale；缩放为 `0x` 时 Compose 直接收敛到
  最终状态。stagger 延迟封顶 270ms，长列表不能因索引增长而无限延后可见内容。
