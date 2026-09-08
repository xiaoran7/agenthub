# 原生 Android 验收记录

## 2026-09-08 v2.3.2 原生 Remote APK

- 环境：Windows 本机 Gateway（账户/设备内存 Store，内容 SQLite），ADB reverse 回环连接；Medium_Phone 模拟器 1080×2400，未连接实体手机。Host 使用 release Runner 2.3.1 与原生 Codex 0.153.4；最终 APK 为 2.3.2 / versionCode 5，Debug 签名。
- 安装：2.3.0 初包安装启动，修复设置页只读文案后覆盖安装最终 2.3.2；系统 dumpsys 核实版本。重启后 Keystore 连接配置可用，原生会话与历史仍可访问。
- 实际界面创建只读工作目录会话并发送两轮文本；第二轮仅要求重复，Codex 返回上一轮相同测试标记。两轮对应同一个 provider_session_id。
- 最终 APK 展示真实 shell 审批（30 秒等待并打印测试标记，不读写文件）；点击允许后状态从 awaiting_permission 变为 running，再点击停止任务，原生 turn/completed 为 interrupted。此证据确认原生回合中断，不声称所有子进程立即终止。
- 停止 Host 后，历史仍可查看且输入禁用；同数据库重启 Host 后必须点击恢复原生会话。恢复后 host_generation 改变、provider_session_id 不变，再发送上下文核对请求仍返回相同标记。
- 自动化：testDebugUnitTest、lintDebug、assembleDebug 均通过。ADB 输入需等待软键盘动画并检查实际文本，避免坐标点击落在尚未收起的键盘上。
- 未覆盖：实体手机、正式签名/商店发布、公网 HTTPS、长时稳定性、桌面与手机同时控制的完整安装版流程。

## 2026-07-29 Compose 动效轮次

### 自动化

- `.\gradlew.bat testDebugUnitTest lintDebug assembleDebug`：4 个 JVM 测试通过，
  Android Lint 与 Debug APK 构建通过；新增测试锁定 45ms stagger 与 270ms 上限。
- 动效只新增 `androidx.compose.animation:animation` 官方 Compose artifact，不改变
  Gateway、Keystore、网络安全或只读请求代码。

### 模拟器运行

- 环境：Android Emulator `Medium_Phone_API_36.1`，1080×2400；调试 APK 覆盖安装。
- 数据：`adb reverse tcp:3001 tcp:3001` 连接临时只读 Gateway，首页实际显示 2 台
  在线设备、4 条 Session、2 条待处理与 1 条最近完成。
- 动效：系统 Animator duration scale 设为 `10x` 时，首页→设备与首页→详情的
  fade/slide 中间帧可见；设为 `0x` 后 300ms 截图直接是完整首页、没有双页残影。
- 导航：详情从 Checkout Web 相邻切换到 Memory Index 成功；Android 系统 Back
  返回带 CURRENT STATUS 的会话首页。
- 视觉：最终首页与设备页在 Paper / Ink / Pink Signal 体系下无裁切，固定底栏、
  Hero、四张指标卡和筛选行保持原布局。

## 2026-07-29 首版基线

### 自动化

- `.\gradlew.bat testDebugUnitTest`：3 个分类、排序和 HTTPS/回环策略测试通过。
- `.\gradlew.bat lintDebug`：Android Lint 通过。
- `.\gradlew.bat assembleDebug`：Android 36 / minSdk 26 调试 APK 构建通过。
- `.\gradlew.bat assembleRelease`：R8 压缩、资源收缩与 lintVital 通过；产物尚未配置
  正式发布签名，不能作为商店交付包。
- APK：`apps/android/app/build/outputs/apk/debug/app-debug.apk`。

### 真实运行

- 环境：Android Emulator `Medium_Phone_API_36.1`，1080×2400。
- 安装：`adb install -r app-debug.apk` 成功，Launcher Activity 正常启动。
- 链路：`adb reverse tcp:3001 tcp:3001` 连接回环 Gateway。
- 数据：临时 Runner 身份完成绑定，Claude 等待授权与 Codex 完成两个 Fixture 经
  Ed25519 签名批次上传；Gateway ACK 2、Runner pending 0。
- UI：首页读取 1 台在线设备、2 条活动会话、1 条待处理与 1 条最近完成；等待会话
  进入详情后显示时间线。
- 导航：详情 `1 / 2` 切换至 `2 / 2` 成功；Android 系统 Back 返回会话首页。
- 配置：保存后强制停止并重启 App，Keystore 加密配置仍可解密并直接恢复首页。

### 尚未验证

- 实体 Android 手机及厂商 ROM 矩阵。
- 原生系统通知、后台刷新和离线快照；首版只做前台 5 秒轮询。
- Release keystore、代码签名、Play 发布包和商店审核。
- 生产 HTTPS Gateway 的公网/私有 CA 现场连接。
