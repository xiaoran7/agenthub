use super::*;

/// The caption takes the page's `--bg` and its text the page's `--fg`, in
/// the `[r, g, b]` triples Chromium's theme manifest takes; the manifest
/// is a manifest Chromium will load, and it lands in `window_shell/`.
#[test]
fn the_window_shell_paints_the_caption_in_the_page_colour() {
    let dir = tempfile::tempdir().expect("temp dir");
    for (theme, bg, fg) in [
        ("light", [0xf4, 0xf5, 0xf8], [0x1c, 0x1c, 0x21]),
        ("dark", [0x1a, 0x1a, 0x1e], [0xed, 0xed, 0xf1]),
    ] {
        let shell = write_shell_extension(dir.path(), theme).expect("shell written");
        assert_eq!(shell, dir.path().join(SHELL_DIR));
        let written = std::fs::read_to_string(shell.join("manifest.json")).expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&written).expect("json");
        assert_eq!(parsed["manifest_version"], 3);
        assert_eq!(parsed["theme"]["colors"]["frame"], serde_json::json!(bg));
        assert_eq!(parsed["theme"]["colors"]["toolbar"], serde_json::json!(bg));
        assert_eq!(parsed["theme"]["colors"]["tab_text"], serde_json::json!(fg));
    }
}

/// A window opened before the reader ever picked a theme is light, and one
/// whose copy says dark opens dark.
#[test]
fn a_window_opens_in_the_theme_the_page_was_left_in() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(remembered_theme(dir.path()), "light");
    std::fs::create_dir_all(dir.path().join(SHELL_DIR)).expect("shell dir");
    std::fs::write(dir.path().join(SHELL_DIR).join("theme.txt"), "dark\n").expect("theme");
    assert_eq!(remembered_theme(dir.path()), "dark");
    std::fs::write(dir.path().join(SHELL_DIR).join("theme.txt"), "light").expect("theme");
    assert_eq!(remembered_theme(dir.path()), "light");
}

#[test]
fn idle_tracker_counts_inactive_ticks_and_resets_on_activity() {
    use super::webview_idle::{IdleAction, IdleTracker};

    let mut tracker = IdleTracker::new(3);
    assert_eq!(tracker.step(false), IdleAction::KeepAlive);
    assert_eq!(tracker.inactive_ticks(), 1);
    assert_eq!(tracker.step(false), IdleAction::KeepAlive);
    assert_eq!(tracker.inactive_ticks(), 2);

    // Becoming visible/foreground resets immediately
    assert_eq!(tracker.step(true), IdleAction::KeepAlive);
    assert_eq!(tracker.inactive_ticks(), 0);

    // Three consecutive inactive ticks trigger Release
    assert_eq!(tracker.step(false), IdleAction::KeepAlive);
    assert_eq!(tracker.step(false), IdleAction::KeepAlive);
    assert_eq!(tracker.step(false), IdleAction::Release);

    // Zero limit disables auto-release
    let mut disabled = IdleTracker::new(0);
    for _ in 0..10 {
        assert_eq!(disabled.step(false), IdleAction::KeepAlive);
    }
    assert_eq!(disabled.inactive_ticks(), 0);
}

#[cfg(target_os = "windows")]
#[test]
fn hash_navigation_script_normalizes_hash_and_dispatches_change() {
    let script = super::desktop_window::build_hash_navigation_script("settings/gateway");
    assert!(script.contains("\"#settings/gateway\""));
    assert!(script.contains("HashChangeEvent"));

    let script2 = super::desktop_window::build_hash_navigation_script("#usage");
    assert!(script2.contains("\"#usage\""));
}

#[cfg(target_os = "windows")]
#[test]
fn window_size_persistence_ignores_below_minimum_and_stores_valid_rect() {
    let dir = tempfile::tempdir().expect("temp dir");
    super::desktop_window::save_window_size(dir.path(), 120, 80);
    assert!(!dir.path().join("app_window.txt").exists());

    super::desktop_window::save_window_size(dir.path(), 900, 650);
    let saved = std::fs::read_to_string(dir.path().join("app_window.txt")).expect("saved");
    assert_eq!(saved.trim(), "900,650");
}
