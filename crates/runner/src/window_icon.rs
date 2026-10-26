//! Sharp taskbar icon for the Edge `--app` window.
//!
//! The desktop console runs in Microsoft Edge's `--app` mode, so Windows draws
//! its taskbar button from the favicon Chromium reports — and Chromium's
//! `Favicons` cache only ever stores 16x16 and 32x32 frames. On a 100%-DPI
//! display the taskbar slot is 24x24, so the shell had to resample a 32px
//! bitmap and the cube went soft.
//!
//! The taskbar draws `ICON_BIG` at exactly `24 * dpi / 96` physical pixels and
//! renders it 1:1 only when the icon's own pixel size matches that slot; any
//! other size is resampled. So we hand the window a native-size frame for each
//! scale factor the window can land on, and Windows has nothing to rescale.
//!
//! Chromium claims the window icon itself once its favicon arrives, so the
//! watcher compares the handle the window currently holds with the one it
//! installed and puts ours back when Chromium has taken it over.

use std::ptr::null;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateIconFromResourceEx, EnumWindows, GetClassNameW, GetWindowTextLengthW,
    GetWindowTextW, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsIconic, IsWindowVisible, LR_DEFAULTCOLOR,
    LoadImageW, PostMessageW, SW_RESTORE, SW_SHOW, SendMessageW, SetForegroundWindow, ShowWindow,
    WM_CLOSE,
};

/// Chromium's app window: class `Chrome_WidgetWin_1`, title from the page.
const EDGE_APP_CLASS: &str = "Chrome_WidgetWin_1";
const EDGE_APP_TITLE: &str = "AgentHub";

const EMBEDDED_ICO: &[u8] = include_bytes!("../resources/icon.ico");

/// Creates a native `HICON` for `size` directly from the embedded `icon.ico`
/// bytes by selecting the closest frame in the ICO directory.
pub fn create_hicon_from_embedded_ico(size: i32) -> *mut core::ffi::c_void {
    if EMBEDDED_ICO.len() < 6 {
        return core::ptr::null_mut();
    }
    let count = u16::from_le_bytes([EMBEDDED_ICO[4], EMBEDDED_ICO[5]]) as usize;
    if count == 0 || EMBEDDED_ICO.len() < 6 + count * 16 {
        return core::ptr::null_mut();
    }
    let target = size.max(16);
    let mut best_offset = 0usize;
    let mut best_len = 0usize;
    let mut best_dist = i32::MAX;

    for i in 0..count {
        let base = 6 + i * 16;
        let w = match EMBEDDED_ICO[base] {
            0 => 256i32,
            n => i32::from(n),
        };
        let bytes_in_res = u32::from_le_bytes([
            EMBEDDED_ICO[base + 8],
            EMBEDDED_ICO[base + 9],
            EMBEDDED_ICO[base + 10],
            EMBEDDED_ICO[base + 11],
        ]) as usize;
        let image_offset = u32::from_le_bytes([
            EMBEDDED_ICO[base + 12],
            EMBEDDED_ICO[base + 13],
            EMBEDDED_ICO[base + 14],
            EMBEDDED_ICO[base + 15],
        ]) as usize;
        if bytes_in_res == 0 || image_offset.saturating_add(bytes_in_res) > EMBEDDED_ICO.len() {
            continue;
        }
        let dist = (w - target).abs();
        if dist < best_dist {
            best_dist = dist;
            best_offset = image_offset;
            best_len = bytes_in_res;
        }
    }

    if best_len == 0 {
        return core::ptr::null_mut();
    }
    let slice = &EMBEDDED_ICO[best_offset..best_offset + best_len];
    unsafe {
        CreateIconFromResourceEx(
            slice.as_ptr(),
            best_len as u32,
            1,
            0x00030000,
            target,
            target,
            LR_DEFAULTCOLOR,
        )
    }
}

const WM_SETICON: u32 = 0x80;
const WM_GETICON: u32 = 0x7F;
/// The taskbar reads `ICON_BIG`; `ICON_SMALL` only affects the title bar and
/// the Alt+Tab row.
const ICON_SLOT_BIG: usize = ICON_BIG as usize;
const ICON_SLOT_SMALL: usize = ICON_SMALL as usize;

/// Resource id of the icon group embedded by `build.rs`.
const ICON_RESOURCE: usize = 1;

/// Frames we keep. The taskbar slot is `24 * dpi / 96` physical pixels, so the
/// sizes worth having are the ones that scale factor produces: 24 at 100%, 30
/// at 125%, 36 at 150%, 42 at 175% and 48 at 200%. 16 serves the small slot.
const SLOT_SIZES: [i32; 6] = [16, 24, 30, 36, 42, 48];

/// The frames, loaded once and kept for the life of the process. The handles
/// are only ever passed back to `SendMessageW`, never dereferenced, so holding
/// them across threads is safe.
static FRAMES: OnceLock<Vec<(i32, usize)>> = OnceLock::new();

/// The icon currently installed on the window, so a later pass can tell
/// whether Chromium has taken the window icon back. The handle is only ever
/// compared, never dereferenced, so it is safe to hold across threads.
static INSTALLED: Mutex<Option<(usize, u32, usize)>> = Mutex::new(None);

/// Guards against a second watcher thread: the desktop window can be reopened
/// from both the tray menu and the web console.
static WATCHER: OnceLock<()> = OnceLock::new();

/// Watches the Edge app window and keeps a native-size icon on it.
///
/// Starts a watcher thread that waits for the window to appear (Edge takes a
/// moment to create it) and then puts our icon back whenever Chromium claims
/// the window icon or the window moves to a differently scaled display.
pub fn watch_desktop_window_icon() {
    if WATCHER.get().is_some() {
        return;
    }
    let _ = WATCHER.set(());
    std::thread::spawn(|| {
        loop {
            let hwnds = find_all_edge_app_windows();
            if let Some(&primary) = hwnds.first() {
                unsafe { apply_icon(primary) };
                for &dup in &hwnds[1..] {
                    unsafe {
                        let _ = PostMessageW(dup, WM_CLOSE, 0, 0);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    });
}

/// If an AgentHub Edge `--app` window is already open, restores and focuses it
/// (closing any accidental duplicate frames) and returns `true`.
pub fn focus_existing_edge_app_window() -> bool {
    let hwnds = find_all_edge_app_windows();
    let Some(&primary) = hwnds.first() else {
        return false;
    };
    unsafe {
        for &dup in &hwnds[1..] {
            let _ = PostMessageW(dup, WM_CLOSE, 0, 0);
        }
        if IsIconic(primary) != 0 {
            let _ = ShowWindow(primary, SW_RESTORE);
        } else {
            let _ = ShowWindow(primary, SW_SHOW);
        }
        let _ = BringWindowToTop(primary);
        let _ = SetForegroundWindow(primary);
        apply_icon(primary);
    }
    true
}

/// Finds all visible Edge app windows belonging to AgentHub.
fn find_all_edge_app_windows() -> Vec<HWND> {
    let mut found: Vec<HWND> = Vec::new();
    unsafe {
        EnumWindows(
            Some(enum_all_edge_apps),
            &mut found as *mut Vec<HWND> as LPARAM,
        );
    }
    found
}

unsafe extern "system" fn enum_all_edge_apps(hwnd: HWND, lparam: LPARAM) -> i32 {
    let list = unsafe { &mut *(lparam as *mut Vec<HWND>) };
    if unsafe { is_edge_app_window(hwnd) } {
        list.push(hwnd);
    }
    1
}

unsafe fn is_edge_app_window(hwnd: HWND) -> bool {
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return false;
    }
    let mut class_buf = [0u16; 64];
    let class_len = unsafe { GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32) };
    if class_len <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&class_buf[..class_len as usize]);
    if class != EDGE_APP_CLASS {
        return false;
    }
    let title_len = unsafe { GetWindowTextLengthW(hwnd) };
    if title_len <= 0 {
        return false;
    }
    let mut title_buf = vec![0u16; title_len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32) };
    if n <= 0 {
        return false;
    }
    let title = String::from_utf16_lossy(&title_buf[..n as usize]);
    title.starts_with(EDGE_APP_TITLE)
}

/// Loads the embedded icon group once, keeping one frame per slot size.
fn frames() -> &'static [(i32, usize)] {
    FRAMES.get_or_init(|| unsafe {
        let hinstance = GetModuleHandleW(null());
        if hinstance.is_null() {
            return Vec::new();
        }
        let mut loaded = Vec::new();
        for size in SLOT_SIZES {
            let mut hicon = LoadImageW(
                hinstance,
                ICON_RESOURCE as *const u16,
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            );
            if hicon.is_null() {
                hicon = create_hicon_from_embedded_ico(size);
            }
            if !hicon.is_null() {
                loaded.push((size, hicon as usize));
            }
        }
        loaded
    })
}

/// Hands the window a native frame for the display it is on.
unsafe fn apply_icon(hwnd: HWND) {
    let frames = frames();
    if frames.is_empty() {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        return;
    }

    // Nothing to do while the window still holds the icon we installed and has
    // not moved to another display.
    let current = unsafe { SendMessageW(hwnd, WM_GETICON, ICON_SLOT_BIG as WPARAM, 0) };
    {
        let installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((h, d, big)) = installed.as_ref() {
            if *h == hwnd as usize && *d == dpi && *big == current as usize {
                return;
            }
        }
    }

    let slot = taskbar_slot_px(dpi);
    // The taskbar wants the frame whose size equals its slot exactly; the
    // closest frame is a fallback for scale factors we do not ship.
    let (_, big) = frames
        .iter()
        .min_by_key(|(size, _)| (*size - slot).abs())
        .copied()
        .unwrap();
    let small_target = (slot / 2).max(16);
    let (_, small) = frames
        .iter()
        .min_by_key(|(size, _)| (*size - small_target).abs())
        .copied()
        .unwrap();

    unsafe {
        SendMessageW(hwnd, WM_SETICON, ICON_SLOT_BIG as WPARAM, big as LPARAM);
        SendMessageW(hwnd, WM_SETICON, ICON_SLOT_SMALL as WPARAM, small as LPARAM);
    }

    let mut installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
    *installed = Some((hwnd as usize, dpi, big));
}

/// The physical pixel size of a taskbar icon slot on a display with `dpi`.
fn taskbar_slot_px(dpi: u32) -> i32 {
    // Windows uses a 24px slot at 96 DPI and scales it with the display.
    ((24_u64 * dpi as u64 + 48) / 96) as i32
}

#[cfg(test)]
mod tests {
    use super::taskbar_slot_px;

    #[test]
    fn slot_follows_display_scale() {
        assert_eq!(taskbar_slot_px(96), 24);
        assert_eq!(taskbar_slot_px(120), 30);
        assert_eq!(taskbar_slot_px(144), 36);
        assert_eq!(taskbar_slot_px(168), 42);
        assert_eq!(taskbar_slot_px(192), 48);
    }
}
