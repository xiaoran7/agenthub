use super::super::*;
use super::*;

use windows_sys::Win32::Foundation::RECT;

pub(crate) unsafe fn draw_home_text(
    g: &Gdi,
    fonts: &FontSet,
    menu: &Menu,
    tab: TrayTab,
    period: TrayUsagePeriod,
    layout: &CardLayout,
    pal: &ThemePalette,
) {
    unsafe {
        let s = layout.scale;
        let tab_text_shift = (9.0 * s).round() as i32;
        g.text_center(
            "配置",
            fonts.tab,
            if tab == TrayTab::Config {
                pal.text
            } else {
                pal.sub
            },
            RECT {
                left: layout.tab_config.left + tab_text_shift,
                top: layout.tab_config.top,
                right: layout.tab_config.right,
                bottom: layout.tab_config.bottom,
            },
        );
        g.text_center(
            "用量",
            fonts.tab,
            if tab == TrayTab::Usage {
                pal.text
            } else {
                pal.sub
            },
            RECT {
                left: layout.tab_usage.left + tab_text_shift,
                top: layout.tab_usage.top,
                right: layout.tab_usage.right,
                bottom: layout.tab_usage.bottom,
            },
        );

        if tab == TrayTab::Usage && layout.period_track.is_some() {
            for (item_period, rect) in layout.period_rects {
                g.text_center(
                    item_period.label(),
                    fonts.period,
                    if item_period == period {
                        pal.text
                    } else {
                        pal.sub
                    },
                    to_win_rect(rect),
                );
            }
        }

        match tab {
            TrayTab::Config => {
                if menu.agents.is_empty() {
                    if let Some((rect, _)) = layout.row_rects.first() {
                        let text_left = rect.left + (51.0 * s).round() as i32;
                        let chev_right = rect.right - (7.0 * s).round() as i32;
                        let chev_sz = (15.0 * s).round() as i32;
                        let text_right = chev_right - chev_sz - (6.0 * s).round() as i32;
                        let (title, sub) = if menu.is_loading {
                            ("正在扫描本地 Agent...", "读取实时路由与本机配置")
                        } else {
                            ("未发现本地 Agent", "点击打开主控台查看或接入")
                        };
                        g.text_ellipsis(
                            title,
                            fonts.name_cn,
                            pal.text,
                            RECT {
                                left: text_left,
                                top: rect.top + (5.0 * s).round() as i32,
                                right: text_right,
                                bottom: rect.top + (rect.height() * 54) / 100,
                            },
                        );
                        g.text_ellipsis(
                            sub,
                            fonts.sub,
                            pal.sub,
                            RECT {
                                left: text_left,
                                top: rect.top + (rect.height() * 48) / 100,
                                right: text_right,
                                bottom: rect.bottom - (4.0 * s).round() as i32,
                            },
                        );
                    }
                } else {
                    for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                        if let Some(agent) = menu.agents.get(idx) {
                            let text_left = rect.left + (51.0 * s).round() as i32;
                            let chev_right = rect.right - (7.0 * s).round() as i32;
                            let chev_sz = (15.0 * s).round() as i32;
                            let pill_w = (52.0 * s).round() as i32;
                            let pill_right = chev_right - chev_sz - (4.0 * s).round() as i32;
                            let pill_left = pill_right - pill_w;

                            let name_font = if agent.display_name.is_ascii() {
                                fonts.name_en
                            } else {
                                fonts.name_cn
                            };
                            g.text_ellipsis(
                                &agent.display_name,
                                name_font,
                                pal.text,
                                RECT {
                                    left: text_left,
                                    top: rect.top + (5.0 * s).round() as i32,
                                    right: pill_left - (6.0 * s).round() as i32,
                                    bottom: rect.top + (rect.height() * 54) / 100,
                                },
                            );
                            g.text_ellipsis(
                                &agent.provider_label,
                                fonts.sub,
                                pal.sub,
                                RECT {
                                    left: text_left,
                                    top: rect.top + (rect.height() * 48) / 100,
                                    right: pill_left - (6.0 * s).round() as i32,
                                    bottom: rect.bottom - (4.0 * s).round() as i32,
                                },
                            );

                            let (badge_label, fg) = if agent.route_mode == "loading" {
                                ("加载中", pal.loading_fg)
                            } else if agent.route_mode == "routed" {
                                ("路由", pal.routed_fg)
                            } else {
                                ("直连", pal.direct_fg)
                            };
                            g.text(
                                badge_label,
                                fonts.pill,
                                fg,
                                RECT {
                                    left: pill_left + (16.0 * s).round() as i32,
                                    top: rect.top,
                                    right: pill_right,
                                    bottom: rect.bottom,
                                },
                            );
                        }
                    }
                }
            }
            TrayTab::Usage => {
                for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                    if let Some(urow) = menu.usage_rows.get(idx) {
                        let text_left = rect.left + (51.0 * s).round() as i32;
                        let right_edge = rect.right - (12.0 * s).round() as i32;
                        let is_ascii = urow.label.is_ascii();
                        g.text_ellipsis(
                            &urow.label,
                            if is_ascii {
                                fonts.name_en
                            } else {
                                fonts.name_cn
                            },
                            pal.text,
                            RECT {
                                left: text_left,
                                top: rect.top + (5.0 * s).round() as i32,
                                right: right_edge - (72.0 * s).round() as i32,
                                bottom: rect.top + (rect.height() * 54) / 100,
                            },
                        );
                        g.text_ellipsis(
                            &urow.sublabel,
                            fonts.sub,
                            pal.sub,
                            RECT {
                                left: text_left,
                                top: rect.top + (rect.height() * 48) / 100,
                                right: right_edge - (62.0 * s).round() as i32,
                                bottom: rect.bottom - (4.0 * s).round() as i32,
                            },
                        );
                        g.text_right(
                            &urow.value,
                            fonts.name_en,
                            pal.text,
                            RECT {
                                left: right_edge - (86.0 * s).round() as i32,
                                top: rect.top,
                                right: right_edge,
                                bottom: rect.bottom,
                            },
                        );
                    }
                }
            }
        }
    }
}

pub(crate) unsafe fn draw_detail_text(
    g: &Gdi,
    fonts: &FontSet,
    menu: &Menu,
    idx: usize,
    layout: &CardLayout,
    pal: &ThemePalette,
) {
    unsafe {
        let s = layout.scale;
        let Some(agent) = menu.agents.get(idx) else {
            return;
        };

        if let Some(back_rect) = layout.detail_back_rect {
            g.text(
                "返回",
                fonts.tab,
                pal.text,
                RECT {
                    left: back_rect.left + (28.0 * s).round() as i32,
                    top: back_rect.top,
                    right: back_rect.right - (6.0 * s).round() as i32,
                    bottom: back_rect.bottom,
                },
            );
        }

        if let Some(hero_rect) = layout.detail_hero_rect {
            let text_left = hero_rect.left + (56.0 * s).round() as i32;
            let pill_w = (56.0 * s).round() as i32;
            let pill_right = hero_rect.right - (10.0 * s).round() as i32;
            let pill_left = pill_right - pill_w;

            let detail_name_font = if agent.display_name.is_ascii() {
                fonts.name_en
            } else {
                fonts.name_cn
            };
            g.text_ellipsis(
                &agent.display_name,
                detail_name_font,
                pal.text,
                RECT {
                    left: text_left,
                    top: hero_rect.top + (6.0 * s).round() as i32,
                    right: pill_left - (8.0 * s).round() as i32,
                    bottom: hero_rect.top + (hero_rect.height() * 52) / 100,
                },
            );
            g.text_ellipsis(
                &agent.provider_label,
                fonts.sub,
                pal.sub,
                RECT {
                    left: text_left,
                    top: hero_rect.top + (hero_rect.height() * 48) / 100,
                    right: pill_left - (8.0 * s).round() as i32,
                    bottom: hero_rect.bottom - (5.0 * s).round() as i32,
                },
            );

            let (badge_label, fg) = if agent.route_mode == "loading" {
                ("加载中", pal.loading_fg)
            } else if agent.route_mode == "routed" {
                ("路由", pal.routed_fg)
            } else {
                ("直连", pal.direct_fg)
            };
            g.text(
                badge_label,
                fonts.pill,
                fg,
                RECT {
                    left: pill_left + (18.0 * s).round() as i32,
                    top: hero_rect.top,
                    right: pill_right,
                    bottom: hero_rect.bottom,
                },
            );
        }

        if let Some(info_rect) = layout.detail_info_rect {
            let lx = info_rect.left + (10.0 * s).round() as i32;
            let rx = info_rect.right - (10.0 * s).round() as i32;
            let label_w = (58.0 * s).round() as i32;
            let val_left = lx + label_w;
            let mid_y = (info_rect.top + info_rect.bottom) / 2;

            g.text(
                "接入端点",
                fonts.pill,
                pal.sub,
                RECT {
                    left: lx,
                    top: info_rect.top + (6.0 * s).round() as i32,
                    right: val_left,
                    bottom: mid_y,
                },
            );
            g.text_ellipsis(
                &agent.detail.endpoint,
                fonts.chip,
                pal.text,
                RECT {
                    left: val_left + (4.0 * s).round() as i32,
                    top: info_rect.top + (6.0 * s).round() as i32,
                    right: rx,
                    bottom: mid_y,
                },
            );

            g.text(
                "配置来源",
                fonts.pill,
                pal.sub,
                RECT {
                    left: lx,
                    top: mid_y,
                    right: val_left,
                    bottom: info_rect.bottom - (6.0 * s).round() as i32,
                },
            );
            g.text_ellipsis(
                &agent.detail.source,
                fonts.chip,
                pal.sub,
                RECT {
                    left: val_left + (4.0 * s).round() as i32,
                    top: mid_y,
                    right: rx,
                    bottom: info_rect.bottom - (6.0 * s).round() as i32,
                },
            );
        }

        if let Some(models_rect) = layout.detail_models_rect {
            let lx = models_rect.left + (10.0 * s).round() as i32;
            let rx = models_rect.right - (10.0 * s).round() as i32;
            let title = if agent.detail.extra_model_count > 0 {
                format!("本地检测模型 (+{})", agent.detail.extra_model_count)
            } else {
                "本地检测模型".to_string()
            };
            g.text(
                &title,
                fonts.pill,
                pal.sub,
                RECT {
                    left: lx,
                    top: models_rect.top + (6.0 * s).round() as i32,
                    right: rx,
                    bottom: models_rect.top + (24.0 * s).round() as i32,
                },
            );

            if layout.detail_model_chips.is_empty() {
                g.text_ellipsis(
                    "未在本地配置文件发现显式模型列表",
                    fonts.sub,
                    pal.sub,
                    RECT {
                        left: lx,
                        top: models_rect.top + (26.0 * s).round() as i32,
                        right: rx,
                        bottom: models_rect.bottom - (6.0 * s).round() as i32,
                    },
                );
            } else {
                for (chip_rect, model_name) in &layout.detail_model_chips {
                    g.text_center_ellipsis(
                        model_name,
                        fonts.chip,
                        pal.chip_fg,
                        RECT {
                            left: chip_rect.left + (4.0 * s).round() as i32,
                            top: chip_rect.top,
                            right: chip_rect.right - (4.0 * s).round() as i32,
                            bottom: chip_rect.bottom,
                        },
                    );
                }
            }
        }

        if let Some(cta_rect) = layout.detail_console_rect {
            g.text_center(
                &agent.detail.console_label,
                fonts.btn,
                pal.btn_primary_fg,
                RECT {
                    left: cta_rect.left + (14.0 * s).round() as i32,
                    top: cta_rect.top,
                    right: cta_rect.right,
                    bottom: cta_rect.bottom,
                },
            );
        }
    }
}
