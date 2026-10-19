# AgentHub Android (`apps/android`)

> **状态**：原生 Android 客户端当前**暂时冻结**（保留 v2.5.0 基线资产），移动端控制由响应式 Web PWA 主力承接。

独立 Kotlin + Jetpack Compose 客户端，通过 Gateway v3 Remote API 消费会话、流式转录、Diff 审批、多选问答与中断，使用 Android Keystore AES-256-GCM 加密保存连接凭据（远程强制 HTTPS，回环允许 HTTP）。

## 构建与本地联调

```powershell
# 运行单测、Lint 与构建 Debug APK（构建后请执行 .\gradlew.bat --stop 释放内存）
.\gradlew.bat testDebugUnitTest lintDebug assembleDebug

# 通过 ADB 反向端口连接本机 Gateway (http://127.0.0.1:3001)
adb reverse tcp:3001 tcp:3001
```
