use std::ffi::c_void;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, IMAGE_ICON, KillTimer, LR_DEFAULTCOLOR, LoadIconW, LoadImageW, MSG,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXICON, SM_CXSMICON, SM_CYICON,
    SM_CYSMICON, SetTimer, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSW,
};

pub use super::desktop_window::{
    cleanup_desktop_window, notify_ui_window_theme, open_desktop_window,
};
pub use super::webview_host::run_ui_window;

const WM_TRAY_CALLBACK: u32 = WM_USER + 101;
const TIMER_HOVER_DWELL: usize = 1001;
static HOVER_ANCHOR: Mutex<Option<POINT>> = Mutex::new(None);
static TRAY_HICON: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(crate) static TRAY_HWND: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static WM_TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub(crate) static TRAY_STATE: OnceLock<TrayState> = OnceLock::new();

pub(crate) struct TrayState {
    pub(crate) web_url: Mutex<String>,
    pub(crate) stopped: Arc<AtomicBool>,
    pub(crate) active_child: Mutex<Option<std::process::Child>>,
}

unsafe fn install_or_refresh_tray_icon(hwnd: HWND) -> bool {
    let hicon_usize = TRAY_HICON.load(Ordering::SeqCst);
    if hicon_usize == 0 {
        return false;
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = hicon_usize as _;
        let tip = "AgentHub\0".encode_utf16();
        for (dest, src) in nid.szTip.iter_mut().zip(tip) {
            *dest = src;
        }

        if Shell_NotifyIconW(NIM_ADD, &nid) != 0 {
            return true;
        }
        Shell_NotifyIconW(NIM_MODIFY, &nid) != 0
    }
}

pub(crate) unsafe fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

unsafe fn allow_message_through_uipi(hwnd: HWND, msg: u32) {
    use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
    let user32_name: Vec<u16> = "user32.dll\0".encode_utf16().collect();
    unsafe {
        let huser32 = GetModuleHandleW(user32_name.as_ptr());
        if (huser32 as usize) != 0 {
            let fn_name = b"ChangeWindowMessageFilterEx\0";
            if let Some(proc) = GetProcAddress(huser32, fn_name.as_ptr()) {
                type FnChangeFilterEx =
                    unsafe extern "system" fn(HWND, u32, u32, *mut c_void) -> i32;
                let func: FnChangeFilterEx = std::mem::transmute(proc);
                const MSGFLT_ALLOW: u32 = 1;
                let _ = func(hwnd, msg, MSGFLT_ALLOW, std::ptr::null_mut());
            }
        }
    }
}

pub fn enable_per_monitor_dpi_awareness() {
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
            SetThreadDpiAwarenessContext,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetProcessDPIAware();
    }
}

pub fn start_tray(web_url: String, stopped: Arc<AtomicBool>) {
    let _ = TRAY_STATE.set(TrayState {
        web_url: Mutex::new(web_url.clone()),
        stopped,
        active_child: Mutex::new(None),
    });

    thread::spawn(|| {
        let _ = crate::claude_gateway::discover_all_client_route_summaries();
        let _ = crate::usage_ledger::digest_cached(crate::usage_ledger::Period::Today, 20);
    });

    thread::spawn(move || unsafe {
        enable_per_monitor_dpi_awareness();
        let hinstance: HINSTANCE = GetModuleHandleW(null());
        let class_name: Vec<u16> = "AgentHubRunnerTrayClass\0".encode_utf16().collect();

        let cx_big = GetSystemMetrics(SM_CXICON);
        let cy_big = GetSystemMetrics(SM_CYICON);
        let hicon_big = LoadImageW(
            hinstance,
            1 as _,
            IMAGE_ICON,
            cx_big,
            cy_big,
            LR_DEFAULTCOLOR,
        );
        let hicon_big = if !hicon_big.is_null() {
            hicon_big as _
        } else {
            let fallback = crate::window_icon::create_hicon_from_embedded_ico(cx_big);
            if !fallback.is_null() {
                fallback as _
            } else {
                LoadIconW(hinstance, 1 as _)
            }
        };

        let cx_sm = GetSystemMetrics(SM_CXSMICON);
        let cy_sm = GetSystemMetrics(SM_CYSMICON);
        let hicon_sm = LoadImageW(hinstance, 1 as _, IMAGE_ICON, cx_sm, cy_sm, LR_DEFAULTCOLOR);
        let hicon_sm = if !hicon_sm.is_null() {
            hicon_sm as _
        } else {
            let fallback = crate::window_icon::create_hicon_from_embedded_ico(cx_sm);
            if !fallback.is_null() {
                fallback as _
            } else {
                LoadIconW(hinstance, 1 as _)
            }
        };
        TRAY_HICON.store(hicon_sm as usize, Ordering::SeqCst);

        let wnd_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(tray_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: hicon_big,
            hCursor: 0 as _,
            hbrBackground: 0 as _,
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
        };

        RegisterClassW(&wnd_class);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            0 as _,
            0 as _,
            hinstance,
            null(),
        );

        if hwnd == 0 as _ {
            return;
        }
        TRAY_HWND.store(hwnd as usize, Ordering::SeqCst);

        let taskbar_msg_name: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
        let taskbar_created = RegisterWindowMessageW(taskbar_msg_name.as_ptr());
        if taskbar_created != 0 {
            WM_TASKBAR_CREATED.store(taskbar_created, Ordering::SeqCst);
            allow_message_through_uipi(hwnd, taskbar_created);
        }

        let mut added = false;
        for _ in 0..15 {
            if install_or_refresh_tray_icon(hwnd) {
                added = true;
                break;
            }
            thread::sleep(std::time::Duration::from_millis(200));
        }
        if !added {
            install_or_refresh_tray_icon(hwnd);
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        remove_tray_icon(hwnd);
        TRAY_HWND.store(0, Ordering::SeqCst);
    });
}

unsafe fn show_tray_popup() {
    let current_url = TRAY_STATE
        .get()
        .and_then(|s| s.web_url.lock().ok())
        .map(|u| u.clone())
        .unwrap_or_else(|| "127.0.0.1:4200".to_string());
    let display_host = current_url
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string();
    unsafe {
        crate::tray_menu::show(
            GetModuleHandleW(null()),
            crate::tray_menu::build_live_menu(display_host),
            tray_choice,
        );
    }
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let taskbar_created = WM_TASKBAR_CREATED.load(Ordering::Relaxed);
        if taskbar_created != 0 && msg == taskbar_created {
            install_or_refresh_tray_icon(hwnd);
            return 0;
        }

        match msg {
            WM_TRAY_CALLBACK => {
                let event = lparam as u32;
                if event == WM_LBUTTONDBLCLK {
                    let _ = HOVER_ANCHOR.lock().map(|mut g| *g = None);
                    KillTimer(hwnd, TIMER_HOVER_DWELL);
                    crate::tray_menu::hide();
                    if let Some(state) = TRAY_STATE.get()
                        && let Ok(url) = state.web_url.lock()
                    {
                        open_desktop_window(&url);
                    }
                } else if event == WM_MOUSEMOVE {
                    if !crate::tray_menu::is_visible()
                        && !crate::tray_menu::closed_within(std::time::Duration::from_millis(250))
                    {
                        let mut cursor: POINT = std::mem::zeroed();
                        GetCursorPos(&mut cursor);
                        let mut anchor_guard =
                            HOVER_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
                        if anchor_guard.is_none() {
                            *anchor_guard = Some(cursor);
                            SetTimer(hwnd, TIMER_HOVER_DWELL, 65, None);
                        }
                    }
                } else if event == WM_LBUTTONUP || event == WM_RBUTTONUP {
                    let _ = HOVER_ANCHOR.lock().map(|mut g| *g = None);
                    KillTimer(hwnd, TIMER_HOVER_DWELL);
                    if crate::tray_menu::is_visible() {
                        if !crate::tray_menu::shown_within(std::time::Duration::from_millis(120)) {
                            crate::tray_menu::hide();
                        }
                    } else if !crate::tray_menu::closed_within(std::time::Duration::from_millis(
                        200,
                    )) {
                        show_tray_popup();
                    }
                }
                0
            }
            WM_TIMER => {
                if wparam == TIMER_HOVER_DWELL {
                    KillTimer(hwnd, TIMER_HOVER_DWELL);
                    let anchor = {
                        let mut guard = HOVER_ANCHOR.lock().unwrap_or_else(|e| e.into_inner());
                        guard.take()
                    };
                    if let Some(anchor) = anchor {
                        let mut cursor: POINT = std::mem::zeroed();
                        GetCursorPos(&mut cursor);
                        if (cursor.x - anchor.x).abs() <= 18
                            && (cursor.y - anchor.y).abs() <= 18
                            && !crate::tray_menu::is_visible()
                            && !crate::tray_menu::closed_within(std::time::Duration::from_millis(
                                200,
                            ))
                        {
                            show_tray_popup();
                        }
                    }
                }
                0
            }
            WM_CLOSE => {
                remove_tray_icon(hwnd);
                let _ = DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Runs the action selected from the tray popup card.
fn tray_choice(id: usize) {
    let open_hash = |suffix: &str| {
        if !suffix.is_empty() {
            super::set_pending_window_hash(Some(suffix.to_string()));
        }
        if let Some(state) = TRAY_STATE.get()
            && let Ok(url) = state.web_url.lock()
        {
            let base = url.trim_end_matches('/');
            let target = if suffix.is_empty() {
                base.to_string()
            } else {
                format!("{base}{suffix}")
            };
            open_desktop_window(&target);
        }
    };
    match id {
        crate::tray_menu::ACTION_OPEN_MAIN => open_hash(""),
        crate::tray_menu::ACTION_OPEN_SETTINGS => open_hash("#settings/gateway"),
        crate::tray_menu::ACTION_OPEN_AGENTS => open_hash("#agents"),
        crate::tray_menu::ACTION_OPEN_USAGE => open_hash("#usage"),
        crate::tray_menu::ACTION_EXIT => {
            cleanup_desktop_window();
            if let Some(state) = TRAY_STATE.get() {
                state.stopped.store(true, Ordering::SeqCst);
            }
            thread::spawn(|| {
                thread::sleep(std::time::Duration::from_secs(3));
                std::process::exit(0);
            });
        }
        _ => {}
    }
}

pub(crate) const WINDOW_DEFAULT: (u32, u32) = (820, 600);
pub(crate) const WINDOW_MIN: (u32, u32) = (640, 460);

pub fn screen_size() -> (u32, u32) {
    unsafe {
        enable_per_monitor_dpi_awareness();
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
        };
        (
            GetSystemMetrics(SM_CXSCREEN).max(1) as u32,
            GetSystemMetrics(SM_CYSCREEN).max(1) as u32,
        )
    }
}

pub fn open_system_browser(url: &str) {
    if !super::is_safe_http_url(url) {
        return;
    }
    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let url_wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        let open_op: Vec<u16> = "open\0".encode_utf16().collect();
        ShellExecuteW(
            0 as _,
            open_op.as_ptr(),
            url_wide.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_data_dir() {
    let agenthub_dir = super::desktop_window::agenthub_home_dir();
    let _ = std::fs::create_dir_all(&agenthub_dir);

    unsafe {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let dir_str = agenthub_dir.to_string_lossy().to_string();
        let dir_wide: Vec<u16> = dir_str.encode_utf16().chain(std::iter::once(0)).collect();
        let explore_op: Vec<u16> = "explore\0".encode_utf16().collect();
        ShellExecuteW(
            0 as _,
            explore_op.as_ptr(),
            dir_wide.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}
