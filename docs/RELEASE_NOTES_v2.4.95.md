## What's Changed in v2.4.95

### 🚀 Highlights & New Features

* **⚡ Web Console Skeleton Shimmer & State Protection**:
  * Added animated skeleton shimmer loading cards to session trees and token usage views, completely eliminating cold-start flash and premature "no sessions" empty states.
  * Enhanced frontend settling logic: requires both 3 consecutive empty polls AND 15s elapsed grace time before declaring an empty state.
  * Increased summary cache TTL to 12s in `web_server.rs` to shield disk I/O, with stale non-empty session preservation during transient discovery timeouts.
* **💎 Segmented Nav Pill Styling & Enlarged Touch Targets**:
  * Enlarged top navigation buttons to 32px height, 13.5px font size, and 16px horizontal padding.
  * Pure transparent container background for inactive tabs; active tabs styled as elevated pure white cards (`#ffffff` background, `#1c1c21` dark text, and subtle elevation drop shadow).
* **📌 Sticky Top Bar & Fixed Sidebar Toggle**:
  * Sticky `.top` header (`position: sticky; top: 0; z-index: 20`) ensuring `#sideToggle` remains permanently visible and reachable on scroll.
  * Dynamic nav slot docking (`#navTopSlot` vs `#navSideSlot`), fixing sidebar toggle responsiveness.
* **📊 Token Volume & Multi-Turn Metrics Aligned with Magpie**:
  * Switched Claude Code transcript collection to full EOF line scanning without 512-line truncation.
  * Aligned token accounting: counting `cache_read_input_tokens` alongside raw input/output tokens (total volume 23.32B tokens, 132,775 requests), matching Magpie's metrics (23.53B tokens, ~185k requests).
* **⚡ Zero-Wait Local Web Console Startup**:
  * Reduced interactive app server startup timeouts to 3s when running in local web mode (`url.is_none()`).
  * Defaulted empty workspaces to current working directory, enabling `agenthub-runner web --port 4200` to launch in under 1 second.
* **📱 Android Cockpit Architecture Decoupling**:
  * Extracted tactile terminal accessory components and companion status widgets into `OrcaComponents.kt`.

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.95-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.4.95-android-debug.apk` (versionCode 95, versionName 2.4.95)
* **Integrity**: `AgentHub-v2.4.95-SHA256.txt`

### 🔗 Attribution
* Upstream layout primitives & tokens from [magpie](https://github.com/yetone/magpie) (MIT).
* Mobile companion UI & accessory row philosophy from [stablyai/orca](https://github.com/stablyai/orca) (Apache 2.0 / MIT).
