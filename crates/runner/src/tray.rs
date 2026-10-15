#[cfg(target_os = "windows")]
mod windows_tray {
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread;

    use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
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
    const ID_TRAY_OPEN_WEB: usize = 2001;
    const ID_TRAY_EXIT: usize = 2002;

    static TRAY_STATE: OnceLock<TrayState> = OnceLock::new();

    struct TrayState {
        web_url: Mutex<String>,
        running: Arc<AtomicBool>,
    }

    pub fn start_tray(web_url: String, running: Arc<AtomicBool>) {
        let _ = TRAY_STATE.set(TrayState {
            web_url: Mutex::new(web_url.clone()),
            running,
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
                                open_in_browser(&url);
                            }
                        }
                    } else if event == WM_RBUTTONUP {
                        let mut pt: POINT = std::mem::zeroed();
                        GetCursorPos(&mut pt);
                        SetForegroundWindow(hwnd);

                        let hmenu = CreatePopupMenu();
                        let open_text: Vec<u16> = "打开 Web 控制台\0".encode_utf16().collect();
                        let exit_text: Vec<u16> = "退出 AgentHub Runner\0".encode_utf16().collect();

                        AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_WEB, open_text.as_ptr());
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
                    if id == ID_TRAY_OPEN_WEB {
                        if let Some(state) = TRAY_STATE.get() {
                            if let Ok(url) = state.web_url.lock() {
                                open_in_browser(&url);
                            }
                        }
                    } else if id == ID_TRAY_EXIT {
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

    fn open_in_browser(url: &str) {
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
    }
}

#[cfg(not(target_os = "windows"))]
mod non_windows_tray {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    pub fn start_tray(_web_url: String, _running: Arc<AtomicBool>) {}
}

#[cfg(target_os = "windows")]
pub use windows_tray::start_tray;

#[cfg(not(target_os = "windows"))]
pub use non_windows_tray::start_tray;
