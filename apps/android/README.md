# Agent-Platform Android

独立 Kotlin + Jetpack Compose 只读客户端。它不是 WebView，也不包含 Runner 或任何
Claude/OpenAI 登录凭据。

## 构建

```powershell
.\gradlew.bat testDebugUnitTest lintDebug assembleDebug
```

调试 APK：`app/build/outputs/apk/debug/app-debug.apk`。

## 本地 Gateway

Gateway 保持监听 `127.0.0.1:3001`，Android 设备通过 ADB 反向端口访问：

```powershell
adb reverse tcp:3001 tcp:3001
```

App 中填写：

- Gateway：`http://127.0.0.1:3001`
- 本机开发用户：与 Gateway 开发认证 owner 一致
- 访问令牌：开发认证可留空；生产 HTTPS Gateway 必填

## 安全边界

- 只调用设备、Session、时间线三个 GET 接口。
- 远程 Gateway 只允许 HTTPS，明文 HTTP 仅允许回环主机。
- Bearer 令牌用 Android Keystore AES-256-GCM 加密，配置禁止系统备份。
- 不读取或保存 Agent Token、transcript、Prompt、代码、Diff、命令和完整路径。
- 首版前台每 5 秒刷新；原生推送、后台任务、发布签名尚未接入。
