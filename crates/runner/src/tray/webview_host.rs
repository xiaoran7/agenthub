use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use anyhow::Result;
use webview2_com::ExecuteScriptCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Controller2,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, HBRUSH, InvalidateRect, UpdateWindow,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GA_ROOT, GWLP_USERDATA,
    GetAncestor, GetClientRect, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    IDC_ARROW, IsChild, IsIconic, IsWindowVisible, IsZoomed, KillTimer, LoadCursorW, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SIZE_MINIMIZED,
    SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WA_INACTIVE,
    WINDOW_EX_STYLE, WM_ACTIVATE, WM_CLOSE, WM_COPYDATA, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOVE, WM_MOVING, WM_SETFOCUS,
    WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{HSTRING, Interface, PCWSTR};

use super::desktop_window::{
    COPYDATA_NAVIGATE_URL, THEME_CHANGED_MSG_NAME, WEBVIEW_WINDOW_CLASS, agenthub_home_dir,
    build_hash_navigation_script, remember_window_rect, save_window_size,
};
use super::webview_com::{
    ComApartment, apply_dwm_titlebar_theme, bg_color_for_theme, bg_colorref,
    configure_and_navigate, create_webview_controller, has_webview2_runtime, url_base,
};
use super::webview_idle::{IdleAction, IdleTracker};

const TIMER_IDLE_ID: usize = 2001;

struct UiHostState {
    agenthub_dir: PathBuf,
    base_url: RefCell<String>,
    dark: Cell<bool>,
    theme_msg: u32,
    idle: RefCell<IdleTracker>,
    controller: RefCell<Option<ICoreWebView2Controller>>,
    webview: RefCell<Option<ICoreWebView2>>,
}

unsafe fn is_window_active_for_idle(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        fg == hwnd || IsChild(hwnd, fg).as_bool() || GetAncestor(fg, GA_ROOT) == hwnd
    }
}

unsafe fn host_state<'a>(hwnd: HWND) -> Option<&'a UiHostState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const UiHostState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

fn release_webview_handles(state: &UiHostState) {
    let _ = state.webview.borrow_mut().take();
    if let Some(ctrl) = state.controller.borrow_mut().take() {
        unsafe {
            let _ = ctrl.Close();
        }
    }
}

fn handle_copydata_navigation(hwnd: HWND, state: &UiHostState, lparam: LPARAM) -> bool {
    let cds = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
    if cds.dwData != COPYDATA_NAVIGATE_URL || cds.lpData.is_null() {
        return false;
    }
    let slice = unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) };
    let Ok(url) = std::str::from_utf8(slice) else {
        return false;
    };
    if !super::is_safe_http_url(url) {
        return false;
    }
    let same_base = url_base(url) == url_base(&state.base_url.borrow());
    if !same_base {
        return false;
    }
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
    state.idle.borrow_mut().reset();

    let wv = state.webview.borrow().clone();
    if let Some(webview) = wv {
        if let Some((_, hash)) = url.split_once('#') {
            let script = HSTRING::from(build_hash_navigation_script(hash));
            let handler = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
            let _ = unsafe { webview.ExecuteScript(&script, &handler) };
        } else {
            let target = HSTRING::from(url);
            let _ = unsafe { webview.Navigate(&target) };
        }
    }
    true
}

unsafe extern "system" fn ui_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let Some(state) = (unsafe { host_state(hwnd) }) else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };

    if state.theme_msg != 0 && msg == state.theme_msg {
        let dark = wparam.0 != 0;
        state.dark.set(dark);
        unsafe {
            apply_dwm_titlebar_theme(hwnd, dark);
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
        if let Some(ctrl) = state.controller.borrow().clone()
            && let Ok(ctrl2) = ctrl.cast::<ICoreWebView2Controller2>()
        {
            let _ = unsafe { ctrl2.SetDefaultBackgroundColor(bg_color_for_theme(dark)) };
        }
        return LRESULT(0);
    }

    match msg {
        WM_ERASEBKGND => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as _);
            let mut rect = RECT::default();
            unsafe {
                let _ = GetClientRect(hwnd, &mut rect);
                let brush = CreateSolidBrush(bg_colorref(state.dark.get()));
                FillRect(hdc, &rect, brush);
                let _ = DeleteObject(brush.into());
            }
            LRESULT(1)
        }
        WM_SIZE => {
            if wparam.0 as u32 != SIZE_MINIMIZED {
                state.idle.borrow_mut().reset();
            }
            if let Some(ctrl) = state.controller.borrow().clone() {
                let mut rect = RECT::default();
                unsafe {
                    let _ = GetClientRect(hwnd, &mut rect);
                    let _ = ctrl.SetBounds(rect);
                }
            }
            LRESULT(0)
        }
        WM_MOVE => {
            if let Some(ctrl) = state.controller.borrow().clone() {
                let _ = unsafe { ctrl.NotifyParentWindowPositionChanged() };
            }
            LRESULT(0)
        }
        WM_MOVING => {
            if let Some(ctrl) = state.controller.borrow().clone() {
                let _ = unsafe { ctrl.NotifyParentWindowPositionChanged() };
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                crate::window_icon::apply_native_icon(hwnd.0 as _);
            }
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE {
                state.idle.borrow_mut().reset();
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETFOCUS => {
            state.idle.borrow_mut().reset();
            if let Some(ctrl) = state.controller.borrow().clone() {
                let _ = unsafe { ctrl.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE | WM_LBUTTONDOWN | WM_MOUSEMOVE | WM_KEYDOWN => {
            state.idle.borrow_mut().reset();
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_COPYDATA => {
            if handle_copydata_navigation(hwnd, state, lparam) {
                LRESULT(1)
            } else {
                LRESULT(0)
            }
        }
        WM_TIMER if wparam.0 == TIMER_IDLE_ID => {
            let active = unsafe { is_window_active_for_idle(hwnd) };
            let action = state.idle.borrow_mut().step(active);
            if action == IdleAction::Release {
                let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                if !IsIconic(hwnd).as_bool() && !IsZoomed(hwnd).as_bool() {
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() {
                        save_window_size(
                            &state.agenthub_dir,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                        );
                    }
                }
                let _ = KillTimer(Some(hwnd), TIMER_IDLE_ID);
            }
            release_webview_handles(state);
            let _ = unsafe { DestroyWindow(hwnd) };
            LRESULT(0)
        }
        WM_DESTROY => {
            release_webview_handles(state);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn run_ui_window(url: &str, idle_seconds: u64, title: &str) -> Result<()> {
    super::enable_per_monitor_dpi_awareness();
    let _com = ComApartment::init();

    if unsafe { !has_webview2_runtime() } {
        super::open_system_browser(url);
        return Ok(());
    }

    let agenthub_dir = agenthub_home_dir();
    let user_data_dir = agenthub_dir.join("webview2_profile");
    let _ = std::fs::create_dir_all(&user_data_dir);
    let (x, y, win_w, win_h) = remember_window_rect(&agenthub_dir);
    let dark = crate::tray::is_dark_theme();

    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
    let hinstance = HINSTANCE(hmodule.0);
    let class_h = HSTRING::from(WEBVIEW_WINDOW_CLASS);
    let title_h = HSTRING::from(title);
    let theme_msg_h = HSTRING::from(THEME_CHANGED_MSG_NAME);
    let theme_msg = unsafe { RegisterWindowMessageW(&theme_msg_h) };

    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(ui_wnd_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        hbrBackground: HBRUSH::default(),
        lpszClassName: PCWSTR(class_h.as_ptr()),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            &class_h,
            &title_h,
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            win_w as i32,
            win_h as i32,
            None,
            None,
            Some(hinstance),
            None,
        )
    }?;

    unsafe {
        crate::window_icon::apply_native_icon(hwnd.0 as _);
        apply_dwm_titlebar_theme(hwnd, dark);
    }

    let state = Box::new(UiHostState {
        agenthub_dir,
        base_url: RefCell::new(url_base(url).to_string()),
        dark: Cell::new(dark),
        theme_msg,
        idle: RefCell::new(IdleTracker::new(idle_seconds)),
        controller: RefCell::new(None),
        webview: RefCell::new(None),
    });
    let state_ptr = Box::into_raw(state);
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize) };

    let init_result = (|| -> windows::core::Result<()> {
        let controller = create_webview_controller(hwnd, &user_data_dir)?;
        let webview = unsafe { controller.CoreWebView2()? };
        configure_and_navigate(hwnd, &controller, &webview, url, dark)?;
        let st = unsafe { &*state_ptr };
        *st.controller.borrow_mut() = Some(controller);
        *st.webview.borrow_mut() = Some(webview);
        Ok(())
    })();

    if init_result.is_err() {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(hwnd);
            drop(Box::from_raw(state_ptr));
        }
        super::open_system_browser(url);
        return Ok(());
    }

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if let Some(ctrl) = (*state_ptr).controller.borrow().clone() {
            let _ = ctrl.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
        }
        let _ = SetTimer(Some(hwnd), TIMER_IDLE_ID, 1000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(state_ptr));
    }
    Ok(())
}
