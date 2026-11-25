use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::windows::{TRAY_HWND, TRAY_STATE, WINDOW_DEFAULT, WINDOW_MIN, remove_tray_icon};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FindWindowW, IsIconic, IsWindow, PostMessageW, RegisterWindowMessageW,
    SMTO_ABORTIFHUNG, SW_RESTORE, SW_SHOW, SendMessageTimeoutW, SetForegroundWindow, ShowWindow,
    WM_CLOSE, WM_COPYDATA,
};

pub(crate) const WEBVIEW_WINDOW_CLASS: &str = "AgentHubRunnerWebViewClass";
pub(crate) const COPYDATA_NAVIGATE_URL: usize = 0x4148_5552;
pub(crate) const THEME_CHANGED_MSG_NAME: &str = "AgentHubRunnerThemeChanged";
const DEFAULT_UI_IDLE_SECONDS: u64 = 300;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn get_or_create_app_job() -> Option<HANDLE> {
    static APP_JOB: OnceLock<Option<usize>> = OnceLock::new();
    let val = APP_JOB.get_or_init(|| unsafe {
        let handle = CreateJobObjectW(null(), null());
        if handle == 0 as _ {
            return None;
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            (&raw const info).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            CloseHandle(handle);
            return None;
        }
        Some(handle as usize)
    });
    val.map(|h| h as HANDLE)
}

pub(crate) fn remember_window_rect(dir: &Path) -> (i32, i32, u32, u32) {
    let (aw, ah) = super::screen_size();
    let (req_w, req_h) = std::fs::read_to_string(dir.join("app_window.txt"))
        .ok()
        .and_then(|text| {
            let mut parts = text.lines().next()?.trim().split(',');
            let w: u32 = parts.next()?.trim().parse().ok()?;
            let h: u32 = parts.next()?.trim().parse().ok()?;
            (w > 0 && h > 0).then_some((w, h))
        })
        .unwrap_or(WINDOW_DEFAULT);
    let cap_w = (aw as f32 * 0.94) as u32;
    let cap_h = (ah as f32 * 0.90) as u32;
    let win_w = req_w.clamp(WINDOW_MIN.0.min(cap_w), cap_w);
    let win_h = req_h.clamp(WINDOW_MIN.1.min(cap_h), cap_h);
    let x = ((aw.saturating_sub(win_w)) / 2) as i32;
    let y = ((ah.saturating_sub(win_h)) / 2) as i32;
    (x, y, win_w, win_h)
}

pub(crate) fn save_window_size(dir: &Path, width: i32, height: i32) {
    let min_w = WINDOW_MIN.0 as i32;
    let min_h = WINDOW_MIN.1 as i32;
    if width < min_w || height < min_h {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join("app_window.txt"), format!("{width},{height}\n"));
}

pub(crate) fn find_live_ui_window() -> Option<HWND> {
    let class_wide: Vec<u16> = WEBVIEW_WINDOW_CLASS
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let hwnd = FindWindowW(class_wide.as_ptr(), null());
        if !hwnd.is_null() && IsWindow(hwnd) != 0 {
            Some(hwnd)
        } else {
            None
        }
    }
}

pub(crate) fn focus_and_navigate_existing(hwnd: HWND, url: &str) -> bool {
    unsafe {
        if IsIconic(hwnd) != 0 {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        let bytes = url.as_bytes();
        let cds = COPYDATASTRUCT {
            dwData: COPYDATA_NAVIGATE_URL,
            cbData: bytes.len() as u32,
            lpData: bytes.as_ptr() as *mut c_void,
        };
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            0,
            (&raw const cds) as LPARAM,
            SMTO_ABORTIFHUNG,
            1500,
            &mut result,
        ) != 0;
        sent && result != 0
    }
}

pub fn notify_ui_window_theme(dark: bool) {
    let Some(hwnd) = find_live_ui_window() else {
        return;
    };
    let msg_wide: Vec<u16> = THEME_CHANGED_MSG_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let msg = RegisterWindowMessageW(msg_wide.as_ptr());
        if msg != 0 {
            let _ = PostMessageW(hwnd, msg, usize::from(dark), 0);
        }
    }
}

pub(crate) fn build_hash_navigation_script(hash: &str) -> String {
    let normalized = if hash.starts_with('#') {
        hash.to_string()
    } else {
        format!("#{hash}")
    };
    let encoded = serde_json::to_string(&normalized).unwrap_or_else(|_| "\"#\"".to_string());
    format!(
        "(function(h){{if(location.hash===h){{window.dispatchEvent(new HashChangeEvent('hashchange'));}}else{{location.hash=h;}}}})({encoded});"
    )
}

fn has_live_tracked_child() -> bool {
    let Some(state) = TRAY_STATE.get() else {
        return false;
    };
    let Ok(mut lock) = state.active_child.lock() else {
        return false;
    };
    let Some(child) = lock.as_mut() else {
        return false;
    };
    match child.try_wait() {
        Ok(None) => true,
        _ => {
            *lock = None;
            false
        }
    }
}

fn reap_stale_tracked_child() {
    let Some(state) = TRAY_STATE.get() else {
        return;
    };
    if let Ok(mut lock) = state.active_child.lock()
        && let Some(mut old) = lock.take()
    {
        if matches!(old.try_wait(), Ok(None)) {
            let _ = old.kill();
            let _ = old.wait();
        }
    }
}

pub fn open_desktop_window(url: &str) {
    let pending_hash = crate::tray::take_pending_window_hash();
    let effective_url: std::borrow::Cow<'_, str> = match pending_hash {
        Some(h) if !url.contains('#') && !h.trim().is_empty() => {
            let clean = url.trim_end_matches('/');
            let trimmed = h.trim();
            let normalized = if trimmed.starts_with('#') {
                trimmed.to_string()
            } else {
                format!("#{trimmed}")
            };
            std::borrow::Cow::Owned(format!("{clean}{normalized}"))
        }
        _ => std::borrow::Cow::Borrowed(url),
    };
    let url = effective_url.as_ref();

    if let Some(hwnd) = find_live_ui_window()
        && focus_and_navigate_existing(hwnd, url)
    {
        return;
    }

    // If a child UI process was just spawned moments ago and hasn't finished
    // registering its window yet, wait briefly instead of spawning a second one.
    if has_live_tracked_child() {
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline {
            if let Some(hwnd) = find_live_ui_window()
                && focus_and_navigate_existing(hwnd, url)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    reap_stale_tracked_child();

    let Ok(exe) = std::env::current_exe() else {
        super::open_system_browser(url);
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args([
        "ui",
        "--url",
        url,
        "--idle-seconds",
        &DEFAULT_UI_IDLE_SECONDS.to_string(),
        "--title",
        "AgentHub",
    ]);
    cmd.creation_flags(CREATE_NO_WINDOW);

    if let Ok(child) = cmd.spawn() {
        if let Some(job) = get_or_create_app_job() {
            unsafe {
                let raw_handle = child.as_raw_handle() as HANDLE;
                let _ = AssignProcessToJobObject(job, raw_handle);
            }
        }
        if let Some(state) = TRAY_STATE.get()
            && let Ok(mut lock) = state.active_child.lock()
        {
            *lock = Some(child);
        }
        return;
    }

    super::open_system_browser(url);
}

pub fn cleanup_desktop_window() {
    if let Some(hwnd) = find_live_ui_window() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
    }
    if let Some(state) = TRAY_STATE.get()
        && let Ok(mut lock) = state.active_child.lock()
        && let Some(mut child) = lock.take()
    {
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    let hwnd_usize = TRAY_HWND.load(Ordering::SeqCst);
    if hwnd_usize != 0 {
        unsafe {
            let hwnd = hwnd_usize as HWND;
            remove_tray_icon(hwnd);
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
    }
}

pub(crate) fn agenthub_home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agenthub")
}
