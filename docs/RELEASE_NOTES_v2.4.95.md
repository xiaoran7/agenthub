## What's Changed in v2.4.95

### Highlights & New Features

- **Web Console Skeleton Shimmer & Empty-State Protection**: Shimmer loading cards plus a 3-poll / 15s grace threshold and 12s summary cache TTL in `web_server.rs` to eliminate cold-start empty flashes.
- **Navigation & Sticky Header**: Enlarged `32px` segmented nav pills (`#ffffff` active card), sticky `.top` header (`z-index: 20`), and dynamic `#navTopSlot`/`#navSideSlot` docking.
- **Magpie Token Accounting & Sub-Second Local Startup**: Full EOF line scanning counting `cache_read_input_tokens` alongside input/output tokens; 3s local mode startup timeout and default CWD fallback (`agenthub-runner web --port 4200` launches in <1s).
- **Android Cockpit Decoupling**: Extracted terminal accessory and companion status widgets into `OrcaComponents.kt`.

### Precompiled Deliverables

- `AgentHub-Runner-v2.4.95-windows-x64.zip` · `AgentHub-v2.4.95-android-debug.apk` (versionCode 95) · `AgentHub-v2.4.95-SHA256.txt`
