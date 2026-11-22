use super::RectI;

pub(crate) fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

pub(crate) fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn fill_round_rect(
    image: &mut [u8],
    w: i32,
    h: i32,
    card_dist: &[f32],
    rect: RectI,
    radius: f32,
    fill: (u8, u8, u8),
    border: Option<(u8, u8, u8)>,
) {
    let x0 = rect.left.max(0);
    let y0 = rect.top.max(0);
    let x1 = rect.right.min(w);
    let y1 = rect.bottom.min(h);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let hw = (rect.right - rect.left) as f32 * 0.5;
    let hh = (rect.bottom - rect.top) as f32 * 0.5;

    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let card_cov = (0.5 - card_dist[i]).clamp(0.0, 1.0);
            if card_cov <= 0.0 {
                continue;
            }
            let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0) * card_cov;
            if cov <= 0.0 {
                continue;
            }
            let mut target = fill;
            if let Some(bc) = border {
                let b_edge = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                target = lerp(fill, bc, b_edge);
            }
            let o = i * 4;
            let mixed = lerp((image[o], image[o + 1], image[o + 2]), target, cov);
            image[o] = mixed.0;
            image[o + 1] = mixed.1;
            image[o + 2] = mixed.2;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn stroke_round_rect(
    image: &mut [u8],
    w: i32,
    h: i32,
    card_dist: &[f32],
    rect: RectI,
    radius: f32,
    stroke_w: f32,
    color: (u8, u8, u8),
) {
    let margin = stroke_w.ceil() as i32 + 2;
    let x0 = (rect.left - margin).max(0);
    let y0 = (rect.top - margin).max(0);
    let x1 = (rect.right + margin).min(w);
    let y1 = (rect.bottom + margin).min(h);
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let hw = (rect.right - rect.left) as f32 * 0.5;
    let hh = (rect.bottom - rect.top) as f32 * 0.5;
    let half = (stroke_w * 0.5).max(0.5);

    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let card_cov = (0.5 - card_dist[i]).clamp(0.0, 1.0);
            if card_cov <= 0.0 {
                continue;
            }
            let d = sd_round_rect(x as f32 + 0.5, y as f32 + 0.5, cx, cy, hw, hh, radius);
            let cov = (half + 0.5 - d.abs()).clamp(0.0, 1.0) * card_cov;
            if cov <= 0.0 {
                continue;
            }
            let o = i * 4;
            let mixed = lerp((image[o], image[o + 1], image[o + 2]), color, cov);
            image[o] = mixed.0;
            image[o + 1] = mixed.1;
            image[o + 2] = mixed.2;
        }
    }
}

pub(crate) fn fill_circle(
    image: &mut [u8],
    w: i32,
    h: i32,
    card_dist: &[f32],
    cx: f32,
    cy: f32,
    radius: f32,
    fill: (u8, u8, u8),
) {
    let x0 = ((cx - radius - 1.5).floor() as i32).max(0);
    let y0 = ((cy - radius - 1.5).floor() as i32).max(0);
    let x1 = ((cx + radius + 1.5).ceil() as i32).min(w);
    let y1 = ((cy + radius + 1.5).ceil() as i32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) as usize;
            let card_cov = (0.5 - card_dist[i]).clamp(0.0, 1.0);
            if card_cov <= 0.0 {
                continue;
            }
            let dx = (x as f32 + 0.5) - cx;
            let dy = (y as f32 + 0.5) - cy;
            let d = (dx * dx + dy * dy).sqrt() - radius;
            let cov = (0.5 - d).clamp(0.0, 1.0) * card_cov;
            if cov <= 0.0 {
                continue;
            }
            let o = i * 4;
            let mixed = lerp((image[o], image[o + 1], image[o + 2]), fill, cov);
            image[o] = mixed.0;
            image[o + 1] = mixed.1;
            image[o + 2] = mixed.2;
        }
    }
}
