//! Title bar and sidebar, laid out from the reference main window (960 × 660):
//! a 144 px sidebar panel with 88 px nav items, and a 44 px title bar with
//! notification / menu / minimise / maximise / close buttons on the right.

use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, MF_SEPARATOR, MF_STRING,
    SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTALIGN, TrackPopupMenu,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::icons;
use crate::pages::{ALL, Page};
use crate::window::{Hit, View};

pub const TITLE_H: f32 = 44.0;
pub const SIDEBAR_W: f32 = 152.0;
const PANEL: (f32, f32, f32) = (7.0, 7.0, 144.0);
const ITEM_H: f32 = 88.0;
const BTN_CY: f32 = 21.0;

mod cc {
    pub const PANEL: u32 = 0x1A1A1A;
    pub const SELECTED: u32 = 0x2E2E2E;
    pub const HOVER: u32 = 0x242424;
    pub const TEXT: u32 = 0xFFFFFF;
    pub const ICON: u32 = 0xE4E4E4;
    pub const TEXT_2: u32 = 0xB0B0B0;
    pub const TEXT_3: u32 = 0x7A7A7A;
    pub const CLOSE_HOVER: u32 = 0xC42B1C;
}

fn item_rect(i: usize) -> D2D_RECT_F {
    rect(11.0, 11.0 + i as f32 * ITEM_H, 136.0, ITEM_H)
}

/// Title buttons, right to left from the window edge (reference offsets).
fn buttons(w: f32) -> [(Hit, f32); 5] {
    [
        (Hit::Close, w - 20.0),
        (Hit::Max, w - 57.0),
        (Hit::Min, w - 92.0),
        (Hit::Menu, w - 129.0),
        (Hit::Bell, w - 201.0),
    ]
}

fn button_rect(cx: f32) -> D2D_RECT_F {
    rect(cx - 17.0, BTN_CY - 15.0, 34.0, 30.0)
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    if y < TITLE_H {
        return buttons(v.w)
            .into_iter()
            .find(|(_, cx)| contains(&button_rect(*cx), x, y))
            .map_or(Hit::None, |(h, _)| h);
    }
    ALL.iter()
        .enumerate()
        .find(|(i, _)| contains(&item_rect(*i), x, y))
        .map_or(Hit::None, |(_, p)| Hit::Nav(*p))
}

pub fn draw(p: &Painter, v: &View) {
    draw_sidebar(p, v);
    draw_title(p, v);
}

/// Flat panel and flat selected card, as in the reference; filled white icons.
fn draw_sidebar(p: &Painter, v: &View) {
    let f = &p.g.f;
    let (x, y, w) = PANEL;
    p.fill_rr(rect(x, y, w, v.h - 2.0 * y), 8.0, col(cc::PANEL));
    for (i, page) in ALL.iter().enumerate() {
        let r = item_rect(i);
        let selected = v.page == *page;
        let bg = if selected {
            cc::SELECTED
        } else if v.hover == Hit::Nav(*page) {
            cc::HOVER
        } else {
            cc::PANEL
        };
        if bg != cc::PANEL {
            p.fill_rr(r, 8.0, col(bg));
        }
        let cx = (r.left + r.right) / 2.0;
        let fg = col(if selected { cc::TEXT } else { cc::ICON });
        icons::nav(p, *page, cx, r.top + 35.0, fg, col(bg));
        let label = rect(r.left, r.top + 54.0, r.right - r.left, 20.0);
        p.text(page.label(), &f.m13sb, label, fg, Align::Center);
    }
    let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
    p.text(
        &ver,
        &f.caption,
        rect(x, v.h - 44.0, w, 20.0),
        col(cc::TEXT_3),
        Align::Center,
    );
}

fn draw_title(p: &Painter, v: &View) {
    let f = &p.g.f;
    let content = rect(SIDEBAR_W, 0.0, v.w - SIDEBAR_W, TITLE_H - 2.0);
    p.text(
        "Bosun SystemCare",
        &f.m13,
        content,
        col(cc::TEXT_2),
        Align::Center,
    );
    for (h, cx) in buttons(v.w) {
        let r = button_rect(cx);
        let hovered = v.hover == h;
        if hovered {
            let bg = if h == Hit::Close {
                col(cc::CLOSE_HOVER)
            } else {
                cola(0xFFFFFF, 0.08)
            };
            p.fill_rr(r, 4.0, bg);
        }
        let glyph = match h {
            Hit::Close => '\u{E8BB}',
            Hit::Max if v.maximized => '\u{E923}',
            Hit::Max => '\u{E922}',
            Hit::Min => '\u{E921}',
            Hit::Menu => '\u{E700}',
            _ => '\u{EA8F}',
        };
        let c = col(if hovered { cc::TEXT } else { cc::TEXT_2 });
        let font = if matches!(h, Hit::Bell | Hit::Menu) {
            &f.icon
        } else {
            &f.icon12
        };
        p.glyph(glyph, font, r, c);
    }
    if let Some(note) = v.note {
        let r = rect(SIDEBAR_W, TITLE_H, v.w - SIDEBAR_W, 22.0);
        p.text(note, &f.caption, r, col(cc::TEXT_2), Align::Center);
    }
}

fn open(target: &str) {
    // SAFETY: plain ShellExecute with owned strings.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// The ☰ menu, dropped down from its button.
pub fn show_menu(hwnd: HWND) {
    // SAFETY: popup menu owned by our window and destroyed after use.
    let choice = unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let w = bosun_ui::gfx::dip(rc.right, dpi);
        let b = button_rect(w - 129.0);
        let mut pt = POINT {
            x: bosun_ui::gfx::px(b.right, dpi),
            y: bosun_ui::gfx::px(b.bottom, dpi),
        };
        let _ = ClientToScreen(hwnd, &mut pt);
        let Ok(menu) = CreatePopupMenu() else { return };
        let add = |id: usize, text: PCWSTR| {
            let _ = AppendMenuW(
                menu,
                if id == 0 { MF_SEPARATOR } else { MF_STRING },
                id,
                text,
            );
        };
        add(1, w!("Performance Monitor"));
        add(2, w!("About Bosun SystemCare"));
        add(0, PCWSTR::null());
        add(3, w!("Exit"));
        let c = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        )
        .0;
        let _ = DestroyMenu(menu);
        c
    };
    match choice {
        1 => {
            // The tray opens its monitor when started a second time.
            if let Some(dir) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            {
                open(&dir.join("bosun-tray.exe").to_string_lossy());
            }
        }
        2 => crate::window::go(hwnd, Page::About),
        // SAFETY: closing our own window ends the process.
        3 => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{Mode, View};

    fn view() -> View {
        View {
            page: Page::Care,
            w: 960.0,
            h: 660.0,
            hover: Hit::None,
            pressed: Hit::None,
            mode: Mode::Recommended,
            scan_hover: 0.0,
            scan_label: (1.0, 0.0),
            note: None,
            maximized: false,
        }
    }

    #[test]
    fn title_buttons_match_reference_positions() {
        let v = view();
        assert_eq!(hit(&v, 940.0, 21.0), Hit::Close);
        assert_eq!(hit(&v, 903.0, 21.0), Hit::Max);
        assert_eq!(hit(&v, 868.0, 21.0), Hit::Min);
        assert_eq!(hit(&v, 831.0, 21.0), Hit::Menu);
        assert_eq!(hit(&v, 759.0, 21.0), Hit::Bell);
        assert_eq!(hit(&v, 500.0, 21.0), Hit::None);
    }

    #[test]
    fn nav_items_are_88_px_apart() {
        let v = view();
        assert_eq!(hit(&v, 80.0, 60.0), Hit::Nav(Page::Care));
        assert_eq!(hit(&v, 80.0, 60.0 + 88.0), Hit::Nav(Page::SpeedUp));
        assert_eq!(hit(&v, 80.0, 60.0 + 4.0 * 88.0), Hit::Nav(Page::About));
    }
}
