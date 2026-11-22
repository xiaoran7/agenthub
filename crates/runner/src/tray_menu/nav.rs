use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Escape,
    Backspace,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Activate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyCommand {
    None,
    Hide,
    SetFocus(HitTarget),
    Apply(TargetOutcome, Option<HitTarget>),
}

pub(crate) fn focus_order(layout: &CardLayout) -> Vec<HitTarget> {
    let mut order = Vec::new();
    match layout.view {
        TrayView::Home => {
            order.push(HitTarget::Tab(TrayTab::Config));
            order.push(HitTarget::Tab(TrayTab::Usage));
            if layout.tab == TrayTab::Usage {
                for p in TrayUsagePeriod::ALL {
                    order.push(HitTarget::Period(p));
                }
            }
            for i in 0..layout.row_rects.len() {
                order.push(HitTarget::Row(i));
            }
        }
        TrayView::Detail(idx) => {
            order.push(HitTarget::DetailBack);
            order.push(HitTarget::DetailOpenConsole(idx));
        }
    }
    order.push(HitTarget::FooterMain);
    order.push(HitTarget::FooterExit);
    order.push(HitTarget::GearSettings);
    order
}

fn vertical_chain(layout: &CardLayout, tab: TrayTab, period: TrayUsagePeriod) -> Vec<HitTarget> {
    let mut chain = Vec::new();
    match layout.view {
        TrayView::Home => {
            chain.push(HitTarget::Tab(tab));
            if tab == TrayTab::Usage {
                chain.push(HitTarget::Period(period));
            }
            for i in 0..layout.row_rects.len() {
                chain.push(HitTarget::Row(i));
            }
            chain.push(HitTarget::FooterMain);
        }
        TrayView::Detail(idx) => {
            chain.push(HitTarget::DetailBack);
            chain.push(HitTarget::DetailOpenConsole(idx));
            chain.push(HitTarget::FooterMain);
        }
    }
    chain
}

fn step_cycle(items: &[HitTarget], current: Option<HitTarget>, forward: bool) -> Option<HitTarget> {
    if items.is_empty() {
        return None;
    }
    let pos = current.and_then(|c| items.iter().position(|&item| item == c));
    let next_idx = match (pos, forward) {
        (None, true) => 0,
        (None, false) => items.len() - 1,
        (Some(i), true) => (i + 1) % items.len(),
        (Some(i), false) => (i + items.len() - 1) % items.len(),
    };
    Some(items[next_idx])
}

pub(crate) fn handle_key(
    input: KeyInput,
    view: TrayView,
    tab: TrayTab,
    period: TrayUsagePeriod,
    focus: Option<HitTarget>,
    layout: &CardLayout,
    menu: &Menu,
) -> KeyCommand {
    match input {
        KeyInput::Escape => {
            if matches!(view, TrayView::Detail(_)) {
                KeyCommand::Apply(TargetOutcome::BackHome, None)
            } else {
                KeyCommand::Hide
            }
        }
        KeyInput::Backspace => {
            if matches!(view, TrayView::Detail(_)) {
                KeyCommand::Apply(TargetOutcome::BackHome, None)
            } else {
                KeyCommand::None
            }
        }
        KeyInput::Tab | KeyInput::BackTab => {
            let order = focus_order(layout);
            match step_cycle(&order, focus, input == KeyInput::Tab) {
                Some(target) => KeyCommand::SetFocus(target),
                None => KeyCommand::None,
            }
        }
        KeyInput::Up | KeyInput::Down => {
            let chain = vertical_chain(layout, tab, period);
            let mapped = match focus {
                Some(HitTarget::Tab(_)) => Some(HitTarget::Tab(tab)),
                Some(HitTarget::Period(_)) => Some(HitTarget::Period(period)),
                Some(HitTarget::FooterExit) => Some(HitTarget::FooterMain),
                other => other,
            };
            match step_cycle(&chain, mapped, input == KeyInput::Down) {
                Some(target) => KeyCommand::SetFocus(target),
                None => KeyCommand::None,
            }
        }
        KeyInput::Left | KeyInput::Right => {
            let is_right = input == KeyInput::Right;
            match (view, focus) {
                (TrayView::Detail(_), Some(HitTarget::DetailBack) | None) if !is_right => {
                    KeyCommand::Apply(TargetOutcome::BackHome, None)
                }
                (_, Some(HitTarget::FooterMain)) => KeyCommand::SetFocus(HitTarget::FooterExit),
                (_, Some(HitTarget::FooterExit)) => KeyCommand::SetFocus(HitTarget::FooterMain),
                (TrayView::Home, Some(HitTarget::Period(p))) => {
                    let next = if is_right { p.next() } else { p.prev() };
                    KeyCommand::Apply(
                        TargetOutcome::SwitchPeriod(next),
                        Some(HitTarget::Period(next)),
                    )
                }
                (TrayView::Home, Some(HitTarget::Tab(t))) => {
                    let next = match t {
                        TrayTab::Config => TrayTab::Usage,
                        TrayTab::Usage => TrayTab::Config,
                    };
                    KeyCommand::Apply(TargetOutcome::SwitchTab(next), Some(HitTarget::Tab(next)))
                }
                (TrayView::Home, None) if tab == TrayTab::Usage => {
                    let next = if is_right {
                        period.next()
                    } else {
                        period.prev()
                    };
                    KeyCommand::Apply(
                        TargetOutcome::SwitchPeriod(next),
                        Some(HitTarget::Period(next)),
                    )
                }
                (TrayView::Home, None) => {
                    let next = match tab {
                        TrayTab::Config => TrayTab::Usage,
                        TrayTab::Usage => TrayTab::Config,
                    };
                    KeyCommand::Apply(TargetOutcome::SwitchTab(next), Some(HitTarget::Tab(next)))
                }
                _ => KeyCommand::None,
            }
        }
        KeyInput::Activate => {
            let Some(target) = focus else {
                return KeyCommand::None;
            };
            let outcome = layout.outcome_for(target, menu);
            let next_focus = match outcome {
                TargetOutcome::OpenDetail(idx) => Some(HitTarget::DetailOpenConsole(idx)),
                TargetOutcome::BackHome => None,
                TargetOutcome::SwitchTab(t) => Some(HitTarget::Tab(t)),
                TargetOutcome::SwitchPeriod(p) => Some(HitTarget::Period(p)),
                TargetOutcome::InvokeAction(_) => None,
            };
            KeyCommand::Apply(outcome, next_focus)
        }
    }
}
