#[cfg(target_os = "windows")]
mod windows_tray {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;

    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        IMAGE_ICON, KillTimer, LR_DEFAULTCOLOR, LoadIconW, LoadImageW, MSG, PostQuitMessage,
        RegisterClassW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SetTimer, TranslateMessage,
        WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSW,
    };

    const WM_TRAY_CALLBACK: u32 = WM_USER + 101;
    /// The thread timer that watches the open menu for a click outside it.
    const MENU_WATCH: usize = 0x4148;
    const ID_TRAY_OPEN_DESKTOP: usize = 2001;
    const ID_TRAY_OPEN_GATEWAY: usize = 2002;
    const ID_TRAY_OPEN_BROWSER: usize = 2003;
    const ID_TRAY_OPEN_DATA_DIR: usize = 2004;
    const ID_TRAY_EXIT: usize = 2005;

    static TRAY_STATE: OnceLock<TrayState> = OnceLock::new();

    struct TrayState {
        web_url: Mutex<String>,
        running: Arc<AtomicBool>,
        active_child: Mutex<Option<std::process::Child>>,
    }

    fn get_or_create_app_job() -> Option<HANDLE> {
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

    pub fn cleanup_desktop_window() {
        if let Some(state) = TRAY_STATE.get() {
            if let Ok(mut lock) = state.active_child.lock() {
                if let Some(mut child) = lock.take() {
                    let _ = child.kill();
                }
            }
        }
    }

    pub fn start_tray(web_url: String, running: Arc<AtomicBool>) {
        let _ = TRAY_STATE.set(TrayState {
            web_url: Mutex::new(web_url.clone()),
            running,
            active_child: Mutex::new(None),
        });

        thread::spawn(move || unsafe {
            let hinstance: HINSTANCE = GetModuleHandleW(null());
            let class_name: Vec<u16> = "AgentHubRunnerTrayClass\0".encode_utf16().collect();

            // 为窗口类与托盘按系统 DPI 指标分别提取原生精准位图，杜绝 Windows 强制插值导致的锯齿拉伸
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
            let hicon_big = if hicon_big != 0 as _ {
                hicon_big as _
            } else {
                LoadIconW(hinstance, 1 as _)
            };

            let cx_sm = GetSystemMetrics(SM_CXSMICON);
            let cy_sm = GetSystemMetrics(SM_CYSMICON);
            let hicon_sm = LoadImageW(hinstance, 1 as _, IMAGE_ICON, cx_sm, cy_sm, LR_DEFAULTCOLOR);
            let hicon_sm = if hicon_sm != 0 as _ {
                hicon_sm as _
            } else {
                LoadIconW(hinstance, 1 as _)
            };

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

            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            nid.uCallbackMessage = WM_TRAY_CALLBACK;
            nid.hIcon = hicon_sm;

            let tip_text = format!("AgentHub Runner ({})\0", web_url);
            let tip_u16: Vec<u16> = tip_text.encode_utf16().collect();
            let copy_len = tip_u16.len().min(nid.szTip.len() - 1);
            nid.szTip[..copy_len].copy_from_slice(&tip_u16[..copy_len]);

            Shell_NotifyIconW(NIM_ADD, &nid);

            // while the menu is up, a slow beat watches whether the reader
            // has clicked outside it; the menu itself is a window of its own
            // and would otherwise never hear about a click that leaves
            SetTimer(0 as _, MENU_WATCH, 120, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                if msg.hwnd == 0 as _ && msg.message == WM_TIMER && msg.wParam == MENU_WATCH {
                    crate::tray_menu::hide_if_clicked_away();
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            KillTimer(0 as _, MENU_WATCH);

            Shell_NotifyIconW(NIM_DELETE, &nid);
        });
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match msg {
                WM_TRAY_CALLBACK => {
                    let event = lparam as u32;
                    if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                        // the menu closes itself when the click leaves it, and
                        // the press that got here is that same click
                        if crate::tray_menu::closed_within(std::time::Duration::from_millis(250)) {
                            crate::tray_menu::hide();
                        } else if let Some(state) = TRAY_STATE.get() {
                            if let Ok(url) = state.web_url.lock() {
                                open_desktop_window(&url);
                            }
                        }
                    } else if event == WM_RBUTTONUP {
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
                        crate::tray_menu::show(
                            GetModuleHandleW(null()),
                            crate::tray_menu::Menu {
                                version: env!("CARGO_PKG_VERSION").to_string(),
                                host: display_host,
                                items: vec![
                                    crate::tray_menu::Item {
                                        id: ID_TRAY_OPEN_DESKTOP,
                                        label: "显示协同控制台",
                                        danger: false,
                                        separator_before: false,
                                    },
                                    crate::tray_menu::Item {
                                        id: ID_TRAY_OPEN_GATEWAY,
                                        label: "查看演进拓扑（网关）",
                                        danger: false,
                                        separator_before: false,
                                    },
                                    crate::tray_menu::Item {
                                        id: ID_TRAY_OPEN_BROWSER,
                                        label: "在系统浏览器中打开",
                                        danger: false,
                                        separator_before: false,
                                    },
                                    crate::tray_menu::Item {
                                        id: ID_TRAY_OPEN_DATA_DIR,
                                        label: "打开数据存储目录",
                                        danger: false,
                                        separator_before: true,
                                    },
                                    crate::tray_menu::Item {
                                        id: ID_TRAY_EXIT,
                                        label: "退出 AgentHub",
                                        danger: true,
                                        separator_before: true,
                                    },
                                ],
                            },
                            tray_choice,
                        );
                    }
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

    /// Runs the row the menu offered; the menu itself is already closed.
    fn tray_choice(id: usize) {
        if id == ID_TRAY_OPEN_DESKTOP {
            if let Some(state) = TRAY_STATE.get() {
                if let Ok(url) = state.web_url.lock() {
                    open_desktop_window(&url);
                }
            }
        } else if id == ID_TRAY_OPEN_GATEWAY {
            // the console's gateway route lives on its settings page
            if let Some(state) = TRAY_STATE.get() {
                if let Ok(url) = state.web_url.lock() {
                    let settings_url = format!("{}#settings/gateway", url.trim_end_matches('/'));
                    open_desktop_window(&settings_url);
                }
            }
        } else if id == ID_TRAY_OPEN_BROWSER {
            if let Some(state) = TRAY_STATE.get() {
                if let Ok(url) = state.web_url.lock() {
                    open_system_browser(&url);
                }
            }
        } else if id == ID_TRAY_OPEN_DATA_DIR {
            open_data_dir();
        } else if id == ID_TRAY_EXIT {
            cleanup_desktop_window();
            if let Some(state) = TRAY_STATE.get() {
                state.running.store(false, Ordering::SeqCst);
            }
            std::process::exit(0);
        }
    }

    /// The console window's size, in one place with the page's own tokens: the
    /// daemon decides how big the frame opens, the page adapts to whatever it gets.
    /// A reading window, not a canvas, as magpie's 660×600 is; this one carries
    /// a rail as well, so 820×600. Sized after 2.4.86 was first tuned.
    const WINDOW_DEFAULT: (u32, u32) = (820, 600);
    const WINDOW_MIN: (u32, u32) = (640, 460);
    /// The console before 2.4.86 opened at 980×720; a frame at least this big is
    /// its remains, not a size the reader picked in this one.
    const OLD_FRAME_MIN: (i32, i32) = (1000, 680);

    /// Where this machine leaves the app window: remembered size (clamped to the
    /// screen and the console's minimum), centred on the screen.
    fn remember_window_rect(dir: &std::path::Path) -> (i32, i32, u32, u32) {
        let (aw, ah) = screen_size();
        let (req_w, req_h) = std::fs::read_to_string(dir.join("app_window.txt"))
            .ok()
            .and_then(|text| {
                let mut parts = text.lines().next()?.trim().split(',');
                let w: u32 = parts.next()?.trim().parse().ok()?;
                let h: u32 = parts.next()?.trim().parse().ok()?;
                (w > 0 && h > 0).then_some((w, h))
            })
            .unwrap_or(WINDOW_DEFAULT);
        // leave the taskbar and window chrome room; never bigger than the screen
        let cap_w = (aw as f32 * 0.94) as u32;
        let cap_h = (ah as f32 * 0.90) as u32;
        let win_w = req_w.clamp(WINDOW_MIN.0.min(cap_w), cap_w);
        let win_h = req_h.clamp(WINDOW_MIN.1.min(cap_h), cap_h);
        // the middle of the screen as the frame's top left
        let x = ((aw.saturating_sub(win_w)) / 2) as i32;
        let y = ((ah.saturating_sub(win_h)) / 2) as i32;
        (x, y, win_w, win_h)
    }

    fn screen_size() -> (u32, u32) {
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
            };
            (
                GetSystemMetrics(SM_CXSCREEN).max(1) as u32,
                GetSystemMetrics(SM_CYSCREEN).max(1) as u32,
            )
        }
    }

    /// Edge keeps the app window's last size and place in the profile's
    /// Preferences and opens it there on every launch, ignoring the size and
    /// position it is asked for. A frame from an older console (at least
    /// 1000×680, or maximised) is resized to the console's size and centred; any
    /// other frame is only moved when it sits off the middle of the screen, so
    /// the size a reader picked is kept.
    fn fit_kept_window(prefs: &std::path::Path) {
        let Ok(text) = std::fs::read_to_string(prefs) else {
            return;
        };
        let Ok(mut prefs_json) = text.parse::<serde_json::Value>() else {
            return;
        };
        let Some(apps) = prefs_json["browser"]["app_window_placement"].as_object_mut() else {
            return;
        };
        let num =
            |v: &serde_json::Value, k: &str| v.get(k).and_then(|n| n.as_i64()).unwrap_or(0) as i32;
        let is_frame = |v: &serde_json::Value| v.get("left").is_some() && v.get("top").is_some();
        // app_window_placement: process id -> display -> profile -> app -> frame
        let mut frames = Vec::new();
        for app in apps.values() {
            let Some(displays) = app.as_object() else {
                continue;
            };
            for display in displays.values() {
                let Some(profiles) = display.as_object() else {
                    continue;
                };
                for held in profiles.values() {
                    if let Some(held) = held.as_object() {
                        frames.extend(held.values().filter(|v| is_frame(v)));
                    }
                }
            }
        }
        let Some(latest) = frames
            .iter()
            .max_by_key(|v| num(v, "right") + num(v, "bottom"))
        else {
            return;
        };
        let (left, top, right, bottom) = (
            num(latest, "left"),
            num(latest, "top"),
            num(latest, "right"),
            num(latest, "bottom"),
        );
        let (aw, ah) = (screen_size().0 as i32, screen_size().1 as i32);
        let stale = num(latest, "maximized") != 0
            || (right - left) >= OLD_FRAME_MIN.0
            || (bottom - top) >= OLD_FRAME_MIN.1;
        let (w, h) = if stale {
            (
                WINDOW_DEFAULT.0 as i32,
                WINDOW_DEFAULT.1.min(ah as u32 * 9 / 10) as i32,
            )
        } else {
            (right - left, bottom - top)
        };
        let (x, y) = ((aw - w) / 2, (ah - h) / 2);
        // a few pixels of slack for the frame's own border
        let slack = 8;
        if !stale && (left - x).abs() <= slack && (top - y).abs() <= slack {
            return;
        }
        for app in apps.values_mut() {
            let Some(displays) = app.as_object_mut() else {
                continue;
            };
            for display in displays.values_mut() {
                let Some(profiles) = display.as_object_mut() else {
                    continue;
                };
                for held in profiles.values_mut() {
                    let Some(held) = held.as_object_mut() else {
                        continue;
                    };
                    for frame in held.values_mut() {
                        if !is_frame(frame) {
                            continue;
                        }
                        frame["left"] = x.into();
                        frame["top"] = y.into();
                        frame["right"] = (x + w).into();
                        frame["bottom"] = (y + h).into();
                        if let Some(maximized) = frame.get_mut("maximized") {
                            *maximized = false.into();
                        }
                    }
                }
            }
        }
        if let Ok(updated) = serde_json::to_string(&prefs_json) {
            let _ = std::fs::write(prefs, updated);
        }
    }

    /// Purges stale Chromium/Edge icon and favicon cache databases so that newly updated
    /// transparent icons are immediately refreshed by the browser process and Windows taskbar,
    /// preventing stale white-squircle favicon bitmaps from persisting.
    fn clean_stale_icon_cache(profile_dir: &std::path::Path) {
        let default_dir = profile_dir.join("Default");
        let stale_files = [
            "Favicons",
            "Favicons-journal",
            "HubApps Icons",
            "HubApps Icons-journal",
        ];
        for name in &stale_files {
            let p = default_dir.join(name);
            if p.exists() {
                let _ = std::fs::remove_file(p);
            }
        }
        for dir_name in &[
            "JumpListIconsRecentClosed",
            "JumpListIconsRecentWorkspacesV2",
        ] {
            let d = default_dir.join(dir_name);
            if d.is_dir() {
                if let Ok(entries) = std::fs::read_dir(d) {
                    for entry in entries.flatten() {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }

    pub fn open_desktop_window(url: &str) {
        use std::os::windows::process::CommandExt;
        use std::path::PathBuf;

        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let edge_candidates = [
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ];

        let mut browser_path = None;
        for path in edge_candidates {
            let p = PathBuf::from(path);
            if p.is_file() {
                browser_path = Some(p);
                break;
            }
        }

        if let Some(edge) = browser_path {
            let home = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let profile_dir = home.join(".agenthub").join("app_profile");

            // If an Edge window was already spawned, kill it first so file locks on Favicons
            // are released and Edge doesn't silently delegate to an existing instance with the old icon.
            if let Some(state) = TRAY_STATE.get() {
                if let Ok(mut lock) = state.active_child.lock() {
                    if let Some(mut old) = lock.take() {
                        let _ = old.kill();
                    }
                }
            }

            // Clear stale Chromium/Edge favicon and icon caches before spawning
            clean_stale_icon_cache(&profile_dir);

            let (pos_x, pos_y, win_w, win_h) = remember_window_rect(&home.join(".agenthub"));
            let prefs = profile_dir.join("Default").join("Preferences");

            // a reader who resizes the window (and the runner is stopped while it
            // sits at that size) gets it back the same: Edge keeps its last
            // window in the profile's Preferences, which outlives the window
            let mut args = vec![
                format!("--app={}", url),
                format!("--user-data-dir={}", profile_dir.display()),
                "--no-first-run".to_string(),
                "--no-default-browser-check".to_string(),
            ];
            // the caption takes the page's colour: written before the window
            // opens, since Chromium reads a theme as it starts
            let agenthub_dir = home.join(".agenthub");
            if let Some(shell) =
                super::write_shell_extension(&agenthub_dir, super::remembered_theme(&agenthub_dir))
            {
                args.push(format!("--load-extension={}", shell.display()));
                args.push(format!("--disable-extensions-except={}", shell.display()));
            }
            if prefs.is_file() {
                fit_kept_window(&prefs);
            }
            // passed on every launch: the first window of a new profile takes
            // them, and Edge ignores both once the profile has a frame
            args.push(format!("--window-size={},{}", win_w, win_h));
            args.push(format!("--window-position={},{}", pos_x, pos_y));
            let mut cmd = std::process::Command::new(edge);
            cmd.args(&args);
            cmd.creation_flags(CREATE_NO_WINDOW);
            if let Ok(child) = cmd.spawn() {
                // Edge draws its taskbar button from the favicon it reports,
                // and that only ever reaches 16px and 32px. A watcher hands
                // the window a native-size icon so the shell has nothing to
                // resample.
                crate::window_icon::watch_desktop_window_icon();
                // 将 Edge 子进程绑定至 Windows Job Object，当 Runner 进程终止时系统自动强杀，杜绝孤儿白屏
                if let Some(job) = get_or_create_app_job() {
                    unsafe {
                        let raw_handle = child.as_raw_handle() as HANDLE;
                        let _ = AssignProcessToJobObject(job, raw_handle);
                    }
                }
                if let Some(state) = TRAY_STATE.get() {
                    if let Ok(mut lock) = state.active_child.lock() {
                        *lock = Some(child);
                    }
                }
                return;
            }
        }

        // Fallback: 使用原生 ShellExecuteW 打开，绝不唤醒 cmd.exe，完全无黑框
        open_system_browser(url);
    }

    pub fn open_system_browser(url: &str) {
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
        let home = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let agenthub_dir = home.join(".agenthub");
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
}

#[cfg(not(target_os = "windows"))]
mod non_windows_tray {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    pub fn start_tray(_web_url: String, _running: Arc<AtomicBool>) {}

    pub fn open_desktop_window(url: &str) {
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
}

#[cfg(target_os = "windows")]
pub use windows_tray::{
    cleanup_desktop_window, open_data_dir, open_desktop_window, open_system_browser, start_tray,
};

#[cfg(not(target_os = "windows"))]
pub use non_windows_tray::{
    cleanup_desktop_window, open_data_dir, open_desktop_window, open_system_browser, start_tray,
};

/// The window's caption, painted the page's own colour. magpie does this on
/// Windows with DWM (`internal/gui/titletint_windows.go`: "Windows' grey bar
/// sat above the page as a strip of its own", so the page tints it); Edge draws
/// its own caption into the client area and refuses DWM's caption colour, so
/// the same surface comes from a theme extension instead. Chromium reads a
/// theme as it starts, so the manifest is written before the window opens, in
/// the theme the page was last left in.
#[allow(dead_code)]
const SHELL_DIR: &str = "window_shell";
#[allow(dead_code)]
const SHELL_LIGHT: (&str, &str) = ("#f4f5f8", "#1c1c21");
#[allow(dead_code)]
const SHELL_DARK: (&str, &str) = ("#1a1a1e", "#ededf1");

/// The theme the reader last left the page in. The console writes
/// `agenthub_theme` to Local Storage, which Chromium keeps in the profile's
/// `Local Storage/leveldb` — not readable as JSON — so the console leaves a
/// copy here through `/api/window-theme` as well; no copy is light.
#[allow(dead_code)]
fn remembered_theme(dir: &std::path::Path) -> &'static str {
    match std::fs::read_to_string(dir.join(SHELL_DIR).join("theme.txt")) {
        Ok(text) if text.trim() == "dark" => "dark",
        _ => "light",
    }
}

/// Writes the theme the reader picked for the window's caption; the next
/// window opens in it.
pub fn remember_window_theme(dark: bool) -> Option<()> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)?;
    let dir = home.join(".agenthub").join(SHELL_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(dir.join("theme.txt"), if dark { "dark" } else { "light" }).ok()?;
    Some(())
}

/// Writes the theme extension the window is launched with: a caption in the
/// page's `--bg`, its text in the page's `--fg`, so the two read as one
/// surface. An `[r, g, b]` triple of 0–255, as Chromium's theme manifest takes
/// colours. Returns the directory to load.
#[allow(dead_code)]
fn write_shell_extension(dir: &std::path::Path, theme: &str) -> Option<std::path::PathBuf> {
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

#[cfg(test)]
mod tests {
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
}
