use super::super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_home_shapes(
    image: &mut [u8],
    w: i32,
    h: i32,
    dist: &[f32],
    menu: &Menu,
    tab: TrayTab,
    period: TrayUsagePeriod,
    layout: &CardLayout,
    hover: Option<HitTarget>,
    pal: &ThemePalette,
) {
    let s = layout.scale;

    // Segmented tabs track + active thumb + tab icons
    fill_round_rect(
        image,
        w,
        h,
        dist,
        layout.tabs_track,
        9.0 * s,
        pal.seg_track,
        Some(pal.row_border),
    );
    let active_tab_rect = match tab {
        TrayTab::Config => layout.tab_config,
        TrayTab::Usage => layout.tab_usage,
    };
    fill_round_rect(
        image,
        w,
        h,
        dist,
        active_tab_rect,
        7.0 * s,
        pal.seg_thumb,
        Some(pal.seg_thumb_border),
    );

    let tab_icon_sz = (14.0 * s).round() as i32;
    for (t_rect, t_kind, t_icon) in [
        (layout.tab_config, TrayTab::Config, TrayIconId::TabConfig),
        (layout.tab_usage, TrayTab::Usage, TrayIconId::TabUsage),
    ] {
        let is_on = tab == t_kind;
        let cx = (t_rect.left + t_rect.right) / 2;
        let cy = (t_rect.top + t_rect.bottom) / 2;
        let icon_left = cx - (22.0 * s).round() as i32;
        blit_icon_rgba(
            image,
            w,
            h,
            dist,
            RectI {
                left: icon_left,
                top: cy - tab_icon_sz / 2,
                right: icon_left + tab_icon_sz,
                bottom: cy - tab_icon_sz / 2 + tab_icon_sz,
            },
            t_icon,
            Some(if is_on { pal.accent } else { pal.sub }),
            None,
        );
    }

    // Usage period segmented pill bar
    if tab == TrayTab::Usage
        && let Some(track) = layout.period_track
    {
        fill_round_rect(
            image,
            w,
            h,
            dist,
            track,
            7.5 * s,
            pal.seg_track,
            Some(pal.row_border),
        );
        for (item_period, rect) in layout.period_rects {
            let is_active = item_period == period;
            let is_hover = hover == Some(HitTarget::Period(item_period));
            if is_active {
                fill_round_rect(
                    image,
                    w,
                    h,
                    dist,
                    rect,
                    5.5 * s,
                    pal.seg_thumb,
                    Some(pal.seg_thumb_border),
                );
            } else if is_hover {
                fill_round_rect(
                    image,
                    w,
                    h,
                    dist,
                    rect,
                    5.5 * s,
                    pal.row_hover,
                    Some(pal.row_hover_border),
                );
            }
        }
    }

    // Home rows
    for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
        let is_hover = hover == Some(HitTarget::Row(idx));
        let row_fill = if is_hover {
            pal.row_hover
        } else {
            pal.row_card
        };
        let row_bdr = if is_hover {
            pal.row_hover_border
        } else {
            pal.row_border
        };
        fill_round_rect(image, w, h, dist, *rect, 10.0 * s, row_fill, Some(row_bdr));

        let badge_sz = (34.0 * s).round() as i32;
        let bx = rect.left + (9.0 * s).round() as i32;
        let by = rect.top + (rect.height() - badge_sz) / 2;
        let badge = RectI {
            left: bx,
            top: by,
            right: bx + badge_sz,
            bottom: by + badge_sz,
        };

        if tab == TrayTab::Config
            && let Some(agent) = menu.agents.get(idx)
        {
            let badge_bg = if menu.dark {
                agent.badge_bg_dark
            } else {
                agent.badge_bg_light
            };
            if agent.full_bleed_icon {
                blit_icon_rgba(image, w, h, dist, badge, agent.icon_id, None, Some(8.5 * s));
            } else {
                fill_round_rect(image, w, h, dist, badge, 9.0 * s, badge_bg, None);
                blit_icon_rgba(image, w, h, dist, badge, agent.icon_id, None, None);
            }

            let pill_w = (52.0 * s).round() as i32;
            let pill_h = (20.0 * s).round() as i32;
            let chev_sz = (15.0 * s).round() as i32;
            let chev_right = rect.right - (7.0 * s).round() as i32;
            let cy = (rect.top + rect.bottom) / 2;
            let pill_right = chev_right - chev_sz - (4.0 * s).round() as i32;
            let pill_rect = RectI {
                left: pill_right - pill_w,
                top: cy - pill_h / 2,
                right: pill_right,
                bottom: cy - pill_h / 2 + pill_h,
            };
            let (pill_bg, dot_rgb) = if agent.route_mode == "loading" {
                (pal.loading_bg, pal.loading_dot)
            } else if agent.route_mode == "routed" {
                (pal.routed_bg, pal.routed_dot)
            } else {
                (pal.direct_bg, pal.direct_dot)
            };
            fill_round_rect(image, w, h, dist, pill_rect, 10.0 * s, pill_bg, None);
            fill_circle(
                image,
                w,
                h,
                dist,
                pill_rect.left as f32 + 9.0 * s,
                cy as f32,
                2.8 * s,
                dot_rgb,
            );

            blit_icon_rgba(
                image,
                w,
                h,
                dist,
                RectI {
                    left: chev_right - chev_sz,
                    top: cy - chev_sz / 2,
                    right: chev_right,
                    bottom: cy - chev_sz / 2 + chev_sz,
                },
                TrayIconId::ChevronRight,
                Some(if is_hover { pal.accent } else { pal.chevron }),
                None,
            );
        } else if tab == TrayTab::Config && menu.agents.is_empty() && idx == 0 {
            let badge_bg = if menu.is_loading {
                pal.loading_bg
            } else {
                pal.routed_bg
            };
            fill_round_rect(image, w, h, dist, badge, 9.0 * s, badge_bg, None);
            blit_icon_rgba(image, w, h, dist, badge, TrayIconId::AgentHub, None, None);

            let chev_sz = (15.0 * s).round() as i32;
            let chev_right = rect.right - (7.0 * s).round() as i32;
            let cy = (rect.top + rect.bottom) / 2;
            blit_icon_rgba(
                image,
                w,
                h,
                dist,
                RectI {
                    left: chev_right - chev_sz,
                    top: cy - chev_sz / 2,
                    right: chev_right,
                    bottom: cy - chev_sz / 2 + chev_sz,
                },
                TrayIconId::ChevronRight,
                Some(if is_hover { pal.accent } else { pal.chevron }),
                None,
            );
        } else if tab == TrayTab::Usage
            && let Some(urow) = menu.usage_rows.get(idx)
        {
            let badge_bg = if menu.dark {
                urow.badge_bg_dark
            } else {
                urow.badge_bg_light
            };
            if urow.full_bleed_icon {
                blit_icon_rgba(image, w, h, dist, badge, urow.icon_id, None, Some(8.5 * s));
            } else {
                fill_round_rect(image, w, h, dist, badge, 9.0 * s, badge_bg, None);
                blit_icon_rgba(image, w, h, dist, badge, urow.icon_id, None, None);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_detail_shapes(
    image: &mut [u8],
    w: i32,
    h: i32,
    dist: &[f32],
    menu: &Menu,
    idx: usize,
    layout: &CardLayout,
    hover: Option<HitTarget>,
    pal: &ThemePalette,
) {
    let s = layout.scale;
    let Some(agent) = menu.agents.get(idx) else {
        return;
    };

    // Back button (`← 返回`)
    if let Some(back_rect) = layout.detail_back_rect {
        let is_hover = hover == Some(HitTarget::DetailBack);
        fill_round_rect(
            image,
            w,
            h,
            dist,
            back_rect,
            8.0 * s,
            if is_hover {
                pal.row_hover
            } else {
                pal.row_card
            },
            Some(if is_hover {
                pal.row_hover_border
            } else {
                pal.row_border
            }),
        );
        let arrow_sz = (14.0 * s).round() as i32;
        let cy = (back_rect.top + back_rect.bottom) / 2;
        let arrow_left = back_rect.left + (10.0 * s).round() as i32;
        blit_icon_rgba(
            image,
            w,
            h,
            dist,
            RectI {
                left: arrow_left,
                top: cy - arrow_sz / 2,
                right: arrow_left + arrow_sz,
                bottom: cy - arrow_sz / 2 + arrow_sz,
            },
            TrayIconId::ArrowLeft,
            Some(if is_hover { pal.accent } else { pal.sub }),
            None,
        );
    }

    // Hero card
    if let Some(hero_rect) = layout.detail_hero_rect {
        fill_round_rect(
            image,
            w,
            h,
            dist,
            hero_rect,
            10.0 * s,
            pal.row_card,
            Some(pal.row_border),
        );
        let badge_sz = (36.0 * s).round() as i32;
        let bx = hero_rect.left + (10.0 * s).round() as i32;
        let by = hero_rect.top + (hero_rect.height() - badge_sz) / 2;
        let badge = RectI {
            left: bx,
            top: by,
            right: bx + badge_sz,
            bottom: by + badge_sz,
        };
        let badge_bg = if menu.dark {
            agent.badge_bg_dark
        } else {
            agent.badge_bg_light
        };
        if agent.full_bleed_icon {
            blit_icon_rgba(image, w, h, dist, badge, agent.icon_id, None, Some(8.5 * s));
        } else {
            fill_round_rect(image, w, h, dist, badge, 9.0 * s, badge_bg, None);
            blit_icon_rgba(image, w, h, dist, badge, agent.icon_id, None, None);
        }

        let pill_w = (56.0 * s).round() as i32;
        let pill_h = (22.0 * s).round() as i32;
        let pill_right = hero_rect.right - (10.0 * s).round() as i32;
        let cy = (hero_rect.top + hero_rect.bottom) / 2;
        let pill_rect = RectI {
            left: pill_right - pill_w,
            top: cy - pill_h / 2,
            right: pill_right,
            bottom: cy - pill_h / 2 + pill_h,
        };
        let (pill_bg, dot_rgb) = if agent.route_mode == "loading" {
            (pal.loading_bg, pal.loading_dot)
        } else if agent.route_mode == "routed" {
            (pal.routed_bg, pal.routed_dot)
        } else {
            (pal.direct_bg, pal.direct_dot)
        };
        fill_round_rect(image, w, h, dist, pill_rect, 11.0 * s, pill_bg, None);
        fill_circle(
            image,
            w,
            h,
            dist,
            pill_rect.left as f32 + 10.0 * s,
            cy as f32,
            2.8 * s,
            dot_rgb,
        );
    }

    // Endpoint & Source info card
    if let Some(info_rect) = layout.detail_info_rect {
        fill_round_rect(
            image,
            w,
            h,
            dist,
            info_rect,
            10.0 * s,
            pal.row_card,
            Some(pal.row_border),
        );
    }

    // Models card + rounded chips
    if let Some(models_rect) = layout.detail_models_rect {
        fill_round_rect(
            image,
            w,
            h,
            dist,
            models_rect,
            10.0 * s,
            pal.row_card,
            Some(pal.row_border),
        );
        for (chip_rect, _) in &layout.detail_model_chips {
            fill_round_rect(
                image,
                w,
                h,
                dist,
                *chip_rect,
                6.0 * s,
                pal.chip_bg,
                Some(pal.chip_border),
            );
        }
    }

    // Primary console CTA button
    if let Some(cta_rect) = layout.detail_console_rect {
        let is_hover = hover == Some(HitTarget::DetailOpenConsole(idx));
        fill_round_rect(
            image,
            w,
            h,
            dist,
            cta_rect,
            8.5 * s,
            if is_hover {
                pal.btn_primary_hover
            } else {
                pal.btn_primary_bg
            },
            Some(pal.btn_primary_border),
        );
        let btn_icon_sz = (14.0 * s).round() as i32;
        let cx = (cta_rect.left + cta_rect.right) / 2;
        let cy = (cta_rect.top + cta_rect.bottom) / 2;
        let icon_left = cx - (62.0 * s).round() as i32;
        blit_icon_rgba(
            image,
            w,
            h,
            dist,
            RectI {
                left: icon_left,
                top: cy - btn_icon_sz / 2,
                right: icon_left + btn_icon_sz,
                bottom: cy - btn_icon_sz / 2 + btn_icon_sz,
            },
            TrayIconId::BtnWindow,
            Some(pal.btn_primary_fg),
            None,
        );
    }
}

pub(crate) fn draw_focus_ring(
    image: &mut [u8],
    w: i32,
    h: i32,
    dist: &[f32],
    layout: &CardLayout,
    target: HitTarget,
    pal: &ThemePalette,
) {
    let Some(rect) = layout.target_rect(target) else {
        return;
    };
    let s = layout.scale;
    let radius = match target {
        HitTarget::Period(_) => 6.0 * s,
        HitTarget::Tab(_) => 7.5 * s,
        HitTarget::Row(_) => 10.0 * s,
        HitTarget::GearSettings | HitTarget::DetailBack => 8.0 * s,
        HitTarget::DetailOpenConsole(_) | HitTarget::FooterMain | HitTarget::FooterExit => 8.5 * s,
    };
    stroke_round_rect(image, w, h, dist, rect, radius, 1.8 * s, pal.focus_ring);
}
