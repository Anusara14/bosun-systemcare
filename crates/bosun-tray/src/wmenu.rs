//! The floating widget's right-click menu, measured from the reference
//! (`.reanimate/widgetmenu`, recorded at 110 %): a 126 px panel opened at the
//! pointer, 27.3 px rows with 13 px text at 13.6, a small arrow 12 px from the
//! right on rows with a submenu; submenus open 2 px to the right, level with
//! their row, and show on hover. Checkable rows have the tick at 14.5 and the
//! text at 29.
//!
//! Everything here does what it says. Shut down, Restart and Log off ask
//! first; Hibernate is greyed out when Windows has it off. The reference's
//! "Temperature by Celsius" has no counterpart (the widget reads no
//! temperature sensor), so the Settings submenu offers "Hide widget" instead.

use std::cell::RefCell;
use std::os::windows::process::CommandExt;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Power::{IsPwrHibernateAllowed, SetSuspendState};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, IDC_ARROW, IDYES, LoadCursorW,
    MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW, RegisterClassExW,
    SW_SHOW, SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE,
    WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{HSTRING, w};

use crate::gfx::{self, Painter, col, cola, rect};
use crate::layered::Surface;
use crate::{Cmd, hinstance, post, shell, tools, widget, with_state};

const MAIN_W: f32 = 126.0;
const SUB_W: [f32; 2] = [126.0, 176.0];
const GAP: f32 = 2.0;
const ROW: f32 = 27.3;
/// Fallback close: checks for a click outside when we never got the focus.
const WATCH_TIMER: usize = 1;
const W: f32 = MAIN_W + GAP + 176.0;
const H: f32 = 7.0 * ROW + 1.0;

const MAIN: [&str; 5] = ["Clean RAM", "Screenshot", "Power", "Settings", "Exit"];
const POWER: [&str; 5] = ["Shut down", "Restart", "Log off", "Hibernate", "Sleep"];
const SETTINGS: [&str; 4] = [
    "Pin it up front",
    "Load at Windows startup",
    "Transparency",
    "Hide widget",
];

mod mc {
    pub const BG: u32 = 0x1E1E1E;
    pub const BORDER: u32 = 0x3A3A3A;
    pub const HOVER: u32 = 0x3C3C3C;
    pub const TEXT: u32 = 0xE6E6E6;
    pub const DIM: u32 = 0x6E6E6E;
}

/// What the pointer is over: (0 = main, 1 = Power, 2 = Settings; row).
type Spot = Option<(u8, usize)>;

struct Menu {
    hwnd: HWND,
    surf: Option<Surface>,
    hover: Spot,
    /// The open submenu: 1 = Power, 2 = Settings.
    sub: Option<u8>,
    /// Ticks in Settings, and whether Hibernate is available.
    checks: [bool; 3],
    hibernate: bool,
}

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

fn with_menu<R>(f: impl FnOnce(&mut Menu) -> R) -> Option<R> {
    MENU.with(|m| m.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn sub_rect(sub: u8) -> (f32, f32, f32, f32) {
    let rows = if sub == 1 {
        POWER.len()
    } else {
        SETTINGS.len()
    };
    let top = ROW * if sub == 1 { 2.0 } else { 3.0 };
    (
        MAIN_W + GAP,
        top,
        SUB_W[sub as usize - 1],
        rows as f32 * ROW,
    )
}

fn spot_at(x: f32, y: f32, sub: Option<u8>) -> Spot {
    if (0.0..MAIN_W).contains(&x) && (0.0..ROW * MAIN.len() as f32).contains(&y) {
        return Some((0, (y / ROW) as usize));
    }
    let s = sub?;
    let (sx, sy, sw, sh) = sub_rect(s);
    ((sx..sx + sw).contains(&x) && (sy..sy + sh).contains(&y))
        .then(|| (s, ((y - sy) / ROW) as usize))
}

fn panel(p: &Painter, x: f32, y: f32, w: f32, rows: usize) {
    let r = rect(x + 0.5, y + 0.5, w - 1.0, rows as f32 * ROW - 1.0);
    p.fill(r, col(mc::BG));
    p.stroke_rr(r, 0.0, 1.0, col(mc::BORDER));
}

fn row_text(p: &Painter, s: &str, x: f32, y: f32, c: u32) {
    p.text(
        s,
        &p.g.f.m13,
        rect(x, y, 170.0, ROW),
        col(c),
        gfx::Align::Left,
    );
}

fn draw(p: &Painter, m: &Menu) {
    p.clear(cola(0, 0.0));
    panel(p, 0.0, 0.0, MAIN_W, MAIN.len());
    for (k, label) in MAIN.iter().enumerate() {
        let y = k as f32 * ROW;
        let open = m
            .sub
            .is_some_and(|s| (s == 1 && k == 2) || (s == 2 && k == 3));
        if m.hover == Some((0, k)) || open {
            p.fill(rect(1.0, y + 1.0, MAIN_W - 2.0, ROW - 1.0), col(mc::HOVER));
        }
        row_text(p, label, 13.6, y, mc::TEXT);
        if k == 2 || k == 3 {
            let (ax, ay) = (MAIN_W - 12.0, y + ROW / 2.0);
            p.fill_poly(
                &[(ax - 2.0, ay - 3.5), (ax + 2.0, ay), (ax - 2.0, ay + 3.5)],
                col(mc::TEXT),
            );
        }
    }
    let Some(s) = m.sub else { return };
    let (sx, sy, sw, _) = sub_rect(s);
    let labels: &[&str] = if s == 1 { &POWER } else { &SETTINGS };
    panel(p, sx, sy, sw, labels.len());
    for (k, label) in labels.iter().enumerate() {
        let y = sy + k as f32 * ROW;
        let off = s == 1 && k == 3 && !m.hibernate;
        if m.hover == Some((s, k)) && !off {
            p.fill(rect(sx + 1.0, y + 1.0, sw - 2.0, ROW - 1.0), col(mc::HOVER));
        }
        let c = if off { mc::DIM } else { mc::TEXT };
        if s == 1 {
            row_text(p, label, sx + 13.6, y, c);
        } else {
            if m.checks.get(k).copied().unwrap_or(false) {
                let (cx, cy) = (sx + 14.5, y + ROW / 2.0);
                p.line(cx - 3.5, cy, cx - 1.0, cy + 2.8, 1.4, col(mc::TEXT));
                p.line(cx - 1.0, cy + 2.8, cx + 4.0, cy - 3.2, 1.4, col(mc::TEXT));
            }
            row_text(p, label, sx + 29.0, y, c);
        }
    }
}

fn render() {
    with_menu(|m| {
        // SAFETY: our own live window.
        let dpi = unsafe { GetDpiForWindow(m.hwnd) }.max(96);
        if m.surf.is_none() {
            m.surf = Surface::new(W, H, dpi).ok();
        }
        let ok = m
            .surf
            .as_ref()
            .is_some_and(|s| s.present(m.hwnd, |p| draw(p, m)));
        if !ok {
            m.surf = None;
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
            lpszClassName: w!("BosunWidgetMenu"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        });
    }
}

fn tray_exe() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| format!("\"{}\"", p.display()))
}

/// Opens the menu at the pointer, kept on the screen.
pub fn show() {
    hide();
    let mut pt = POINT::default();
    // SAFETY: reads the pointer position.
    let _ = unsafe { GetCursorPos(&mut pt) };
    // SAFETY: creates our own popup; sized for its DPI below.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("BosunWidgetMenu"),
            w!("Widget menu"),
            WS_POPUP,
            pt.x,
            pt.y,
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
    let main_h = gfx::px(ROW * MAIN.len() as f32, dpi);
    let wa = shell::work_area(pt);
    let x = pt.x.min(wa.right - w).max(wa.left);
    let y = if pt.y + h <= wa.bottom {
        pt.y
    } else {
        (pt.y - main_h).max(wa.top).min(wa.bottom - h)
    };
    let s = with_state(|s| (s.settings.widget_pinned, s.settings.widget_transparent));
    let (pinned, transparent) = s.unwrap_or((true, false));
    MENU.with(|m| {
        *m.borrow_mut() = Some(Menu {
            hwnd,
            surf: None,
            hover: None,
            sub: None,
            checks: [
                pinned,
                bosun_core::startup::autostart(bosun_core::startup::AUTOSTART_NAME),
                transparent,
            ],
            // SAFETY: a plain query.
            hibernate: unsafe { IsPwrHibernateAllowed() },
        })
    });
    render();
    // SAFETY: placing, showing and focusing our own window (the right-click
    // that opened it is our input, so it may take the foreground).
    unsafe {
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), WATCH_TIMER, 80, None);
    }
}

pub fn hide() {
    let hwnd = MENU.with(|m| {
        m.try_borrow_mut()
            .ok()
            .and_then(|mut g| g.take())
            .map(|m| m.hwnd)
    });
    if let Some(h) = hwnd {
        // SAFETY: destroying our own window after dropping its state.
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn confirm(text: &str) -> bool {
    // SAFETY: a modal message box with owned strings.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            w!("Bosun SystemCare"),
            MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        ) == IDYES
    }
}

fn shutdown(args: &[&str]) {
    let _ = std::process::Command::new("shutdown")
        .args(args)
        .creation_flags(0x0800_0000)
        .spawn();
}

fn power(k: usize) {
    const ASK: &str = "Apps that are still open may lose unsaved work.";
    match k {
        0 if confirm(&format!("Shut down the PC now?\n\n{ASK}")) => shutdown(&["/s", "/t", "0"]),
        1 if confirm(&format!("Restart the PC now?\n\n{ASK}")) => shutdown(&["/r", "/t", "0"]),
        2 if confirm(&format!("Sign out of Windows now?\n\n{ASK}")) => shutdown(&["/l"]),
        // SAFETY: plain power-state requests.
        3 => unsafe {
            let _ = SetSuspendState(true, false, false);
        },
        4 => unsafe {
            let _ = SetSuspendState(false, false, false);
        },
        _ => {}
    }
}

fn setting(k: usize, on: bool) {
    match k {
        0 => {
            with_state(|s| {
                s.settings.widget_pinned = !on;
                s.settings.save();
            });
            widget::apply_pin();
        }
        1 => {
            let cmd = tray_exe();
            let _ = bosun_core::startup::set_autostart(
                bosun_core::startup::AUTOSTART_NAME,
                if on { None } else { cmd.as_deref() },
            );
        }
        2 => {
            with_state(|s| {
                s.settings.widget_transparent = !on;
                s.settings.save();
            });
            widget::refresh();
        }
        _ => post(Cmd::ToggleWidget),
    }
}

fn click(spot: Spot) {
    let Some((level, k)) = spot else { return };
    let checks = with_menu(|m| m.checks).unwrap_or_default();
    let hibernate = with_menu(|m| m.hibernate).unwrap_or(false);
    match (level, k) {
        (0, 2) | (0, 3) => return, // submenus open on hover
        (1, 3) if !hibernate => return,
        _ => {}
    }
    // close first: the confirmations and power actions take over the screen
    hide();
    match (level, k) {
        (0, 0) => post(Cmd::CleanRam),
        (0, 1) => tools::screenshot(),
        (0, _) => post(Cmd::Exit),
        (1, k) => power(k),
        (_, k) => setting(k, checks.get(k).copied().unwrap_or(false)),
    }
}

/// Windows may refuse us the foreground (then no WM_ACTIVATE ever comes): a
/// mouse button held down while the pointer is off our panels closes the menu
/// like any other menu.
fn clicked_elsewhere(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: plain state queries on our own window.
    unsafe {
        if GetForegroundWindow() == hwnd {
            return false; // WM_ACTIVATE handles this case
        }
        let down = |k: u16| GetAsyncKeyState(k as i32) as u16 & 0x8000 != 0;
        if !down(VK_LBUTTON.0) && !down(VK_RBUTTON.0) {
            return false;
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = ScreenToClient(hwnd, &mut pt);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let (x, y) = (gfx::dip(pt.x, dpi), gfx::dip(pt.y, dpi));
        with_menu(|m| spot_at(x, y, m.sub)).flatten().is_none()
    }
}

fn local(hwnd: HWND, lp: LPARAM) -> (f32, f32) {
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
    (gfx::dip(x, dpi), gfx::dip(y, dpi))
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = local(hwnd, lp);
            let changed = with_menu(|m| {
                let spot = spot_at(x, y, m.sub);
                let sub = match spot {
                    Some((0, 2)) => Some(1),
                    Some((0, 3)) => Some(2),
                    Some((0, _)) => None,
                    _ => m.sub,
                };
                let changed = spot != m.hover || sub != m.sub;
                m.hover = spot;
                m.sub = sub;
                changed
            });
            if changed == Some(true) {
                render();
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = local(hwnd, lp);
            let spot = with_menu(|m| spot_at(x, y, m.sub)).flatten();
            click(spot);
            return LRESULT(0);
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            hide();
            return LRESULT(0);
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            hide();
            return LRESULT(0);
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wp.0 == WATCH_TIMER => {
            if clicked_elsewhere(hwnd) {
                hide();
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

    #[test]
    fn rows_and_submenus_sit_where_the_reference_has_them() {
        assert_eq!(spot_at(50.0, 5.0, None), Some((0, 0)));
        assert_eq!(spot_at(50.0, ROW * 4.5, None), Some((0, 4)));
        assert_eq!(spot_at(50.0, ROW * 5.5, None), None);
        // Power opens level with its row, 2 px to the right
        assert_eq!(sub_rect(1).1, ROW * 2.0);
        assert_eq!(
            spot_at(MAIN_W + GAP + 5.0, ROW * 2.5, Some(1)),
            Some((1, 0))
        );
        assert_eq!(
            spot_at(MAIN_W + GAP + 5.0, ROW * 6.5, Some(1)),
            Some((1, 4))
        );
        assert_eq!(
            spot_at(MAIN_W + GAP + 150.0, ROW * 3.5, Some(2)),
            Some((2, 0))
        );
        assert_eq!(spot_at(MAIN_W + GAP + 5.0, ROW * 2.5, None), None);
    }

    /// Renders both submenus: `cargo test -p bosun-tray menu_snapshot -- --ignored`
    #[test]
    #[ignore]
    fn menu_snapshot() {
        let out = std::env::var("BOSUN_SNAPSHOT_DIR").unwrap_or_else(|_| ".".into());
        // SAFETY: COM for this test thread, then the drawing factories.
        let _ = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            )
        };
        bosun_ui::init().unwrap();
        for (sub, hover) in [(1u8, (1u8, 1usize)), (2, (2, 1))] {
            let m = Menu {
                hwnd: HWND::default(),
                surf: None,
                hover: Some(hover),
                sub: Some(sub),
                checks: [true, true, false],
                hibernate: false,
            };
            let path = std::path::Path::new(&out).join(format!("wmenu{sub}.png"));
            bosun_ui::snapshot::save_png(
                crate::graphics(),
                W as u32,
                H as u32,
                col(0x3A2A4A),
                &path,
                |p| draw(p, &m),
            )
            .unwrap();
        }
    }
}
