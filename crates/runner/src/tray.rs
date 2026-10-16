#[cfg(target_os = "windows")]
mod windows_tray {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostQuitMessage,
        RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, MF_SEPARATOR,
        MF_STRING, MSG, TPM_RIGHTBUTTON, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
        WM_NULL, WM_RBUTTONUP, WM_USER, WNDCLASSW,
    };

    const WM_TRAY_CALLBACK: u32 = WM_USER + 101;
    const ID_TRAY_OPEN_DESKTOP: usize = 2001;
    const ID_TRAY_OPEN_DATA_DIR: usize = 2002;
    const ID_TRAY_EXIT: usize = 2003;

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

            let wnd_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(tray_wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: LoadIconW(hinstance, 1 as _),
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
            nid.hIcon = LoadIconW(hinstance, 1 as _);

            let tip_text = format!("AgentHub Runner ({})\0", web_url);
            let tip_u16: Vec<u16> = tip_text.encode_utf16().collect();
            let copy_len = tip_u16.len().min(nid.szTip.len() - 1);
            nid.szTip[..copy_len].copy_from_slice(&tip_u16[..copy_len]);

            Shell_NotifyIconW(NIM_ADD, &nid);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

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
                        if let Some(state) = TRAY_STATE.get() {
                            if let Ok(url) = state.web_url.lock() {
                                open_desktop_window(&url);
                            }
                        }
                    } else if event == WM_RBUTTONUP {
                        let mut pt: POINT = std::mem::zeroed();
                        GetCursorPos(&mut pt);
                        SetForegroundWindow(hwnd);

                        let hmenu = CreatePopupMenu();
                        let open_desktop_text: Vec<u16> = "打开桌面控制台\0".encode_utf16().collect();
                        let open_dir_text: Vec<u16> = "打开数据目录 (~/.agenthub)\0".encode_utf16().collect();
                        let exit_text: Vec<u16> = "退出 AgentHub Runner\0".encode_utf16().collect();

                        AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_DESKTOP, open_desktop_text.as_ptr());
                        AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_DATA_DIR, open_dir_text.as_ptr());
                        AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
                        AppendMenuW(hmenu, MF_STRING, ID_TRAY_EXIT, exit_text.as_ptr());

                        TrackPopupMenu(
                            hmenu,
                            TPM_RIGHTBUTTON,
                            pt.x,
                            pt.y,
                            0,
                            hwnd,
                            null(),
                        );
                        PostMessageW(hwnd, WM_NULL, 0, 0);
                        DestroyMenu(hmenu);
                    }
                    0
                }
                WM_COMMAND => {
                    let id = wparam as usize;
                    if id == ID_TRAY_OPEN_DESKTOP {
                        if let Some(state) = TRAY_STATE.get() {
                            if let Ok(url) = state.web_url.lock() {
                                open_desktop_window(&url);
                            }
                        }
                    } else if id == ID_TRAY_OPEN_DATA_DIR {
                        open_data_dir();
                    } else if id == ID_TRAY_EXIT {
                        cleanup_desktop_window();
                        if let Some(state) = TRAY_STATE.get() {
                            state.running.store(false, Ordering::SeqCst);
                        }
                        DestroyWindow(hwnd);
                        std::process::exit(0);
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

            // 计算屏幕中心坐标，像 Magpie 一样舒适居中显示 (940x720)
            let (pos_x, pos_y, win_w, win_h) = unsafe {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
                };
                let sw = GetSystemMetrics(SM_CXSCREEN);
                let sh = GetSystemMetrics(SM_CYSCREEN);
                let w = 940;
                let h = 720;
                let x = ((sw - w) / 2).max(0);
                let y = ((sh - h) / 2).max(0);
                (x, y, w, h)
            };

            let mut cmd = std::process::Command::new(edge);
            cmd.args([
                &format!("--app={}", url),
                &format!("--window-size={},{}", win_w, win_h),
                &format!("--window-position={},{}", pos_x, pos_y),
                &format!("--user-data-dir={}", profile_dir.display()),
                "--no-first-run",
                "--no-default-browser-check",
            ]);
            cmd.creation_flags(CREATE_NO_WINDOW);
            if let Ok(child) = cmd.spawn() {
                // 将 Edge 子进程绑定至 Windows Job Object，当 Runner 进程终止时系统自动强杀，杜绝孤儿白屏
                if let Some(job) = get_or_create_app_job() {
                    unsafe {
                        let raw_handle = child.as_raw_handle() as HANDLE;
                        let _ = AssignProcessToJobObject(job, raw_handle);
                    }
                }
                if let Some(state) = TRAY_STATE.get() {
                    if let Ok(mut lock) = state.active_child.lock() {
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
                        *lock = Some(child);
                    }
                }
                return;
            }
        }

        // Fallback: 使用原生 ShellExecuteW 打开，绝不唤醒 cmd.exe，完全无黑框
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

    fn open_data_dir() {
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
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

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

    pub fn cleanup_desktop_window() {}
}

#[cfg(target_os = "windows")]
pub use windows_tray::{cleanup_desktop_window, open_desktop_window, start_tray};

#[cfg(not(target_os = "windows"))]
pub use non_windows_tray::{cleanup_desktop_window, open_desktop_window, start_tray};
