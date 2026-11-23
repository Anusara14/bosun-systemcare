//! Performance Monitor flyout, laid out pixel-for-pixel from the reference
//! (520 Ã— 385): title bar, five 100 px tabs with ring gauges, a framed process
//! table with section rows, and a footer with tools + End Task. Black palette.
//! Hides when it loses focus and drops its render target and caches when hidden.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bosun_core::format;
use bosun_core::procs::{Group, end_task, trim_working_sets};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, ID2D1Bitmap, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, MONITOR_DEFAULTTONEAREST, MonitorFromPoint, PAINTSTRUCT,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, DestroyIcon, GetClientRect, GetCursorPos,
    GetWindowRect, HICON, HTCAPTION, HTCLIENT, HWND_TOPMOST, IDC_ARROW, LoadCursorW,
    RegisterClassExW, SW_HIDE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
    WA_INACTIVE, WM_ACTIVATE, WM_DPICHANGED, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCHITTEST, WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HSTRING, w};

use crate::gfx::{self, Align, Painter, col, cola, contains, glyph, rect};
mod chrome;
mod footer;
mod list;

use chrome::*;
use footer::*;
use list::*;

use crate::{Cmd, Data, graphics, hinstance, post, shell, tools, widget, with_data, with_state};

const WM_MOUSELEAVE: u32 = 0x02A3;

// ---------- reference geometry (DIPs, window-relative) ----------
const W: f32 = 520.0;
const H: f32 = 385.0;
const TITLE_CY: f32 = 18.0;
const TAB_X: f32 = 10.0;
const TAB_W: f32 = 100.0;
const TAB_TOP: f32 = 40.0;
const RING_CY: f32 = 80.5;
const RING_R: f32 = 20.5;
const RING_W: f32 = 3.0;
const LABEL_CY: f32 = 120.0;
const FRAME_L: f32 = 10.0;
const FRAME_R: f32 = 509.0;
const FRAME_TOP: f32 = 140.0;
const HEAD_BOTTOM: f32 = 168.0;
const LIST_TOP: f32 = 169.0;
const LIST_BOTTOM: f32 = 346.0;
const ROW_H: f32 = 30.0;
const COL_NAME_X: f32 = 21.0;
const COL_VALUE_X: f32 = 271.0;
const SEP1_X: f32 = 261.0;
const SEP2_X: f32 = 346.0;
const FOOT_CY: f32 = 365.0;
const END_TASK: (f32, f32, f32, f32) = (411.0, 353.0, 98.0, 24.0);
const BACK: (f32, f32, f32) = (478.0, 18.0, 8.0);
const CLOSE: (f32, f32) = (501.0, 18.0);
const REOPEN_GUARD: Duration = Duration::from_millis(300);

/// Reference palette mapped to black/charcoal; Bosun blue for accents.
mod mc {
    pub const BG: u32 = 0x151515;
    pub const BORDER: u32 = 0x3A3A3A;
    pub const LINE: u32 = 0x2E2E2E;
    pub const PANEL: u32 = 0x222222;
    pub const LIST: u32 = 0x0E0E0E;
    pub const TRACK: u32 = 0x363636;
    pub const ACCENT: u32 = 0x60CDFF;
    pub const RING: u32 = 0x1E9CFF;
    pub const TEXT: u32 = 0xE6E6E6;
    pub const TEXT_2: u32 = 0xB0B0B0;
    pub const TEXT_3: u32 = 0x8C8C8C;
    pub const ICON: u32 = 0x9A9A9A;
    pub const BTN: u32 = 0x2E2E2E;
    pub const BTN_OFF: u32 = 0x2A2A2A;
    pub const SCROLL: u32 = 0x4A4A4A;
    pub const CHECK_BORDER: u32 = 0x5A5A5A;
    pub const HOVER: u32 = 0xFFFFFF;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Ram,
    Cpu,
    Disk,
    Metrics,
    Network,
}

const TABS: [Tab; 5] = [Tab::Ram, Tab::Cpu, Tab::Disk, Tab::Metrics, Tab::Network];

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Ram => "RAM",
            Tab::Cpu => "CPU",
            Tab::Disk => "Disk",
            Tab::Metrics => "Metrics",
            Tab::Network => "Network",
        }
    }
    fn is_process(self) -> bool {
        matches!(self, Tab::Ram | Tab::Cpu | Tab::Disk)
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Hit {
    None,
    Back,
    Close,
    Title,
    Tab(Tab),
    Section(usize),
    Row(String),
    Clean,
    Scan,
    Turbo,
    Shot,
    ShotMenu,
    EndTask,
}

// ---------- window state ----------

struct Monitor {
    hwnd: HWND,
    dpi: u32,
    visible: bool,
    rt: Option<(ID2D1HwndRenderTarget, ID2D1SolidColorBrush)>,
    icons: HashMap<String, Option<ID2D1Bitmap>>,
    tab: Tab,
    scroll: f32,
    content_h: f32,
    hover: Hit,
    checked: HashSet<String>,
    collapsed: [bool; 2],
    toast: Option<(String, Instant)>,
    hidden_at: Option<Instant>,
    turbo: bool,
}

thread_local! {
    static MONITOR: RefCell<Option<Monitor>> = const { RefCell::new(None) };
}

fn with_mon<R>(f: impl FnOnce(&mut Monitor) -> R) -> Option<R> {
    MONITOR.with(|m| m.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn is_open() -> bool {
    with_mon(|m| m.visible).unwrap_or(false)
}

pub fn invalidate() {
    if let Some(h) = with_mon(|m| m.hwnd) {
        // SAFETY: our own window.
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

pub fn register_class() {
    // SAFETY: registering a class with a static name and proc.
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(proc),
            hInstance: hinstance(),
            lpszClassName: w!("BosunMonitor"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
    }
}

fn create() -> Option<HWND> {
    // SAFETY: creates our popup and sets DWM attributes on it.
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("BosunMonitor"),
            w!("Performance Monitor"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(hinstance()),
            None,
        )
        .ok()?;
        let round = DWMWCP_ROUND;
        let dark: i32 = 1;
        // COLORREF is 0x00BBGGRR; the border is grey so channel order does not matter.
        let border = COLORREF(mc::BORDER);
        let set = |attr, v: *const std::ffi::c_void| DwmSetWindowAttribute(hwnd, attr, v, 4);
        let _ = set(DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _);
        let _ = set(DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _);
        let _ = set(DWMWA_BORDER_COLOR, &border as *const _ as _);
        Some(hwnd)
    }
}

/// Under (or above) the widget, else bottom-right above the tray.
fn placement() -> (i32, i32, i32, i32, u32) {
    let mut cur = POINT::default();
    // SAFETY: plain cursor query.
    let _ = unsafe { GetCursorPos(&mut cur) };
    let anchor = widget::rect_on_screen();
    let probe = anchor.map_or(cur, |r| POINT {
        x: r.left,
        y: r.top,
    });
    let wa = shell::work_area(probe);
    let (mut dx, mut dy) = (96u32, 96u32);
    let mon =
        // SAFETY: plain monitor lookup.
        unsafe { MonitorFromPoint(probe, MONITOR_DEFAULTTONEAREST) };
    // SAFETY: monitor handle comes from MonitorFromPoint.
    let _ = unsafe { GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    let dpi = dx.max(96);
    let (w, h) = (gfx::px(W, dpi), gfx::px(H, dpi));
    let gap = gfx::px(8.0, dpi);
    // Opens where the widget was: same top edge, right edges aligned.
    let (x, y) = match anchor {
        Some(r) => (
            (r.right - w).clamp(wa.left, (wa.right - w).max(wa.left)),
            r.top.clamp(wa.top, (wa.bottom - h).max(wa.top)),
        ),
        None => (wa.right - w - gap, wa.bottom - h - gap),
    };
    (x, y, w, h, dpi)
}

fn new_state(hwnd: HWND) -> Monitor {
    Monitor {
        hwnd,
        dpi: 96,
        visible: false,
        rt: None,
        icons: HashMap::new(),
        tab: Tab::Ram,
        scroll: 0.0,
        content_h: 0.0,
        hover: Hit::None,
        checked: HashSet::new(),
        collapsed: [false, false],
        toast: None,
        hidden_at: None,
        turbo: false,
    }
}

/// Returns false when the request is the click that just closed the flyout.
pub fn show() -> bool {
    let recent = with_mon(|m| m.hidden_at.is_some_and(|t| t.elapsed() < REOPEN_GUARD));
    if recent == Some(true) {
        return false;
    }
    if MONITOR.with(|m| m.borrow().is_none()) {
        let Some(hwnd) = create() else { return false };
        MONITOR.with(|m| *m.borrow_mut() = Some(new_state(hwnd)));
    }
    let (x, y, w, h, dpi) = placement();
    let Some(hwnd) = with_mon(|m| {
        m.visible = true;
        m.dpi = dpi;
        m.scroll = 0.0;
        m.hwnd
    }) else {
        return false;
    };
    // SAFETY: showing and focusing our own popup (outside any state borrow).
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

pub fn hide() {
    let Some(hwnd) = with_mon(|m| {
        m.visible = false;
        m.hidden_at = Some(Instant::now());
        m.rt = None;
        m.icons = HashMap::new();
        m.checked.clear();
        m.toast = None;
        m.hwnd
    }) else {
        return;
    };
    // SAFETY: hiding our own window.
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

// ---------- painting ----------

fn ensure_target(m: &mut Monitor) -> bool {
    if m.rt.is_some() {
        return true;
    }
    let mut rc = RECT::default();
    // SAFETY: render target bound to our own window's client area.
    unsafe {
        let _ = GetClientRect(m.hwnd, &mut rc);
        // Software rendering: a GPU device would load ~50 MB of driver for a small flyout.
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            dpiX: m.dpi as f32,
            dpiY: m.dpi as f32,
            ..Default::default()
        };
        let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: m.hwnd,
            pixelSize: D2D_SIZE_U {
                width: rc.right as u32,
                height: rc.bottom as u32,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let Ok(rt) = graphics().d2d.CreateHwndRenderTarget(&props, &hp) else {
            return false;
        };
        let Ok(br) = rt.CreateSolidColorBrush(&col(mc::TEXT), None) else {
            return false;
        };
        m.rt = Some((rt, br));
    }
    true
}

fn file_icon(path: &str) -> Option<HICON> {
    let mut sfi = SHFILEINFOW::default();
    // SAFETY: sfi is a valid out-struct; caller destroys the returned icon.
    let r = unsafe {
        SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_SMALLICON,
        )
    };
    (r != 0 && !sfi.hIcon.is_invalid()).then_some(sfi.hIcon)
}

fn paint(m: &mut Monitor, data: &Data) {
    if !ensure_target(m) {
        return;
    }
    let Some((rt, br)) = m.rt.clone() else { return };
    // SAFETY: Begin/EndDraw pair on our render target.
    unsafe { rt.BeginDraw() };
    {
        let base: &ID2D1RenderTarget = &rt;
        let p = Painter {
            rt: base,
            br: &br,
            g: graphics(),
        };
        p.clear(col(mc::BG));
        draw_title(&p, m);
        draw_frame(&p, m);
        draw_tabs(&p, m, data);
        match m.tab {
            t if t.is_process() => draw_processes(&p, m, &data.groups),
            Tab::Metrics => draw_metrics(&p, data),
            _ => draw_network(&p, data),
        }
        draw_footer(&p, m);
    }
    // SAFETY: ends the frame; a lost device is recreated next paint.
    if unsafe { rt.EndDraw(None, None) }.is_err() {
        m.rt = None;
    }
}

pub(super) fn hover_fill(p: &Painter, on: bool, r: D2D_RECT_F) {
    if on {
        p.fill(r, cola(mc::HOVER, 0.05));
    }
}

fn tab_rect(i: usize) -> D2D_RECT_F {
    rect(
        TAB_X + i as f32 * TAB_W,
        TAB_TOP,
        TAB_W,
        FRAME_TOP - TAB_TOP,
    )
}

// ---------- input ----------

fn hit(m: &Monitor, groups: &[Group], x: f32, y: f32) -> Hit {
    let (bx, by, br) = BACK;
    if (x - bx).hypot(y - by) <= br + 2.0 {
        return Hit::Back;
    }
    let (cx, cy) = CLOSE;
    if contains(&rect(cx - 9.0, cy - 9.0, 18.0, 18.0), x, y) {
        return Hit::Close;
    }
    if y < TAB_TOP {
        return Hit::Title;
    }
    if let Some(i) = (0..5).find(|&i| contains(&tab_rect(i), x, y)) {
        return Hit::Tab(TABS[i]);
    }
    for (h, r) in footer_tools() {
        if contains(&r, x, y) {
            return h;
        }
    }
    if contains(&end_rect(), x, y) {
        return Hit::EndTask;
    }
    if m.tab.is_process() && y > LIST_TOP && y < LIST_BOTTOM && x > FRAME_L && x < 498.0 {
        let i = ((y - LIST_TOP + m.scroll) / ROW_H).floor();
        if i >= 0.0 {
            return match lines(groups, m.tab, m.collapsed).get(i as usize) {
                Some(Line::Section { idx, .. }) => Hit::Section(*idx),
                Some(Line::Row(g)) => Hit::Row(g.key.clone()),
                None => Hit::None,
            };
        }
    }
    Hit::None
}

fn coords(lp: LPARAM, dpi: u32) -> (f32, f32) {
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
    (gfx::dip(x, dpi), gfx::dip(y, dpi))
}

fn end_checked(m: &mut Monitor) {
    let pids: Vec<u32> = with_state(|s| {
        s.data
            .groups
            .iter()
            .filter(|g| m.checked.contains(&g.key) && !g.protected)
            .flat_map(|g| g.pids.iter().copied())
            .collect()
    })
    .unwrap_or_default();
    let n = m.checked.len();
    let failed = end_task(&pids);
    let msg = if failed == 0 {
        format!("Ended {n} {}", if n == 1 { "app" } else { "apps" })
    } else {
        format!(
            "Couldn't end {failed} of {} tasks (they may need admin rights)",
            pids.len()
        )
    };
    m.toast = Some((msg, Instant::now()));
    m.checked.clear();
    post(Cmd::Refresh);
}

fn click(m: &mut Monitor, h: Hit) {
    match h {
        Hit::Back => post(Cmd::HideMonitor),
        Hit::Close => post(Cmd::Dismiss),
        Hit::Tab(t) => {
            m.tab = t;
            m.scroll = 0.0;
        }
        Hit::Section(i) => m.collapsed[i] = !m.collapsed[i],
        Hit::Row(key) => {
            let protected =
                with_state(|s| s.data.groups.iter().any(|g| g.key == key && g.protected));
            if protected != Some(true) && !m.checked.remove(&key) {
                m.checked.insert(key);
            }
        }
        Hit::Clean => {
            let freed = trim_working_sets();
            let msg = if freed > 0 {
                format!("Memory released: {}", format::bytes(freed))
            } else {
                "Memory is already in good shape".to_string()
            };
            m.toast = Some((msg, Instant::now()));
            post(Cmd::Refresh);
        }
        Hit::Scan if shell::main_app_path().is_some() => {
            post(Cmd::OpenMain);
            post(Cmd::HideMonitor);
        }
        Hit::Turbo => {
            let msg = match tools::toggle_turbo() {
                Ok(on) => {
                    m.turbo = on;
                    if on {
                        "Turbo Boost: ON (High performance plan)"
                    } else {
                        "Turbo Boost: OFF"
                    }
                }
                Err(e) => e,
            };
            m.toast = Some((msg.to_string(), Instant::now()));
        }
        Hit::Shot => {
            post(Cmd::HideMonitor);
            tools::screenshot();
        }
        Hit::ShotMenu => shot_menu(m.hwnd),
        Hit::EndTask if can_end(m) => end_checked(m),
        _ => {}
    }
}

fn track_leave(hwnd: HWND) {
    let mut tme = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: requests WM_MOUSELEAVE for our window.
    let _ = unsafe { TrackMouseEvent(&mut tme) };
}

fn on_mouse(hwnd: HWND, msg: u32, lp: LPARAM, wp: WPARAM) {
    let hit_at =
        |m: &Monitor, x, y| with_state(|s| hit(m, &s.data.groups, x, y)).unwrap_or(Hit::None);
    let changed = with_mon(|m| match msg {
        WM_MOUSEMOVE => {
            let (x, y) = coords(lp, m.dpi);
            let h = hit_at(m, x, y);
            track_leave(hwnd);
            std::mem::replace(&mut m.hover, h.clone()) != h
        }
        WM_MOUSELEAVE => std::mem::replace(&mut m.hover, Hit::None) != Hit::None,
        WM_LBUTTONDOWN => {
            let (x, y) = coords(lp, m.dpi);
            let h = hit_at(m, x, y);
            click(m, h);
            true
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            m.scroll = (m.scroll - delta * ROW_H * 3.0).max(0.0);
            true
        }
        _ => false,
    });
    if changed == Some(true) {
        invalidate();
    }
}

fn nc_hit(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: default hit test first; only the empty title area becomes a drag handle.
    let def = unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    if def.0 as u32 != HTCLIENT {
        return def;
    }
    let mut r = RECT::default();
    // SAFETY: our own window.
    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let x = gfx::dip((lp.0 & 0xFFFF) as i16 as i32 - r.left, dpi);
    let y = gfx::dip(((lp.0 >> 16) & 0xFFFF) as i16 as i32 - r.top, dpi);
    if y < TAB_TOP - 4.0 && x < BACK.0 - BACK.2 - 4.0 {
        return LRESULT(HTCAPTION as isize);
    }
    def
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: Begin/EndPaint pair validates the window.
            unsafe { BeginPaint(hwnd, &mut ps) };
            MONITOR.with(|mm| {
                if let Ok(mut g) = mm.try_borrow_mut()
                    && let Some(m) = g.as_mut()
                    && with_data(|d| paint(m, d)).is_none()
                {
                    paint(m, &Data::default());
                }
            });
            // SAFETY: matches BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_NCHITTEST => return nc_hit(hwnd, msg, wp, lp),
        WM_MOUSEMOVE | WM_MOUSELEAVE | WM_LBUTTONDOWN | WM_MOUSEWHEEL => {
            on_mouse(hwnd, msg, lp, wp);
            return LRESULT(0);
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            if is_open() {
                post(Cmd::HideMonitor);
            }
        }
        WM_SIZE => {
            let (w, h) = ((lp.0 & 0xFFFF) as u32, ((lp.0 >> 16) & 0xFFFF) as u32);
            with_mon(|m| {
                if let Some((rt, _)) = &m.rt {
                    // SAFETY: resizing our render target to the new client size.
                    let _ = unsafe {
                        rt.Resize(&D2D_SIZE_U {
                            width: w,
                            height: h,
                        })
                    };
                }
            });
        }
        WM_DPICHANGED => {
            let dpi = (wp.0 & 0xFFFF) as u32;
            with_mon(|m| {
                m.dpi = dpi;
                m.rt = None;
                m.icons = HashMap::new();
            });
            // SAFETY: lParam points at the suggested RECT for the new DPI.
            let r = unsafe { &*(lp.0 as *const RECT) };
            let (w, h) = (gfx::px(W, dpi), gfx::px(H, dpi));
            // SAFETY: resizing our own window.
            unsafe {
                let _ = SetWindowPos(hwnd, None, r.left, r.top, w, h, Default::default());
            }
            return LRESULT(0);
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(key: &str, mem: u64, cpu: f32, win: bool) -> Group {
        Group {
            key: key.into(),
            name: key.into(),
            path: None,
            pids: vec![1],
            private_bytes: mem,
            cpu_pct: cpu,
            io_bps: 0,
            has_window: win,
            protected: false,
        }
    }

    #[test]
    fn hit_test_matches_reference_layout() {
        let m = new_state(HWND(std::ptr::null_mut()));
        let gs = vec![g("a", 10, 0.0, true)];
        assert_eq!(hit(&m, &gs, 478.0, 18.0), Hit::Back);
        assert_eq!(hit(&m, &gs, 501.0, 18.0), Hit::Close);
        assert_eq!(hit(&m, &gs, 160.0, 90.0), Hit::Tab(Tab::Cpu));
        assert_eq!(hit(&m, &gs, 100.0, 184.0), Hit::Section(0));
        assert_eq!(hit(&m, &gs, 100.0, 214.0), Hit::Row("a".into()));
        assert_eq!(hit(&m, &gs, 450.0, 365.0), Hit::EndTask);
        assert_eq!(hit(&m, &gs, 30.0, 365.0), Hit::Clean);
    }
}
