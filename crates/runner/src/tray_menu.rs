//! The tray's menu, drawn by us.
//!
//! A `TrackPopupMenu` menu is painted by the shell: its type size, row height
//! and colours all come from the system, and the one lever over its theme is
//! `SetPreferredAppMode`, whose `2` is *force dark*, not *allow dark* — which
//! is how the console ended up with a dark slab over a light desktop. So the
//! menu is a window of our own: a light card drawn into a layered window, with
//! the type and the rhythm chosen here.
//!
//! The shell still owns dismissal: the window takes the foreground the way
//! `TrackPopupMenu` does, so a click anywhere else arrives as `WM_ACTIVATE` /
//! `WA_INACTIVE` and closes it. `Esc`, a second press of the tray icon, and
//! choosing a row close it the same way.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CLIP_DEFAULT_PRECIS,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    DeleteDC, DeleteObject, DrawTextW, Ellipse, FW_NORMAL, FW_SEMIBOLD, FillRect, GetDC,
    GetMonitorInfoW, GetStockObject, GetTextExtentPoint32W, HDC, HFONT, HGDIOBJ, LineTo,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MoveToEx, NULL_BRUSH,
    OUT_DEFAULT_PRECIS, PS_SOLID, Polygon, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowRect, HWND_TOPMOST, RegisterClassW,
    SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow,
    SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WA_INACTIVE, WM_ACTIVATE,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const POPUP_CLASS: &str = "AgentHubRunnerPopupClass\0";

// The card, in logical pixels; one DPI scale sets the whole thing.
const CARD_W: i32 = 292;
const PAD_X: i32 = 15;
const HEADER_H: i32 = 68;
const ROW_H: i32 = 43;
const SEP_H: i32 = 13;
const FOOT_H: i32 = 11;
const TEXT_X: i32 = 45;
const RADIUS: i32 = 17;
/// Room for the shadow, so it is not clipped by the window.
const SHADOW_PAD: i32 = 16;
/// How long after the card appears a lost foreground is the tray flyout
/// closing rather than the reader clicking somewhere else.
const SETTLE: Duration = Duration::from_millis(400);

const C_CARD: (u8, u8, u8) = (0xff, 0xff, 0xff);
const C_BORDER: (u8, u8, u8) = (0xdc, 0xdc, 0xe4);
const C_HAIRLINE: (u8, u8, u8) = (0xee, 0xee, 0xf2);
const C_TEXT: (u8, u8, u8) = (0x1f, 0x1f, 0x24);
const C_MUTED: (u8, u8, u8) = (0x8a, 0x8a, 0x93);
const C_BRAND: (u8, u8, u8) = (0x02, 0x84, 0xc7);
const C_GREEN: (u8, u8, u8) = (0x18, 0x9a, 0x58);
const C_DANGER: (u8, u8, u8) = (0xd6, 0x45, 0x45);
const C_HOVER: (u8, u8, u8) = (0xf2, 0xf2, 0xf7);
const C_HOVER_DANGER: (u8, u8, u8) = (0xfd, 0xf0, 0xf0);

fn colorref(c: (u8, u8, u8)) -> COLORREF {
    c.0 as u32 | ((c.1 as u32) << 8) | ((c.2 as u32) << 16)
}

/// One row the menu offers. `id` is the caller's own action number.
pub struct Item {
    pub id: usize,
    pub label: &'static str,
    pub danger: bool,
    /// A hairline above this row, with air around it.
    pub separator_before: bool,
}

pub struct Menu {
    pub version: String,
    pub host: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Copy)]
enum Row {
    Item {
        id: usize,
        label: &'static str,
        danger: bool,
    },
    Separator,
}

/// A row's place on the card, in device pixels from the card's top.
#[derive(Clone, Copy)]
struct RowGeom {
    row: Row,
    top: i32,
    height: i32,
}

struct Layout {
    scale: f32,
    win_w: i32,
    win_h: i32,
    card_x: i32,
    card_y: i32,
    card_w: i32,
    card_h: i32,
    rows: Vec<RowGeom>,
}

impl Layout {
    fn new(menu: &Menu, dpi: u32) -> Self {
        let scale = dpi as f32 / 96.0;
        let px = |v: i32| (v as f32 * scale).round() as i32;
        let mut rows = Vec::new();
        let mut y = px(HEADER_H);
        for item in &menu.items {
            if item.separator_before {
                rows.push(RowGeom {
                    row: Row::Separator,
                    top: y,
                    height: px(SEP_H),
                });
                y += px(SEP_H);
            }
            rows.push(RowGeom {
                row: Row::Item {
                    id: item.id,
                    label: item.label,
                    danger: item.danger,
                },
                top: y,
                height: px(ROW_H),
            });
            y += px(ROW_H);
        }
        let card_w = px(CARD_W);
        let card_h = y + px(FOOT_H);
        let pad = px(SHADOW_PAD);
        Self {
            scale,
            win_w: card_w + pad * 2,
            win_h: card_h + pad * 2,
            card_x: pad,
            card_y: pad,
            card_w,
            card_h,
            rows,
        }
    }

    /// The item row under a point in window coordinates.
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        if x < self.card_x || x >= self.card_x + self.card_w {
            return None;
        }
        let y = y - self.card_y;
        self.rows
            .iter()
            .position(|r| matches!(r.row, Row::Item { .. }) && y >= r.top && y < r.top + r.height)
    }

    /// The id of the item row at `index`, if it is one.
    fn id_at(&self, index: usize) -> Option<usize> {
        match self.rows.get(index)?.row {
            Row::Item { id, .. } => Some(id),
            Row::Separator => None,
        }
    }
}

struct State {
    hwnd: HWND,
    menu: Menu,
    choice: fn(usize),
    layout: Layout,
    hover: Option<usize>,
    pressed: Option<usize>,
    visible: bool,
    hidden_at: Option<Instant>,
    /// When the card was last put on screen. Windows tells a window it has
    /// lost the foreground as the tray flyout closes over the press that
    /// opened the menu, so a deactivation in the first moments is that, not
    /// the reader clicking away.
    shown_at: Option<Instant>,
    /// Bumped by every `show`, so the cached card is redrawn when a new menu
    /// goes up and reused while the pointer moves over it.
    session: u64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Whether the menu closed this recently: a press of the tray icon lands right
/// after the popup has already closed itself, and means "close", not "open".
pub fn closed_within(window: Duration) -> bool {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|st| st.hidden_at)
            .is_some_and(|at| at.elapsed() < window)
    })
}

fn empty_menu() -> Menu {
    Menu {
        version: String::new(),
        host: String::new(),
        items: Vec::new(),
    }
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

/// Shows the menu at the cursor. `choice` is called on the tray thread with
/// the chosen item's id, once the menu has closed.
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
        let layout = Layout::new(&menu, dpi_at(cursor));

        let placement = STATE.with(|s| {
            let mut guard = s.borrow_mut();
            let state = guard.get_or_insert_with(|| State {
                hwnd,
                menu: empty_menu(),
                choice,
                layout: Layout::new(&empty_menu(), 96),
                hover: None,
                pressed: None,
                visible: false,
                hidden_at: None,
                shown_at: None,
                session: 0,
            });
            state.hwnd = hwnd;
            state.menu = menu;
            state.choice = choice;
            state.hover = None;
            state.pressed = None;
            state.visible = true;
            state.hidden_at = None;
            state.shown_at = Some(Instant::now());
            state.layout = layout;
            state.session = state.session.wrapping_add(1);
            // above the cursor as the shell's menus are, below it when there
            // is no room, never past the screen's edges
            let work = work_area(cursor);
            let (win_w, win_h, card_x, card_y) = (
                state.layout.win_w,
                state.layout.win_h,
                state.layout.card_x,
                state.layout.card_y,
            );
            let x = (cursor.x - card_x).clamp(
                work.left + 4,
                (work.right - win_w + card_x).max(work.left + 4),
            );
            let mut y = cursor.y - win_h + card_y - 4;
            if y < work.top + 4 {
                y = (cursor.y - 12).min(work.bottom - win_h + card_y);
            }
            y = y.max(work.top - card_y + 4);
            POINT { x, y }
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
        // the foreground is what turns a click elsewhere into WA_INACTIVE,
        // which is how the menu closes itself
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
    }
}

/// Puts the menu away if the reader clicked somewhere that is not it. A menu
/// opened from the tray flyout loses the foreground when the flyout closes
/// over the very press that opened it, and Windows does not reliably hand it
/// back, so a click elsewhere is not always reported as `WA_INACTIVE`. Called
/// on the tray thread's own timer: one pass, no polling loop.
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
        // the card's place in screen coordinates
        let mut rect: RECT = zeroed();
        GetWindowRect(hwnd, &mut rect);
        let inside = cursor.x >= rect.left + card_x
            && cursor.x < rect.left + card_x + card_w
            && cursor.y >= rect.top + card_y
            && cursor.y < rect.top + card_y + card_h;
        if !inside {
            hide();
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
            hCursor: 0 as _,
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

/// Runs the chosen id, after the menu is out of the way.
fn choose(id: usize) {
    let choice = STATE.with(|s| s.borrow().as_ref().map(|st| st.choice));
    hide();
    if let Some(choice) = choice {
        choice(id);
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
                let chosen = STATE.with(|s| {
                    let guard = s.borrow();
                    let st = guard.as_ref()?;
                    let hit = st.layout.hit(x, y)?;
                    (Some(hit) == st.pressed)
                        .then(|| st.layout.id_at(hit))
                        .flatten()
                });
                match chosen {
                    Some(id) => choose(id),
                    None => hide(),
                }
                0
            }
            WM_KEYDOWN => {
                match wparam as u16 {
                    VK_ESCAPE => {
                        hide();
                    }
                    VK_RETURN => {
                        let chosen = STATE.with(|s| {
                            let guard = s.borrow();
                            let st = guard.as_ref()?;
                            st.layout.id_at(st.hover?)
                        });
                        if let Some(id) = chosen {
                            choose(id);
                        }
                    }
                    VK_DOWN | VK_UP => {
                        let changed = STATE.with(|s| {
                            let mut guard = s.borrow_mut();
                            let Some(st) = guard.as_mut() else {
                                return false;
                            };
                            let items: Vec<usize> = st
                                .layout
                                .rows
                                .iter()
                                .enumerate()
                                .filter(|(_, r)| matches!(r.row, Row::Item { .. }))
                                .map(|(i, _)| i)
                                .collect();
                            if items.is_empty() {
                                return false;
                            }
                            let at = st.hover.and_then(|h| items.iter().position(|&i| i == h));
                            let next = if wparam as u16 == VK_DOWN {
                                at.map(|i| (i + 1) % items.len()).unwrap_or(0)
                            } else {
                                at.map(|i| (i + items.len() - 1) % items.len())
                                    .unwrap_or(items.len() - 1)
                            };
                            st.hover = Some(items[next]);
                            true
                        });
                        if changed {
                            repaint(hwnd);
                        }
                    }
                    _ => {}
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
            WM_ERASEBKGND => 1,
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
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

/// Draws the card and hands the bitmap to the layered window at `at`.
///
/// The card's distance field, its shadow and the type are drawn once and kept
/// for as long as the menu is the same; a hover change only redoes that one
/// row's band, so sweeping the pointer down the rows does not repaint the
/// whole window.
unsafe fn paint(hwnd: HWND, at: POINT) {
    unsafe {
        STATE.with(|s| {
            let guard = s.borrow();
            let Some(st) = guard.as_ref() else {
                return;
            };
            FRAME.with(|f| {
                let mut slot = f.borrow_mut();
                if slot
                    .as_ref()
                    .is_none_or(|frame| frame.session != st.session)
                {
                    *slot = Some(Frame::new(st.session, &st.menu, &st.layout));
                }
                let Some(frame) = slot.as_mut() else {
                    return;
                };
                frame.set_hover(st.hover, &st.layout, Some(&st.menu));
                frame.present(hwnd, at);
            });
        });
    }
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

/// The card's pixels for one layout and menu, straight RGBA, plus the DIB the
/// layered window is fed from, kept between repaints.
struct Frame {
    /// Which `show` the frame was drawn for: the card is redrawn only when a
    /// new menu is put up, never just because the pointer moved.
    session: u64,
    w: i32,
    h: i32,
    /// Signed distance to the card's rounded rect, per pixel, so the colour
    /// pass never evaluates the shape a second time.
    dist: Vec<f32>,
    shadow: Vec<f32>,
    /// The card and its type with no row hovered: what a hover row paints over.
    base: Vec<u8>,
    image: Vec<u8>,
    hover: Option<usize>,
    upload: Option<Upload>,
    /// Rows of `image` the DIB has not taken yet.
    dirty: Option<(i32, i32)>,
}

impl Frame {
    fn new(session: u64, menu: &Menu, layout: &Layout) -> Self {
        let w = layout.win_w;
        let h = layout.win_h;
        let scale = layout.scale;

        let card_cx = layout.card_x as f32 + layout.card_w as f32 / 2.0;
        let card_cy = layout.card_y as f32 + layout.card_h as f32 / 2.0;
        let card_hw = layout.card_w as f32 / 2.0;
        let card_hh = layout.card_h as f32 / 2.0;
        let radius = RADIUS as f32 * scale;

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
            (9.0 * scale).round().max(1.0) as usize,
            (3.0 * scale).round() as i32,
            0.20,
        );

        // start fully transparent: the window is layered, and anything the card
        // does not cover must let the desktop through
        let mut base = vec![0u8; (w * h * 4) as usize];
        for (i, px) in base.chunks_exact_mut(4).enumerate() {
            let (r, g, b, a) = card_color(dist[i], shadow[i]);
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = a;
        }
        // GDI antialiases glyphs against what is already on the card, so the
        // type comes after the surfaces and the pixels are copied back
        unsafe { draw_content(&mut base, w, h, menu, layout) };

        let image = base.clone();
        let upload = unsafe { Upload::new(w, h) };
        // `dirty` covers the whole window only if there is a DIB that missed
        // it; with no DIB nothing can be presented either way
        let dirty = upload.as_ref().map(|_| (0, h));
        Self {
            session,
            w,
            h,
            dist,
            shadow,
            base,
            image,
            hover: None,
            upload,
            dirty,
        }
    }

    /// Points the card at `hover`, redoing only the bands that changed: the
    /// row being left is put back as it was drawn, the row being entered gets
    /// its colour and then its type over it.
    fn set_hover(&mut self, hover: Option<usize>, layout: &Layout, menu: Option<&Menu>) {
        if self.hover == hover {
            return;
        }
        let leaving = self.hover.and_then(|i| band_of(layout, i));
        let entering = hover.and_then(|i| band_of(layout, i));
        self.hover = hover;
        if leaving.is_none() && entering.is_none() {
            return;
        }
        if let Some((y0, y1)) = leaving {
            // `base` is the card with the type already on it
            self.restore_band(y0, y1);
            self.mark_dirty(y0, y1);
        }
        let (Some((y0, y1)), Some(index), Some(menu)) = (entering, hover, menu) else {
            return;
        };
        self.paint_band(layout, index, y0, y1);
        unsafe { draw_type(&mut self.image, self.w, self.h, menu, layout, y0, y1) };
        self.mark_dirty(y0, y1);
    }

    /// Puts rows `y0..y1` back to the card as it was drawn, hover and all.
    fn restore_band(&mut self, y0: i32, y1: i32) {
        let w = self.w;
        let band = (y0.clamp(0, self.h) * w * 4) as usize..(y1.clamp(0, self.h) * w * 4) as usize;
        self.image[band.clone()].copy_from_slice(&self.base[band]);
    }

    /// Redoes rows `y0..y1` of the card with `index`'s row hovered.
    fn paint_band(&mut self, layout: &Layout, index: usize, y0: i32, y1: i32) {
        let Some(row) = layout.rows.get(index) else {
            return;
        };
        let color = match row.row {
            Row::Item { danger: true, .. } => C_HOVER_DANGER,
            _ => C_HOVER,
        };
        let scale = layout.scale;
        let inset = (2.0 * scale) as i32;
        let top = layout.card_y + row.top + inset;
        let bottom = layout.card_y + row.top + row.height - inset;
        let bh = (bottom - top) as f32;
        let bw = layout.card_w as f32 - 16.0 * scale;
        let hcx = 8.0 * scale + bw / 2.0;
        let hcy = top as f32 + bh / 2.0;
        let hr = 12.0 * scale;

        let w = self.w;
        for y in y0.clamp(0, self.h)..y1.clamp(0, self.h) {
            for x in 0..w {
                let i = (y * w + x) as usize;
                let (r, g, b, a) = card_color(self.dist[i], self.shadow[i]);
                let o = i * 4;
                self.image[o] = r;
                self.image[o + 1] = g;
                self.image[o + 2] = b;
                self.image[o + 3] = a;
                if a == 0 {
                    continue;
                }
                let cov = (0.5 - self.dist[i]).clamp(0.0, 1.0);
                let hd = sd_round_rect(
                    x as f32 + 0.5,
                    y as f32 + 0.5,
                    hcx,
                    hcy,
                    bw / 2.0,
                    bh / 2.0,
                    hr,
                );
                let hcov = (0.5 - hd).clamp(0.0, 1.0) * cov;
                let c = lerp((r, g, b), color, hcov);
                self.image[o] = c.0;
                self.image[o + 1] = c.1;
                self.image[o + 2] = c.2;
            }
        }
    }

    fn mark_dirty(&mut self, y0: i32, y1: i32) {
        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(y0), b.max(y1)),
            None => (y0, y1),
        });
    }

    unsafe fn present(&mut self, hwnd: HWND, at: POINT) {
        unsafe {
            let Some(upload) = self.upload.as_ref() else {
                return;
            };
            if let Some((y0, y1)) = self.dirty.take() {
                upload.put(&self.image, y0, y1);
            }
            upload.present(hwnd, at, self.w, self.h);
        }
    }
}

/// The rows of the card a row index covers, window coordinates.
fn band_of(layout: &Layout, index: usize) -> Option<(i32, i32)> {
    let row = layout.rows.get(index)?;
    Some((
        layout.card_y + row.top,
        layout.card_y + row.top + row.height,
    ))
}

/// The card's colour at one pixel: the rounded rect's fill, its border, and
/// the shadow under it, as straight RGBA.
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

/// A DIB section kept for the life of a frame, so repainting a band rewrites
/// only that band's pixels instead of allocating and refilling the window.
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

    /// Premultiplies rows `y0..y1` of the card into the DIB, which is what the
    /// layered window reads.
    unsafe fn put(&self, image: &[u8], y0: i32, y1: i32) {
        unsafe {
            let y0 = y0.clamp(0, self.h);
            let y1 = y1.clamp(y0, self.h);
            let start = (y0 * self.w * 4) as usize;
            let end = (y1 * self.w * 4) as usize;
            let dst = std::slice::from_raw_parts_mut(self.bits.add(start), end - start);
            for (px, src) in dst
                .chunks_exact_mut(4)
                .zip(image[start..end].chunks_exact(4))
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

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

/// Signed distance to a rounded rectangle, in pixels: negative inside.
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

/// The card as straight RGBA: the shadow, the card, the border and the hover
/// row, then the type and the mark drawn over them by GDI.
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

/// A 32-bit DIB to draw into, with the card's pixels already in it.
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
            // GDI reads and writes BGR; the alpha the card computed stays in
            // the caller's buffer and is put back in `finish`
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, px.len());
            for (d, s) in dst.chunks_exact_mut(4).zip(px.chunks_exact(4)) {
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

    /// Copies GDI's result back over the card's colour, keeping its alpha.
    unsafe fn finish(self, px: &mut [u8]) {
        unsafe { self.finish_band(px, 0, px.len()) }
    }

    /// Copies the bytes `from..to` of GDI's result back, keeping the card's
    /// alpha outside the drawn band. GDI wrote outside it too, but those pixels
    /// still hold the colour they were given.
    unsafe fn finish_band(self, px: &mut [u8], from: usize, to: usize) {
        unsafe {
            let src = std::slice::from_raw_parts(self.bits, px.len());
            for (d, s) in px[from..to]
                .chunks_exact_mut(4)
                .zip(src[from..to].chunks_exact(4))
            {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
            }
            SelectObject(self.hdc, self.old);
            DeleteObject(self.dib as HGDIOBJ);
            DeleteDC(self.hdc);
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

unsafe fn draw_content(px: &mut [u8], w: i32, h: i32, menu: &Menu, layout: &Layout) {
    unsafe {
        let Some(g) = Gdi::new(px, w, h) else {
            return;
        };
        let s = layout.scale;
        let card_x = layout.card_x;
        let card_y = layout.card_y;
        let card_w = layout.card_w;
        let card_h = layout.card_h;

        let f_brand = font(s, 15.5, FW_SEMIBOLD, "Segoe UI");
        let f_item = font(s, 16.0, FW_NORMAL, "Microsoft YaHei UI");
        let f_meta = font(s, 12.5, FW_NORMAL, "Microsoft YaHei UI");
        let f_ver = font(s, 12.5, FW_NORMAL, "Segoe UI");

        // the mark: the hexagon the icon is drawn from, at header size
        let mx = card_x as f32 + (PAD_X as f32 + 11.0) * s;
        let my = card_y as f32 + 21.0 * s;
        draw_mark(&g, mx, my, 10.0 * s, s);

        let brand_x = card_x + ((PAD_X + 31) as f32 * s) as i32;
        g.text(
            "AgentHub",
            f_brand,
            C_TEXT,
            RECT {
                left: brand_x,
                top: card_y + (7.0 * s) as i32,
                right: card_x + card_w,
                bottom: card_y + (33.0 * s) as i32,
            },
        );
        let brand_w = g.text_width("AgentHub", f_brand);
        g.text(
            &format!("v{}", menu.version),
            f_ver,
            C_MUTED,
            RECT {
                left: brand_x + brand_w + (6.0 * s) as i32,
                top: card_y + (9.0 * s) as i32,
                right: card_x + card_w,
                bottom: card_y + (32.0 * s) as i32,
            },
        );

        // the state line: a green dot and where the console is listening
        let dot_x = card_x + ((PAD_X + 31) as f32 * s) as i32;
        let dot_y = card_y + (48.0 * s) as i32;
        let dot_r = 3.0 * s;
        let brush = CreateSolidBrush(colorref(C_GREEN));
        let old_brush = SelectObject(g.hdc, brush as HGDIOBJ);
        let pen = CreatePen(PS_SOLID, 0, colorref(C_GREEN));
        let old_pen = SelectObject(g.hdc, pen as HGDIOBJ);
        Ellipse(
            g.hdc,
            (dot_x as f32 - dot_r).round() as i32,
            (dot_y as f32 - dot_r).round() as i32,
            (dot_x as f32 + dot_r).round() as i32,
            (dot_y as f32 + dot_r).round() as i32,
        );
        SelectObject(g.hdc, old_pen);
        SelectObject(g.hdc, old_brush);
        DeleteObject(pen as HGDIOBJ);
        DeleteObject(brush as HGDIOBJ);

        g.text(
            &format!("运行中 · {}", menu.host),
            f_meta,
            C_MUTED,
            RECT {
                left: dot_x + (9.0 * s) as i32,
                top: card_y + (37.0 * s) as i32,
                right: card_x + card_w - (PAD_X as f32 * s) as i32,
                bottom: card_y + (60.0 * s) as i32,
            },
        );

        // the hairline under the header
        g.fill(
            card_x + (PAD_X as f32 * s) as i32,
            card_y + card_h.min((HEADER_H as f32 * s) as i32) - 1,
            card_w - 2 * (PAD_X as f32 * s) as i32,
            1,
            C_HAIRLINE,
        );

        for row in &layout.rows {
            match row.row {
                Row::Separator => {
                    g.fill(
                        card_x + (PAD_X as f32 * s) as i32,
                        card_y + row.top + row.height / 2,
                        card_w - 2 * (PAD_X as f32 * s) as i32,
                        1,
                        C_HAIRLINE,
                    );
                }
                Row::Item { label, danger, .. } => {
                    let c = if danger { C_DANGER } else { C_TEXT };
                    g.text(label, f_item, c, item_rect(layout, row));
                }
            }
        }

        g.finish(px);

        for f in [f_brand, f_item, f_meta, f_ver] {
            DeleteObject(f as HGDIOBJ);
        }
    }
}

/// Where an item's label sits on the card.
fn item_rect(layout: &Layout, row: &RowGeom) -> RECT {
    let s = layout.scale;
    RECT {
        left: layout.card_x + (TEXT_X as f32 * s) as i32,
        top: layout.card_y + row.top,
        right: layout.card_x + layout.card_w - (PAD_X as f32 * s) as i32,
        bottom: layout.card_y + row.top + row.height,
    }
}

/// Redraws the type that falls in rows `y0..y1` of `px`, leaving the rest of
/// the card alone: what a hover repaint needs after it has recoloured a band.
///
/// # Safety
/// `px` must be a straight-RGBA card of `w` x `h`, laid out as `layout`.
unsafe fn draw_type(px: &mut [u8], w: i32, h: i32, menu: &Menu, layout: &Layout, y0: i32, y1: i32) {
    unsafe {
        let Some(g) = Gdi::new(px, w, h) else {
            return;
        };
        let s = layout.scale;
        let f_brand = font(s, 15.5, FW_SEMIBOLD, "Segoe UI");
        let f_item = font(s, 16.0, FW_NORMAL, "Microsoft YaHei UI");
        let f_meta = font(s, 12.5, FW_NORMAL, "Microsoft YaHei UI");
        let f_ver = font(s, 12.5, FW_NORMAL, "Segoe UI");

        let card_x = layout.card_x;
        let card_y = layout.card_y;
        let card_w = layout.card_w;

        // the header's pieces, when the band reaches them
        let mx = card_x as f32 + (PAD_X as f32 + 11.0) * s;
        let my = card_y as f32 + 21.0 * s;
        if intersects(y0, y1, my - 12.0 * s, my + 12.0 * s) {
            draw_mark(&g, mx, my, 10.0 * s, s);
        }
        let brand_x = card_x + ((PAD_X + 31) as f32 * s) as i32;
        if intersects(y0, y1, card_y as f32 + 7.0 * s, card_y as f32 + 33.0 * s) {
            g.text(
                "AgentHub",
                f_brand,
                C_TEXT,
                RECT {
                    left: brand_x,
                    top: card_y + (7.0 * s) as i32,
                    right: card_x + card_w,
                    bottom: card_y + (33.0 * s) as i32,
                },
            );
            let brand_w = g.text_width("AgentHub", f_brand);
            g.text(
                &format!("v{}", menu.version),
                f_ver,
                C_MUTED,
                RECT {
                    left: brand_x + brand_w + (6.0 * s) as i32,
                    top: card_y + (9.0 * s) as i32,
                    right: card_x + card_w,
                    bottom: card_y + (32.0 * s) as i32,
                },
            );
        }
        let dot_x = card_x + ((PAD_X + 31) as f32 * s) as i32;
        let dot_y = card_y + (48.0 * s) as i32;
        let dot_r = 3.0 * s;
        if intersects(y0, y1, dot_y as f32 - dot_r, dot_y as f32 + dot_r) {
            let brush = CreateSolidBrush(colorref(C_GREEN));
            let old_brush = SelectObject(g.hdc, brush as HGDIOBJ);
            let pen = CreatePen(PS_SOLID, 0, colorref(C_GREEN));
            let old_pen = SelectObject(g.hdc, pen as HGDIOBJ);
            Ellipse(
                g.hdc,
                (dot_x as f32 - dot_r).round() as i32,
                (dot_y as f32 - dot_r).round() as i32,
                (dot_x as f32 + dot_r).round() as i32,
                (dot_y as f32 + dot_r).round() as i32,
            );
            SelectObject(g.hdc, old_pen);
            SelectObject(g.hdc, old_brush);
            DeleteObject(pen as HGDIOBJ);
            DeleteObject(brush as HGDIOBJ);
        }
        if intersects(y0, y1, card_y as f32 + 37.0 * s, card_y as f32 + 60.0 * s) {
            g.text(
                &format!("运行中 · {}", menu.host),
                f_meta,
                C_MUTED,
                RECT {
                    left: dot_x + (9.0 * s) as i32,
                    top: card_y + (37.0 * s) as i32,
                    right: card_x + card_w - (PAD_X as f32 * s) as i32,
                    bottom: card_y + (60.0 * s) as i32,
                },
            );
        }
        let hair = card_y + layout.card_h.min((HEADER_H as f32 * s) as i32) - 1;
        if intersects(y0, y1, hair as f32, hair as f32 + 1.0) {
            g.fill(
                card_x + (PAD_X as f32 * s) as i32,
                hair,
                card_w - 2 * (PAD_X as f32 * s) as i32,
                1,
                C_HAIRLINE,
            );
        }

        // the rows in the band: their label, or a separator's hairline
        for row in &layout.rows {
            let top = card_y + row.top;
            let bottom = top + row.height;
            if !intersects(y0, y1, top as f32, bottom as f32) {
                continue;
            }
            match row.row {
                Row::Separator => g.fill(
                    card_x + (PAD_X as f32 * s) as i32,
                    top + row.height / 2,
                    card_w - 2 * (PAD_X as f32 * s) as i32,
                    1,
                    C_HAIRLINE,
                ),
                Row::Item { label, danger, .. } => {
                    let c = if danger { C_DANGER } else { C_TEXT };
                    g.text(label, f_item, c, item_rect(layout, row));
                }
            }
        }

        // only the band was drawn: copy that much back, not the whole card
        let from = (y0.clamp(0, h) * w * 4) as usize;
        let to = (y1.clamp(0, h) * w * 4) as usize;
        g.finish_band(px, from, to);
        for f in [f_brand, f_item, f_meta, f_ver] {
            DeleteObject(f as HGDIOBJ);
        }
    }
}

/// Whether rows `y0..y1` can hold anything drawn between `top` and `bottom`.
fn intersects(y0: i32, y1: i32, top: f32, bottom: f32) -> bool {
    (y1 as f32) > top && (y0 as f32) < bottom
}

/// The icon's own hexagon, drawn small: six nodes on a ring, the spokes to the
/// middle and the core, so the menu reads as the same product as the tray.
unsafe fn draw_mark(g: &Gdi, cx: f32, cy: f32, r: f32, s: f32) {
    unsafe {
        let node = 2.3 * s;
        let pts: Vec<(f32, f32)> = (0..6)
            .map(|i| {
                let a = (-90.0f32 + 60.0 * i as f32).to_radians();
                (cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();

        let pen = CreatePen(
            PS_SOLID,
            (1.6 * s).round().max(1.0) as i32,
            colorref(C_BRAND),
        );
        let old_pen = SelectObject(g.hdc, pen as HGDIOBJ);
        let old_brush = SelectObject(g.hdc, GetStockObject(NULL_BRUSH));
        for (x, y) in &pts {
            MoveToEx(
                g.hdc,
                cx.round() as i32,
                cy.round() as i32,
                std::ptr::null_mut(),
            );
            LineTo(g.hdc, x.round() as i32, y.round() as i32);
        }
        let mut poly: Vec<POINT> = pts
            .iter()
            .map(|(x, y)| POINT {
                x: x.round() as i32,
                y: y.round() as i32,
            })
            .collect();
        Polygon(g.hdc, poly.as_mut_ptr(), poly.len() as i32);
        SelectObject(g.hdc, old_brush);
        SelectObject(g.hdc, old_pen);
        DeleteObject(pen as HGDIOBJ);

        // nodes: a white disc inside a brand ring, their inner edge on the
        // rim so the rim is not lost at this size
        let white = CreateSolidBrush(colorref((0xff, 0xff, 0xff)));
        let ring = CreatePen(
            PS_SOLID,
            (1.2 * s).round().max(1.0) as i32,
            colorref(C_BRAND),
        );
        let old_brush = SelectObject(g.hdc, white as HGDIOBJ);
        let old_pen = SelectObject(g.hdc, ring as HGDIOBJ);
        for (x, y) in &pts {
            Ellipse(
                g.hdc,
                (x - node).round() as i32,
                (y - node).round() as i32,
                (x + node).round() as i32,
                (y + node).round() as i32,
            );
        }
        // the core
        let core = 2.7 * s;
        let core_brush = CreateSolidBrush(colorref(C_BRAND));
        let core_pen = CreatePen(PS_SOLID, 0, colorref(C_BRAND));
        SelectObject(g.hdc, core_brush as HGDIOBJ);
        SelectObject(g.hdc, core_pen as HGDIOBJ);
        Ellipse(
            g.hdc,
            (cx - core).round() as i32,
            (cy - core).round() as i32,
            (cx + core).round() as i32,
            (cy + core).round() as i32,
        );
        SelectObject(g.hdc, old_pen);
        SelectObject(g.hdc, old_brush);
        DeleteObject(core_pen as HGDIOBJ);
        DeleteObject(core_brush as HGDIOBJ);
        DeleteObject(ring as HGDIOBJ);
        DeleteObject(white as HGDIOBJ);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu {
            version: "2.5.3".into(),
            host: "127.0.0.1:4200".into(),
            items: vec![
                Item {
                    id: 1,
                    label: "打开控制台",
                    danger: false,
                    separator_before: false,
                },
                Item {
                    id: 2,
                    label: "重启网关",
                    danger: false,
                    separator_before: true,
                },
                Item {
                    id: 3,
                    label: "退出",
                    danger: true,
                    separator_before: true,
                },
            ],
        }
    }

    /// A hover repaints its own rows and nothing else: the card's shadow, its
    /// type in the header and the other rows all keep the pixels they had.
    #[test]
    fn a_hover_repaints_only_its_own_row() {
        let menu = menu();
        let layout = Layout::new(&menu, 192);
        let mut frame = Frame::new(1, &menu, &layout);
        assert_eq!(frame.base.len(), (layout.win_w * layout.win_h * 4) as usize);
        assert!(
            frame.base.chunks_exact(4).any(|px| px[3] > 0),
            "the card is drawn"
        );

        let before = frame.image.clone();
        // an item row, not the separator above it: only items take a hover
        let index = layout
            .rows
            .iter()
            .position(|r| matches!(r.row, Row::Item { .. }))
            .unwrap();
        frame.set_hover(Some(index), &layout, Some(&menu));
        let after = frame.image.clone();

        let w = layout.win_w as usize;
        let row = &layout.rows[index];
        let top = (layout.card_y + row.top) as usize;
        let bottom = (layout.card_y + row.top + row.height) as usize;
        assert!(top < bottom);
        let inset = (4.0 * layout.scale) as usize;
        for y in 0..layout.win_h as usize {
            let span = y * w * 4..(y + 1) * w * 4;
            if y < top || y >= bottom {
                assert_eq!(before[span.clone()], after[span], "row {y} is untouched");
            } else if y >= top + inset && y < bottom - inset {
                assert_ne!(before[span.clone()], after[span], "row {y} is repainted");
            }
        }

        // leaving the row puts the card back exactly as it was drawn
        frame.set_hover(None, &layout, Some(&menu));
        assert_eq!(frame.image, before);
    }

    /// Moving straight from one row to the next leaves neither row hovered in
    /// the wrong colour, and both rows are the card's own pixels again.
    #[test]
    fn moving_between_rows_puts_the_first_row_back() {
        let menu = menu();
        let layout = Layout::new(&menu, 96);
        let mut frame = Frame::new(1, &menu, &layout);
        let plain = frame.image.clone();

        frame.set_hover(Some(0), &layout, Some(&menu));
        frame.set_hover(Some(2), &layout, Some(&menu));
        let w = layout.win_w as usize;
        let first = &layout.rows[0];
        let top = (layout.card_y + first.top) as usize;
        let bottom = (layout.card_y + first.top + first.height) as usize;
        assert_eq!(
            frame.image[top * w * 4..bottom * w * 4],
            plain[top * w * 4..bottom * w * 4],
            "the row left behind is the card again"
        );
        frame.set_hover(None, &layout, Some(&menu));
        assert_eq!(frame.image, plain);
    }

    /// A new menu redraws the card; the same one reuses it.
    #[test]
    fn the_frame_belongs_to_the_menu_it_was_drawn_for() {
        let menu = menu();
        let layout = Layout::new(&menu, 96);
        let frame = Frame::new(7, &menu, &layout);
        assert_eq!(frame.session, 7);
        let other = Layout::new(&menu, 192);
        assert_ne!(Layout::new(&menu, 96).win_w, other.win_w);
    }
}
