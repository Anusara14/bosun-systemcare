//! "Memory released" card shown under the widget after Clean RAM.
//! 230 Ã— 123 px, 7 px below the pill and 2 px left of the knob (reference layout).

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IDC_ARROW, IDC_HAND, KillTimer, LoadCursorW,
    RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetCursor, SetTimer,
    SetWindowPos, ShowWindow, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;

use crate::gfx::{self, Align, Painter, col, cola, contains, rect};
use crate::layered::Surface;
use crate::{hinstance, shell, widget, with_state};

const WM_MOUSELEAVE: u32 = 0x02A3;
const CLOSE_TIMER: usize = 9;
const AUTO_CLOSE_MS: u32 = 6000;

const W: f32 = 230.0;
const H: f32 = 123.0;
/// Offset from the widget's top-left corner.
const DX: f32 = -2.0;
const DY: f32 = 32.0;

mod pc {
    pub const BG: u32 = 0x161616;
    pub const BORDER: u32 = 0x2E2E2E;
    pub const CLOSE: u32 = 0x8A8A8A;
    pub const CLOSE_HOVER: u32 = 0xE0E0E0;
    pub const LABEL: u32 = 0xB0B0B0;
    pub const VALUE: u32 = 0xE6E6E6;
    pub const LINK: u32 = 0xE6E6E6;
    pub const LINK_HOVER: u32 = 0xFFFFFF;
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Part {
    None,
    Close,
    Link,
}

const CLOSE_RECT: (f32, f32, f32, f32) = (205.0, 3.0, 21.0, 21.0);
const LINK_Y: f32 = 86.0;

fn part_at(x: f32, y: f32, link_w: f32) -> Part {
    let (cx, cy, cw, ch) = CLOSE_RECT;
    if contains(&rect(cx, cy, cw, ch), x, y) {
        Part::Close
    } else if contains(&rect(20.0, LINK_Y, link_w, 20.0), x, y) {
        Part::Link
    } else {
        Part::None
    }
}

struct Popup {
    hwnd: HWND,
    surf: Option<Surface>,
    freed: u64,
    hover: Part,
}

thread_local! {
    static POPUP: RefCell<Option<Popup>> = const { RefCell::new(None) };
}

fn with_popup<R>(f: impl FnOnce(&mut Popup) -> R) -> Option<R> {
    POPUP.with(|p| p.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn link_text() -> &'static str {
    if with_state(|s| s.settings.auto_ram_clean).unwrap_or(false) {
        "Disable Auto RAM Clean"
    } else {
        "Enable Auto RAM Clean"
    }
}

/// Reference style: "2.3 GB", "512 MB".
fn released(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / (1024.0 * 1024.0))
    }
}

fn draw(p: &Painter, freed: u64, hover: Part) {
    let f = &p.g.f;
    p.clear(cola(0, 0.0));
    let card = rect(0.5, 0.5, W - 1.0, H - 1.0);
    p.fill_rr(card, 6.0, col(pc::BG));
    p.stroke_rr(card, 6.0, 1.0, col(pc::BORDER));
    // close Ã—, 8 px, centred at (215.5, 13.5)
    let c = col(if hover == Part::Close {
        pc::CLOSE_HOVER
    } else {
        pc::CLOSE
    });
    let (x0, y0, s) = (211.5, 9.5, 8.0);
    p.line(x0, y0, x0 + s, y0 + s, 1.6, c);
    p.line(x0 + s, y0, x0, y0 + s, 1.6, c);
    p.text(
        "Memory released:",
        &f.pop_label,
        rect(20.0, 20.0, 190.0, 20.0),
        col(pc::LABEL),
        Align::Left,
    );
    let value = released(freed);
    p.text(
        &value,
        &f.pop_value,
        rect(20.0, 40.0, 190.0, 28.0),
        col(pc::VALUE),
        Align::Left,
    );
    let link = link_text();
    let lw = p.g.measure(link, &f.pop_label);
    let lc = col(if hover == Part::Link {
        pc::LINK_HOVER
    } else {
        pc::LINK
    });
    p.text(
        link,
        &f.pop_label,
        rect(20.0, LINK_Y, lw + 2.0, 20.0),
        lc,
        Align::Left,
    );
    p.line(20.0, LINK_Y + 16.5, 20.0 + lw, LINK_Y + 16.5, 1.0, lc);
}

fn render() {
    with_popup(|pp| {
        // SAFETY: our own live window.
        let dpi = unsafe { GetDpiForWindow(pp.hwnd) }.max(96);
        if pp.surf.is_none() {
            pp.surf = Surface::new(W, H, dpi).ok();
        }
        let (freed, hover) = (pp.freed, pp.hover);
        let ok = pp
            .surf
            .as_ref()
            .is_some_and(|s| s.present(pp.hwnd, |p| draw(p, freed, hover)));
        if !ok {
            pp.surf = None;
        }
    });
}

pub fn register_class() {
    // SAFETY: registering a class with a static name and proc.
    unsafe {
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: hinstance(),
            lpszClassName: w!("BosunCleanPopup"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
    }
}

/// Under the widget; above it if there is no room below.
fn position(widget: RECT, dpi: u32) -> (i32, i32, i32, i32) {
    let (w, h) = (gfx::px(W, dpi), gfx::px(H, dpi));
    let x = widget.left + gfx::px(DX, dpi);
    let below = widget.top + gfx::px(DY, dpi);
    let wa = shell::work_area(windows::Win32::Foundation::POINT {
        x: widget.left,
        y: widget.top,
    });
    let y = if below + h <= wa.bottom {
        below
    } else {
        widget.top - gfx::px(7.0, dpi) - h
    };
    (x.clamp(wa.left, (wa.right - w).max(wa.left)), y, w, h)
}

pub fn show(freed: u64) {
    hide();
    let Some((wr, dpi)) = widget::placement() else {
        return;
    };
    let (x, y, w, h) = position(wr, dpi);
    // SAFETY: creates, places and shows our own non-activating popup.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            w!("BosunCleanPopup"),
            w!("Memory released"),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(hinstance()),
            None,
        )
    };
    let Ok(hwnd) = hwnd else { return };
    POPUP.with(|p| {
        *p.borrow_mut() = Some(Popup {
            hwnd,
            surf: None,
            freed,
            hover: Part::None,
        })
    });
    render();
    // SAFETY: our own window.
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), CLOSE_TIMER, AUTO_CLOSE_MS, None);
    }
}

pub fn hide() {
    let hwnd = POPUP.with(|p| {
        p.try_borrow_mut()
            .ok()
            .and_then(|mut g| g.take())
            .map(|pp| pp.hwnd)
    });
    if let Some(h) = hwnd {
        // SAFETY: destroying our own window after dropping its state.
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn local(hwnd: HWND, lp: LPARAM) -> (f32, f32) {
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
    (gfx::dip(x, dpi), gfx::dip(y, dpi))
}

fn set_hover(hwnd: HWND, p: Part) {
    let changed = with_popup(|pp| std::mem::replace(&mut pp.hover, p) != p);
    if changed == Some(true) {
        render();
        // Hovering keeps the card open: restart the auto-close countdown.
        // SAFETY: our own timer.
        unsafe { SetTimer(Some(hwnd), CLOSE_TIMER, AUTO_CLOSE_MS, None) };
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let link_w = || crate::graphics().measure(link_text(), &crate::graphics().f.pop_label);
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = local(hwnd, lp);
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: requests WM_MOUSELEAVE for our window.
            let _ = unsafe { TrackMouseEvent(&mut tme) };
            set_hover(hwnd, part_at(x, y, link_w()));
            return LRESULT(0);
        }
        WM_MOUSELEAVE => set_hover(hwnd, Part::None),
        WM_SETCURSOR => {
            if with_popup(|pp| pp.hover).is_some_and(|h| h != Part::None) {
                // SAFETY: system cursor.
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return LRESULT(1);
            }
        }
        WM_LBUTTONUP => {
            let (x, y) = local(hwnd, lp);
            match part_at(x, y, link_w()) {
                Part::Close => hide(),
                Part::Link => {
                    with_state(|s| {
                        s.settings.auto_ram_clean = !s.settings.auto_ram_clean;
                        s.settings.save();
                    });
                    render();
                }
                Part::None => {}
            }
            return LRESULT(0);
        }
        WM_TIMER if wp.0 == CLOSE_TIMER => {
            // SAFETY: our own timer.
            let _ = unsafe { KillTimer(Some(hwnd), CLOSE_TIMER) };
            hide();
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

    #[test]
    fn released_matches_reference_format() {
        assert_eq!(released(2_469_606_195), "2.3 GB");
        assert_eq!(released(536_870_912), "512 MB");
        assert_eq!(released(0), "0 MB");
    }

    #[test]
    fn hit_regions() {
        assert_eq!(part_at(215.0, 13.0, 150.0), Part::Close);
        assert_eq!(part_at(60.0, 96.0, 150.0), Part::Link);
        assert_eq!(part_at(60.0, 50.0, 150.0), Part::None);
        assert_eq!(part_at(200.0, 96.0, 150.0), Part::None); // past the link text
    }
}
