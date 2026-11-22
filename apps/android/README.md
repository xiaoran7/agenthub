# AgentHub Android (`apps/android`)

> **状态**：v2.6.0 起重写为 Remote v3 原生客户端并解除冻结。数据层按 `packages/contracts/src/remote.ts` 强类型建模，逻辑层（事件窗口、幂等命令、四级会话树）为纯 Kotlin 且有单测覆盖，UI 为 Jetpack Compose（固定底栏 + 全屏详情）。移动端与响应式 Web 控制台共用同一套 Remote v3 网关契约。

独立 Kotlin + Jetpack Compose 客户端，通过 Gateway v3 Remote API 消费会话、流式转录、Diff 审批、多选问答与中断，使用 Android Keystore AES-256-GCM 加密保存连接凭据（远程强制 HTTPS，回环允许 HTTP）。

## 构建与本地联调

```powershell
# 运行单测、Lint 与构建 Debug APK（构建后请执行 .\gradlew.bat --stop 释放内存）
.\gradlew.bat testDebugUnitTest lintDebug assembleDebug

# 通过 ADB 反向端口连接本机 Gateway (http://127.0.0.1:3001)
adb reverse tcp:3001 tcp:3001
```
