//! CC Switch-style layered system tray popup card (`UpdateLayeredWindow` + SDF + GDI).
//!
//! Renders a crisp DPI-scaled light rounded card with:
//! - Header: `"AgentHub"` title + gear settings icon button
//! - Interactive segmented switcher: `配置` / `用量` (switches tabs in-place)
//! - `配置` tab: per-agent rows with tinted brand icon badge, bold client name,
//!   real upstream provider + `直连` / `路由` pill, and `›` chevron
//! - `用量` tab: live today's requests, tokens, cache hit rate, estimated cost,
//!   and top active models
//! - Footer: `打开主窗口` and `退出` side-by-side buttons

/// Action IDs returned to the tray host when a command target is clicked.
pub const ACTION_OPEN_MAIN: usize = 2001;
pub const ACTION_OPEN_SETTINGS: usize = 2002;
pub const ACTION_OPEN_AGENTS: usize = 2003;
pub const ACTION_OPEN_USAGE: usize = 2004;
pub const ACTION_EXIT: usize = 2005;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayTab {
    #[default]
    Config,
    Usage,
}

#[derive(Debug, Clone)]
pub struct TrayAgentRow {
    pub action_id: usize,
    pub client_id: String,
    pub display_name: String,
    pub badge_glyph: &'static str,
    pub brand_rgb: (u8, u8, u8),
    pub brand_bg_rgb: (u8, u8, u8),
    pub provider_label: String,
    /// `"direct"` -> green `直连`, `"routed"` -> blue `路由`
    pub route_mode: String,
}

#[derive(Debug, Clone)]
pub struct TrayUsageRow {
    pub action_id: usize,
    pub label: String,
    pub sublabel: String,
    pub value: String,
    pub accent_rgb: (u8, u8, u8),
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub version: String,
    pub host: String,
    pub agents: Vec<TrayAgentRow>,
    pub usage_rows: Vec<TrayUsageRow>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            host: "127.0.0.1:4200".to_string(),
            agents: Vec::new(),
            usage_rows: Vec::new(),
        }
    }
}

fn brand_palette(client_id: &str) -> (&'static str, &'static str, (u8, u8, u8), (u8, u8, u8)) {
    match client_id {
        "claude_code" => ("Claude", "C", (0xd9, 0x77, 0x57), (0xfd, 0xf2, 0xed)),
        "claude_desktop" => (
            "Claude Desktop",
            "D",
            (0x63, 0x66, 0xf1),
            (0xee, 0xf2, 0xff),
        ),
        "codex" => ("Codex", "X", (0x10, 0xa3, 0x7f), (0xec, 0xfd, 0xf5)),
        "gemini_cli" => ("Gemini", "G", (0x25, 0x63, 0xeb), (0xef, 0xf6, 0xff)),
        "zcode" => ("ZCode", "Z", (0x7c, 0x3a, 0xed), (0xf5, 0xf3, 0xff)),
        "grok_build" => ("Grok", "K", (0x18, 0x18, 0x1b), (0xf4, 0xf4, 0xf5)),
        _ => ("Agent", "A", (0x4b, 0x55, 0x63), (0xf3, 0xf4, 0xf6)),
    }
}

fn compact_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Builds live tray card data from cached client route discovery and today's usage ledger.
pub fn build_live_menu(host: String) -> Menu {
    let summaries = crate::claude_gateway::discover_all_client_route_summaries();
    let mut agents = Vec::new();
    for s in summaries {
        let is_core = matches!(
            s.agent_id,
            "claude_code" | "claude_desktop" | "codex" | "gemini_cli" | "zcode"
        );
        if !is_core && !s.is_routed {
            continue;
        }
        let (short_name, glyph, fg, bg) = brand_palette(s.agent_id);
        let action_id = if s.is_routed {
            ACTION_OPEN_SETTINGS
        } else {
            ACTION_OPEN_AGENTS
        };
        let route_mode = if s.is_routed {
            "routed".to_string()
        } else {
            "direct".to_string()
        };
        agents.push(TrayAgentRow {
            action_id,
            client_id: s.agent_id.to_string(),
            display_name: short_name.to_string(),
            badge_glyph: glyph,
            brand_rgb: fg,
            brand_bg_rgb: bg,
            provider_label: if s.provider_name.trim().is_empty() {
                "Official".to_string()
            } else {
                s.provider_name
            },
            route_mode,
        });
    }

    let (_, today) = crate::usage_ledger::digest_cached(crate::usage_ledger::Period::Today, 20);
    let prompt_total = today
        .input
        .saturating_add(today.cache_read)
        .saturating_add(today.cache_write);
    let hit_pct = if prompt_total > 0 {
        ((today.cache_read as f64 / prompt_total as f64) * 100.0).round() as u32
    } else {
        0
    };

    let mut usage_rows = vec![
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: "今日请求与会话".to_string(),
            sublabel: format!("活跃会话 {} 个 · 失败 {} 次", today.sessions, today.errors),
            value: format!("{} 次", today.calls),
            accent_rgb: (0x25, 0x63, 0xeb),
        },
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: "今日 Token 消耗".to_string(),
            sublabel: format!(
                "输出 {} · 缓存命中 {}%",
                compact_tokens(today.output),
                hit_pct
            ),
            value: compact_tokens(today.tokens),
            accent_rgb: (0x10, 0xa3, 0x7f),
        },
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: "今日预估成本".to_string(),
            sublabel: "按官方标准费率折算".to_string(),
            value: format!("${:.2}", today.cost),
            accent_rgb: (0x16, 0xa3, 0x4a),
        },
    ];

    for top in today.by.model.iter().take(2) {
        usage_rows.push(TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: top.name.clone(),
            sublabel: format!("{} 次请求 · ${:.2}", top.calls, top.cost),
            value: compact_tokens(top.tokens),
            accent_rgb: (0xd9, 0x77, 0x57),
        });
    }

    Menu {
        version: env!("CARGO_PKG_VERSION").to_string(),
        host,
        agents,
        usage_rows,
    }
}

// Logical card geometry (scaled by monitor DPI / 96.0)
pub(crate) const CARD_W: i32 = 308;
pub(crate) const PAD_X: i32 = 14;
pub(crate) const HEADER_H: i32 = 44;
pub(crate) const TABS_H: i32 = 34;
pub(crate) const TABS_GAP: i32 = 8;
pub(crate) const AGENT_ROW_H: i32 = 52;
pub(crate) const FOOTER_H: i32 = 46;
pub(crate) const RADIUS: i32 = 14;
pub(crate) const SHADOW_PAD: i32 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitTarget {
    GearSettings,
    Tab(TrayTab),
    Row(usize),
    FooterMain,
    FooterExit,
}

#[derive(Debug, Clone, Copy)]
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

pub(crate) struct CardLayout {
    pub scale: f32,
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
    pub row_rects: Vec<(RectI, usize)>,
    pub sep_y: i32,
    pub footer_main: RectI,
    pub footer_exit: RectI,
}

impl CardLayout {
    pub fn compute(menu: &Menu, tab: TrayTab, dpi: u32) -> Self {
        let scale = (dpi.max(96) as f32) / 96.0;
        let px = |v: i32| (v as f32 * scale).round() as i32;

        let pad = px(SHADOW_PAD);
        let card_x = pad;
        let card_y = pad;
        let card_w = px(CARD_W);
        let pad_x = px(PAD_X);

        let gear_size = px(28);
        let gear_top = card_y + (px(HEADER_H) - gear_size) / 2;
        let gear_rect = RectI {
            left: card_x + card_w - pad_x - gear_size,
            top: gear_top,
            right: card_x + card_w - pad_x,
            bottom: gear_top + gear_size,
        };

        let tabs_top = card_y + px(HEADER_H);
        let tabs_h = px(TABS_H);
        let tabs_track = RectI {
            left: card_x + pad_x,
            top: tabs_top,
            right: card_x + card_w - pad_x,
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

        let mut y = tabs_track.bottom + px(TABS_GAP);
        let row_h = px(AGENT_ROW_H);
        let count = match tab {
            TrayTab::Config => menu.agents.len().max(1),
            TrayTab::Usage => menu.usage_rows.len().max(1),
        };
        let mut row_rects = Vec::with_capacity(count);
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
                    left: card_x + px(8),
                    top: y,
                    right: card_x + card_w - px(8),
                    bottom: y + row_h,
                },
                action_id,
            ));
            y += row_h;
        }

        y += px(6);
        let sep_y = y;
        let footer_h = px(FOOTER_H);
        let footer_top = sep_y + px(6);
        let footer_bottom = sep_y + footer_h - px(6);
        let footer_gap = px(8);
        let footer_mid = card_x + (card_w * 58) / 100;
        let footer_main = RectI {
            left: card_x + pad_x,
            top: footer_top,
            right: footer_mid - footer_gap / 2,
            bottom: footer_bottom,
        };
        let footer_exit = RectI {
            left: footer_mid + footer_gap / 2,
            top: footer_top,
            right: card_x + card_w - pad_x,
            bottom: footer_bottom,
        };

        let card_h = (sep_y - card_y) + footer_h;
        Self {
            scale,
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
            row_rects,
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
        if self.tab_config.contains(x, y) {
            return Some(HitTarget::Tab(TrayTab::Config));
        }
        if self.tab_usage.contains(x, y) {
            return Some(HitTarget::Tab(TrayTab::Usage));
        }
        for (i, (rect, _)) in self.row_rects.iter().enumerate() {
            if rect.contains(x, y) {
                return Some(HitTarget::Row(i));
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

    pub fn action_for(&self, target: HitTarget) -> Option<usize> {
        match target {
            HitTarget::GearSettings => Some(ACTION_OPEN_SETTINGS),
            HitTarget::Tab(_) => None,
            HitTarget::Row(i) => self.row_rects.get(i).map(|(_, id)| *id),
            HitTarget::FooterMain => Some(ACTION_OPEN_MAIN),
            HitTarget::FooterExit => Some(ACTION_EXIT),
        }
    }
}

#[cfg(target_os = "windows")]
mod win32 {
    use super::*;
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush,
        DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_RIGHT,
        DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_BOLD, FW_NORMAL,
        FW_SEMIBOLD, FillRect, GetDC, GetMonitorInfoW, GetTextExtentPoint32W, HDC, HFONT, HGDIOBJ,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, ReleaseDC,
        SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    };
    use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE, VK_TAB,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowRect, HCURSOR, HWND_TOPMOST,
        IDC_ARROW, LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow,
        ULW_ALPHA, UpdateLayeredWindow, WA_INACTIVE, WM_ACTIVATE, WM_ERASEBKGND, WM_KEYDOWN,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    };

    const POPUP_CLASS: &str = "AgentHubRunnerPopupClass\0";
    const SETTLE: Duration = Duration::from_millis(400);

    const C_CARD: (u8, u8, u8) = (0xff, 0xff, 0xff);
    const C_BORDER: (u8, u8, u8) = (0xe2, 0xe4, 0xec);
    const C_HAIRLINE: (u8, u8, u8) = (0xee, 0xef, 0xf4);
    const C_TRACK: (u8, u8, u8) = (0xf2, 0xf3, 0xf7);
    const C_TEXT: (u8, u8, u8) = (0x18, 0x18, 0x1b);
    const C_SUB: (u8, u8, u8) = (0x71, 0x71, 0x7a);
    const C_CHEVRON: (u8, u8, u8) = (0xa1, 0xa1, 0xaa);
    const C_HOVER: (u8, u8, u8) = (0xf4, 0xf5, 0xf9);
    const C_BTN_BG: (u8, u8, u8) = (0xf6, 0xf7, 0xfa);
    const C_DANGER: (u8, u8, u8) = (0xdc, 0x26, 0x26);
    const C_HOVER_DANGER: (u8, u8, u8) = (0xfe, 0xf2, 0xf2);
    const C_DIRECT_BG: (u8, u8, u8) = (0xdc, 0xfc, 0xe7);
    const C_DIRECT_FG: (u8, u8, u8) = (0x15, 0x80, 0x3d);
    const C_ROUTED_BG: (u8, u8, u8) = (0xdb, 0xea, 0xfe);
    const C_ROUTED_FG: (u8, u8, u8) = (0x1d, 0x4e, 0xd8);

    fn colorref(c: (u8, u8, u8)) -> COLORREF {
        c.0 as u32 | ((c.1 as u32) << 8) | ((c.2 as u32) << 16)
    }

    fn to_win_rect(r: RectI) -> RECT {
        RECT {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    struct State {
        hwnd: HWND,
        menu: Menu,
        tab: TrayTab,
        dpi: u32,
        anchor: POINT,
        choice: fn(usize),
        layout: CardLayout,
        hover: Option<HitTarget>,
        pressed: Option<HitTarget>,
        visible: bool,
        hidden_at: Option<Instant>,
        shown_at: Option<Instant>,
        session: u64,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
        static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
    }

    pub fn closed_within(window: Duration) -> bool {
        STATE.with(|s| {
            s.borrow()
                .as_ref()
                .and_then(|st| st.hidden_at)
                .is_some_and(|at| at.elapsed() < window)
        })
    }

    pub fn is_visible() -> bool {
        STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.visible))
    }

    fn dpi_at(pt: POINT) -> u32 {
        unsafe {
            let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            if monitor.is_null() {
                return 96;
            }
            let (mut x, mut y) = (96u32, 96u32);
            if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) == 0 {
                x.max(96)
            } else {
                96
            }
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

    fn place_near_anchor(anchor: POINT, layout: &CardLayout) -> POINT {
        let work = work_area(anchor);
        let x = (anchor.x - layout.card_w / 2 - layout.card_x).clamp(
            work.left + 4,
            (work.right - layout.win_w + layout.card_x - 6).max(work.left + 4),
        );
        let mut y = anchor.y - layout.win_h + layout.card_y - 6;
        if y < work.top + 4 {
            y = (anchor.y - 12).min(work.bottom - layout.win_h + layout.card_y - 4);
        }
        y = y.max(work.top - layout.card_y + 4);
        POINT { x, y }
    }

    /// Shows the tray popup card near the cursor.
    ///
    /// # Safety
    /// `hinstance` must be this process's module handle.
    pub unsafe fn show(hinstance: HINSTANCE, menu: Menu, choice: fn(usize)) {
        unsafe {
            let hwnd = ensure_window(hinstance);
            if hwnd.is_null() {
                return;
            }
            let mut cursor: POINT = zeroed();
            GetCursorPos(&mut cursor);
            let dpi = dpi_at(cursor);
            let tab = STATE
                .with(|s| s.borrow().as_ref().map(|st| st.tab))
                .unwrap_or_default();
            let layout = CardLayout::compute(&menu, tab, dpi);
            let placement = place_near_anchor(cursor, &layout);

            STATE.with(|s| {
                let mut guard = s.borrow_mut();
                let next_session = guard
                    .as_ref()
                    .map(|st| st.session.wrapping_add(1))
                    .unwrap_or(1);
                *guard = Some(State {
                    hwnd,
                    menu,
                    tab,
                    dpi,
                    anchor: cursor,
                    choice,
                    layout,
                    hover: None,
                    pressed: None,
                    visible: true,
                    hidden_at: None,
                    shown_at: Some(Instant::now()),
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
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
            SetFocus(hwnd);
        }
    }

    pub fn hide_if_clicked_away() {
        unsafe {
            let mut cursor: POINT = zeroed();
            GetCursorPos(&mut cursor);
            let state = STATE.with(|s| {
                s.borrow().as_ref().map(|st| {
                    (
                        st.hwnd,
                        st.visible,
                        st.shown_at,
                        st.layout.card_x,
                        st.layout.card_y,
                        st.layout.card_w,
                        st.layout.card_h,
                    )
                })
            });
            let Some((hwnd, visible, shown_at, card_x, card_y, card_w, card_h)) = state else {
                return;
            };
            if !visible || shown_at.is_none_or(|at| at.elapsed() < SETTLE) {
                return;
            }
            let mut rect: RECT = zeroed();
            GetWindowRect(hwnd, &mut rect);
            let inside = cursor.x >= rect.left + card_x
                && cursor.x < rect.left + card_x + card_w
                && cursor.y >= rect.top + card_y
                && cursor.y < rect.top + card_y + card_h;
            if !inside {
                // Only auto-dismiss when mouse leaves if foreground was already lost
                let fg = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
                if fg != hwnd {
                    hide();
                }
            }
        }
    }

    pub fn hide() {
        let hwnd = STATE.with(|s| s.borrow().as_ref().map(|st| st.hwnd));
        if let Some(hwnd) = hwnd {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        STATE.with(|s| {
            if let Some(st) = s.borrow_mut().as_mut() {
                if st.visible {
                    st.visible = false;
                    st.hidden_at = Some(Instant::now());
                }
                st.hover = None;
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

    fn choose(id: usize) {
        let choice = STATE.with(|s| s.borrow().as_ref().map(|st| st.choice));
        hide();
        if let Some(choice) = choice {
            choice(id);
        }
    }

    unsafe fn switch_tab(hwnd: HWND, new_tab: TrayTab) {
        let maybe_pos = STATE.with(|s| {
            let mut guard = s.borrow_mut();
            let st = guard.as_mut()?;
            if st.tab == new_tab {
                return None;
            }
            st.tab = new_tab;
            st.layout = CardLayout::compute(&st.menu, st.tab, st.dpi);
            st.hover = None;
            st.pressed = None;
            st.session = st.session.wrapping_add(1);
            Some(place_near_anchor(st.anchor, &st.layout))
        });
        if let Some(pos) = maybe_pos {
            unsafe { paint(hwnd, pos) };
        }
    }

    unsafe extern "system" fn popup_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            let x = (lparam & 0xffff) as i16 as i32;
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            match msg {
                WM_MOUSEMOVE => {
                    let mut track: TRACKMOUSEEVENT = zeroed();
                    track.cbSize = size_of::<TRACKMOUSEEVENT>() as u32;
                    track.dwFlags = TME_LEAVE;
                    track.hwndTrack = hwnd;
                    TrackMouseEvent(&mut track);
                    let changed = STATE.with(|s| {
                        let mut guard = s.borrow_mut();
                        let Some(st) = guard.as_mut() else {
                            return false;
                        };
                        let hit = st.layout.hit(x, y);
                        if hit != st.hover {
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
                        let mut guard = s.borrow_mut();
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
                    STATE.with(|s| {
                        if let Some(st) = s.borrow_mut().as_mut() {
                            st.pressed = st.layout.hit(x, y);
                        }
                    });
                    0
                }
                WM_LBUTTONUP => {
                    let hit = STATE.with(|s| {
                        let guard = s.borrow();
                        let st = guard.as_ref()?;
                        let cur = st.layout.hit(x, y)?;
                        (Some(cur) == st.pressed).then_some((cur, st.layout.action_for(cur)))
                    });
                    match hit {
                        Some((HitTarget::Tab(tab), _)) => switch_tab(hwnd, tab),
                        Some((_, Some(action_id))) => choose(action_id),
                        _ => {}
                    }
                    0
                }
                WM_KEYDOWN => {
                    let vk = wparam as u16;
                    if vk == VK_ESCAPE {
                        hide();
                    } else if vk == VK_TAB {
                        let next = STATE
                            .with(|s| {
                                s.borrow().as_ref().map(|st| match st.tab {
                                    TrayTab::Config => TrayTab::Usage,
                                    TrayTab::Usage => TrayTab::Config,
                                })
                            })
                            .unwrap_or_default();
                        switch_tab(hwnd, next);
                    }
                    0
                }
                WM_ACTIVATE => {
                    if (wparam & 0xffff) as u32 == WA_INACTIVE {
                        let settled = STATE.with(|s| {
                            s.borrow()
                                .as_ref()
                                .and_then(|st| st.shown_at)
                                .is_none_or(|at| at.elapsed() > SETTLE)
                        });
                        if settled {
                            hide();
                        }
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
        }
    }

    fn arrow_cursor() -> HCURSOR {
        static ARROW: OnceLock<usize> = OnceLock::new();
        *ARROW.get_or_init(|| unsafe { LoadCursorW(0 as _, IDC_ARROW) } as usize) as HCURSOR
    }

    unsafe fn repaint(hwnd: HWND) {
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

    unsafe fn paint(hwnd: HWND, at: POINT) {
        unsafe {
            STATE.with(|s| {
                let guard = s.borrow();
                let Some(st) = guard.as_ref() else {
                    return;
                };
                FRAME.with(|f| {
                    let mut slot = f.borrow_mut();
                    let rebuild = slot
                        .as_ref()
                        .is_none_or(|frame| frame.session != st.session || frame.hover != st.hover);
                    if rebuild {
                        *slot = Some(Frame::new(
                            st.session, &st.menu, st.tab, &st.layout, st.hover,
                        ));
                    }
                    if let Some(frame) = slot.as_mut() {
                        frame.present(hwnd, at);
                    }
                });
            });
        }
    }

    struct Frame {
        session: u64,
        hover: Option<HitTarget>,
        w: i32,
        h: i32,
        image: Vec<u8>,
        upload: Option<Upload>,
        dirty: bool,
    }

    impl Frame {
        fn new(
            session: u64,
            menu: &Menu,
            tab: TrayTab,
            layout: &CardLayout,
            hover: Option<HitTarget>,
        ) -> Self {
            let w = layout.win_w;
            let h = layout.win_h;
            let s = layout.scale;

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
                0.18,
            );

            let mut image = vec![0u8; (w * h * 4) as usize];
            for (i, px) in image.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (r, g, b, a) = card_color(dist[i], shadow[i]);
                px[0] = r;
                px[1] = g;
                px[2] = b;
                px[3] = a;
            }

            // Segmented tabs track + active white pill
            fill_round_rect(
                &mut image,
                w,
                h,
                &dist,
                layout.tabs_track,
                9.0 * s,
                C_TRACK,
                None,
            );
            let active_tab_rect = match tab {
                TrayTab::Config => layout.tab_config,
                TrayTab::Usage => layout.tab_usage,
            };
            fill_round_rect(
                &mut image,
                w,
                h,
                &dist,
                active_tab_rect,
                7.0 * s,
                C_CARD,
                Some(C_BORDER),
            );

            // Gear hover pill
            if hover == Some(HitTarget::GearSettings) {
                fill_round_rect(
                    &mut image,
                    w,
                    h,
                    &dist,
                    layout.gear_rect,
                    8.0 * s,
                    C_HOVER,
                    None,
                );
            }

            // Rows: hover fill + brand/accent badge + route pill
            for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                if hover == Some(HitTarget::Row(idx)) {
                    fill_round_rect(&mut image, w, h, &dist, *rect, 9.0 * s, C_HOVER, None);
                }
                if tab == TrayTab::Config
                    && let Some(agent) = menu.agents.get(idx)
                {
                    let badge_sz = (32.0 * s).round() as i32;
                    let bx = rect.left + (8.0 * s).round() as i32;
                    let by = rect.top + (rect.height() - badge_sz) / 2;
                    let badge = RectI {
                        left: bx,
                        top: by,
                        right: bx + badge_sz,
                        bottom: by + badge_sz,
                    };
                    fill_round_rect(
                        &mut image,
                        w,
                        h,
                        &dist,
                        badge,
                        9.0 * s,
                        agent.brand_bg_rgb,
                        None,
                    );
                } else if tab == TrayTab::Usage
                    && let Some(urow) = menu.usage_rows.get(idx)
                {
                    let bar_w = (4.0 * s).round().max(2.0) as i32;
                    let bar_h = (26.0 * s).round() as i32;
                    let bx = rect.left + (10.0 * s).round() as i32;
                    let by = rect.top + (rect.height() - bar_h) / 2;
                    fill_round_rect(
                        &mut image,
                        w,
                        h,
                        &dist,
                        RectI {
                            left: bx,
                            top: by,
                            right: bx + bar_w,
                            bottom: by + bar_h,
                        },
                        2.0 * s,
                        urow.accent_rgb,
                        None,
                    );
                }
            }

            // Footer buttons (`打开主窗口` and `退出`)
            let main_bg = if hover == Some(HitTarget::FooterMain) {
                C_HOVER
            } else {
                C_BTN_BG
            };
            fill_round_rect(
                &mut image,
                w,
                h,
                &dist,
                layout.footer_main,
                8.0 * s,
                main_bg,
                Some(C_BORDER),
            );
            let exit_bg = if hover == Some(HitTarget::FooterExit) {
                C_HOVER_DANGER
            } else {
                C_BTN_BG
            };
            fill_round_rect(
                &mut image,
                w,
                h,
                &dist,
                layout.footer_exit,
                8.0 * s,
                exit_bg,
                Some(C_BORDER),
            );

            // Typography & crisp badges via GDI
            unsafe { draw_typography(&mut image, w, h, &dist, menu, tab, layout, hover) };

            let upload = unsafe { Upload::new(w, h) };
            let dirty = upload.is_some();
            Self {
                session,
                hover,
                w,
                h,
                image,
                upload,
                dirty,
            }
        }

        unsafe fn present(&mut self, hwnd: HWND, at: POINT) {
            unsafe {
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

    #[allow(clippy::too_many_arguments)]
    fn fill_round_rect(
        image: &mut [u8],
        w: i32,
        h: i32,
        card_dist: &[f32],
        rect: RectI,
        radius: f32,
        fill: (u8, u8, u8),
        border: Option<(u8, u8, u8)>,
    ) {
        let cx = rect.left as f32 + rect.width() as f32 / 2.0;
        let cy = rect.top as f32 + rect.height() as f32 / 2.0;
        let hw = rect.width() as f32 / 2.0;
        let hh = rect.height() as f32 / 2.0;
        let y0 = (rect.top - 2).clamp(0, h);
        let y1 = (rect.bottom + 2).clamp(0, h);
        let x0 = (rect.left - 2).clamp(0, w);
        let x1 = (rect.right + 2).clamp(0, w);
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

    fn card_color(dist: f32, shadow: f32) -> (u8, u8, u8, u8) {
        let cov = (0.5 - dist).clamp(0.0, 1.0);
        let alpha = cov + shadow * (1.0 - cov);
        if alpha <= 0.001 {
            return (0, 0, 0, 0);
        }
        let border = (1.0 - (dist + 0.5).abs()).clamp(0.0, 1.0);
        let rgb = lerp(C_CARD, C_BORDER, border);
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
                let mut bits: *mut c_void = std::ptr::null_mut();
                let dib = CreateDIBSection(
                    hdc,
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bits,
                    std::ptr::null_mut(),
                    0,
                );
                if dib.is_null() {
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
            unsafe {
                let y0 = y0.clamp(0, self.h);
                let y1 = y1.clamp(y0, self.h);
                let start = (y0 * self.w * 4) as usize;
                let end = (y1 * self.w * 4) as usize;
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

        unsafe fn present(&self, hwnd: HWND, at: POINT, w: i32, h: i32) {
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

    fn sd_round_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
        let qx = (px - cx).abs() - (hw - r);
        let qy = (py - cy).abs() - (hh - r);
        let ax = qx.max(0.0);
        let ay = qy.max(0.0);
        (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
    }

    fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
        let t = t.clamp(0.0, 1.0);
        let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
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

    struct Gdi {
        hdc: HDC,
        dib: windows_sys::Win32::Graphics::Gdi::HBITMAP,
        old: HGDIOBJ,
        bits: *mut u8,
    }

    impl Gdi {
        unsafe fn new(px: &[u8], w: i32, h: i32) -> Option<Self> {
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
                let mut bits: *mut c_void = std::ptr::null_mut();
                let dib = CreateDIBSection(
                    hdc,
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bits,
                    std::ptr::null_mut(),
                    0,
                );
                if dib.is_null() {
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
                })
            }
        }

        unsafe fn discard(self) {
            unsafe {
                SelectObject(self.hdc, self.old);
                DeleteObject(self.dib as HGDIOBJ);
                DeleteDC(self.hdc);
            }
        }

        unsafe fn finish(self, px: &mut [u8]) {
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
                self.discard();
            }
        }

        unsafe fn fill(&self, x: i32, y: i32, w: i32, h: i32, c: (u8, u8, u8)) {
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

        unsafe fn text(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
            unsafe {
                let old = SelectObject(self.hdc, font as HGDIOBJ);
                SetBkMode(self.hdc, TRANSPARENT as i32);
                SetTextColor(self.hdc, colorref(c));
                let mut text: Vec<u16> = s.encode_utf16().collect();
                let mut rect = rect;
                DrawTextW(
                    self.hdc,
                    text.as_mut_ptr(),
                    text.len() as i32,
                    &mut rect,
                    DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                );
                SelectObject(self.hdc, old);
            }
        }

        unsafe fn text_center(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
            unsafe {
                let old = SelectObject(self.hdc, font as HGDIOBJ);
                SetBkMode(self.hdc, TRANSPARENT as i32);
                SetTextColor(self.hdc, colorref(c));
                let mut text: Vec<u16> = s.encode_utf16().collect();
                let mut rect = rect;
                DrawTextW(
                    self.hdc,
                    text.as_mut_ptr(),
                    text.len() as i32,
                    &mut rect,
                    DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                );
                SelectObject(self.hdc, old);
            }
        }

        unsafe fn text_right(&self, s: &str, font: HFONT, c: (u8, u8, u8), rect: RECT) {
            unsafe {
                let old = SelectObject(self.hdc, font as HGDIOBJ);
                SetBkMode(self.hdc, TRANSPARENT as i32);
                SetTextColor(self.hdc, colorref(c));
                let mut text: Vec<u16> = s.encode_utf16().collect();
                let mut rect = rect;
                DrawTextW(
                    self.hdc,
                    text.as_mut_ptr(),
                    text.len() as i32,
                    &mut rect,
                    DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX,
                );
                SelectObject(self.hdc, old);
            }
        }

        unsafe fn text_width(&self, s: &str, font: HFONT) -> i32 {
            unsafe {
                let old = SelectObject(self.hdc, font as HGDIOBJ);
                let mut wide: Vec<u16> = s.encode_utf16().collect();
                let mut size: SIZE = zeroed();
                GetTextExtentPoint32W(self.hdc, wide.as_mut_ptr(), wide.len() as i32, &mut size);
                SelectObject(self.hdc, old);
                size.cx
            }
        }
    }

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
                ANTIALIASED_QUALITY as u32,
                DEFAULT_PITCH as u32,
                name.as_mut_ptr(),
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn draw_typography(
        px: &mut [u8],
        w: i32,
        h: i32,
        card_dist: &[f32],
        menu: &Menu,
        tab: TrayTab,
        layout: &CardLayout,
        hover: Option<HitTarget>,
    ) {
        unsafe {
            // Pre-compute pill background positions before locking GDI over `px`
            let s = layout.scale;
            if tab == TrayTab::Config {
                let Some(probe) = Gdi::new(px, w, h) else {
                    return;
                };
                let f_sub = font(s, 11.5, FW_NORMAL, "Microsoft YaHei UI");
                let f_pill = font(s, 10.5, FW_SEMIBOLD, "Microsoft YaHei UI");
                let mut pills = Vec::new();
                for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                    if let Some(agent) = menu.agents.get(idx) {
                        let text_left = rect.left + (48.0 * s).round() as i32;
                        let prov_w = probe.text_width(&agent.provider_label, f_sub);
                        let badge_label = if agent.route_mode == "routed" {
                            "路由"
                        } else {
                            "直连"
                        };
                        let pill_w =
                            probe.text_width(badge_label, f_pill) + (12.0 * s).round() as i32;
                        let pill_h = (16.0 * s).round() as i32;
                        let sub_cy = rect.top + (rect.height() * 68) / 100;
                        let px_left = (text_left + prov_w + (6.0 * s).round() as i32)
                            .min(rect.right - pill_w - (22.0 * s).round() as i32);
                        let pill_rect = RectI {
                            left: px_left,
                            top: sub_cy - pill_h / 2,
                            right: px_left + pill_w,
                            bottom: sub_cy - pill_h / 2 + pill_h,
                        };
                        let bg = if agent.route_mode == "routed" {
                            C_ROUTED_BG
                        } else {
                            C_DIRECT_BG
                        };
                        pills.push((pill_rect, bg));
                    }
                }
                for f in [f_sub, f_pill] {
                    DeleteObject(f as HGDIOBJ);
                }
                probe.discard();
                for (pill_rect, bg) in pills {
                    fill_round_rect(px, w, h, card_dist, pill_rect, 4.5 * s, bg, None);
                }
            }

            let Some(g) = Gdi::new(px, w, h) else {
                return;
            };
            let pad_x = (PAD_X as f32 * s).round() as i32;

            let f_brand = font(s, 15.0, FW_BOLD, "Segoe UI");
            let f_gear = font(s, 15.0, FW_NORMAL, "Segoe UI Symbol");
            let f_tab_on = font(s, 12.5, FW_SEMIBOLD, "Microsoft YaHei UI");
            let f_tab_off = font(s, 12.5, FW_NORMAL, "Microsoft YaHei UI");
            let f_name = font(s, 13.5, FW_SEMIBOLD, "Segoe UI");
            let f_name_cn = font(s, 13.0, FW_SEMIBOLD, "Microsoft YaHei UI");
            let f_glyph = font(s, 14.0, FW_BOLD, "Segoe UI");
            let f_sub = font(s, 11.5, FW_NORMAL, "Microsoft YaHei UI");
            let f_pill = font(s, 10.5, FW_SEMIBOLD, "Microsoft YaHei UI");
            let f_chev = font(s, 16.0, FW_NORMAL, "Segoe UI");
            let f_btn = font(s, 12.5, FW_SEMIBOLD, "Microsoft YaHei UI");

            // Header: "AgentHub" + gear icon
            let header_rect = RECT {
                left: layout.card_x + pad_x,
                top: layout.card_y,
                right: layout.gear_rect.left - (4.0 * s) as i32,
                bottom: layout.card_y + (HEADER_H as f32 * s).round() as i32,
            };
            g.text("AgentHub", f_brand, C_TEXT, header_rect);
            g.text_center("⚙", f_gear, C_SUB, to_win_rect(layout.gear_rect));

            // Segmented Tabs: 配置 | 用量
            g.text_center(
                "配置",
                if tab == TrayTab::Config {
                    f_tab_on
                } else {
                    f_tab_off
                },
                if tab == TrayTab::Config {
                    C_TEXT
                } else {
                    C_SUB
                },
                to_win_rect(layout.tab_config),
            );
            g.text_center(
                "用量",
                if tab == TrayTab::Usage {
                    f_tab_on
                } else {
                    f_tab_off
                },
                if tab == TrayTab::Usage { C_TEXT } else { C_SUB },
                to_win_rect(layout.tab_usage),
            );

            // Body rows
            match tab {
                TrayTab::Config => {
                    for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                        if let Some(agent) = menu.agents.get(idx) {
                            let badge_sz = (32.0 * s).round() as i32;
                            let bx = rect.left + (8.0 * s).round() as i32;
                            let by = rect.top + (rect.height() - badge_sz) / 2;
                            g.text_center(
                                agent.badge_glyph,
                                f_glyph,
                                agent.brand_rgb,
                                RECT {
                                    left: bx,
                                    top: by,
                                    right: bx + badge_sz,
                                    bottom: by + badge_sz,
                                },
                            );

                            let text_left = rect.left + (48.0 * s).round() as i32;
                            let chev_right = rect.right - (10.0 * s).round() as i32;
                            g.text(
                                &agent.display_name,
                                f_name,
                                C_TEXT,
                                RECT {
                                    left: text_left,
                                    top: rect.top + (5.0 * s).round() as i32,
                                    right: chev_right - (14.0 * s).round() as i32,
                                    bottom: rect.top + (rect.height() * 52) / 100,
                                },
                            );

                            let prov_w = g.text_width(&agent.provider_label, f_sub);
                            let sub_top = rect.top + (rect.height() * 48) / 100;
                            let sub_bottom = rect.bottom - (4.0 * s).round() as i32;
                            g.text(
                                &agent.provider_label,
                                f_sub,
                                C_SUB,
                                RECT {
                                    left: text_left,
                                    top: sub_top,
                                    right: (text_left + prov_w + 4)
                                        .min(chev_right - (48.0 * s).round() as i32),
                                    bottom: sub_bottom,
                                },
                            );

                            let (badge_label, fg) = if agent.route_mode == "routed" {
                                ("路由", C_ROUTED_FG)
                            } else {
                                ("直连", C_DIRECT_FG)
                            };
                            let pill_w =
                                g.text_width(badge_label, f_pill) + (12.0 * s).round() as i32;
                            let pill_h = (16.0 * s).round() as i32;
                            let sub_cy = rect.top + (rect.height() * 68) / 100;
                            let px_left = (text_left + prov_w + (6.0 * s).round() as i32)
                                .min(rect.right - pill_w - (22.0 * s).round() as i32);
                            g.text_center(
                                badge_label,
                                f_pill,
                                fg,
                                RECT {
                                    left: px_left,
                                    top: sub_cy - pill_h / 2,
                                    right: px_left + pill_w,
                                    bottom: sub_cy - pill_h / 2 + pill_h,
                                },
                            );

                            g.text_right(
                                "›",
                                f_chev,
                                C_CHEVRON,
                                RECT {
                                    left: chev_right - (16.0 * s).round() as i32,
                                    top: rect.top,
                                    right: chev_right,
                                    bottom: rect.bottom,
                                },
                            );
                        }
                    }
                }
                TrayTab::Usage => {
                    for (idx, (rect, _)) in layout.row_rects.iter().enumerate() {
                        if let Some(urow) = menu.usage_rows.get(idx) {
                            let text_left = rect.left + (22.0 * s).round() as i32;
                            let right_edge = rect.right - (10.0 * s).round() as i32;
                            g.text(
                                &urow.label,
                                f_name_cn,
                                C_TEXT,
                                RECT {
                                    left: text_left,
                                    top: rect.top + (5.0 * s).round() as i32,
                                    right: right_edge - (76.0 * s).round() as i32,
                                    bottom: rect.top + (rect.height() * 52) / 100,
                                },
                            );
                            g.text(
                                &urow.sublabel,
                                f_sub,
                                C_SUB,
                                RECT {
                                    left: text_left,
                                    top: rect.top + (rect.height() * 48) / 100,
                                    right: right_edge - (64.0 * s).round() as i32,
                                    bottom: rect.bottom - (4.0 * s).round() as i32,
                                },
                            );
                            g.text_right(
                                &urow.value,
                                f_name,
                                C_TEXT,
                                RECT {
                                    left: right_edge - (88.0 * s).round() as i32,
                                    top: rect.top,
                                    right: right_edge,
                                    bottom: rect.bottom,
                                },
                            );
                        }
                    }
                }
            }

            // Footer hairline + two buttons
            g.fill(
                layout.card_x + pad_x,
                layout.sep_y,
                layout.card_w - 2 * pad_x,
                1,
                C_HAIRLINE,
            );
            g.text_center("打开主窗口", f_btn, C_TEXT, to_win_rect(layout.footer_main));
            let exit_fg = if hover == Some(HitTarget::FooterExit) {
                C_DANGER
            } else {
                C_SUB
            };
            g.text_center("退出", f_btn, exit_fg, to_win_rect(layout.footer_exit));

            g.finish(px);
            for f in [
                f_brand, f_gear, f_tab_on, f_tab_off, f_name, f_name_cn, f_glyph, f_sub, f_pill,
                f_chev, f_btn,
            ] {
                DeleteObject(f as HGDIOBJ);
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use win32::{closed_within, hide, hide_if_clicked_away, is_visible, show};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_layout_hits_tabs_gear_rows_and_footer() {
        let menu = build_live_menu("127.0.0.1:4200".to_string());
        assert!(
            menu.agents.len() >= 4,
            "core agent rows (Claude, Claude Desktop, Codex, Gemini) are always populated"
        );
        assert!(
            menu.usage_rows.len() >= 3,
            "usage KPI rows are always populated"
        );

        let layout = CardLayout::compute(&menu, TrayTab::Config, 96);
        let gear_mid = (
            (layout.gear_rect.left + layout.gear_rect.right) / 2,
            (layout.gear_rect.top + layout.gear_rect.bottom) / 2,
        );
        assert_eq!(
            layout.hit(gear_mid.0, gear_mid.1),
            Some(HitTarget::GearSettings)
        );
        assert_eq!(
            layout.action_for(HitTarget::GearSettings),
            Some(ACTION_OPEN_SETTINGS)
        );

        let usage_tab_mid = (
            (layout.tab_usage.left + layout.tab_usage.right) / 2,
            (layout.tab_usage.top + layout.tab_usage.bottom) / 2,
        );
        assert_eq!(
            layout.hit(usage_tab_mid.0, usage_tab_mid.1),
            Some(HitTarget::Tab(TrayTab::Usage))
        );
        assert_eq!(layout.action_for(HitTarget::Tab(TrayTab::Usage)), None);

        let main_btn_mid = (
            (layout.footer_main.left + layout.footer_main.right) / 2,
            (layout.footer_main.top + layout.footer_main.bottom) / 2,
        );
        assert_eq!(
            layout.hit(main_btn_mid.0, main_btn_mid.1),
            Some(HitTarget::FooterMain)
        );
        assert_eq!(
            layout.action_for(HitTarget::FooterMain),
            Some(ACTION_OPEN_MAIN)
        );

        let exit_btn_mid = (
            (layout.footer_exit.left + layout.footer_exit.right) / 2,
            (layout.footer_exit.top + layout.footer_exit.bottom) / 2,
        );
        assert_eq!(
            layout.hit(exit_btn_mid.0, exit_btn_mid.1),
            Some(HitTarget::FooterExit)
        );
        assert_eq!(layout.action_for(HitTarget::FooterExit), Some(ACTION_EXIT));
    }
}
