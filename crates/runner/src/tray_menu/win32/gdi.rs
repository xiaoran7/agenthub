use super::super::*;
use super::*;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_NATURAL_QUALITY, CLIP_DEFAULT_PRECIS,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_RIGHT,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_NORMAL, FW_SEMIBOLD, FillRect,
    GetDC, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};

pub(crate) struct Gdi {
    hdc: HDC,
    dib: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    len: usize,
    active: bool,
}

impl Drop for Gdi {
    fn drop(&mut self) {
        self.cleanup();
    }
}

impl Gdi {
    pub(crate) unsafe fn new(px: &[u8], w: i32, h: i32) -> Option<Self> {
        if w <= 0 || h <= 0 {
            return None;
        }
        let expected_len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        if px.len() != expected_len {
            return None;
        }
        unsafe {
            let mut bmi: BITMAPINFO = zeroed();
            bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h;
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let screen = GetDC(0 as _);
            let hdc = CreateCompatibleDC(screen);
            ReleaseDC(0 as _, screen);
            if hdc.is_null() {
                return None;
            }
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(
                hdc,
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if dib.is_null() || bits.is_null() {
                if !dib.is_null() {
                    DeleteObject(dib as HGDIOBJ);
                }
                DeleteDC(hdc);
                return None;
            }
            let old = SelectObject(hdc, dib as HGDIOBJ);
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, px.len());
            for (d, s) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(px.as_chunks::<4>().0)
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 0xff;
            }
            Some(Self {
                hdc,
                dib,
                old,
                bits: bits as *mut u8,
                len: expected_len,
                active: true,
            })
        }
    }

    fn cleanup(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        unsafe {
            SelectObject(self.hdc, self.old);
            DeleteObject(self.dib as HGDIOBJ);
            DeleteDC(self.hdc);
        }
    }

    pub(crate) unsafe fn finish(mut self, px: &mut [u8]) {
        if !self.active || px.len() != self.len {
            self.cleanup();
            return;
        }
        unsafe {
            let src = std::slice::from_raw_parts(self.bits, px.len());
            for (d, s) in px
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src.as_chunks::<4>().0)
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
            }
            self.cleanup();
        }
    }

    pub(crate) unsafe fn fill(&self, x: i32, y: i32, w: i32, h: i32, c: (u8, u8, u8)) {
        unsafe {
            let brush = CreateSolidBrush(colorref(c));
            let rect = RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            FillRect(self.hdc, &rect, brush);
            DeleteObject(brush as HGDIOBJ);
        }
    }

    pub(crate) unsafe fn text(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
        unsafe {
            self.draw_with_flags(s, font, c, rect, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
        }
    }

    pub(crate) unsafe fn text_ellipsis(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
        unsafe {
            self.draw_with_flags(
                s,
                font,
                c,
                rect,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        }
    }

    pub(crate) unsafe fn text_center(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
        unsafe {
            self.draw_with_flags(
                s,
                font,
                c,
                rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
        }
    }

    pub(crate) unsafe fn text_center_ellipsis(
        &self,
        s: &str,
        font: HFONT,
        c: (u8, u8, u8),
        rect: RECT,
    ) {
        unsafe {
            self.draw_with_flags(
                s,
                font,
                c,
                rect,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        }
    }

    pub(crate) unsafe fn text_right(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
        unsafe {
            self.draw_with_flags(
                s,
                font,
                c,
                rect,
                DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
            );
        }
    }

    unsafe fn draw_with_flags(
        &self,
        s: &str,
        font: HFONT,
        c: (u8, u8, u8),
        mut rect: RECT,
        flags: u32,
    ) {
        unsafe {
            let old = SelectObject(self.hdc, font as HGDIOBJ);
            SetBkMode(self.hdc, TRANSPARENT as i32);
            SetTextColor(self.hdc, colorref(c));
            let mut text: Vec<u16> = s.encode_utf16().collect();
            DrawTextW(
                self.hdc,
                text.as_mut_ptr(),
                text.len() as i32,
                &mut rect,
                flags,
            );
            SelectObject(self.hdc, old);
        }
    }
}

/// Creates a ClearType subpixel-antialiased font at physical pixel height `(size * scale).round()`.
fn font(scale: f32, size: f32, weight: u32, face: &str) -> HFONT {
    let mut name: Vec<u16> = face.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        CreateFontW(
            -((size * scale).round() as i32),
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_NATURAL_QUALITY,
            DEFAULT_PITCH as u32,
            name.as_mut_ptr(),
        )
    }
}

pub(crate) struct FontSet {
    pub(crate) brand: HFONT,
    pub(crate) tab: HFONT,
    pub(crate) period: HFONT,
    pub(crate) name_en: HFONT,
    pub(crate) name_cn: HFONT,
    pub(crate) sub: HFONT,
    pub(crate) pill: HFONT,
    pub(crate) chip: HFONT,
    pub(crate) btn: HFONT,
}

impl FontSet {
    fn new(s: f32) -> Self {
        Self {
            brand: font(s, 15.5, FW_SEMIBOLD, "Segoe UI Semibold"),
            tab: font(s, 13.0, FW_NORMAL, "Microsoft YaHei UI"),
            period: font(s, 11.5, FW_NORMAL, "Microsoft YaHei UI"),
            name_en: font(s, 14.0, FW_SEMIBOLD, "Segoe UI Semibold"),
            name_cn: font(s, 13.5, FW_NORMAL, "Microsoft YaHei UI"),
            sub: font(s, 12.0, FW_NORMAL, "Microsoft YaHei UI"),
            pill: font(s, 11.0, FW_NORMAL, "Microsoft YaHei UI"),
            chip: font(s, 11.0, FW_NORMAL, "Segoe UI"),
            btn: font(s, 13.0, FW_NORMAL, "Microsoft YaHei UI"),
        }
    }
}

impl Drop for FontSet {
    fn drop(&mut self) {
        unsafe {
            for f in [
                self.brand,
                self.tab,
                self.period,
                self.name_en,
                self.name_cn,
                self.sub,
                self.pill,
                self.chip,
                self.btn,
            ] {
                DeleteObject(f as HGDIOBJ);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn draw_typography(
    px: &mut [u8],
    w: i32,
    h: i32,
    menu: &Menu,
    view: TrayView,
    tab: TrayTab,
    period: TrayUsagePeriod,
    layout: &CardLayout,
    hover: Option<HitTarget>,
    pal: &ThemePalette,
) {
    unsafe {
        let Some(g) = Gdi::new(px, w, h) else {
            return;
        };
        let s = layout.scale;
        let pad_x = (PAD_X as f32 * s).round() as i32;
        let fonts = FontSet::new(s);

        // Header
        let logo_sz = (22.0 * s).round() as i32;
        let header_text_left = layout.card_x + pad_x + logo_sz + (8.0 * s).round() as i32;
        let header_bottom = layout.card_y + (HEADER_H as f32 * s).round() as i32;
        g.text(
            "AgentHub",
            fonts.brand,
            pal.text,
            RECT {
                left: header_text_left,
                top: layout.card_y,
                right: header_text_left + (84.0 * s).round() as i32,
                bottom: header_bottom,
            },
        );

        let status_pill_left = layout.card_x + pad_x + logo_sz + (88.0 * s).round() as i32;
        let status_pill_w = (54.0 * s).round() as i32;
        let (status_text, status_fg) = if menu.is_loading {
            ("加载中", pal.loading_fg)
        } else {
            ("运行中", pal.direct_fg)
        };
        g.text(
            status_text,
            fonts.pill,
            status_fg,
            RECT {
                left: status_pill_left + (15.0 * s).round() as i32,
                top: layout.card_y,
                right: status_pill_left + status_pill_w,
                bottom: header_bottom,
            },
        );

        match view {
            TrayView::Home => draw_home_text(&g, &fonts, menu, tab, period, layout, pal),
            TrayView::Detail(idx) => draw_detail_text(&g, &fonts, menu, idx, layout, pal),
        }

        // Footer hairline + buttons
        g.fill(
            layout.card_x + pad_x,
            layout.sep_y,
            layout.card_w - 2 * pad_x,
            1,
            pal.hairline,
        );
        g.text_center(
            "打开主窗口",
            fonts.btn,
            pal.btn_primary_fg,
            RECT {
                left: layout.footer_main.left + (12.0 * s).round() as i32,
                top: layout.footer_main.top,
                right: layout.footer_main.right,
                bottom: layout.footer_main.bottom,
            },
        );
        let exit_fg = if hover == Some(HitTarget::FooterExit) {
            pal.btn_sec_hover_fg
        } else {
            pal.btn_sec_fg
        };
        g.text_center(
            "退出",
            fonts.btn,
            exit_fg,
            RECT {
                left: layout.footer_exit.left + (10.0 * s).round() as i32,
                top: layout.footer_exit.top,
                right: layout.footer_exit.right,
                bottom: layout.footer_exit.bottom,
            },
        );

        g.finish(px);
    }
}
