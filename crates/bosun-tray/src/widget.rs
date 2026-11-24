//! Floating desktop widget, laid out pixel-for-pixel from the reference:
//! a 29 px rocket knob (click = Clean RAM), a 20 px pill with RAM/CPU, our
//! logo (opens the main app) and an arrow button (opens the monitor).

use std::cell::RefCell;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, HTCAPTION, HTCLIENT, IDC_ARROW,
    IDC_HAND, KillTimer, LoadCursorW, RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOZORDER, SetCursor, SetTimer, SetWindowPos, ShowWindow, WM_DPICHANGED, WM_EXITSIZEMOVE,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOVING, WM_NCHITTEST, WM_NCRBUTTONUP, WM_RBUTTONUP,
    WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use crate::gfx::{self, Align, Painter, col, cola, rect};
use crate::layered::Surface;
use crate::{Cmd, hinstance, popup, post, shell, with_state};

const WM_MOUSELEAVE: u32 = 0x02A3;
const ANIM_TIMER: usize = 7;

/// Size in DIPs (equals pixels at 100 % scaling).
pub const W: f32 = 200.0;
pub const H: f32 = 30.0;
const SNAP: i32 = 16;

/// (centre x, centre y, radius) of the rocket knob and the arrow button.
const KNOB: (f32, f32, f32) = (15.0, 15.0, 14.5);
const ARROW: (f32, f32, f32) = (190.0, 15.0, 9.0);
const PILL_TOP: f32 = 5.0;
const PILL_H: f32 = 20.0;

/// Reference geometry, black palette (accent stays the Bosun blue).
mod wc {
    pub const BG: u32 = 0x111111;
    pub const KNOB_RING: u32 = 0x2E2E2E;
    pub const KNOB_RING_HOVER: u32 = 0x3D3D3D;
    pub const ROCKET: u32 = 0x1E9CFF;
    pub const TEXT: u32 = 0xB8B8B8;
    pub const BTN: u32 = 0x262626;
    pub const BTN_HOVER: u32 = 0x333333;
    pub const ARROW: u32 = 0x1E9CFF;
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    None,
    Rocket,
    Open,
    Body,
}

fn dist(x: f32, y: f32, (cx, cy, _): (f32, f32, f32)) -> f32 {
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt()
}

fn part_at(x: f32, y: f32) -> Part {
    let in_pill = (PILL_TOP..PILL_TOP + PILL_H).contains(&y);
    if dist(x, y, KNOB) <= KNOB.2 {
        Part::Rocket
    } else if dist(x, y, ARROW) <= ARROW.2 + 0.5 {
        Part::Open
    } else if (KNOB.0..W).contains(&x) && in_pill {
        Part::Body
    } else {
        Part::None
    }
}

// ---------- clean animation ----------

const SHAKE: f32 = 0.30;
const LAUNCH: f32 = 0.50;
const RETURN: f32 = 0.40;

struct Anim {
    start: Instant,
    from: u32,
    /// (freed bytes, new RAM %, when the result arrived)
    result: Option<(u64, u32, Instant)>,
}

/// What one frame of the animation looks like.
#[derive(Debug, PartialEq)]
struct Frame {
    dx: f32,
    dy: f32,
    alpha: f32,
    flame: f32,
    spinner: Option<f32>,
    ram: Option<u32>,
    done: bool,
}

const STILL: Frame = Frame {
    dx: 0.0,
    dy: 0.0,
    alpha: 1.0,
    flame: 0.0,
    spinner: None,
    ram: None,
    done: false,
};

/// Shake → launch off the top (spinner runs until the clean finishes) →
/// land from below while the RAM figure counts down to the new value.
fn frame(a: &Anim, now: Instant) -> Frame {
    let t = now.duration_since(a.start).as_secs_f32();
    if t < SHAKE {
        return Frame {
            dx: (t * 90.0).sin() * 0.9,
            flame: 0.4,
            ..STILL
        };
    }
    let launch_end = SHAKE + LAUNCH;
    let landing = a
        .result
        .map(|(_, _, at)| (t - launch_end).min(now.duration_since(at).as_secs_f32()));
    match landing {
        Some(r) if t >= launch_end && r >= 0.0 => {
            let k = (r / RETURN).clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - k).powi(3);
            let to = a.result.map_or(a.from, |(_, to, _)| to);
            let shown = a.from as f32 + (to as f32 - a.from as f32) * ease;
            Frame {
                dy: 14.0 * (1.0 - ease),
                alpha: ease,
                flame: 0.3 * (1.0 - k),
                ram: Some(shown.round() as u32),
                done: k >= 1.0,
                ..STILL
            }
        }
        _ => {
            let k = ((t - SHAKE) / LAUNCH).clamp(0.0, 1.0);
            Frame {
                dy: -26.0 * k * k,
                alpha: 1.0 - k,
                flame: 1.0,
                spinner: Some(t * 1.4 % 1.0),
                ..STILL
            }
        }
    }
}

// ---------- window state ----------

struct Widget {
    hwnd: HWND,
    dpi: u32,
    surf: Option<Surface>,
    shown: Option<(u32, u32, Part)>,
    hover: Part,
    anim: Option<Anim>,
}

thread_local! {
    static WIDGET: RefCell<Option<Widget>> = const { RefCell::new(None) };
}

fn with_widget<R>(f: impl FnOnce(&mut Widget) -> R) -> Option<R> {
    WIDGET.with(|w| w.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn draw_knob(p: &Painter, hover: Part, fr: &Frame) {
    let (kx, ky, kr) = KNOB;
    p.circle(kx, ky, kr, col(wc::BG));
    let ring = if hover == Part::Rocket {
        wc::KNOB_RING_HOVER
    } else {
        wc::KNOB_RING
    };
    p.ring(kx, ky, 9.5, 4.0, col(ring));
    if let Some(s) = fr.spinner {
        p.arc_at(kx, ky, 9.5, 4.0, s, 0.28, col(wc::ARROW));
    }
    p.clip(rect(0.0, 0.0, 2.0 * kx, H));
    let body = cola(wc::ROCKET, fr.alpha);
    p.rocket(
        kx + fr.dx,
        ky + 1.0 + fr.dy,
        body,
        cola(wc::BG, fr.alpha),
        fr.flame,
    );
    p.unclip();
}

/// "RAM:75" in 13 px with a 10 px "%" on the same baseline; CPU starts at x = 101.
fn draw_readouts(p: &Painter, ram: u32, cpu: u32) {
    let f = &p.g.f;
    let mut x = 39.0;
    for (label, v) in [("RAM:", ram), ("CPU:", cpu)] {
        let s = format!("{label}{v}");
        let tw = p.g.measure(&s, &f.w_text);
        p.text(
            &s,
            &f.w_text,
            rect(x, PILL_TOP, tw + 2.0, PILL_H),
            col(wc::TEXT),
            Align::Left,
        );
        let pw = p.g.measure("%", &f.w_pct);
        let pct = rect(x + tw + 1.0, PILL_TOP + 1.5, pw + 2.0, PILL_H);
        p.text("%", &f.w_pct, pct, col(wc::TEXT), Align::Left);
        x = (x + tw + pw + 10.0).max(101.0);
    }
}

fn draw(p: &Painter, ram: u32, cpu: u32, hover: Part, fr: &Frame) {
    p.clear(cola(0, 0.0));
    p.fill_rr(
        rect(KNOB.0, PILL_TOP, W - KNOB.0, PILL_H),
        PILL_H / 2.0,
        col(wc::BG),
    );
    let (kx, ky, kr) = KNOB;
    p.fillets(kx, ky, kr, PILL_TOP, PILL_TOP + PILL_H, 4.0, col(wc::BG));
    draw_knob(p, hover, fr);
    draw_readouts(p, fr.ram.unwrap_or(ram), cpu);
    let (ax, ay, ar) = ARROW;
    p.circle(
        ax,
        ay,
        ar,
        col(if hover == Part::Open {
            wc::BTN_HOVER
        } else {
            wc::BTN
        }),
    );
    p.arrow(ax, ay, 8.0, 1.7, col(wc::ARROW));
}

/// Redraws only when numbers, hover or the animation changed.
pub fn render() {
    let Some((ram, cpu)) = with_state(|s| {
        (
            s.data.vitals.mem.load_pct,
            s.data.vitals.cpu_pct.round() as u32,
        )
    }) else {
        return;
    };
    let landed = with_widget(|wd| {
        let fr = wd.anim.as_ref().map(|a| frame(a, Instant::now()));
        if fr.is_none() && wd.shown == Some((ram, cpu, wd.hover)) {
            return None;
        }
        if wd.surf.is_none() {
            wd.surf = Surface::new(W, H, wd.dpi).ok();
        }
        let fr = fr.unwrap_or(STILL);
        let hover = wd.hover;
        // "Transparency" in the widget's menu: see-through at 70 %
        let alpha = if with_state(|s| s.settings.widget_transparent).unwrap_or(false) {
            179
        } else {
            255
        };
        let ok = wd
            .surf
            .as_ref()
            .is_some_and(|s| s.present_alpha(wd.hwnd, alpha, |p| draw(p, ram, cpu, hover, &fr)));
        if !ok {
            wd.surf = None;
        }
        wd.shown = if wd.anim.is_some() {
            None
        } else {
            Some((ram, cpu, hover))
        };
        if !fr.done {
            return None;
        }
        let freed = wd.anim.take().and_then(|a| a.result).map_or(0, |(f, ..)| f);
        Some((wd.hwnd, freed))
    });
    if let Some(Some((hwnd, freed))) = landed {
        // SAFETY: stopping our own animation timer.
        let _ = unsafe { KillTimer(Some(hwnd), ANIM_TIMER) };
        render();
        popup::show(freed);
    }
}

/// Starts the rocket animation (no-op if one is running or the widget is hidden).
pub fn start_clean() {
    let ram = with_state(|s| s.data.vitals.mem.load_pct).unwrap_or(0);
    let hwnd = with_widget(|wd| {
        if wd.anim.is_some() {
            return None;
        }
        wd.anim = Some(Anim {
            start: Instant::now(),
            from: ram,
            result: None,
        });
        Some(wd.hwnd)
    })
    .flatten();
    if let Some(hwnd) = hwnd {
        popup::hide();
        // SAFETY: ~60 fps timer on our own window, killed when the animation ends.
        unsafe { SetTimer(Some(hwnd), ANIM_TIMER, 16, None) };
    }
}

pub fn is_cleaning() -> bool {
    with_widget(|wd| wd.anim.is_some()).unwrap_or(false)
}

/// The background clean finished; the rocket lands and the popup shows the result.
pub fn clean_done(freed: u64, ram_now: u32) {
    let animating = with_widget(|wd| match wd.anim.as_mut() {
        Some(a) => {
            a.result = Some((freed, ram_now, Instant::now()));
            true
        }
        None => false,
    });
    if animating == Some(false) {
        popup::show(freed);
    }
}

pub fn register_class() {
    // SAFETY: registering a window class with a static name and proc.
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: hinstance(),
            lpszClassName: w!("BosunWidget"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
    }
}

fn default_pos(w: i32) -> (i32, i32) {
    let wa = shell::work_area(POINT::default());
    (wa.right - w - 24, wa.top + 24)
}

pub fn show() {
    if WIDGET.with(|w| w.borrow().is_some()) {
        return;
    }
    let pos = with_state(|s| s.settings.widget_pos).flatten();
    // SAFETY: creates our layered popup; size is fixed up for its real DPI below.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            w!("BosunWidget"),
            w!("Bosun SystemCare widget"),
            WS_POPUP,
            pos.map_or(0, |p| p.0),
            pos.map_or(0, |p| p.1),
            1,
            1,
            None,
            None,
            Some(hinstance()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    // SAFETY: hwnd was just created.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let (w, h) = (gfx::px(W, dpi), gfx::px(H, dpi));
    let (x, y) = pos.unwrap_or_else(|| default_pos(w));
    // SAFETY: positioning and showing our own window.
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    WIDGET.with(|wd| {
        *wd.borrow_mut() = Some(Widget {
            hwnd,
            dpi,
            surf: None,
            shown: None,
            hover: Part::None,
            anim: None,
        })
    });
    apply_pin();
    render();
}

/// Redraws the widget even if nothing it shows changed (a setting did).
pub fn refresh() {
    with_widget(|wd| wd.shown = None);
    render();
}

/// "Pin it up front": above other windows, or an ordinary window.
pub fn apply_pin() {
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
    };
    let pinned = with_state(|s| s.settings.widget_pinned).unwrap_or(true);
    if let Some(hwnd) = with_widget(|wd| wd.hwnd) {
        let after = if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: re-ordering our own window.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

pub fn hide() {
    crate::wmenu::hide();
    popup::hide();
    let hwnd = WIDGET.with(|w| w.borrow_mut().take().map(|wd| wd.hwnd));
    if let Some(h) = hwnd {
        // SAFETY: destroying our own window; its state was already dropped.
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

/// Screen rectangle and DPI, for placing the monitor and the clean popup.
pub fn placement() -> Option<(RECT, u32)> {
    let (hwnd, dpi) = WIDGET.with(|w| w.borrow().as_ref().map(|wd| (wd.hwnd, wd.dpi)))?;
    let mut r = RECT::default();
    // SAFETY: reading our own window rect.
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some((r, dpi))
}

pub fn rect_on_screen() -> Option<RECT> {
    placement().map(|(r, _)| r)
}

fn client_dip(hwnd: HWND, lp: LPARAM, screen: bool) -> (f32, f32) {
    let (mut x, mut y) = (
        (lp.0 & 0xFFFF) as i16 as i32,
        ((lp.0 >> 16) & 0xFFFF) as i16 as i32,
    );
    if screen {
        let mut r = RECT::default();
        // SAFETY: reading our own window rect.
        let _ = unsafe { GetWindowRect(hwnd, &mut r) };
        x -= r.left;
        y -= r.top;
    }
    // SAFETY: hwnd is our live window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    (gfx::dip(x, dpi), gfx::dip(y, dpi))
}

fn set_hover(p: Part) {
    if with_widget(|wd| std::mem::replace(&mut wd.hover, p) != p) == Some(true) {
        render();
    }
}

/// Keeps the widget inside the work area and snaps it to edges within `SNAP` px.
fn snap(r: &mut RECT) {
    let (w, h) = gfx::rect_size(r);
    let wa = shell::work_area(POINT {
        x: r.left + w / 2,
        y: r.top + h / 2,
    });
    let x = snap_axis(r.left, w, wa.left, wa.right);
    let y = snap_axis(r.top, h, wa.top, wa.bottom);
    *r = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
}

fn snap_axis(pos: i32, size: i32, lo: i32, hi: i32) -> i32 {
    let p = pos.clamp(lo, (hi - size).max(lo));
    if p - lo < SNAP {
        lo
    } else if hi - (p + size) < SNAP {
        hi - size
    } else {
        p
    }
}

fn save_pos(hwnd: HWND) {
    let mut r = RECT::default();
    // SAFETY: reading our own window rect.
    if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
        with_state(|s| {
            s.settings.widget_pos = Some((r.left, r.top));
            s.settings.save();
        });
    }
}

fn on_click(part: Part) {
    match part {
        Part::Rocket => post(Cmd::CleanRam),
        Part::Open => post(Cmd::ToggleMonitor),
        _ => {}
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let (x, y) = client_dip(hwnd, lp, true);
            return LRESULT(match part_at(x, y) {
                Part::Rocket | Part::Open => HTCLIENT,
                _ => HTCAPTION,
            } as isize);
        }
        WM_SETCURSOR => {
            let hover = with_widget(|wd| wd.hover).unwrap_or(Part::None);
            if matches!(hover, Part::Rocket | Part::Open) {
                // SAFETY: system cursor.
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            let (x, y) = client_dip(hwnd, lp, false);
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: requests WM_MOUSELEAVE for our window.
            let _ = unsafe { TrackMouseEvent(&mut tme) };
            set_hover(part_at(x, y));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => set_hover(Part::None),
        WM_LBUTTONUP => {
            let (x, y) = client_dip(hwnd, lp, false);
            on_click(part_at(x, y));
            return LRESULT(0);
        }
        WM_RBUTTONUP | WM_NCRBUTTONUP => {
            crate::wmenu::show();
            return LRESULT(0);
        }
        WM_TIMER if wp.0 == ANIM_TIMER => render(),
        WM_MOVING => {
            // SAFETY: for WM_MOVING, lParam points at the proposed window RECT.
            snap(unsafe { &mut *(lp.0 as *mut RECT) });
            popup::hide();
            return LRESULT(1);
        }
        WM_EXITSIZEMOVE => save_pos(hwnd),
        WM_DPICHANGED => {
            let dpi = (wp.0 & 0xFFFF) as u32;
            with_widget(|wd| {
                wd.dpi = dpi;
                wd.surf = None;
                wd.shown = None;
            });
            // SAFETY: lParam points at the suggested RECT for the new DPI.
            let r = unsafe { &*(lp.0 as *const RECT) };
            let flags = SWP_NOZORDER | SWP_NOACTIVATE;
            // SAFETY: resizing our own window.
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    gfx::px(W, dpi),
                    gfx::px(H, dpi),
                    flags,
                );
            }
            render();
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
    use std::time::Duration;

    #[test]
    fn hit_regions_match_layout() {
        assert_eq!(part_at(15.0, 15.0), Part::Rocket);
        assert_eq!(part_at(190.0, 15.0), Part::Open);
        assert_eq!(part_at(168.0, 15.0), Part::Body);
        assert_eq!(part_at(80.0, 15.0), Part::Body);
        assert_eq!(part_at(80.0, 2.0), Part::None); // transparent above the pill
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_axis(10, 100, 0, 1000), 0);
        assert_eq!(snap_axis(890, 100, 0, 1000), 900);
        assert_eq!(snap_axis(-500, 100, 0, 1000), 0);
        assert_eq!(snap_axis(400, 100, 0, 1000), 400);
    }

    #[test]
    fn animation_waits_for_the_result_then_lands() {
        let t0 = Instant::now();
        let s = Duration::from_secs_f32;
        let mut a = Anim {
            start: t0,
            from: 80,
            result: None,
        };
        assert!(frame(&a, t0 + s(0.1)).dx.abs() <= 0.9);
        assert!(frame(&a, t0 + s(0.7)).dy < -5.0);
        let waiting = frame(&a, t0 + s(5.0));
        assert!(waiting.spinner.is_some() && !waiting.done);
        a.result = Some((1 << 30, 60, t0 + s(5.0)));
        let mid = frame(&a, t0 + s(5.2));
        assert!(mid.ram.is_some_and(|r| (60..80).contains(&r)));
        let end = frame(&a, t0 + s(5.5));
        assert!(end.done);
        assert_eq!(end.ram, Some(60));
        assert!(end.dy.abs() < 0.01);
    }
}
