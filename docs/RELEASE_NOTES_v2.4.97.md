## What's Changed in v2.4.97

### Highlights & New Features

- **Multi-Agent Token, Cache & Request Alignment**: Fixed ZCode SQLite `LIMIT 1` subquery defect, rolled up `sess_subagent_...` via `parent_map`, optimized assistant message SQL aggregation (`5.13s -> 0.39s` with 5s memory cache), and enriched Codex cache/call metrics from `~/.cache/magpie/sessions.json` and `thread_history_1.sqlite`.
- **Dashboard Shimmer & Sticky Nav**: 5-row skeleton shimmer placeholders, `32px` elevated white nav pills, and sticky `.top` header keeping `#sideToggle` reachable on scroll.
- **Android Orca Visual Alignment**: Unified `OrcaSpace`/`OrcaRadius`/`OrcaSize`/`OrcaType` graphite dark tokens, `OrcaRow`/`OrcaListGroup`/`OrcaStatStrip` grouping, and 60 FPS lifecycle-aware polling.

### Precompiled Deliverables

- `AgentHub-Runner-v2.4.97-windows-x64.zip` · `AgentHub-v2.4.97-android-debug.apk` · `AgentHub-v2.4.97-SHA256.txt`
