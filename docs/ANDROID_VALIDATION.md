# 原生 Android 客户端验收记录

`apps/android` 自 v2.6.0 起重写为对齐 `@agent-pulse/contracts/remote` 的 Kotlin + Jetpack Compose 原生 Remote v3 客户端（Keystore AES-256-GCM 凭据存储、增量转录折叠、三主题切换）。

## 关键验收基线

- **v2.6.0 ~ v2.6.21 Remote v3 原生重写与硬化（2026-11）**：
  - 跑通 `testDebugUnitTest`（189 项单测全绿）、`lintDebug`（0 error）与 `assembleDebug`；
  - 覆盖 Cookie 会话认证、`EventWindow` 快照锚点裁剪、`CommandLedger` 幂等键、四级会话树、SAF 附件 UTF-8 字符边界嗅探、绝对行数阈值乐观气泡、SSE 首行验活与 `SessionSignals.kt` 独立归约。
- **v2.3.2 & v2.0 早期模拟器实测（2026-07 ~ 2026-09）**：
  - `Medium_Phone`（API 36，1080x2400）通过 `adb reverse` 完成真实 Runner + Codex 多轮续聊、Diff 审批、`interrupted` 急停、Host 换代 `resume` 及 Animator `0x`/`10x` 无残影校验。
- **待独立验收项**：多品牌真机 ROM 后台保活差异与正式应用商店签名包（模拟器须用 `x86_64`/universal ABI，真机用 `arm64-v8a`）。
