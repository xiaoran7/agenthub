use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RectI {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectI {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetOutcome {
    SwitchTab(TrayTab),
    SwitchPeriod(TrayUsagePeriod),
    OpenDetail(usize),
    BackHome,
    InvokeAction(usize),
}

pub(crate) struct CardLayout {
    pub scale: f32,
    pub view: TrayView,
    pub tab: TrayTab,
    pub period: TrayUsagePeriod,
    pub win_w: i32,
    pub win_h: i32,
    pub card_x: i32,
    pub card_y: i32,
    pub card_w: i32,
    pub card_h: i32,
    pub gear_rect: RectI,
    pub tabs_track: RectI,
    pub tab_config: RectI,
    pub tab_usage: RectI,
    pub period_track: Option<RectI>,
    pub period_rects: [(TrayUsagePeriod, RectI); 4],
    pub row_rects: Vec<(RectI, usize)>,
    pub detail_back_rect: Option<RectI>,
    pub detail_hero_rect: Option<RectI>,
    pub detail_info_rect: Option<RectI>,
    pub detail_models_rect: Option<RectI>,
    pub detail_model_chips: Vec<(RectI, String)>,
    pub detail_console_rect: Option<RectI>,
    pub sep_y: i32,
    pub footer_main: RectI,
    pub footer_exit: RectI,
}

impl CardLayout {
    pub fn compute(
        menu: &Menu,
        view: TrayView,
        tab: TrayTab,
        period: TrayUsagePeriod,
        dpi: u32,
    ) -> Self {
        let scale = (dpi.max(96) as f32) / 96.0;
        let px = |v: i32| (v as f32 * scale).round() as i32;

        let pad = px(SHADOW_PAD);
        let card_x = pad;
        let card_y = pad;
        let card_w = px(CARD_W);
        let pad_x = px(PAD_X);
        let content_left = card_x + pad_x;
        let content_right = card_x + card_w - pad_x;

        let gear_size = px(28);
        let gear_top = card_y + (px(HEADER_H) - gear_size) / 2;
        let gear_rect = RectI {
            left: content_right - gear_size,
            top: gear_top,
            right: content_right,
            bottom: gear_top + gear_size,
        };

        let tabs_top = card_y + px(HEADER_H);
        let tabs_h = px(TABS_H);
        let tabs_track = RectI {
            left: content_left,
            top: tabs_top,
            right: content_right,
            bottom: tabs_top + tabs_h,
        };
        let inset = px(3);
        let mid_x = (tabs_track.left + tabs_track.right) / 2;
        let tab_config = RectI {
            left: tabs_track.left + inset,
            top: tabs_track.top + inset,
            right: mid_x - px(1),
            bottom: tabs_track.bottom - inset,
        };
        let tab_usage = RectI {
            left: mid_x + px(1),
            top: tabs_track.top + inset,
            right: tabs_track.right - inset,
            bottom: tabs_track.bottom - inset,
        };

        let zero_rect = RectI {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut period_rects = [
            (TrayUsagePeriod::Today, zero_rect),
            (TrayUsagePeriod::Days7, zero_rect),
            (TrayUsagePeriod::Days30, zero_rect),
            (TrayUsagePeriod::All, zero_rect),
        ];
        let mut period_track = None;
        let mut row_rects = Vec::new();
        let mut detail_back_rect = None;
        let mut detail_hero_rect = None;
        let mut detail_info_rect = None;
        let mut detail_models_rect = None;
        let mut detail_model_chips = Vec::new();
        let mut detail_console_rect = None;

        let sep_y = match view {
            TrayView::Home => {
                let mut y = tabs_track.bottom;
                if tab == TrayTab::Usage {
                    y += px(PERIOD_BAR_GAP);
                    let track = RectI {
                        left: content_left,
                        top: y,
                        right: content_right,
                        bottom: y + px(PERIOD_BAR_H),
                    };
                    let p_in = px(2);
                    let p_gap = px(2);
                    let inner_w = track.width() - p_in * 2 - p_gap * 3;
                    let seg_w = inner_w / 4;
                    let mut sx = track.left + p_in;
                    for (i, p) in TrayUsagePeriod::ALL.into_iter().enumerate() {
                        let right = if i == 3 {
                            track.right - p_in
                        } else {
                            sx + seg_w
                        };
                        period_rects[i] = (
                            p,
                            RectI {
                                left: sx,
                                top: track.top + p_in,
                                right,
                                bottom: track.bottom - p_in,
                            },
                        );
                        sx = right + p_gap;
                    }
                    period_track = Some(track);
                    y = track.bottom + px(TABS_GAP);
                } else {
                    y += px(TABS_GAP);
                }

                let row_h = px(AGENT_ROW_H);
                let row_gap = px(AGENT_ROW_GAP);
                let count = match tab {
                    TrayTab::Config => menu.agents.len().max(1),
                    TrayTab::Usage => menu.usage_rows.len().max(1),
                };
                row_rects.reserve(count);
                for idx in 0..count {
                    let action_id = match tab {
                        TrayTab::Config => menu
                            .agents
                            .get(idx)
                            .map(|r| r.action_id)
                            .unwrap_or(ACTION_OPEN_AGENTS),
                        TrayTab::Usage => menu
                            .usage_rows
                            .get(idx)
                            .map(|r| r.action_id)
                            .unwrap_or(ACTION_OPEN_USAGE),
                    };
                    row_rects.push((
                        RectI {
                            left: content_left,
                            top: y,
                            right: content_right,
                            bottom: y + row_h,
                        },
                        action_id,
                    ));
                    y += row_h + if idx + 1 < count { row_gap } else { 0 };
                }
                y + px(8)
            }
            TrayView::Detail(idx) => {
                let mut y = card_y + px(HEADER_H) + px(2);
                let back_rect = RectI {
                    left: content_left,
                    top: y,
                    right: content_left + px(82),
                    bottom: y + px(DETAIL_BACK_H),
                };
                detail_back_rect = Some(back_rect);
                y += px(DETAIL_BACK_H) + px(DETAIL_GAP);

                let hero_rect = RectI {
                    left: content_left,
                    top: y,
                    right: content_right,
                    bottom: y + px(DETAIL_HERO_H),
                };
                detail_hero_rect = Some(hero_rect);
                y += px(DETAIL_HERO_H) + px(DETAIL_GAP);

                let info_rect = RectI {
                    left: content_left,
                    top: y,
                    right: content_right,
                    bottom: y + px(DETAIL_INFO_H),
                };
                detail_info_rect = Some(info_rect);
                y += px(DETAIL_INFO_H) + px(DETAIL_GAP);

                let models_rect = RectI {
                    left: content_left,
                    top: y,
                    right: content_right,
                    bottom: y + px(DETAIL_MODELS_H),
                };
                if let Some(agent) = menu.agents.get(idx)
                    && !agent.detail.models.is_empty()
                {
                    let n = agent.detail.models.len().min(3);
                    let inner_l = models_rect.left + px(10);
                    let inner_r = models_rect.right - px(10);
                    let gap = px(5);
                    let chip_w = (inner_r - inner_l - gap * (n as i32 - 1)) / n as i32;
                    let chip_top = models_rect.top + px(28);
                    let chip_bot = models_rect.bottom - px(8);
                    let mut cx = inner_l;
                    for (ci, model_name) in agent.detail.models.iter().take(n).enumerate() {
                        let right = if ci + 1 == n { inner_r } else { cx + chip_w };
                        detail_model_chips.push((
                            RectI {
                                left: cx,
                                top: chip_top,
                                right,
                                bottom: chip_bot,
                            },
                            model_name.clone(),
                        ));
                        cx = right + gap;
                    }
                }
                detail_models_rect = Some(models_rect);
                y += px(DETAIL_MODELS_H) + px(DETAIL_GAP);

                let cta_rect = RectI {
                    left: content_left,
                    top: y,
                    right: content_right,
                    bottom: y + px(DETAIL_CTA_H),
                };
                detail_console_rect = Some(cta_rect);
                y + px(DETAIL_CTA_H) + px(8)
            }
        };

        let footer_h = px(FOOTER_H);
        let footer_top = sep_y + px(6);
        let footer_bottom = sep_y + footer_h - px(6);
        let footer_gap = px(8);
        let footer_mid = card_x + (card_w * 58) / 100;
        let footer_main = RectI {
            left: content_left,
            top: footer_top,
            right: footer_mid - footer_gap / 2,
            bottom: footer_bottom,
        };
        let footer_exit = RectI {
            left: footer_mid + footer_gap / 2,
            top: footer_top,
            right: content_right,
            bottom: footer_bottom,
        };

        let card_h = (sep_y - card_y) + footer_h;
        Self {
            scale,
            view,
            tab,
            period,
            win_w: card_w + pad * 2,
            win_h: card_h + pad * 2,
            card_x,
            card_y,
            card_w,
            card_h,
            gear_rect,
            tabs_track,
            tab_config,
            tab_usage,
            period_track,
            period_rects,
            row_rects,
            detail_back_rect,
            detail_hero_rect,
            detail_info_rect,
            detail_models_rect,
            detail_model_chips,
            detail_console_rect,
            sep_y,
            footer_main,
            footer_exit,
        }
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<HitTarget> {
        if x < self.card_x
            || x >= self.card_x + self.card_w
            || y < self.card_y
            || y >= self.card_y + self.card_h
        {
            return None;
        }
        if self.gear_rect.contains(x, y) {
            return Some(HitTarget::GearSettings);
        }
        match self.view {
            TrayView::Home => {
                if self.tab_config.contains(x, y) {
                    return Some(HitTarget::Tab(TrayTab::Config));
                }
                if self.tab_usage.contains(x, y) {
                    return Some(HitTarget::Tab(TrayTab::Usage));
                }
                if self.tab == TrayTab::Usage && self.period_track.is_some() {
                    for (p, rect) in self.period_rects {
                        if rect.contains(x, y) {
                            return Some(HitTarget::Period(p));
                        }
                    }
                }
                for (i, (rect, _)) in self.row_rects.iter().enumerate() {
                    if rect.contains(x, y) {
                        return Some(HitTarget::Row(i));
                    }
                }
            }
            TrayView::Detail(idx) => {
                if self.detail_back_rect.is_some_and(|r| r.contains(x, y)) {
                    return Some(HitTarget::DetailBack);
                }
                if self.detail_console_rect.is_some_and(|r| r.contains(x, y)) {
                    return Some(HitTarget::DetailOpenConsole(idx));
                }
            }
        }
        if self.footer_main.contains(x, y) {
            return Some(HitTarget::FooterMain);
        }
        if self.footer_exit.contains(x, y) {
            return Some(HitTarget::FooterExit);
        }
        None
    }

    pub fn outcome_for(&self, target: HitTarget, menu: &Menu) -> TargetOutcome {
        match target {
            HitTarget::GearSettings => TargetOutcome::InvokeAction(ACTION_OPEN_SETTINGS),
            HitTarget::Tab(tab) => TargetOutcome::SwitchTab(tab),
            HitTarget::Period(period) => TargetOutcome::SwitchPeriod(period),
            HitTarget::Row(i) => {
                if self.view == TrayView::Home
                    && self.tab == TrayTab::Config
                    && i < menu.agents.len()
                {
                    TargetOutcome::OpenDetail(i)
                } else {
                    let action_id = self
                        .row_rects
                        .get(i)
                        .map(|(_, id)| *id)
                        .unwrap_or(ACTION_OPEN_USAGE);
                    TargetOutcome::InvokeAction(action_id)
                }
            }
            HitTarget::DetailBack => TargetOutcome::BackHome,
            HitTarget::DetailOpenConsole(idx) => {
                let action_id = menu
                    .agents
                    .get(idx)
                    .map(|a| a.detail.console_action_id)
                    .unwrap_or(ACTION_OPEN_AGENTS);
                TargetOutcome::InvokeAction(action_id)
            }
            HitTarget::FooterMain => TargetOutcome::InvokeAction(ACTION_OPEN_MAIN),
            HitTarget::FooterExit => TargetOutcome::InvokeAction(ACTION_EXIT),
        }
    }

    pub fn target_rect(&self, target: HitTarget) -> Option<RectI> {
        match target {
            HitTarget::GearSettings => Some(self.gear_rect),
            HitTarget::Tab(TrayTab::Config) => Some(self.tab_config),
            HitTarget::Tab(TrayTab::Usage) => Some(self.tab_usage),
            HitTarget::Period(p) => self
                .period_rects
                .iter()
                .find(|(item, _)| *item == p)
                .map(|(_, r)| *r),
            HitTarget::Row(i) => self.row_rects.get(i).map(|(r, _)| *r),
            HitTarget::DetailBack => self.detail_back_rect,
            HitTarget::DetailOpenConsole(_) => self.detail_console_rect,
            HitTarget::FooterMain => Some(self.footer_main),
            HitTarget::FooterExit => Some(self.footer_exit),
        }
    }
}
