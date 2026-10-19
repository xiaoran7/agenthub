# 原生 Android 验收记录

> 原生 `apps/android` 当前处于冻结基线（v2.5.0），移动端主力由 Web PWA 承接；本文件保留历史模拟器实测与自动化验收记录。

## 1. 2026-09-08 v2.3.2 原生 Remote APK 验收

- **环境**：Windows 本机 Gateway（SQLite 内容库）+ `adb reverse` 回环；`Medium_Phone` 模拟器（1080×2400）；Runner 2.3.1 + Codex 0.153.4；APK 2.3.2（versionCode 5，Debug 签名）。
- **续聊与审批**：创建只读目录会话并完成两轮同 `provider_session_id` 续聊；触发真实 Shell 审批（`awaiting_permission` -> 允许 -> `running`），点击停止任务后原生 `turn/completed` 返回 `interrupted`。
- **重启恢复**：停止 Host 后历史可读且输入禁用；重启 Host 并点击恢复会话后 `host_generation` 递增、`provider_session_id` 保持不变，上下文核对一致。
- **自动化**：`testDebugUnitTest`、`lintDebug`、`assembleDebug` 全部通过。

## 2. 2026-07-29 首版基线与 Compose 动效验收

- **自动化门禁**：`.\gradlew.bat testDebugUnitTest lintDebug assembleDebug assembleRelease` 全部通过；动效测试锁定 45ms stagger 与 270ms 上限。
- **模拟器实测（`Medium_Phone_API_36.1` 1080×2400）**：
  - 临时 Runner 身份完成 Ed25519 签名批次上传（Gateway ACK 2、Runner pending 0）；
  - 验证首页指标卡、相邻会话切换、系统 Back 导航、Keystore 加密配置重启恢复及 Animator `10x`/`0x` 无残影切换。

## 3. 未覆盖边界

- 实体 Android 手机与厂商 ROM 矩阵、正式商店签名发布包、原生系统级后台推送与公网 HTTPS 现场连接。
