fn pending_window_hash_slot() -> &'static std::sync::Mutex<Option<String>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<String>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn set_pending_window_hash(hash: Option<String>) {
    if let Ok(mut guard) = pending_window_hash_slot().lock() {
        *guard = hash;
    }
}

pub fn take_pending_window_hash() -> Option<String> {
    pending_window_hash_slot()
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

/// Returns whether the console/tray should render in dark mode.
/// Checks `~/.agenthub/window_shell/theme.txt` first (written by the web console),
/// and falls back to light mode (`#f4f5f8`) which matches the console default.
pub fn is_dark_theme() -> bool {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from);
    if let Some(dir) = home {
        let theme_file = dir.join(".agenthub").join(SHELL_DIR).join("theme.txt");
        if let Ok(text) = std::fs::read_to_string(theme_file) {
            let trimmed = text.trim();
            if trimmed == "dark" {
                return true;
            }
            if trimmed == "light" {
                return false;
            }
        }
    }
    false
}

/// The window's caption, painted the page's own colour. magpie does this on
/// Windows with DWM (`internal/gui/titletint_windows.go`: "Windows' grey bar
/// sat above the page as a strip of its own", so the page tints it); Edge draws
/// its own caption into the client area and refuses DWM's caption colour, so
/// the same surface comes from a theme extension instead. Chromium reads a
/// theme as it starts, so the manifest is written before the window opens, in
/// the theme the page was last left in.
#[allow(dead_code)]
pub(crate) const SHELL_DIR: &str = "window_shell";
#[allow(dead_code)]
const SHELL_LIGHT: (&str, &str) = ("#f4f5f8", "#1c1c21");
#[allow(dead_code)]
const SHELL_DARK: (&str, &str) = ("#1a1a1e", "#ededf1");

/// The theme the reader last left the page in. The console writes
/// `agenthub_theme` to Local Storage, which Chromium keeps in the profile's
/// `Local Storage/leveldb` — not readable as JSON — so the console leaves a
/// copy here through `/api/window-theme` as well; no copy is light.
#[allow(dead_code)]
pub(crate) fn remembered_theme(dir: &std::path::Path) -> &'static str {
    match std::fs::read_to_string(dir.join(SHELL_DIR).join("theme.txt")) {
        Ok(text) if text.trim() == "dark" => "dark",
        _ => "light",
    }
}

/// Writes the theme the reader picked for the window's caption and notifies any
/// open native UI window immediately so its titlebar and background switch live.
pub fn remember_window_theme(dark: bool) -> Option<()> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)?;
    let dir = home.join(".agenthub").join(SHELL_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(dir.join("theme.txt"), if dark { "dark" } else { "light" }).ok()?;
    #[cfg(target_os = "windows")]
    super::windows::notify_ui_window_theme(dark);
    Some(())
}

/// Writes the theme extension the window is launched with: a caption in the
/// page's `--bg`, its text in the page's `--fg`, so the two read as one
/// surface. An `[r, g, b]` triple of 0–255, as Chromium's theme manifest takes
/// colours. Returns the directory to load.
#[allow(dead_code)]
pub(crate) fn write_shell_extension(
    dir: &std::path::Path,
    theme: &str,
) -> Option<std::path::PathBuf> {
    let (bg, fg) = if theme == "dark" {
        SHELL_DARK
    } else {
        SHELL_LIGHT
    };
    let rgb = |hex: &str| -> Option<[u8; 3]> {
        let hex = hex.strip_prefix('#')?;
        Some([
            u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
            u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
            u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
        ])
    };
    let (bg, fg) = (rgb(bg)?, rgb(fg)?);
    let shell = dir.join(SHELL_DIR);
    std::fs::create_dir_all(&shell).ok()?;
    let manifest = serde_json::json!({
        "manifest_version": 3,
        "name": "AgentHub Window",
        "version": "1.0",
        "theme": { "colors": {
            "frame": bg, "frame_inactive": bg,
            "frame_incognito": bg, "frame_incognito_inactive": bg,
            "toolbar": bg, "tab_text": fg, "bookmark_text": fg
        } }
    });
    std::fs::write(shell.join("manifest.json"), manifest.to_string()).ok()?;
    Some(shell)
}
