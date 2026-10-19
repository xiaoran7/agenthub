## What's Changed in v2.5.0

### Highlights & New Features

- **Settings as a Dedicated View (`#view-settings`)**: Aligned with upstream magpie—clicking the top-bar gear (`Ctrl+,` / `Esc`) swaps `.view-slot` into a 5-section settings view (General, Appearance, Device & Pairing, Gateway, About) with `.list.prefs` + `.row.pref` rows and cross-section keyword search.
- **Consolidated Top Bar**: Moved theme, motion (`agenthub_motion`), poll interval (`2/4/10s`), sidebar toggle, manual rescan, device pairing, and daemon exit into Settings rows.
- **Debug Build Hot Reload**: Debug builds serve `resources/` directly from disk with mtime cache-busting (`?v=2.5.0-<nanos>`) and long-polling `GET /api/dev/wait?since=<stamp>`; release builds remain single-binary embedded.
- **View Transition & Refresh Fixes**: Monotonic transition sequence IDs prevent rapid-click stale renders; 300ms fallback prevents hidden-window hangs; restored `#refreshBtn` spinner state.

### Precompiled Deliverables

- `AgentHub-Runner-v2.5.0-windows-x64.zip` · `AgentHub-v2.5.0-android-debug.apk` (versionCode 100) · `AgentHub-v2.5.0-SHA256.txt`
