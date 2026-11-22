use super::super::*;
use super::*;
use std::mem::{size_of, zeroed};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK,
    VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, MA_ACTIVATE, SetCursor, WA_INACTIVE, WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER,
};

fn choose(id: usize) {
    let choice = STATE.with(|s| {
        s.try_borrow()
            .ok()
            .and_then(|g| g.as_ref().map(|st| st.choice))
    });
    hide();
    if let Some(choice) = choice {
        choice(id);
    }
}

unsafe fn apply_outcome(hwnd: HWND, outcome: TargetOutcome, next_focus: Option<HitTarget>) {
    match outcome {
        TargetOutcome::InvokeAction(action_id) => choose(action_id),
        TargetOutcome::SwitchTab(new_tab) => {
            let pos = STATE.with(|s| {
                let mut guard = s.try_borrow_mut().ok()?;
                let st = guard.as_mut()?;
                st.view = TrayView::Home;
                st.tab = new_tab;
                st.layout = CardLayout::compute(&st.menu, st.view, st.tab, st.period, st.dpi);
                st.hover = None;
                st.pressed = None;
                st.focus = next_focus;
                st.session = st.session.wrapping_add(1);
                Some(place_near_anchor(st.anchor, &st.layout))
            });
            if let Some(pos) = pos {
                unsafe { paint(hwnd, pos) };
            }
        }
        TargetOutcome::SwitchPeriod(new_period) => {
            let is_loading = STATE
                .with(|s| {
                    s.try_borrow()
                        .ok()
                        .and_then(|g| g.as_ref().map(|st| st.menu.is_loading))
                })
                .unwrap_or(false);
            let usage_rows = build_usage_rows_for_period(new_period, is_loading);
            let pos = STATE.with(|s| {
                let mut guard = s.try_borrow_mut().ok()?;
                let st = guard.as_mut()?;
                st.period = new_period;
                st.menu.period = new_period;
                st.menu.usage_rows = usage_rows;
                st.layout = CardLayout::compute(&st.menu, st.view, st.tab, st.period, st.dpi);
                st.hover = None;
                st.pressed = None;
                st.focus = next_focus;
                st.session = st.session.wrapping_add(1);
                Some(place_near_anchor(st.anchor, &st.layout))
            });
            if let Some(pos) = pos {
                unsafe { paint(hwnd, pos) };
            }
        }
        TargetOutcome::OpenDetail(idx) => {
            let pos = STATE.with(|s| {
                let mut guard = s.try_borrow_mut().ok()?;
                let st = guard.as_mut()?;
                if idx >= st.menu.agents.len() {
                    return None;
                }
                st.view = TrayView::Detail(idx);
                st.layout = CardLayout::compute(&st.menu, st.view, st.tab, st.period, st.dpi);
                st.hover = None;
                st.pressed = None;
                st.focus = next_focus;
                st.session = st.session.wrapping_add(1);
                Some(place_near_anchor(st.anchor, &st.layout))
            });
            if let Some(pos) = pos {
                unsafe { paint(hwnd, pos) };
            }
        }
        TargetOutcome::BackHome => {
            let pos = STATE.with(|s| {
                let mut guard = s.try_borrow_mut().ok()?;
                let st = guard.as_mut()?;
                st.view = TrayView::Home;
                st.layout = CardLayout::compute(&st.menu, st.view, st.tab, st.period, st.dpi);
                st.hover = None;
                st.pressed = None;
                st.focus = next_focus;
                st.session = st.session.wrapping_add(1);
                Some(place_near_anchor(st.anchor, &st.layout))
            });
            if let Some(pos) = pos {
                unsafe { paint(hwnd, pos) };
            }
        }
    }
}

unsafe fn map_vk_to_key_input(vk: u16) -> Option<KeyInput> {
    match vk {
        VK_ESCAPE => Some(KeyInput::Escape),
        VK_BACK => Some(KeyInput::Backspace),
        VK_TAB => {
            let shift = unsafe { (GetKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0 };
            Some(if shift {
                KeyInput::BackTab
            } else {
                KeyInput::Tab
            })
        }
        VK_UP => Some(KeyInput::Up),
        VK_DOWN => Some(KeyInput::Down),
        VK_LEFT => Some(KeyInput::Left),
        VK_RIGHT => Some(KeyInput::Right),
        VK_RETURN | VK_SPACE => Some(KeyInput::Activate),
        _ => None,
    }
}

pub(crate) unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let x = (lparam & 0xffff) as i16 as i32;
        let y = ((lparam >> 16) & 0xffff) as i16 as i32;
        match msg {
            WM_MOUSEACTIVATE => MA_ACTIVATE as LRESULT,
            WM_APP_TRAY_MENU_READY => {
                apply_pending_refresh(hwnd);
                0
            }
            WM_TIMER => {
                if wparam == TIMER_HOVER_WATCH {
                    hide_if_clicked_away();
                }
                0
            }
            WM_MOUSEMOVE => {
                let mut track: TRACKMOUSEEVENT = zeroed();
                track.cbSize = size_of::<TRACKMOUSEEVENT>() as u32;
                track.dwFlags = TME_LEAVE;
                track.hwndTrack = hwnd;
                TrackMouseEvent(&mut track);
                let changed = STATE.with(|s| {
                    let Ok(mut guard) = s.try_borrow_mut() else {
                        return false;
                    };
                    let Some(st) = guard.as_mut() else {
                        return false;
                    };
                    st.last_inside_at = Instant::now();
                    let hit = st.layout.hit(x, y);
                    let focus_cleared = st.focus.take().is_some();
                    if hit != st.hover || focus_cleared {
                        st.hover = hit;
                        true
                    } else {
                        false
                    }
                });
                if changed {
                    repaint(hwnd);
                }
                0
            }
            WM_MOUSELEAVE => {
                let changed = STATE.with(|s| {
                    let Ok(mut guard) = s.try_borrow_mut() else {
                        return false;
                    };
                    let Some(st) = guard.as_mut() else {
                        return false;
                    };
                    if st.hover.is_some() {
                        st.hover = None;
                        true
                    } else {
                        false
                    }
                });
                if changed {
                    repaint(hwnd);
                }
                0
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                let changed = STATE.with(|s| {
                    let Ok(mut guard) = s.try_borrow_mut() else {
                        return false;
                    };
                    let Some(st) = guard.as_mut() else {
                        return false;
                    };
                    let hit = st.layout.hit(x, y);
                    st.pressed = hit;
                    let focus_cleared = st.focus.take().is_some();
                    hit.is_some() || focus_cleared
                });
                if changed {
                    repaint(hwnd);
                }
                0
            }
            WM_LBUTTONUP => {
                ReleaseCapture();
                let outcome = STATE.with(|s| {
                    let mut guard = s.try_borrow_mut().ok()?;
                    let st = guard.as_mut()?;
                    let pressed = st.pressed.take()?;
                    let cur = st.layout.hit(x, y);
                    let is_hit = if cur == Some(pressed) {
                        true
                    } else if let Some(rect) = st.layout.target_rect(pressed) {
                        let pad = (6.0 * st.layout.scale).round() as i32;
                        x >= rect.left - pad
                            && x <= rect.right + pad
                            && y >= rect.top - pad
                            && y <= rect.bottom + pad
                    } else {
                        false
                    };
                    is_hit.then(|| st.layout.outcome_for(pressed, &st.menu))
                });
                if let Some(outcome) = outcome {
                    apply_outcome(hwnd, outcome, None);
                } else {
                    repaint(hwnd);
                }
                0
            }
            WM_KEYDOWN => {
                if let Some(input) = map_vk_to_key_input(wparam as u16) {
                    let cmd = STATE.with(|s| {
                        let guard = s.try_borrow().ok()?;
                        let st = guard.as_ref()?;
                        Some(handle_key(
                            input, st.view, st.tab, st.period, st.focus, &st.layout, &st.menu,
                        ))
                    });
                    match cmd {
                        Some(KeyCommand::Hide) => hide(),
                        Some(KeyCommand::SetFocus(target)) => {
                            STATE.with(|s| {
                                if let Ok(mut guard) = s.try_borrow_mut()
                                    && let Some(st) = guard.as_mut()
                                {
                                    st.focus = Some(target);
                                }
                            });
                            repaint(hwnd);
                        }
                        Some(KeyCommand::Apply(outcome, next_focus)) => {
                            apply_outcome(hwnd, outcome, next_focus);
                        }
                        Some(KeyCommand::None) | None => {}
                    }
                }
                0
            }
            WM_ACTIVATE => {
                if (wparam & 0xffff) as u32 == WA_INACTIVE {
                    hide_if_clicked_away();
                }
                0
            }
            WM_SETCURSOR => {
                SetCursor(arrow_cursor());
                1
            }
            WM_ERASEBKGND => 1,
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }))
    .unwrap_or(0)
}
