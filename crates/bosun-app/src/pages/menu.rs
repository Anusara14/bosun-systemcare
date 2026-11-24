//! The ☰ menu panel and the About dialog, measured from the reference
//! (`.reanimate/phase6`, scaled from 150 %).
//!
//! Menu: a full-height panel 609 px wide on the right, the rest dimmed. Left
//! column: 14 px items 32 px apart (cap centres 33…, 152…, 271…) with a 16 px
//! icon at +29 and the text at +53, dividers at 124 and 243. Right column at
//! +297: "Skin:" over a 129 × 88 thumbnail at 48, then labelled settings.
//! IObit's links to its support site, store and translations have no
//! equivalent here; the column shows real settings instead of fonts and skins
//! we don't offer.
//!
//! About: a 799 × 519 dialog. Logo, name, version and licence on the left
//! (centred on +217), a divider at +400, acknowledgements on the right.

use bosun_ui::gfx::{Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::protect::toggle;
use crate::pages::speedup::text_at;
use crate::window::{Hit, View};

const PANEL_W: f32 = 609.0;
const AUTOSTART: &str = bosun_core::startup::AUTOSTART_NAME;

/// What the menu's settings column shows.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct MenuInfo {
    pub widget: bool,
    pub tray_start: bool,
    pub auto_ram: bool,
    pub advisor: bool,
}

impl MenuInfo {
    pub fn load() -> Self {
        MenuInfo {
            widget: crate::tray_ini::get("widget_visible", true),
            tray_start: bosun_core::startup::autostart(AUTOSTART),
            auto_ram: crate::tray_ini::get("auto_ram_clean", false),
            advisor: bosun_core::homepage::advisor_on(),
        }
    }
}

/// Flips setting `k` (see `SETTINGS`); `Err` carries a message for the user.
pub fn flip(info: &MenuInfo, k: u8) -> Result<(), &'static str> {
    let saved = match k {
        0 => crate::tray_ini::set("widget_visible", !info.widget),
        1 => {
            let tray = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("bosun-tray.exe")))
                .filter(|t| t.is_file())
                .ok_or("bosun-tray.exe isn't next to Bosun.")?;
            let cmd = format!("\"{}\"", tray.display());
            bosun_core::startup::set_autostart(
                AUTOSTART,
                (!info.tray_start).then_some(cmd.as_str()),
            )
        }
        2 => crate::tray_ini::set("auto_ram_clean", !info.auto_ram),
        _ => bosun_core::homepage::set_advisor(!info.advisor),
    };
    saved.map_err(|_| "Couldn't save that setting.")
}

/// (label above, line beside the switch)
const SETTINGS: [(&str, &str); 4] = [
    ("Floating widget:", "Show RAM and CPU on the desktop"),
    ("Windows startup:", "Start the Bosun tray with Windows"),
    ("Auto RAM Clean:", "Trim memory when RAM use runs high"),
    ("Homepage Advisor:", "Warn if the browser homepage changes"),
];
const SETTING_Y: [f32; 4] = [164.0, 234.0, 304.0, 374.0];

/// (label, cap centre y); dividers sit at 124 and 243.
const ITEMS: [(&str, f32); 8] = [
    ("Quick Settings", 33.0),
    ("Settings…", 65.0),
    ("Performance Monitor", 97.0),
    ("Rescue Center", 152.0),
    ("Journal Folder", 184.0),
    ("Windows Update", 216.0),
    ("About", 271.0),
    ("Exit", 303.0),
];

fn panel(v: &View) -> D2D_RECT_F {
    rect(v.w - PANEL_W, 0.0, PANEL_W, v.h)
}

fn item_rect(v: &View, k: usize) -> D2D_RECT_F {
    rect(v.w - PANEL_W + 11.0, ITEMS[k].1 - 14.0, 243.0, 28.0)
}

fn switch_rect(v: &View, k: usize) -> D2D_RECT_F {
    rect(v.w - 43.0 - 31.0, SETTING_Y[k] + 30.0 - 7.0, 31.0, 14.0)
}

pub fn menu_hit(v: &View, x: f32, y: f32) -> Hit {
    if !contains(&panel(v), x, y) {
        return Hit::MenuOutside;
    }
    if let Some(k) = (0..ITEMS.len()).find(|&k| contains(&item_rect(v, k), x, y)) {
        return Hit::MenuItem(k as u8);
    }
    for l in [false, true] {
        if contains(&skin_rect(v, l), x, y) {
            return Hit::MenuSkin(u8::from(l));
        }
    }
    for k in 0..SETTINGS.len() {
        let s = switch_rect(v, k);
        // the whole line toggles, like a label for its switch
        let row = rect(
            v.w - PANEL_W + 297.0,
            s.top - 6.0,
            s.right - (v.w - PANEL_W + 297.0),
            26.0,
        );
        if contains(&row, x, y) {
            return Hit::MenuToggle(k as u8);
        }
    }
    Hit::None
}

/// The menu's 16 px icons. Most are Segoe Fluent Icons glyphs, crisp at any
/// DPI like the reference's solid set; the two without a glyph are drawn.
fn item_icon(p: &Painter, k: usize, cx: f32, cy: f32) {
    let c = col(0xEDEDED);
    let r = rect(cx - 10.0, cy - 10.0, 20.0, 20.0);
    let glyph = |g: char| p.glyph(g, &p.g.f.icon, r, c);
    match k {
        0 => {
            // two switches, the upper one off and the lower one on
            p.stroke_rr(rect(cx - 8.0, cy - 7.5, 16.0, 6.5), 3.25, 1.5, c);
            p.circle(cx + 4.6, cy - 4.25, 1.9, c);
            p.fill_rr(rect(cx - 8.25, cy + 1.0, 16.5, 7.0), 3.5, c);
            p.circle(cx - 4.6, cy + 4.5, 2.0, col(0x232323));
        }
        1 => glyph('\u{E713}'), // Setting
        2 => glyph('\u{E9D9}'), // Diagnostic
        3 => {
            // lifebuoy: a solid ring with four bands cut out
            p.ring(cx, cy, 5.9, 4.2, c);
            for i in 0..4 {
                let t = std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::FRAC_PI_2;
                let (s, co) = t.sin_cos();
                p.line(
                    cx + 3.5 * co,
                    cy + 3.5 * s,
                    cx + 8.4 * co,
                    cy + 8.4 * s,
                    1.7,
                    col(0x232323),
                );
            }
        }
        4 => glyph('\u{E8D5}'), // FolderOpen
        5 => glyph('\u{E895}'), // Sync
        6 => glyph('\u{E946}'), // Info
        _ => glyph('\u{E7E8}'), // PowerButton
    }
}

/// A skin preview: the real Care screen drawn at 13 % in that skin, like the
/// reference's screenshots, on a soft shadow; the chosen skin gets a blue
/// outline and a corner check, the other lifts its outline on hover.
fn skin_thumb(p: &Painter, r: D2D_RECT_F, light: bool, chosen: bool, hot: bool) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    p.untinted(|| {
        for (grow, a) in [(3.0, 0.10), (1.5, 0.16)] {
            let s = rect(
                r.left - grow,
                r.top - grow + 1.5,
                w + 2.0 * grow,
                h + 2.0 * grow,
            );
            p.fill_rr(s, 5.0 + grow, cola(0x000000, a));
        }
    });
    let was = bosun_ui::gfx::is_light();
    bosun_ui::gfx::set_light(light);
    let mini = crate::window::idle_view();
    let s = w / mini.w;
    p.clip(r);
    p.fill(r, col(crate::backdrop::BASE));
    p.scaled(r.left, r.top + (h - mini.h * s) / 2.0, s, || {
        crate::chrome::draw(p, &mini);
        crate::pages::care::draw(p, &mini);
    });
    p.unclip();
    bosun_ui::gfx::set_light(was);
    p.untinted(|| {
        let edge = rect(r.left + 0.5, r.top + 0.5, w - 1.0, h - 1.0);
        if chosen {
            p.stroke_rr(
                rect(r.left - 1.0, r.top - 1.0, w + 2.0, h + 2.0),
                5.0,
                2.0,
                col(0x61ADF8),
            );
            p.fill_poly(
                &[
                    (r.right - 18.0, r.bottom + 1.0),
                    (r.right + 1.0, r.bottom - 18.0),
                    (r.right + 1.0, r.bottom + 1.0),
                ],
                col(0x61ADF8),
            );
            let k = col(0xFFFFFF);
            p.line(
                r.right - 10.0,
                r.bottom - 5.5,
                r.right - 7.0,
                r.bottom - 2.5,
                1.6,
                k,
            );
            p.line(
                r.right - 7.0,
                r.bottom - 2.5,
                r.right - 2.0,
                r.bottom - 8.5,
                1.6,
                k,
            );
        } else {
            let c = if hot {
                cola(0x61ADF8, 0.8)
            } else {
                cola(0x808080, 0.45)
            };
            p.stroke_rr(edge, 4.0, 1.0, c);
        }
    });
}

fn skin_rect(v: &View, light: bool) -> D2D_RECT_F {
    rect(
        v.w - PANEL_W + 297.0 + if light { 140.0 } else { 0.0 },
        48.0,
        129.0,
        88.0,
    )
}

pub fn draw_menu(p: &Painter, v: &View, info: &MenuInfo) {
    let f = &p.g.f;
    p.fill(rect(0.0, 0.0, v.w, v.h), cola(0x000000, 0.55));
    let pr = panel(v);
    p.fill(pr, col(0x232323));
    p.line(pr.left, 0.0, pr.left, v.h, 1.0, col(0x2E2E2E));
    let x0 = pr.left;
    for (k, (label, cy)) in ITEMS.iter().enumerate() {
        let hot = v.hover == Hit::MenuItem(k as u8);
        if hot || k == 0 {
            p.fill_rr(
                item_rect(v, k),
                3.0,
                col(if hot { 0x343434 } else { 0x2B2B2B }),
            );
        }
        item_icon(p, k, x0 + 29.0, *cy);
        text_at(p, label, &f.body, 14.0, x0 + 53.0, 200.0, *cy, 0xE6E6E6);
    }
    for y in [124.0, 243.0] {
        p.line(x0 + 15.0, y, x0 + 249.0, y, 1.0, col(0x3A3A3A));
    }
    let rx = x0 + 297.0;
    text_at(p, "Skin:", &f.body, 14.0, rx, 40.0, 31.0, 0xB8B8B8);
    let light = bosun_ui::gfx::is_light();
    let name = if light { "Light" } else { "Dark" };
    text_at(p, name, &f.m18, 16.0, rx + 33.0, 60.0, 30.0, 0xF2F2F2);
    for l in [false, true] {
        let hot = v.hover == Hit::MenuSkin(u8::from(l));
        skin_thumb(p, skin_rect(v, l), l, l == light, hot);
    }
    for (k, (label, line)) in SETTINGS.iter().enumerate() {
        let y = SETTING_Y[k];
        text_at(p, label, &f.body, 14.0, rx, 240.0, y, 0xB8B8B8);
        let s = switch_rect(v, k);
        text_at(
            p,
            line,
            &f.small,
            13.0,
            rx,
            s.left - rx - 10.0,
            y + 30.0,
            0xE6E6E6,
        );
        let on = [info.widget, info.tray_start, info.auto_ram, info.advisor][k];
        toggle(p, s, on, v.hover == Hit::MenuToggle(k as u8));
    }
    p.logo(x0 + 21.0, v.h - 27.0, 16.0);
    let ver = format!("Bosun SystemCare {}", env!("CARGO_PKG_VERSION"));
    text_at(
        p,
        &ver,
        &f.body,
        14.0,
        x0 + 44.0,
        300.0,
        v.h - 19.0,
        0x8A8A8A,
    );
}

fn dialog(v: &View) -> D2D_RECT_F {
    rect(
        ((v.w - 799.0) / 2.0).floor(),
        ((v.h - 519.0) / 2.0).floor(),
        799.0,
        519.0,
    )
}

/// (text, cap centre y relative to the dialog), links on the left half.
const LINKS: [(&str, f32); 3] = [
    ("Rescue Center", 386.0),
    ("Journal Folder", 386.0),
    ("How to uninstall", 408.0),
];

fn link_rects(p_measure: impl Fn(&str) -> f32, d: &D2D_RECT_F) -> [D2D_RECT_F; 3] {
    let cx = d.left + 217.0;
    let (w0, w1, w2) = (
        p_measure(LINKS[0].0),
        p_measure(LINKS[1].0),
        p_measure(LINKS[2].0),
    );
    let gap = 21.0;
    let x0 = cx - (w0 + gap + w1) / 2.0;
    [
        rect(x0, d.top + LINKS[0].1 - 9.0, w0, 18.0),
        rect(x0 + w0 + gap, d.top + LINKS[1].1 - 9.0, w1, 18.0),
        rect(cx - w2 / 2.0, d.top + LINKS[2].1 - 9.0, w2, 18.0),
    ]
}

/// Link widths at 13 px Segoe UI, close enough for hit-testing without a
/// text layout (the drawing measures exactly).
fn approx_w(s: &str) -> f32 {
    s.chars().count() as f32 * 6.4
}

pub fn about_hit(v: &View, x: f32, y: f32) -> Hit {
    let d = dialog(v);
    if !contains(&d, x, y) || contains(&rect(d.right - 34.0, d.top, 34.0, 34.0), x, y) {
        return Hit::AboutClose;
    }
    link_rects(approx_w, &d)
        .iter()
        .position(|r| contains(r, x, y))
        .map_or(Hit::None, |k| Hit::AboutLink(k as u8))
}

const THANKS: [&str; 7] = [
    "windows-rs (Microsoft): Rust for Windows,",
    "MIT or Apache-2.0",
    "windows-numerics: vector maths",
    "serde_json: reading browser settings,",
    "MIT or Apache-2.0",
    "winget (Microsoft): Software Updater",
    "Direct2D and DirectWrite: drawing and text",
];

pub fn draw_about(p: &Painter, v: &View) {
    let f = &p.g.f;
    p.fill(rect(0.0, 0.0, v.w, v.h), cola(0x000000, 0.6));
    let d = dialog(v);
    p.fill_rr(d, 6.0, col(0x242424));
    p.stroke_rr(d, 6.0, 1.0, col(0x333333));
    let close = v.hover == Hit::AboutClose;
    let (xx, xy) = (d.right - 17.0, d.top + 17.0);
    let xc = col(if close { 0xFFFFFF } else { 0xA0A0A0 });
    p.line(xx - 4.5, xy - 4.5, xx + 4.5, xy + 4.5, 1.2, xc);
    p.line(xx + 4.5, xy - 4.5, xx - 4.5, xy + 4.5, 1.2, xc);

    let cx = d.left + 217.0;
    p.logo(cx - 50.0, d.top + 88.0, 100.0);
    let mid = |s: &str, font, size: f32, y: f32, c: u32| {
        let w = p.g.measure(s, font);
        text_at(p, s, font, size, cx - w / 2.0, w + 4.0, d.top + y, c);
    };
    mid("Bosun SystemCare", &f.m18, 18.0, 219.0, 0xE6E6E6);
    let ver = format!("Version {}", env!("CARGO_PKG_VERSION"));
    mid(&ver, &f.m13, 13.0, 244.0, 0xD0D0D0);
    mid("NO WARRANTY", &f.m13, 13.0, 278.0, 0xD0D0D0);
    mid(
        "This program is provided as is, without",
        &f.m13,
        13.0,
        305.0,
        0xB8B8B8,
    );
    mid(
        "warranty of any kind. Every change is journaled.",
        &f.m13,
        13.0,
        324.0,
        0xB8B8B8,
    );
    mid(
        "Copyright © 2026 Bosun SystemCare.",
        &f.m13,
        13.0,
        364.0,
        0xB8B8B8,
    );
    for (k, r) in link_rects(|s| p.g.measure(s, &f.m13), &d)
        .iter()
        .enumerate()
    {
        let c = if v.hover == Hit::AboutLink(k as u8) {
            0xFFFFFF
        } else {
            0xD8D8D8
        };
        text_at(
            p,
            LINKS[k].0,
            &f.m13,
            13.0,
            r.left,
            r.right - r.left + 4.0,
            d.top + LINKS[k].1,
            c,
        );
        p.line(
            r.left,
            d.top + LINKS[k].1 + 7.5,
            r.right,
            d.top + LINKS[k].1 + 7.5,
            1.0,
            col(c),
        );
    }
    let [a, b, _] = link_rects(|s| p.g.measure(s, &f.m13), &d);
    let sep = (a.right + b.left) / 2.0;
    p.line(sep, d.top + 380.0, sep, d.top + 392.0, 1.0, col(0x555555));

    p.line(
        d.left + 400.0,
        d.top + 82.0,
        d.left + 400.0,
        d.top + 430.0,
        1.0,
        col(0x333333),
    );
    let w = p.g.measure("Acknowledgements", &f.m13);
    text_at(
        p,
        "Acknowledgements",
        &f.m13,
        13.0,
        d.left + 589.0 - w / 2.0,
        w + 4.0,
        d.top + 96.0,
        0xD0D0D0,
    );
    for (k, line) in THANKS.iter().enumerate() {
        text_at(
            p,
            line,
            &f.m13,
            13.0,
            d.left + 459.0,
            330.0,
            d.top + 134.0 + 19.0 * k as f32,
            0xB8B8B8,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_and_dialog_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        assert_eq!(panel(&v).left, 351.0);
        assert_eq!(menu_hit(&v, 400.0, 65.0), Hit::MenuItem(1));
        assert_eq!(menu_hit(&v, 100.0, 300.0), Hit::MenuOutside);
        let s = switch_rect(&v, 0);
        assert_eq!((s.right, s.top + 7.0), (917.0, 194.0));
        assert_eq!(menu_hit(&v, 700.0, 194.0), Hit::MenuToggle(0));
        let d = dialog(&v);
        assert_eq!((d.left, d.top), (80.0, 70.0));
        assert_eq!(about_hit(&v, 10.0, 10.0), Hit::AboutClose);
        assert_eq!(about_hit(&v, d.right - 17.0, d.top + 17.0), Hit::AboutClose);
    }
}
