use std::sync::mpsc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, CreateCoreWebView2EnvironmentWithOptions,
    GetAvailableCoreWebView2BrowserVersionString, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Controller2,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    NewWindowRequestedEventHandler, WindowCloseRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{COLORREF, E_POINTER, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, PostMessageW, WM_CLOSE};
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR};

pub(crate) struct ComApartment(bool);

impl ComApartment {
    pub(crate) fn init() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self(hr.is_ok())
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn url_base(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

pub(crate) fn bg_color_for_theme(dark: bool) -> COREWEBVIEW2_COLOR {
    if dark {
        COREWEBVIEW2_COLOR {
            A: 255,
            R: 0x1a,
            G: 0x1a,
            B: 0x1e,
        }
    } else {
        COREWEBVIEW2_COLOR {
            A: 255,
            R: 0xf4,
            G: 0xf5,
            B: 0xf8,
        }
    }
}

pub(crate) fn bg_colorref(dark: bool) -> COLORREF {
    if dark {
        COLORREF(0x001e_1a1a)
    } else {
        COLORREF(0x00f8_f5f4)
    }
}

fn text_colorref(dark: bool) -> COLORREF {
    if dark {
        COLORREF(0x00f1_eded)
    } else {
        COLORREF(0x0021_1c1c)
    }
}

pub(crate) unsafe fn apply_dwm_titlebar_theme(hwnd: HWND, dark: bool) {
    let use_dark = BOOL::from(dark);
    let caption = bg_colorref(dark).0;
    let text = text_colorref(dark).0;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const use_dark).cast(),
            size_of::<BOOL>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            (&raw const caption).cast(),
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            (&raw const text).cast(),
            size_of::<u32>() as u32,
        );
    }
}

pub(crate) unsafe fn has_webview2_runtime() -> bool {
    let mut ver = PWSTR::null();
    if unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut ver) }.is_err() {
        return false;
    }
    !take_pwstr(ver).trim().is_empty()
}

pub(crate) fn create_webview_controller(
    hwnd: HWND,
    user_data_dir: &std::path::Path,
) -> Result<ICoreWebView2Controller, windows::core::Error> {
    let user_data_h = HSTRING::from(user_data_dir.to_string_lossy().as_ref());
    let (env_tx, env_rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            CreateCoreWebView2EnvironmentWithOptions(PCWSTR::null(), &user_data_h, None, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |err, env| {
            err?;
            let _ = env_tx.send(env.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            Ok(())
        }),
    )
    .map_err(|e| match e {
        webview2_com::Error::WindowsError(we) => we,
        _ => windows::core::Error::from(E_POINTER),
    })?;
    let env = env_rx
        .recv()
        .map_err(|_| windows::core::Error::from(E_POINTER))??;

    let (ctrl_tx, ctrl_rx) = mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            env.CreateCoreWebView2Controller(hwnd, &handler)
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(move |err, ctrl| {
            err?;
            let _ = ctrl_tx.send(ctrl.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            Ok(())
        }),
    )
    .map_err(|e| match e {
        webview2_com::Error::WindowsError(we) => we,
        _ => windows::core::Error::from(E_POINTER),
    })?;
    ctrl_rx
        .recv()
        .map_err(|_| windows::core::Error::from(E_POINTER))?
}

pub(crate) fn configure_and_navigate(
    hwnd: HWND,
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    url: &str,
    dark: bool,
) -> windows::core::Result<()> {
    unsafe {
        if let Ok(ctrl2) = controller.cast::<ICoreWebView2Controller2>() {
            let _ = ctrl2.SetDefaultBackgroundColor(bg_color_for_theme(dark));
        }
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        controller.SetBounds(rect)?;
        controller.SetIsVisible(true)?;

        if let Ok(settings) = webview.Settings() {
            let _ = settings.SetIsStatusBarEnabled(false);
        }

        let mut close_token = 0i64;
        let close_hwnd = hwnd.0 as usize;
        let _ = webview.add_WindowCloseRequested(
            &WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                let target = HWND(close_hwnd as _);
                let _ = PostMessageW(Some(target), WM_CLOSE, WPARAM(0), LPARAM(0));
                Ok(())
            })),
            &mut close_token,
        );

        let base_origin = url_base(url).to_string();
        let mut new_win_token = 0i64;
        let _ = webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                if let Some(args) = args {
                    let _ = args.SetHandled(true);
                    let mut uri_pwstr = PWSTR::null();
                    if args.Uri(&mut uri_pwstr).is_ok() {
                        let uri = take_pwstr(uri_pwstr);
                        if !uri.is_empty() {
                            if url_base(&uri) == base_origin
                                && let Some(wv) = sender
                            {
                                let target = HSTRING::from(uri.as_str());
                                let _ = wv.Navigate(&target);
                            } else {
                                super::open_system_browser(&uri);
                            }
                        }
                    }
                }
                Ok(())
            })),
            &mut new_win_token,
        );

        let target = HSTRING::from(url);
        webview.Navigate(&target)?;
    }
    Ok(())
}
