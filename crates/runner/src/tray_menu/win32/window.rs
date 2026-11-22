use super::super::*;
use super::*;
use std::cell::RefCell;
use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, GetCursorPos, GetWindowRect, HCURSOR, HWND_TOPMOST, IDC_ARROW, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, WM_APP, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const POPUP_CLASS: &str = "AgentHubRunnerPopupClass\0";
pub(crate) const TIMER_HOVER_WATCH: usize = 0x5448;
pub(crate) const WM_APP_TRAY_MENU_READY: u32 = WM_APP + 0x41;
const SETTLE: Duration = Duration::from_millis(150);

static REFRESH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static PENDING_MENU: Mutex<Option<(TrayUsagePeriod, Menu)>> = Mutex::new(None);

pub(crate) fn colorref(c: (u8, u8, u8)) -> COLORREF {
    c.0 as u32 | ((c.1 as u32) << 8) | ((c.2 as u32) << 16)
}

pub(crate) fn to_win_rect(r: RectI) -> RECT {
    RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

pub(crate) struct State {
    pub(crate) hwnd: HWND,
    pub(crate) menu: Menu,
    pub(crate) view: TrayView,
    pub(crate) tab: TrayTab,
    pub(crate) period: TrayUsagePeriod,
    pub(crate) dpi: u32,
    pub(crate) anchor: POINT,
    pub(crate) choice: fn(usize),
    pub(crate) layout: CardLayout,
    pub(crate) hover: Option<HitTarget>,
    pub(crate) focus: Option<HitTarget>,
    pub(crate) pressed: Option<HitTarget>,
    pub(crate) visible: bool,
    pub(crate) hidden_at: Option<Instant>,
    pub(crate) shown_at: Option<Instant>,
    pub(crate) last_inside_at: Instant,
    pub(crate) last_refreshed_at: Instant,
    pub(crate) session: u64,
}

thread_local! {
    pub(crate) static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

pub fn closed_within(window: Duration) -> bool {
    STATE.with(|s| {
        s.try_borrow()
            .ok()
            .and_then(|g| g.as_ref().and_then(|st| st.hidden_at))
            .is_some_and(|at| at.elapsed() < window)
    })
}

pub fn shown_within(window: Duration) -> bool {
    STATE.with(|s| {
        s.try_borrow()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .filter(|st| st.visible)
                    .and_then(|st| st.shown_at)
            })
            .is_some_and(|at| at.elapsed() < window)
    })
}

pub fn is_visible() -> bool {
    STATE.with(|s| {
        s.try_borrow()
            .ok()
            .is_some_and(|g| g.as_ref().is_some_and(|st| st.visible))
    })
}

fn dpi_at(pt: POINT, hwnd: HWND) -> u32 {
    unsafe {
        let sys_dpi = GetDpiForSystem().max(96);
        let win_dpi = if !hwnd.is_null() {
            GetDpiForWindow(hwnd)
        } else {
            0
        };
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if !monitor.is_null() {
            let (mut x, mut y) = (96u32, 96u32);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0 {
                return x.max(win_dpi).max(sys_dpi).max(96);
            }
        }
        win_dpi.max(sys_dpi).max(96)
    }
}

fn work_area(pt: POINT) -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        }
    }
}

pub(crate) fn place_near_anchor(anchor: POINT, layout: &CardLayout) -> POINT {
    let work = work_area(anchor);
    let min_x = work.left + 8 - layout.card_x;
    let max_x = work.right - 8 - layout.card_w - layout.card_x;
    let x = (anchor.x - layout.card_w / 2 - layout.card_x).clamp(min_x, max_x.max(min_x));

    let target_card_bottom = (anchor.y - 10).min(work.bottom - 12);
    let mut y = target_card_bottom - layout.card_h - layout.card_y;
    let min_y = work.top + 8 - layout.card_y;
    y = y.max(min_y);
    POINT { x, y }
}

/// Shows the tray popup card near the cursor.
///
/// # Safety
/// `hinstance` must be this process's module handle.
pub unsafe fn show(hinstance: HINSTANCE, mut menu: Menu, choice: fn(usize)) {
    unsafe {
        crate::tray::enable_per_monitor_dpi_awareness();
        menu.dark = crate::tray::is_dark_theme();
        let hwnd = ensure_window(hinstance);
        if hwnd.is_null() {
            return;
        }
        let mut cursor: POINT = zeroed();
        GetCursorPos(&mut cursor);
        let dpi = dpi_at(cursor, hwnd);
        let (tab, period) = STATE
            .with(|s| s.borrow().as_ref().map(|st| (st.tab, st.period)))
            .unwrap_or((TrayTab::Config, TrayUsagePeriod::Today));
        if menu.period != period {
            menu.usage_rows = build_usage_rows_for_period(period, menu.is_loading);
            menu.period = period;
        }
        let view = TrayView::Home;
        let layout = CardLayout::compute(&menu, view, tab, period, dpi);
        let placement = place_near_anchor(cursor, &layout);

        STATE.with(|s| {
            let Ok(mut guard) = s.try_borrow_mut() else {
                return;
            };
            let next_session = guard
                .as_ref()
                .map(|st| st.session.wrapping_add(1))
                .unwrap_or(1);
            let now = Instant::now();
            *guard = Some(State {
                hwnd,
                menu,
                view,
                tab,
                period,
                dpi,
                anchor: cursor,
                choice,
                layout,
                hover: None,
                focus: None,
                pressed: None,
                visible: true,
                hidden_at: None,
                shown_at: Some(now),
                last_inside_at: now,
                last_refreshed_at: now,
                session: next_session,
            });
        });

        paint(hwnd, placement);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, TIMER_HOVER_WATCH, 50, None);
    }
}

pub fn hide_if_clicked_away() {
    unsafe {
        let mut cursor: POINT = zeroed();
        GetCursorPos(&mut cursor);
        let snapshot = STATE.with(|s| {
            s.try_borrow().ok()?.as_ref().map(|st| {
                (
                    st.hwnd,
                    st.visible,
                    st.shown_at,
                    st.last_inside_at,
                    st.last_refreshed_at,
                    st.anchor,
                    st.layout.card_x,
                    st.layout.card_y,
                    st.layout.card_w,
                    st.layout.card_h,
                    st.menu.is_loading,
                )
            })
        });
        let Some((
            hwnd,
            visible,
            shown_at,
            last_inside_at,
            last_refreshed_at,
            anchor,
            card_x,
            card_y,
            card_w,
            card_h,
            is_loading,
        )) = snapshot
        else {
            return;
        };
        if !visible {
            return;
        }
        let mut rect: RECT = zeroed();
        GetWindowRect(hwnd, &mut rect);

        let card_left = rect.left + card_x;
        let card_right = rect.left + card_x + card_w;
        let card_top = rect.top + card_y;
        let card_bottom = rect.top + card_y + card_h;

        let inside_card = cursor.x >= card_left - 10
            && cursor.x <= card_right + 10
            && cursor.y >= card_top - 10
            && cursor.y <= card_bottom + 10;

        let near_anchor = (cursor.x - anchor.x).abs() <= 24 && (cursor.y - anchor.y).abs() <= 24;

        let bridge_left = (card_left - 8).min(anchor.x - 20);
        let bridge_right = (card_right + 8).max(anchor.x + 20);
        let bridge_top = card_bottom - 6;
        let bridge_bottom = (anchor.y + 16).max(card_bottom + 24);
        let in_bridge = cursor.x >= bridge_left
            && cursor.x <= bridge_right
            && cursor.y >= bridge_top
            && cursor.y <= bridge_bottom;

        if inside_card || near_anchor || in_bridge {
            let should_refresh = last_refreshed_at.elapsed() >= Duration::from_millis(1800)
                || (is_loading && last_refreshed_at.elapsed() >= Duration::from_millis(200));
            let refresh_args = STATE.with(|s| {
                let mut guard = s.try_borrow_mut().ok()?;
                let st = guard.as_mut()?;
                st.last_inside_at = Instant::now();
                should_refresh.then(|| (st.menu.host.clone(), st.period))
            });
            if let Some((host, period)) = refresh_args {
                request_async_refresh(hwnd, host, period);
            }
            return;
        }

        let mouse_clicked_outside = (GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0)
            || (GetAsyncKeyState(VK_RBUTTON as i32) as u16 & 0x8000 != 0);
        if mouse_clicked_outside {
            hide();
            return;
        }

        if shown_at.is_some_and(|at| at.elapsed() < SETTLE) {
            return;
        }
        if last_inside_at.elapsed() >= Duration::from_millis(180) {
            hide();
        }
    }
}

fn request_async_refresh(hwnd: HWND, host: String, period: TrayUsagePeriod) {
    if REFRESH_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    struct RefreshGuard;
    impl Drop for RefreshGuard {
        fn drop(&mut self) {
            REFRESH_IN_FLIGHT.store(false, Ordering::Release);
        }
    }
    let hwnd_raw = hwnd as usize;
    if std::thread::Builder::new()
        .name("tray-menu-refresh".into())
        .spawn(move || {
            let _guard = RefreshGuard;
            let fresh_menu = build_live_menu_for_period(host, period);
            if let Ok(mut slot) = PENDING_MENU.lock() {
                *slot = Some((period, fresh_menu));
            }
            unsafe {
                let _ = PostMessageW(hwnd_raw as HWND, WM_APP_TRAY_MENU_READY, 0, 0);
            }
        })
        .is_err()
    {
        REFRESH_IN_FLIGHT.store(false, Ordering::Release);
    }
}

pub(crate) unsafe fn apply_pending_refresh(hwnd: HWND) {
    let Some((expected_period, fresh_menu)) = PENDING_MENU.lock().ok().and_then(|mut g| g.take())
    else {
        return;
    };
    let new_pos = STATE.with(|s| {
        let mut guard = s.try_borrow_mut().ok()?;
        let st = guard.as_mut()?;
        if !st.visible || st.period != expected_period {
            return None;
        }
        st.menu = fresh_menu;
        if let TrayView::Detail(idx) = st.view
            && idx >= st.menu.agents.len()
        {
            st.view = TrayView::Home;
            st.focus = None;
        }
        st.layout = CardLayout::compute(&st.menu, st.view, st.tab, st.period, st.dpi);
        st.last_refreshed_at = Instant::now();
        st.session = st.session.wrapping_add(1);
        Some(place_near_anchor(st.anchor, &st.layout))
    });
    if let Some(pos) = new_pos {
        unsafe { paint(hwnd, pos) };
    }
}

pub fn hide() {
    let hwnd = STATE.with(|s| {
        s.try_borrow()
            .ok()
            .and_then(|g| g.as_ref().map(|st| st.hwnd))
    });
    if let Some(hwnd) = hwnd {
        unsafe {
            KillTimer(hwnd, TIMER_HOVER_WATCH);
            ShowWindow(hwnd, SW_HIDE);
        }
    }
    STATE.with(|s| {
        let Ok(mut guard) = s.try_borrow_mut() else {
            return;
        };
        if let Some(st) = guard.as_mut() {
            if st.visible {
                st.visible = false;
                st.hidden_at = Some(Instant::now());
            }
            st.view = TrayView::Home;
            st.hover = None;
            st.focus = None;
            st.pressed = None;
        }
    });
}

unsafe fn ensure_window(hinstance: HINSTANCE) -> HWND {
    static WINDOW: OnceLock<usize> = OnceLock::new();
    if let Some(hwnd) = WINDOW.get() {
        return *hwnd as HWND;
    }
    unsafe {
        crate::tray::enable_per_monitor_dpi_awareness();
        let class: Vec<u16> = POPUP_CLASS.encode_utf16().collect();
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(popup_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: 0 as _,
            hCursor: arrow_cursor(),
            hbrBackground: 0 as _,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            0 as _,
            0 as _,
            hinstance,
            std::ptr::null(),
        );
        if !hwnd.is_null() {
            let _ = WINDOW.set(hwnd as usize);
        }
        hwnd
    }
}

pub(crate) fn arrow_cursor() -> HCURSOR {
    static ARROW: OnceLock<usize> = OnceLock::new();
    *ARROW.get_or_init(|| unsafe { LoadCursorW(0 as _, IDC_ARROW) } as usize) as HCURSOR
}

pub(crate) unsafe fn repaint(hwnd: HWND) {
    unsafe {
        let mut rect: RECT = zeroed();
        GetWindowRect(hwnd, &mut rect);
        paint(
            hwnd,
            POINT {
                x: rect.left,
                y: rect.top,
            },
        );
    }
}

pub(crate) unsafe fn paint(hwnd: HWND, at: POINT) {
    unsafe {
        STATE.with(|s| {
            let Ok(guard) = s.try_borrow() else {
                return;
            };
            let Some(st) = guard.as_ref() else {
                return;
            };
            FRAME.with(|f| {
                let Ok(mut slot) = f.try_borrow_mut() else {
                    return;
                };
                let rebuild = slot.as_ref().is_none_or(|frame| {
                    frame.session != st.session
                        || frame.hover != st.hover
                        || frame.focus != st.focus
                });
                if rebuild {
                    *slot = Some(Frame::new(
                        st.session, &st.menu, st.view, st.tab, st.period, &st.layout, st.hover,
                        st.focus,
                    ));
                }
                if let Some(frame) = slot.as_mut() {
                    frame.present(hwnd, at);
                }
            });
        });
    }
}
