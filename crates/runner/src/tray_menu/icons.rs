use super::*;

pub(crate) fn sample_bilinear_premul(frame: &[u8], u: f32, v: f32) -> (f32, f32, f32, f32) {
    let max_idx = (ICON_SIDE - 1) as f32;
    let x = u.clamp(0.0, max_idx);
    let y = v.clamp(0.0, max_idx);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(ICON_SIDE - 1);
    let y1 = (y0 + 1).min(ICON_SIDE - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;

    let fetch = |ix: usize, iy: usize| -> (f32, f32, f32, f32) {
        let o = (iy * ICON_SIDE + ix) * 4;
        let a = frame[o + 3] as f32 / 255.0;
        (
            (frame[o] as f32 / 255.0) * a,
            (frame[o + 1] as f32 / 255.0) * a,
            (frame[o + 2] as f32 / 255.0) * a,
            a,
        )
    };

    let c00 = fetch(x0, y0);
    let c10 = fetch(x1, y0);
    let c01 = fetch(x0, y1);
    let c11 = fetch(x1, y1);

    let b = |p00: f32, p10: f32, p01: f32, p11: f32| -> f32 {
        let top = p00 + (p10 - p00) * tx;
        let bot = p01 + (p11 - p01) * tx;
        top + (bot - top) * ty
    };

    (
        b(c00.0, c10.0, c01.0, c11.0),
        b(c00.1, c10.1, c01.1, c11.1),
        b(c00.2, c10.2, c01.2, c11.2),
        b(c00.3, c10.3, c01.3, c11.3),
    )
}

/// Blits a `64x64` RGBA icon from `tray_icons_64.bin.zst` into `rect` using 3x3
/// supersampled bilinear filtering (9x MSAA area downsampling), optionally rounded
/// by `clip_radius` and optionally re-tinted with `tint_rgb`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blit_icon_rgba(
    image: &mut [u8],
    w: i32,
    h: i32,
    card_dist: &[f32],
    rect: RectI,
    icon_id: TrayIconId,
    tint_rgb: Option<(u8, u8, u8)>,
    clip_radius: Option<f32>,
) {
    let Some(frame) = icon_frame(icon_id) else {
        return;
    };
    let rw = rect.width().max(1) as f32;
    let rh = rect.height().max(1) as f32;
    let x0 = rect.left.max(0);
    let y0 = rect.top.max(0);
    let x1 = rect.right.min(w);
    let y1 = rect.bottom.min(h);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let hw = rw * 0.5;
    let hh = rh * 0.5;
    let scale_x = ICON_SIDE as f32 / rw;
    let scale_y = ICON_SIDE as f32 / rh;
    let sub_offsets: [f32; 3] = [0.2, 0.5, 0.8];

    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let mut mask_cov = (0.5 - card_dist[i]).clamp(0.0, 1.0);
            if mask_cov <= 0.0 {
                continue;
            }
            if let Some(r) = clip_radius {
                let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, r);
                mask_cov *= (0.5 - d).clamp(0.0, 1.0);
                if mask_cov <= 0.0 {
                    continue;
                }
            }

            let mut sr = 0.0f32;
            let mut sg = 0.0f32;
            let mut sb = 0.0f32;
            let mut sa = 0.0f32;
            for oy in sub_offsets {
                let v = ((y - rect.top) as f32 + oy) * scale_y - 0.5;
                for ox in sub_offsets {
                    let u = ((x - rect.left) as f32 + ox) * scale_x - 0.5;
                    let (pr, pg, pb, pa) = sample_bilinear_premul(frame, u, v);
                    sr += pr;
                    sg += pg;
                    sb += pb;
                    sa += pa;
                }
            }
            let inv = 1.0 / 9.0;
            let a = (sa * inv * mask_cov).clamp(0.0, 1.0);
            if a <= 0.002 {
                continue;
            }
            let (fg_r, fg_g, fg_b) = if let Some((tr, tg, tb)) = tint_rgb {
                (tr as f32 / 255.0, tg as f32 / 255.0, tb as f32 / 255.0)
            } else {
                let raw_a = (sa * inv).max(1e-4);
                (
                    (sr * inv / raw_a).clamp(0.0, 1.0),
                    (sg * inv / raw_a).clamp(0.0, 1.0),
                    (sb * inv / raw_a).clamp(0.0, 1.0),
                )
            };

            let o = i * 4;
            let dst_r = image[o] as f32 / 255.0;
            let dst_g = image[o + 1] as f32 / 255.0;
            let dst_b = image[o + 2] as f32 / 255.0;
            image[o] = ((dst_r * (1.0 - a) + fg_r * a) * 255.0).round() as u8;
            image[o + 1] = ((dst_g * (1.0 - a) + fg_g * a) * 255.0).round() as u8;
            image[o + 2] = ((dst_b * (1.0 - a) + fg_b * a) * 255.0).round() as u8;
        }
    }
}
