use super::*;

/// 嵌入式自包含静态图标资源（包含全尺寸 ICO 与高清晰度多分辨率 PNG 簇）
pub(crate) const FAVICON_ICO: &[u8] = include_bytes!("../../resources/icon.ico");
pub(crate) const ICON_SVG: &str = include_str!("../../resources/icon.svg");
pub(crate) const ICON_16_PNG: &[u8] = include_bytes!("../../resources/icon-16.png");
pub(crate) const ICON_20_PNG: &[u8] = include_bytes!("../../resources/icon-20.png");
pub(crate) const ICON_24_PNG: &[u8] = include_bytes!("../../resources/icon-24.png");
pub(crate) const ICON_28_PNG: &[u8] = include_bytes!("../../resources/icon-28.png");
pub(crate) const ICON_30_PNG: &[u8] = include_bytes!("../../resources/icon-30.png");
pub(crate) const ICON_32_PNG: &[u8] = include_bytes!("../../resources/icon-32.png");
pub(crate) const ICON_36_PNG: &[u8] = include_bytes!("../../resources/icon-36.png");
pub(crate) const ICON_40_PNG: &[u8] = include_bytes!("../../resources/icon-40.png");
pub(crate) const ICON_42_PNG: &[u8] = include_bytes!("../../resources/icon-42.png");
pub(crate) const ICON_48_PNG: &[u8] = include_bytes!("../../resources/icon-48.png");
pub(crate) const ICON_56_PNG: &[u8] = include_bytes!("../../resources/icon-56.png");
pub(crate) const ICON_64_PNG: &[u8] = include_bytes!("../../resources/icon-64.png");
pub(crate) const ICON_72_PNG: &[u8] = include_bytes!("../../resources/icon-72.png");
pub(crate) const ICON_96_PNG: &[u8] = include_bytes!("../../resources/icon-96.png");
pub(crate) const ICON_128_PNG: &[u8] = include_bytes!("../../resources/icon-128.png");
pub(crate) const ICON_192_PNG: &[u8] = include_bytes!("../../resources/icon-192.png");
pub(crate) const ICON_256_PNG: &[u8] = include_bytes!("../../resources/icon-256.png");
pub(crate) const ICON_512_PNG: &[u8] = include_bytes!("../../resources/icon-512.png");

/// 嵌入式自包含 CDS (Claude Design System) 单页前端
pub(crate) const INDEX_HTML: &str = include_str!("../../resources/dashboard.html");
pub(crate) const DASHBOARD_CSS_PARTS: &[(&str, &str)] = &[
    (
        "dashboard/01-tokens-and-primitives.css",
        include_str!("../../resources/dashboard/01-tokens-and-primitives.css"),
    ),
    (
        "dashboard/02-layout-and-views.css",
        include_str!("../../resources/dashboard/02-layout-and-views.css"),
    ),
    (
        "dashboard/03-settings-and-motion.css",
        include_str!("../../resources/dashboard/03-settings-and-motion.css"),
    ),
    (
        "dashboard/04-usage-and-plugins.css",
        include_str!("../../resources/dashboard/04-usage-and-plugins.css"),
    ),
];
/// the console's behaviour, split into cohesive domain modules so `node --check`,
/// formatters, and line-limit checks can read each part independently
pub(crate) const DASHBOARD_JS_PARTS: &[(&str, &str)] = &[
    (
        "dashboard/01-core-state.js",
        include_str!("../../resources/dashboard/01-core-state.js"),
    ),
    (
        "dashboard/02-nav-and-refresh.js",
        include_str!("../../resources/dashboard/02-nav-and-refresh.js"),
    ),
    (
        "dashboard/03-agents-and-sessions.js",
        include_str!("../../resources/dashboard/03-agents-and-sessions.js"),
    ),
    (
        "dashboard/04-sessions-detail-and-workspaces.js",
        include_str!("../../resources/dashboard/04-sessions-detail-and-workspaces.js"),
    ),
    (
        "dashboard/05-capabilities-and-device.js",
        include_str!("../../resources/dashboard/05-capabilities-and-device.js"),
    ),
    (
        "dashboard/06-usage-core.js",
        include_str!("../../resources/dashboard/06-usage-core.js"),
    ),
    (
        "dashboard/07-usage-charts.js",
        include_str!("../../resources/dashboard/07-usage-charts.js"),
    ),
    (
        "dashboard/08-usage-filters.js",
        include_str!("../../resources/dashboard/08-usage-filters.js"),
    ),
    (
        "dashboard/09-usage-tables-and-export.js",
        include_str!("../../resources/dashboard/09-usage-tables-and-export.js"),
    ),
    (
        "dashboard/10-settings-and-plugins.js",
        include_str!("../../resources/dashboard/10-settings-and-plugins.js"),
    ),
    (
        "dashboard/11-stream-theme-and-device-actions.js",
        include_str!("../../resources/dashboard/11-stream-theme-and-device-actions.js"),
    ),
];

fn join_embedded_parts(parts: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (idx, (_, content)) in parts.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(content);
    }
    out
}

pub(crate) fn dashboard_css_embedded() -> &'static str {
    static BUNDLE: OnceLock<String> = OnceLock::new();
    BUNDLE.get_or_init(|| join_embedded_parts(DASHBOARD_CSS_PARTS))
}

pub(crate) fn dashboard_js_embedded() -> &'static str {
    static BUNDLE: OnceLock<String> = OnceLock::new();
    BUNDLE.get_or_init(|| join_embedded_parts(DASHBOARD_JS_PARTS))
}

/// A debug build (`cargo run`) reads the page, the sheet and the script from
/// `resources/` on every request and watches them, as magpie's `make dev`
/// does: an edit is in the open window at the next poll, with no rebuild and
/// no restart. A release build serves the copies embedded above and carries
/// no watcher at all.
pub(crate) const DEV_RELOAD_JS: &str = r#"(async () => {
  let seen = "";
  for (;;) {
    try {
      const r = await fetch("/api/dev/wait?since=" + seen, { cache: "no-store" });
      const j = await r.json();
      if (seen && j.stamp !== seen) { location.reload(); return; }
      seen = j.stamp;
    } catch (e) { await new Promise((r) => setTimeout(r, 1000)); }
  }
})();"#;

pub(crate) fn dev_assets() -> bool {
    // a debug build reads the console from disk; the tests read the copies
    // embedded in the test binary, so what they assert is what ships
    cfg!(all(debug_assertions, not(test)))
}

/// The console's three files, as this build will serve them: from memory in a
/// release, from disk in a debug build.
pub(crate) fn asset_text(embedded: &'static str, name: &str) -> String {
    if dev_assets() {
        if let Ok(text) = std::fs::read_to_string(assets_dir().join(name)) {
            return text;
        }
    }
    embedded.to_owned()
}

pub(crate) fn asset_bundle_text(parts: &[(&str, &str)], embedded: &'static str) -> String {
    if dev_assets() {
        let base = assets_dir();
        let mut out = String::new();
        for (idx, (rel_path, fallback)) in parts.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            match std::fs::read_to_string(base.join(rel_path)) {
                Ok(text) => out.push_str(&text),
                Err(_) => out.push_str(fallback),
            }
        }
        return out;
    }
    embedded.to_owned()
}

pub(crate) fn assets_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

/// The newest modification time under `resources/`, as one string: a change
/// to any of the console's files changes it, and the page reloads.
fn scan_newest_mtime(dir: &Path, newest: &mut std::time::SystemTime) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    scan_newest_mtime(&entry.path(), newest);
                } else if meta.is_file() {
                    if let Ok(modified) = meta.modified() {
                        if modified > *newest {
                            *newest = modified;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn assets_stamp() -> String {
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    scan_newest_mtime(&assets_dir(), &mut newest);
    newest
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Bumped with every release; the page links the sheet and script under this
/// name.
pub(crate) const ASSETS_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), ".1");

/// The console's layout tokens, one place for both sides of the wire: the page
/// keeps the same numbers as its fallback, and the daemon stamps this object in
/// front of them so the window size, the poll cadence and the gateway port are
/// declared once.
pub(crate) fn app_layout_json(port: u16, csrf_token: &str) -> String {
    let mut layout = json!({
        "version": ASSETS_VERSION,
        "webPort": port,
        "gatewayPort": 3425,
        "csrfToken": csrf_token,
        "window": {
            "width": 820, "height": 600, "minWidth": 640, "minHeight": 460,
            "sideMinWidth": 168, "sideWidth": 212, "sideMaxWidth": 300
        },
        // the rail opens with the window beside at least a 560px column of
        // text; the button in its head folds it into the header, and the choice
        // outlives the window. On a first visit a narrow window starts folded
        "sidebarAt": 768, "sidebarNarrow": 200, "contentMax": 1100,
        "inset": 16, "radius": 14, "radiusSm": 10, "topBar": 46,
        "dashboardPollMs": 4000, "firstProbeRetries": 2, "firstProbeRetryMs": 1500,
        "emptyGraceMs": 15000, "emptyStrikes": 3,
        "themeBg": { "light": "#f4f5f8", "dark": "#1a1a1e" },
        "storage": {
            "theme": "agenthub_theme", "view": "agenthub_view",
            "layout": "agenthub_layout", "sideWidth": "agenthub_side_w",
            "closedGroups": "agenthub_closed_groups",
            "customGateway": "agenthub_custom_gateway", "sideNav": "agenthub_side_nav",
            "settingsTab": "agenthub_settings_tab", "pollMs": "agenthub_poll_ms",
            "motion": "agenthub_motion",
            "usageTab": "agenthub_usage_tab", "usagePeriod": "agenthub_usage_period",
            "usageEvery": "agenthub_usage_every", "ledMetric": "agenthub_led_metric",
            "ledSplit": "agenthub_led_split", "ledHeat": "agenthub_led_heat"
        }
    });
    if dev_assets() {
        layout["devAssets"] = json!(true);
    }
    layout.to_string()
}

/// The page as served: the layout JSON data tag first, then the stylesheet and
/// script URLs at this build's version.
pub(crate) fn render_index_html(port: u16, csrf_token: &str) -> String {
    let layout = format!(
        r#"<script id="app-layout-data" type="application/json">{}</script>"#,
        app_layout_json(port, csrf_token).replace("</", r"<\/")
    );
    // in a release the page is the copy embedded in the binary; a debug build
    // reads `resources/dashboard.html` so an edit shows without a rebuild
    let page = asset_text(INDEX_HTML, "dashboard.html");
    let placeholder = r#"<script id="app-layout-data" type="application/json">{}</script>"#;
    let html = if page.contains(placeholder) {
        page.replacen(placeholder, &layout, 1)
    } else if let Some(head_end) = page.find("</head>") {
        format!("{}  {layout}\n{}", &page[..head_end], &page[head_end..])
    } else {
        format!("{layout}\n{page}")
    };
    // a debug build asks for the sheet and the script under a name that
    // changes with the files themselves, so a reload after an edit can never
    // be answered from a copy of the one before it
    let version = if dev_assets() {
        format!("{ASSETS_VERSION}-{}", assets_stamp())
    } else {
        ASSETS_VERSION.to_owned()
    };
    html.replace("__ASSETS_VERSION__", &version)
}

/// 嵌入式 PWA Web App Manifest，用于驱动 Edge 桌面应用模式的主题色与窗口特征
const MANIFEST_JSON: &str = r##"{
  "name": "AgentHub",
  "short_name": "AgentHub",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#18181b",
  "theme_color": "#18181b",
  "icons": [
    {
      "src": "/icon-28.png?v=__ASSETS_VERSION__",
      "sizes": "28x28",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-30.png?v=__ASSETS_VERSION__",
      "sizes": "30x30",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-36.png?v=__ASSETS_VERSION__",
      "sizes": "36x36",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-42.png?v=__ASSETS_VERSION__",
      "sizes": "42x42",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-48.png?v=__ASSETS_VERSION__",
      "sizes": "48x48",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-56.png?v=__ASSETS_VERSION__",
      "sizes": "56x56",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-72.png?v=__ASSETS_VERSION__",
      "sizes": "72x72",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-96.png?v=__ASSETS_VERSION__",
      "sizes": "96x96",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-128.png?v=__ASSETS_VERSION__",
      "sizes": "128x128",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-192.png?v=__ASSETS_VERSION__",
      "sizes": "192x192",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-256.png?v=__ASSETS_VERSION__",
      "sizes": "256x256",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-512.png?v=__ASSETS_VERSION__",
      "sizes": "512x512",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon.png?v=__ASSETS_VERSION__",
      "sizes": "256x256",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon.svg?v=__ASSETS_VERSION__",
      "sizes": "any",
      "type": "image/svg+xml",
      "purpose": "any"
    }
  ]
}"##;

pub(crate) fn render_manifest_json() -> String {
    let version = if dev_assets() {
        format!("{ASSETS_VERSION}-{}", assets_stamp())
    } else {
        ASSETS_VERSION.to_owned()
    };
    MANIFEST_JSON.replace("__ASSETS_VERSION__", &version)
}
