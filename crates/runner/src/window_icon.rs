//! Crisp native taskbar and titlebar icons for the AgentHub desktop window.
//!
//! Windows draws the taskbar button (`ICON_BIG`) at `24 * dpi / 96` physical
//! pixels and renders it 1:1 only when the `HICON` frame size matches that slot.
//! We load a small table of exact scale sizes from the embedded ICO resource
//! (100%/125%/150%/175%/200%) and apply the exact frame on creation and on
//! `WM_DPICHANGED`.

use std::ptr::null;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
    SendMessageW,
};

const EMBEDDED_ICO: &[u8] = include_bytes!("../resources/icon.ico");
const WM_SETICON: u32 = 0x80;
const WM_GETICON: u32 = 0x7F;
const ICON_SLOT_BIG: usize = ICON_BIG as usize;
const ICON_SLOT_SMALL: usize = ICON_SMALL as usize;
const ICON_RESOURCE: usize = 1;
const SLOT_SIZES: [i32; 6] = [16, 24, 30, 36, 42, 48];

static FRAMES: OnceLock<Vec<(i32, usize)>> = OnceLock::new();
static INSTALLED: Mutex<Option<(usize, u32, usize)>> = Mutex::new(None);

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

/// Applies DPI-matched `ICON_BIG` and `ICON_SMALL` frames to the native WebView2 window.
///
/// # Safety
/// `hwnd` must be a valid Win32 window handle or null.
pub unsafe fn apply_native_icon(hwnd: HWND) {
    let frames = frames();
    if frames.is_empty() || hwnd.is_null() {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        return;
    }

    let current = unsafe { SendMessageW(hwnd, WM_GETICON, ICON_SLOT_BIG as WPARAM, 0) };
    {
        let installed = INSTALLED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((h, d, big)) = installed.as_ref()
            && *h == hwnd as usize
            && *d == dpi
            && *big == current as usize
        {
            return;
        }
    }

    let slot = taskbar_slot_px(dpi);
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
