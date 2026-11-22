#[cfg(target_os = "windows")]
pub mod desktop_window;
#[cfg(target_os = "windows")]
mod webview_com;
#[cfg(target_os = "windows")]
pub mod webview_host;
pub mod webview_idle;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::{
    cleanup_desktop_window, enable_per_monitor_dpi_awareness, open_data_dir, open_desktop_window,
    open_system_browser, run_ui_window, screen_size, start_tray,
};

#[cfg(not(target_os = "windows"))]
pub mod non_windows;
#[cfg(not(target_os = "windows"))]
pub use non_windows::{
    cleanup_desktop_window, open_data_dir, open_desktop_window, open_system_browser, run_ui_window,
    start_tray,
};

pub mod theme;
pub use theme::*;

/// Only `http://` and `https://` URLs with a non-empty host and no control chars
/// are allowed when opening the system browser or driving WebView navigation.
pub(crate) fn is_safe_http_url(raw: &str) -> bool {
    if raw.chars().any(|c| c.is_control()) {
        return false;
    }
    let Ok(parsed) = url::Url::parse(raw) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
}

#[cfg(test)]
mod tests;
