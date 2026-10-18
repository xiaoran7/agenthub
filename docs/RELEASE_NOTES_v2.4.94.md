## What's Changed in v2.4.94

### 🚀 Highlights & New Features

* **📱 Mobile Companion Cockpit & Virtual Terminal Accessory Row**:
  * Inspired by the mission-control philosophy of [stablyai/orca](https://github.com/stablyai/orca), `apps/android` now features a dedicated multi-device cockpit interface.
  * Virtual accessory keyboard bar (Esc, Tab, Ctrl-C, arrows) optimized for tactile smartphone operation—approving permissions and inspecting live terminal output on the go is now effortless.
* **⚡ Embedded Runner Web Console Skeleton Shimmer & Navigation Insets**:
  * Added animated shimmer skeleton placeholders across session trees, model stats, and agent metrics, completely eliminating cold-start blank screens.
  * Elevated sticky top navigation bar (46px) with responsive label folding and high-contrast active cards for desktop and tablet displays.
  * Accurate multi-turn request count tracking (`request_count` and `calls`) aligned with upstream official agent accounting.
* **🛡️ 100% Green CI & GitHub Community Standards**:
  * Automated GitHub Actions pipeline validating Rust 2024 (`cargo clippy`, `cargo fmt`, `cargo test`) and Node.js 22 (`npm test`, `typecheck`, `format:check`).
  * 100% GitHub Community Standards health score with standard Code of Conduct, Contributing Guidelines, Security Policy, and Issue/PR templates.
* **🌐 Generative Engine Optimization (GEO)**:
  * Full `llms.txt` and `llms-full.txt` standards for accurate indexing by Perplexity, ChatGPT Search, Claude Web, and Cursor without AI hallucinations.

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.94-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.4.94-android-debug.apk` (versionCode 94, versionName 2.4.94)
* **Integrity**: `AgentHub-v2.4.94-SHA256.txt`

### 🔗 Attribution
* Upstream layout primitives & tokens from [magpie](https://github.com/yetone/magpie) (MIT).
* Mobile companion UI & accessory row philosophy from [stablyai/orca](https://github.com/stablyai/orca) (Apache 2.0 / MIT).

---
**Full Changelog**: https://github.com/xiaoran7/agenthub/compare/b60364a...a455951
