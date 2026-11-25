use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub fn start_tray(_web_url: String, _stopped: Arc<AtomicBool>) {}

pub fn open_desktop_window(url: &str) {
    if !super::is_safe_http_url(url) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn open_system_browser(url: &str) {
    open_desktop_window(url);
}

pub fn open_data_dir() {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = home.join(".agenthub");
    let _ = std::fs::create_dir_all(&dir);
}

pub fn cleanup_desktop_window() {}

pub fn run_ui_window(url: &str, _idle_seconds: u64, _title: &str) -> anyhow::Result<()> {
    open_system_browser(url);
    Ok(())
}
