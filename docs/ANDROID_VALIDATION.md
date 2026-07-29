# 原生 Android 验收记录

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
