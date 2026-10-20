//! Paints the tray menu on screen and leaves it there, so its type, spacing
//! and colours can be looked at without hunting for the tray icon.
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
            agenthub_runner::tray_menu::Menu {
                version: env!("CARGO_PKG_VERSION").to_string(),
                host: "127.0.0.1:4200".to_string(),
                items: vec![
                    agenthub_runner::tray_menu::Item {
                        id: 1,
                        label: "显示协同控制台",
                        danger: false,
                        separator_before: false,
                    },
                    agenthub_runner::tray_menu::Item {
                        id: 2,
                        label: "查看演进拓扑（网关）",
                        danger: false,
                        separator_before: false,
                    },
                    agenthub_runner::tray_menu::Item {
                        id: 3,
                        label: "在系统浏览器中打开",
                        danger: false,
                        separator_before: false,
                    },
                    agenthub_runner::tray_menu::Item {
                        id: 4,
                        label: "打开数据存储目录",
                        danger: false,
                        separator_before: true,
                    },
                    agenthub_runner::tray_menu::Item {
                        id: 5,
                        label: "退出 AgentHub",
                        danger: true,
                        separator_before: true,
                    },
                ],
            },
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
