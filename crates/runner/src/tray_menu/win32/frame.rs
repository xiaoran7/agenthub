use super::super::*;
use super::*;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};

pub(crate) struct Frame {
    pub(crate) session: u64,
    pub(crate) hover: Option<HitTarget>,
    pub(crate) focus: Option<HitTarget>,
    w: i32,
    h: i32,
    image: Vec<u8>,
    upload: Option<Upload>,
    dirty: bool,
}

impl Frame {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session: u64,
        menu: &Menu,
        view: TrayView,
        tab: TrayTab,
        period: TrayUsagePeriod,
        layout: &CardLayout,
        hover: Option<HitTarget>,
        focus: Option<HitTarget>,
    ) -> Self {
        let w = layout.win_w;
        let h = layout.win_h;
        let s = layout.scale;
        let pal = ThemePalette::for_theme(menu.dark);

        let card_cx = layout.card_x as f32 + layout.card_w as f32 / 2.0;
        let card_cy = layout.card_y as f32 + layout.card_h as f32 / 2.0;
        let card_hw = layout.card_w as f32 / 2.0;
        let card_hh = layout.card_h as f32 / 2.0;
        let radius = RADIUS as f32 * s;

        let mut dist = vec![0f32; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                dist[(y * w + x) as usize] = sd_round_rect(
                    x as f32 + 0.5,
                    y as f32 + 0.5,
                    card_cx,
                    card_cy,
                    card_hw,
                    card_hh,
                    radius,
                );
            }
        }
        let mask: Vec<f32> = dist.iter().map(|d| (0.5 - d).clamp(0.0, 1.0)).collect();
        let shadow = shadow_of(
            &mask,
            w,
            h,
            (10.0 * s).round().max(1.0) as usize,
            (4.0 * s).round() as i32,
            if menu.dark { 0.38 } else { 0.18 },
        );

        let mut image = vec![0u8; (w * h * 4) as usize];
        for (i, px) in image.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (r, g, b, a) = card_color(dist[i], shadow[i], &pal);
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = a;
        }

        // Header: AgentHub logo + status pill + gear button
        let pad_x = (PAD_X as f32 * s).round() as i32;
        let logo_sz = (22.0 * s).round() as i32;
        let logo_top = layout.card_y + ((HEADER_H as f32 * s).round() as i32 - logo_sz) / 2;
        let logo_rect = RectI {
            left: layout.card_x + pad_x,
            top: logo_top,
            right: layout.card_x + pad_x + logo_sz,
            bottom: logo_top + logo_sz,
        };
        blit_icon_rgba(
            &mut image,
            w,
            h,
            &dist,
            logo_rect,
            TrayIconId::AgentHub,
            None,
            None,
        );

        let status_pill_w = (54.0 * s).round() as i32;
        let status_pill_h = (18.0 * s).round() as i32;
        let status_pill_left = logo_rect.right + (88.0 * s).round() as i32;
        let status_pill_top =
            layout.card_y + ((HEADER_H as f32 * s).round() as i32 - status_pill_h) / 2;
        let status_pill_rect = RectI {
            left: status_pill_left,
            top: status_pill_top,
            right: status_pill_left + status_pill_w,
            bottom: status_pill_top + status_pill_h,
        };
        let (status_bg, status_dot) = if menu.is_loading {
            (pal.loading_bg, pal.loading_dot)
        } else {
            (pal.direct_bg, pal.direct_dot)
        };
        fill_round_rect(
            &mut image,
            w,
            h,
            &dist,
            status_pill_rect,
            9.0 * s,
            status_bg,
            None,
        );
        fill_circle(
            &mut image,
            w,
            h,
            &dist,
            status_pill_rect.left as f32 + 9.0 * s,
            (status_pill_rect.top + status_pill_rect.bottom) as f32 * 0.5,
            2.8 * s,
            status_dot,
        );

        let gear_bg = if hover == Some(HitTarget::GearSettings) {
            pal.row_hover
        } else {
            pal.row_card
        };
        let gear_border = if hover == Some(HitTarget::GearSettings) {
            pal.row_hover_border
        } else {
            pal.row_border
        };
        fill_round_rect(
            &mut image,
            w,
            h,
            &dist,
            layout.gear_rect,
            8.0 * s,
            gear_bg,
            Some(gear_border),
        );
        let gear_inset = (5.0 * s).round() as i32;
        blit_icon_rgba(
            &mut image,
            w,
            h,
            &dist,
            RectI {
                left: layout.gear_rect.left + gear_inset,
                top: layout.gear_rect.top + gear_inset,
                right: layout.gear_rect.right - gear_inset,
                bottom: layout.gear_rect.bottom - gear_inset,
            },
            TrayIconId::Gear,
            Some(if hover == Some(HitTarget::GearSettings) {
                pal.accent
            } else {
                pal.sub
            }),
            None,
        );

        match view {
            TrayView::Home => {
                render_home_shapes(
                    &mut image, w, h, &dist, menu, tab, period, layout, hover, &pal,
                );
            }
            TrayView::Detail(idx) => {
                render_detail_shapes(&mut image, w, h, &dist, menu, idx, layout, hover, &pal);
            }
        }

        // Footer buttons
        let main_hover = hover == Some(HitTarget::FooterMain);
        fill_round_rect(
            &mut image,
            w,
            h,
            &dist,
            layout.footer_main,
            8.5 * s,
            if main_hover {
                pal.btn_primary_hover
            } else {
                pal.btn_primary_bg
            },
            Some(pal.btn_primary_border),
        );
        let btn_icon_sz = (15.0 * s).round() as i32;
        let main_cx = (layout.footer_main.left + layout.footer_main.right) / 2;
        let main_cy = (layout.footer_main.top + layout.footer_main.bottom) / 2;
        let main_icon_left = main_cx - (42.0 * s).round() as i32;
        blit_icon_rgba(
            &mut image,
            w,
            h,
            &dist,
            RectI {
                left: main_icon_left,
                top: main_cy - btn_icon_sz / 2,
                right: main_icon_left + btn_icon_sz,
                bottom: main_cy - btn_icon_sz / 2 + btn_icon_sz,
            },
            TrayIconId::BtnWindow,
            Some(pal.btn_primary_fg),
            None,
        );

        let exit_hover = hover == Some(HitTarget::FooterExit);
        let exit_fg = if exit_hover {
            pal.btn_sec_hover_fg
        } else {
            pal.btn_sec_fg
        };
        fill_round_rect(
            &mut image,
            w,
            h,
            &dist,
            layout.footer_exit,
            8.5 * s,
            if exit_hover {
                pal.btn_sec_hover
            } else {
                pal.btn_sec_bg
            },
            Some(pal.btn_sec_border),
        );
        let exit_cx = (layout.footer_exit.left + layout.footer_exit.right) / 2;
        let exit_cy = (layout.footer_exit.top + layout.footer_exit.bottom) / 2;
        let exit_icon_left = exit_cx - (22.0 * s).round() as i32;
        blit_icon_rgba(
            &mut image,
            w,
            h,
            &dist,
            RectI {
                left: exit_icon_left,
                top: exit_cy - btn_icon_sz / 2,
                right: exit_icon_left + btn_icon_sz,
                bottom: exit_cy - btn_icon_sz / 2 + btn_icon_sz,
            },
            TrayIconId::BtnExit,
            Some(exit_fg),
            None,
        );

        if let Some(target) = focus {
            draw_focus_ring(&mut image, w, h, &dist, layout, target, &pal);
        }

        unsafe {
            draw_typography(
                &mut image, w, h, menu, view, tab, period, layout, hover, &pal,
            );
        }

        Self {
            session,
            hover,
            focus,
            w,
            h,
            image,
            upload: None,
            dirty: true,
        }
    }

    pub(crate) unsafe fn present(&mut self, hwnd: HWND, at: POINT) {
        unsafe {
            if self.upload.is_none() {
                self.upload = Upload::new(self.w, self.h);
                self.dirty = true;
            }
            let Some(upload) = self.upload.as_ref() else {
                return;
            };
            if self.dirty {
                upload.put(&self.image, 0, self.h);
                self.dirty = false;
            }
            upload.present(hwnd, at, self.w, self.h);
        }
    }
}

fn card_color(dist: f32, shadow: f32, pal: &ThemePalette) -> (u8, u8, u8, u8) {
    let cov = (0.5 - dist).clamp(0.0, 1.0);
    let alpha = cov + shadow * (1.0 - cov);
    if alpha <= 0.001 {
        return (0, 0, 0, 0);
    }
    let border = (1.0 - (dist + 0.5).abs()).clamp(0.0, 1.0);
    let rgb = lerp(pal.surface, pal.surface_border, border);
    (rgb.0, rgb.1, rgb.2, (alpha * 255.0).round() as u8)
}

struct Upload {
    hdc: HDC,
    dib: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Upload {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        if w <= 0 || h <= 0 {
            return None;
        }
        (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
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
            Some(Self {
                hdc,
                dib,
                old,
                bits: bits as *mut u8,
                w,
                h,
            })
        }
    }

    unsafe fn put(&self, image: &[u8], y0: i32, y1: i32) {
        if self.w <= 0 || self.h <= 0 || self.bits.is_null() {
            return;
        }
        let y0 = y0.clamp(0, self.h);
        let y1 = y1.clamp(y0, self.h);
        let Some(total_bytes) = (self.w as usize)
            .checked_mul(self.h as usize)
            .and_then(|px| px.checked_mul(4))
        else {
            return;
        };
        let start = (y0 as usize) * (self.w as usize) * 4;
        let end = (y1 as usize) * (self.w as usize) * 4;
        if end <= start || end > total_bytes || end > image.len() {
            return;
        }
        unsafe {
            let dst = std::slice::from_raw_parts_mut(self.bits.add(start), end - start);
            for (px, src) in dst
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(image[start..end].as_chunks::<4>().0)
            {
                let a = src[3] as u32;
                px[0] = ((src[2] as u32 * a + 127) / 255) as u8;
                px[1] = ((src[1] as u32 * a + 127) / 255) as u8;
                px[2] = ((src[0] as u32 * a + 127) / 255) as u8;
                px[3] = src[3];
            }
        }
    }

    pub(crate) unsafe fn present(&self, hwnd: HWND, at: POINT, w: i32, h: i32) {
        unsafe {
            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: 1,
            };
            let src_pt = POINT { x: 0, y: 0 };
            let size = SIZE { cx: w, cy: h };
            let screen = GetDC(0 as _);
            UpdateLayeredWindow(
                hwnd, screen, &at, &size, self.hdc, &src_pt, 0, &blend, ULW_ALPHA,
            );
            ReleaseDC(0 as _, screen);
        }
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old);
            DeleteObject(self.dib as HGDIOBJ);
            DeleteDC(self.hdc);
        }
    }
}

fn shadow_of(mask: &[f32], w: i32, h: i32, radius: usize, dy: i32, strength: f32) -> Vec<f32> {
    if radius == 0 {
        return vec![0.0; mask.len()];
    }
    let mut a = vec![0f32; mask.len()];
    for y in 0..h {
        for x in 0..w {
            let sy = y - dy;
            if sy >= 0 && sy < h {
                a[(y * w + x) as usize] = mask[(sy * w + x) as usize];
            }
        }
    }
    let r = radius.max(1) as i32;
    let mut tmp = vec![0f32; a.len()];
    for _ in 0..2 {
        box_h(&a, &mut tmp, w, h, r);
        box_v(&tmp, &mut a, w, h, r);
    }
    for v in a.iter_mut() {
        *v *= strength;
    }
    a
}

fn box_h(src: &[f32], dst: &mut [f32], w: i32, h: i32, r: i32) {
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let base = y * w;
        let mut sum: f32 = 0.0;
        for x in -r..=r {
            if x >= 0 && x < w {
                sum += src[(base + x) as usize];
            }
        }
        for x in 0..w {
            dst[(base + x) as usize] = sum / n;
            let out = x - r;
            let inn = x + r + 1;
            if out >= 0 {
                sum -= src[(base + out) as usize];
            }
            if inn < w {
                sum += src[(base + inn) as usize];
            }
        }
    }
}

fn box_v(src: &[f32], dst: &mut [f32], w: i32, h: i32, r: i32) {
    let n = (2 * r + 1) as f32;
    for x in 0..w {
        let mut sum: f32 = 0.0;
        for y in -r..=r {
            if y >= 0 && y < h {
                sum += src[(y * w + x) as usize];
            }
        }
        for y in 0..h {
            dst[(y * w + x) as usize] = sum / n;
            let out = y - r;
            let inn = y + r + 1;
            if out >= 0 {
                sum -= src[(out * w + x) as usize];
            }
            if inn < h {
                sum += src[(inn * w + x) as usize];
            }
        }
    }
}
