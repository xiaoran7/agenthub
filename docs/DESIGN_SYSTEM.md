# Agent-Platform 跨端设计系统

## 1. 目标

Web、桌面宽屏与原生 Android 使用同一套产品语言：安静的纸张背景、高对比黑白信息
层级、细描边大圆角卡片，以及只用于提醒和品牌识别的粉色信号点。参考图只决定视觉
方向，不改变 Agent-Platform 的设备、Session、等待状态和隐私边界。

## 2. 核心令牌

| 语义 | Light | Dark | Web | Android |
| --- | --- | --- | --- | --- |
| Paper | `#FBFAF7` | `#11110F` | `--bg` | `Paper` |
| Surface | `#FFFFFF` | `#1B1B18` | `--surface` | color scheme surface |
| Ink | `#111111` | `#F5F2EC` | `--text` / `--accent` | `Ink` |
| Muted | `#777570` | `#B9B5AD` | `--muted` | `Muted` |
| Line | `#E2DFD9` | `#3A3933` | `--border` | `Line` |
| Pink Signal | `#EF7FA7` | `#EF7FA7` | `--signal` | `PinkSignal` |

状态语义色仍独立：工作中为蓝、等待为琥珀、完成为绿、失败为红。Pink Signal 只用于
品牌标记、数量提醒和视觉锚点，不能代替状态文字。

## 3. 组件规则

- 页面标题使用 34–44px/sp 超粗字重和紧凑字距；正文保持 14–16px/sp。
- 主卡圆角 24–32px/dp，控件圆角 16–20px/dp，筛选与导航使用完整胶囊。
- 卡片以 1px/dp `Line` 描边为主，阴影只用于悬浮导航和层级切换。
- 移动底栏使用四个 Agent-Platform 业务入口：全部、待处理、设备、设置。
- Session 分类始终是全部、进行中、待处理、最近完成、历史；各端不得改名为互斥栏目。
- 点击目标至少 44px/dp；不能只靠颜色表达等待、在线或选中。

## 4. 响应式对应

- 桌面 Web：全局概览、筛选、列表和 sticky 详情并列。
- 手机 Web/PWA：相同读模型，固定底栏、筛选抽屉和全屏详情。
- 原生 Android：相同读 API 与分类，Compose 页面栈、系统返回和 Keystore 配置。

跨端一致指视觉令牌、信息层级和业务语义一致，不要求三种布局逐像素复制。
