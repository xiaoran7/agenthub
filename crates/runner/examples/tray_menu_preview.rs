//! Paints the CC Switch-style tray popup card on screen and leaves it there so
//! its typography, tabs (`配置` / `用量`), agent pills and footer buttons can be
//! inspected directly.
//!
//! `cargo run -p agenthub-runner --example tray_menu_preview`

#[cfg(target_os = "windows")]
fn main() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, TranslateMessage,
    };

    unsafe {
        let hinstance =
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        agenthub_runner::tray_menu::show(
            hinstance,
            agenthub_runner::tray_menu::build_live_menu("127.0.0.1:4200".to_string()),
            |id| println!("chose {id}"),
        );
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0 as _, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}
