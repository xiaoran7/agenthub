## What's Changed in v2.5.1

### Highlights & New Features

- **Four Main Views + Unified Settings**: Main navigation streamlined to **Sessions / Agents / Usage / Workspaces**; Models and Gateway topology views removed (delegated to upstream CPA); Capabilities and Device & Gateway merged into Settings sections.
- **Magpie-Aligned Usage Page & Ledger (`GET /api/usage/ledger`)**: Overview / Requests / Sessions / Context tabs with 1d / 7d / 30d / All range filters, 5 KPI cards, stacked trend chart, 53-week heatmap, and CSV export; per-request deduplication by `message.id` + `requestId`, 10-minute retry-success pairing, and tokens/s calculation.
- **Upstream Magpie Vendor Icons**: Embedded 19 SVG + 1 PNG vendor marks from `magpie/internal/gui/assets/icons/` under `/icons/<name>.<ext>` with dark-mode monochrome inversion.
- **Five Official Agent Products**: Strictly scoped Agent cards to Coding Assistant (`claude_code`), Claude Desktop (`claude_desktop`), Codex, Grok, and Gemini CLI.

### Precompiled Deliverables

- `AgentHub-Runner-v2.5.1-windows-x64.zip` · `AgentHub-v2.5.0-android-debug.apk` (frozen baseline) · `AgentHub-v2.5.1-SHA256.txt`
