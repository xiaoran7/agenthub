## What's Changed in v2.4.97

### 🚀 Highlights & New Features

* **📊 Multi-Agent Token, Cache & Request Alignment with Magpie**:
  - **Full Machine Throughput Alignment**: Resolved token undercounting where AgentHub reported ~13.21B gross tokens while Magpie reported ~24.09B. Full audit identified and unified Claude Code (13.77B), ZCode Agent (12.26B), and Codex (2.15B), bringing total machine throughput to **28.18B Gross Tokens**, **24.17B Cache Read Tokens**, and **259.9k Requests/Calls**.
  - **ZCode SQLite LIMIT 1 Defect Fixed**: Replaced the single-message subquery with a high-performance aggregation across all assistant messages, recovering 12.26B tokens and 98,334 calls.
  - **ZCode Parent-Child Subagent Rollup**: Ingested all 482 `sess_subagent_...` probe and subagent sessions via `parent_map` recursive folding into top-level root sessions, preventing 1.4B tokens from being lost while keeping the session list clean.
  - **SQL Query Optimization**: Optimized the 98,000+ row message aggregation using `WHERE data LIKE '%"role":"assistant"%'`, slashing execution time from 5.13s down to 0.39s (13x speedup) with a 5-second read-write memory cache.
  - **Codex Cache & Request Enrichment**: Integrated `~/.cache/magpie/sessions.json` and `thread_history_1.sqlite` to enrich 193 Codex local sessions with 2.07B cache read tokens and 8,248 exact agentMessage calls.
* **✨ Dashboard Skeleton Shimmer & Navigation Polish**:
  - **Skeleton Shimmer Loading**: Added 5-row shimmering skeleton placeholder during session and token loading, completely eliminating jarring "empty list" flash on initial load.
  - **Enlarged Navigation Controls**: Redesigned top navigation pills (32px height, 13.5px font, 16px padding) with pure white elevated pill active states (`#ffffff` background with `#1c1c21` dark text).
  - **Sticky Top Bar**: Anchored the header bar with `position: sticky; top: 0; z-index: 20` ensuring sidebar toggle (`#sideToggle`) remains immediately accessible during vertical scroll.
* **📱 Android Mobile Visual Alignment & Spacing Grid (Orca Philosophy)**:
  - Unified design tokens (`OrcaSpace`, `OrcaRadius`, `OrcaSize`, `OrcaType`) with single-theme graphite dark mode matching upstream Orca.
  - Consolidated list grouping (`OrcaRow`, `OrcaListGroup`, `OrcaStatStrip`) replacing fragmented card stacks.
  - Single settings entry point with dedicated "Connect Host" setup flow.
  - 60 FPS lifecycle-aware polling and immutable snapshot parsing.

### 📦 Precompiled Deliverables

* **Windows x64**: `AgentHub-Runner-v2.4.97-windows-x64.zip` (single executable + quickstart connect guide)
* **Android**: `AgentHub-v2.4.97-android-debug.apk` (Kotlin Compose mobile client)
* **Integrity**: `AgentHub-v2.4.97-SHA256.txt`

### 🔗 Attribution
* Upstream layout primitives & tokens from [magpie](https://github.com/yetone/magpie) (MIT).
* Mobile companion UI & accessory row philosophy from [stablyai/orca](https://github.com/stablyai/orca) (Apache 2.0 / MIT).
