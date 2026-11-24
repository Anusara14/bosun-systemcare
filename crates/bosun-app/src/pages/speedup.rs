//! Speed Up page, measured from the reference recording (`.reanimate/speedup`,
//! window-relative DIPs at 960 × 660):
//!
//! | element        | geometry                                                  |
//! |----------------|-----------------------------------------------------------|
//! | banner icon    | 49 px orange ring centred (197.5, 82.5)                   |
//! | banner text    | x 240; 20 px title cap centre 71.5; 15 px line at 96       |
//! | cards          | 2 × 3, 375 × 159, 11 px gaps, from (172, 131)             |
//! | card icon      | 43 px box at +28, +21                                     |
//! | card text      | x +95: 16 px title cap centre +27; 13 px lines +52.5, +69.5 |
//! | card button    | 133 × 30 pill, 16 px from the right, 15 px from the bottom |
//! | TuneUp toggles | 32 × 14, right edge 18 px in, rows +103.5, +125.5         |
//!
//! Cards whose engine isn't built yet say so when clicked instead of faking it.

use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::window::{Hit, View};

const CARD_X: f32 = 172.0;
const CARD_Y: f32 = 131.0;
const CARD_W: f32 = 375.0;
const CARD_H: f32 = 159.0;
const GAP: f32 = 11.0;
const BTN_W: f32 = 133.0;
const BTN_H: f32 = 30.0;

mod sp {
    pub const CARD: u32 = 0x1C1C1C;
    pub const TITLE: u32 = 0xF2F2F2;
    pub const TEXT: u32 = 0xB8B8B8;
    pub const ICON: u32 = 0xD4D4D4;
    pub const ORANGE: u32 = 0xF5A56A;
    pub const BLUE: u32 = 0x61ADF8;
    pub const LINE: u32 = 0x2C2C2C;
}

/// What the page shows; gathered when the page opens.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct SpeedInfo {
    /// Startup entries we'd turn off.
    pub optimizable: usize,
    /// tray.ini: Auto RAM Clean, and the floating widget (Performance Monitor).
    pub auto_ram: bool,
    pub monitor: bool,
    /// Turbo Boost is on, and its mode.
    pub turbo_on: bool,
    pub turbo_mode: bosun_core::turbo::Mode,
}

/// Card `i` (0..6, row-major) scaled to the window: columns share the width.
fn card(v: &View, i: usize) -> D2D_RECT_F {
    let w = (v.w - CARD_X - 27.0 - GAP) / 2.0;
    let (c, r) = (i % 2, i / 2);
    let w = w.max(CARD_W);
    rect(
        CARD_X + c as f32 * (w + GAP),
        CARD_Y + r as f32 * (CARD_H + GAP),
        w,
        CARD_H,
    )
}

fn button(c: &D2D_RECT_F) -> D2D_RECT_F {
    rect(
        c.right - 16.0 - BTN_W,
        c.bottom - 15.0 - BTN_H,
        BTN_W,
        BTN_H,
    )
}

fn toggle(c: &D2D_RECT_F, row: usize) -> D2D_RECT_F {
    rect(
        c.right - 18.0 - 32.0,
        c.top + 103.5 + 22.0 * row as f32 - 7.0,
        32.0,
        14.0,
    )
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    let b0 = button(&card(v, 0));
    if contains(&rect(b0.right - 32.0, b0.top, 32.0, BTN_H), x, y) {
        return Hit::TurboModeCard;
    }
    if contains(&rect(b0.left - 80.0, b0.top, 72.0, BTN_H), x, y) {
        return Hit::TurboConfigure;
    }
    for i in [0, 1, 2, 3] {
        if contains(&button(&card(v, i)), x, y) {
            return Hit::Card(i as u8);
        }
    }
    let rt = card(v, 4);
    for row in 0..2 {
        let t = toggle(&rt, row);
        if contains(&rect(t.left - 4.0, t.top - 4.0, 40.0, 22.0), x, y) {
            return Hit::Toggle(row as u8);
        }
    }
    Hit::None
}

#[allow(clippy::too_many_arguments)]
fn text_at(
    p: &Painter,
    s: &str,
    f: &IDWriteTextFormat,
    size: f32,
    x: f32,
    w: f32,
    cap_y: f32,
    c: u32,
) {
    let cy = cap_y - 0.064 * size;
    p.text(s, f, rect(x, cy - size, w, 2.0 * size), col(c), Align::Left);
}

pub fn draw(p: &Painter, v: &View) {
    let f = &p.g.f;
    let s = v.speed;
    // Banner
    let (bx, by) = (197.5, 82.5);
    if s.optimizable > 0 {
        p.ring(bx, by, 23.0, 2.4, col(sp::ORANGE));
        p.circle(bx, by - 9.0, 2.0, col(sp::ORANGE));
        p.line(bx, by - 3.0, bx, by + 10.0, 3.0, col(sp::ORANGE));
        text_at(
            p,
            "Your PC is not at its top performance.",
            &f.h1,
            20.0,
            240.0,
            640.0,
            71.5,
            sp::TITLE,
        );
        let n = s.optimizable.to_string();
        let wn = p.g.measure(&n, &f.pop_label);
        text_at(p, &n, &f.pop_label, 15.0, 240.0, wn + 4.0, 96.0, sp::ORANGE);
        let rest = " startup items need to be disabled or delayed to start your PC faster.";
        text_at(
            p,
            rest,
            &f.pop_label,
            15.0,
            240.0 + wn,
            620.0,
            96.0,
            sp::TEXT,
        );
    } else {
        p.circle(bx, by, 24.0, col(sp::BLUE));
        let c = col(0x0A0A0A);
        p.line(bx - 10.0, by + 1.0, bx - 3.0, by + 8.0, 3.0, c);
        p.line(bx - 3.0, by + 8.0, bx + 11.0, by - 7.0, 3.0, c);
        text_at(
            p,
            "Your PC is running at its best.",
            &f.h1,
            20.0,
            240.0,
            640.0,
            71.5,
            sp::TITLE,
        );
        text_at(
            p,
            "No startup items need your attention.",
            &f.pop_label,
            15.0,
            240.0,
            620.0,
            96.0,
            sp::TEXT,
        );
    }
    let opt = s.optimizable;
    let cards: [(&str, Vec<String>, Option<&str>); 6] = [
        (
            if s.turbo_on {
                "Turbo Boost: ON"
            } else {
                "Turbo Boost: OFF"
            },
            vec![
                "Stop unnecessary programs to release".into(),
                "RAM and boost your PC.".into(),
            ],
            Some(if s.turbo_on { "Turn Off" } else { "Turn On" }),
        ),
        (
            "Startup Optimizer",
            vec![
                format!("{opt} startup items can be optimized to boot"),
                "your PC faster.".into(),
            ],
            Some("Optimize"),
        ),
        (
            "Driver Updates",
            vec![
                "Check Windows Update for newer drivers".into(),
                "for your hardware.".into(),
            ],
            Some("Check"),
        ),
        (
            "App/Extension Cleaner",
            vec![
                "Uninstall unnecessary apps and browser".into(),
                "extensions for a cleaner and smoother PC.".into(),
            ],
            Some("Manage"),
        ),
        (
            "Real-Time TuneUp",
            vec![
                "Automatically clean and optimize your PC".into(),
                "for top PC performance.".into(),
            ],
            None,
        ),
        (
            "More Optimization Tools",
            vec![
                "Further improve your PC's performance with".into(),
                "more tools.".into(),
            ],
            None,
        ),
    ];
    for (i, (title, lines, btn)) in cards.iter().enumerate() {
        let c = card(v, i);
        p.fill_rr(c, 6.0, col(sp::CARD));
        icon(
            p,
            i,
            c.left + 49.5,
            c.top + 42.5,
            i <= 1 && (i == 0 || opt > 0),
        );
        let tx = c.left + 95.0;
        draw_title(p, title, tx, c.top + 27.0);
        for (k, line) in lines.iter().enumerate() {
            let y = c.top + 48.5 + 17.0 * k as f32;
            if i == 1 && k == 0 {
                // the count in orange, as in the reference
                let n = opt.to_string();
                let wn = p.g.measure(&n, &f.m13);
                text_at(p, &n, &f.m13, 13.0, tx, wn + 2.0, y, sp::ORANGE);
                let rest = &line[n.len()..];
                text_at(
                    p,
                    rest,
                    &f.m13,
                    13.0,
                    tx + wn,
                    c.right - tx - 16.0,
                    y,
                    sp::TEXT,
                );
            } else {
                text_at(p, line, &f.m13, 13.0, tx, c.right - tx - 16.0, y, sp::TEXT);
            }
        }
        if let Some(label) = btn {
            let b = button(&c);
            pill(p, b, label, v.hover == Hit::Card(i as u8));
            if i == 0 {
                // "Configure" link left of the button, as in the reference
                let f = &p.g.f.m13;
                let w = p.g.measure("Configure", f);
                let lx = b.left - 10.0 - w;
                let c = if v.hover == Hit::TurboConfigure {
                    0xFFFFFF
                } else {
                    sp::TEXT
                };
                text_at(p, "Configure", f, 13.0, lx, w + 4.0, b.top + 15.0, c);
                p.line(lx, b.top + 22.0, lx + w, b.top + 22.0, 1.0, col(c));
                // split-button caret, as in the reference
                p.line(
                    b.right - 32.0,
                    b.top + 8.0,
                    b.right - 32.0,
                    b.bottom - 8.0,
                    1.0,
                    cola(0x0A0A0A, 0.35),
                );
                p.caret_down(b.right - 16.0, b.top + 15.5, 8.0, col(0x141414));
            }
        }
    }
    draw_tuneup(p, v, &card(v, 4));
    draw_tools(p, &card(v, 5));
}

/// "Turbo Boost: OFF" draws "OFF" in orange.
fn draw_title(p: &Painter, title: &str, x: f32, cap_y: f32) {
    let f = &p.g.f.m14sb;
    match title.split_once(": ") {
        Some((a, b)) => {
            let head = format!("{a}: ");
            let w = p.g.measure(&head, f);
            text_at(p, &head, f, 16.0, x, w + 2.0, cap_y, sp::TITLE);
            text_at(p, b, f, 16.0, x + w, 120.0, cap_y, sp::ORANGE);
        }
        None => text_at(p, title, f, 16.0, x, 300.0, cap_y, sp::TITLE),
    }
}

fn pill(p: &Painter, r: D2D_RECT_F, label: &str, hover: bool) {
    let x = (r.left + r.right) / 2.0;
    let rad = BTN_H / 2.0;
    let stops = [(0.0, col(0x8DC3F8)), (1.0, col(0x6DB4F7))];
    p.fill_rr_linear(r, rad, (x, r.top), (x, r.bottom), &stops);
    if hover {
        p.fill_rr(r, rad, cola(0xFFFFFF, 0.14));
    }
    let r = if label == "Turn On" {
        rect(r.left, r.top, r.right - r.left - 32.0, BTN_H)
    } else {
        r
    };
    p.text(label, &p.g.f.m14sb, r, col(0x141414), Align::Center);
}

fn draw_tuneup(p: &Painter, v: &View, c: &D2D_RECT_F) {
    let tx = c.left + 95.0;
    p.line(
        tx,
        c.top + 86.0,
        c.right - 18.0,
        c.top + 86.0,
        1.0,
        col(sp::LINE),
    );
    let rows = [
        ("Auto RAM Clean", v.speed.auto_ram),
        ("Performance Monitor", v.speed.monitor),
    ];
    for (i, (label, on)) in rows.iter().enumerate() {
        let y = c.top + 103.5 + 22.0 * i as f32;
        text_at(p, label, &p.g.f.m13, 13.0, tx, 200.0, y, sp::TEXT);
        let t = toggle(c, i);
        let r = 7.0;
        if *on {
            p.fill_rr(t, r, col(sp::BLUE));
            p.circle(t.right - 7.0, t.top + 7.0, 4.5, col(0xFFFFFF));
        } else {
            p.fill_rr(t, r, col(0x3A3A3A));
            p.circle(t.left + 7.0, t.top + 7.0, 4.5, col(0xE6E6E6));
        }
        if v.hover == Hit::Toggle(i as u8) {
            p.stroke_rr(t, r, 1.0, cola(0xFFFFFF, 0.35));
        }
    }
}

/// Round tool buttons along the card's bottom (inactive until their tools land).
fn draw_tools(p: &Painter, c: &D2D_RECT_F) {
    let tx = c.left + 95.0;
    p.line(
        tx,
        c.top + 86.0,
        c.right - 18.0,
        c.top + 86.0,
        1.0,
        col(sp::LINE),
    );
    for k in 0..5 {
        let cx = tx + 6.0 + 16.5 + 43.0 * k as f32;
        let cy = c.top + 117.0;
        p.ring(cx, cy, 16.0, 1.6, col(0x3E3E3E));
        if k == 4 {
            for d in [-6.0, 0.0, 6.0] {
                p.circle(cx + d, cy, 1.6, col(sp::TEXT));
            }
        } else {
            p.ring(cx, cy, 7.0, 1.6, col(0x5A5A5A));
        }
    }
}

/// 43 px line icons, centred on (cx, cy); `badge` adds the orange "i".
fn icon(p: &Painter, i: usize, cx: f32, cy: f32, badge: bool) {
    let c = col(sp::ICON);
    let w = 2.4;
    match i {
        // Turbo: gauge arc with a needle
        0 => {
            p.arc_at(cx, cy + 2.0, 17.0, w, 0.625, 0.75, c);
            p.line(cx, cy + 2.0, cx + 9.0, cy - 6.0, w, c);
            p.circle(cx, cy + 2.0, 3.0, c);
            p.line(cx - 7.0, cy + 15.0, cx + 7.0, cy + 15.0, w, c);
        }
        // Startup: rocket
        1 => {
            p.curve(
                [
                    (cx - 10.0, cy + 10.0),
                    (cx - 6.0, cy - 6.0),
                    (cx + 6.0, cy - 14.0),
                    (cx + 16.0, cy - 16.0),
                ],
                w,
                c,
            );
            p.curve(
                [
                    (cx - 10.0, cy + 10.0),
                    (cx + 6.0, cy + 6.0),
                    (cx + 14.0, cy - 6.0),
                    (cx + 16.0, cy - 16.0),
                ],
                w,
                c,
            );
            p.ring(cx + 6.0, cy - 6.0, 3.2, 2.0, c);
            p.line(cx - 12.0, cy + 6.0, cx - 18.0, cy + 12.0, w, c);
            p.line(cx - 6.0, cy + 12.0, cx - 12.0, cy + 18.0, w, c);
        }
        // Drivers: chip with pins
        2 => {
            p.stroke_rr(rect(cx - 11.0, cy - 15.0, 22.0, 30.0), 3.0, w, c);
            p.ring(cx, cy, 5.5, w, c);
            for d in [-8.0, 0.0, 8.0] {
                p.line(cx - 15.0, cy + d, cx - 11.0, cy + d, w, c);
                p.line(cx + 11.0, cy + d, cx + 15.0, cy + d, w, c);
            }
        }
        // App cleaner: brush
        3 => {
            p.line(cx, cy - 17.0, cx, cy - 4.0, w, c);
            p.stroke_rr(rect(cx - 8.0, cy - 5.0, 16.0, 6.0), 2.0, w, c);
            p.curve(
                [
                    (cx - 9.0, cy + 1.0),
                    (cx - 13.0, cy + 8.0),
                    (cx - 14.0, cy + 13.0),
                    (cx - 15.0, cy + 15.0),
                ],
                w,
                c,
            );
            p.curve(
                [
                    (cx + 9.0, cy + 1.0),
                    (cx + 13.0, cy + 8.0),
                    (cx + 14.0, cy + 13.0),
                    (cx + 15.0, cy + 15.0),
                ],
                w,
                c,
            );
            p.line(cx - 15.0, cy + 15.0, cx + 15.0, cy + 15.0, w, c);
        }
        // Real-time: clock with a blue tick badge
        4 => {
            p.ring(cx, cy, 16.0, w, c);
            p.line(cx, cy, cx, cy - 9.0, w, c);
            p.line(cx, cy, cx + 6.0, cy + 4.0, w, c);
            p.circle(cx + 13.0, cy + 13.0, 6.5, col(sp::BLUE));
            let k = col(0x0A0A0A);
            p.line(cx + 10.0, cy + 13.2, cx + 12.3, cy + 15.5, 1.6, k);
            p.line(cx + 12.3, cy + 15.5, cx + 16.2, cy + 10.8, 1.6, k);
        }
        // More tools: toolbox
        _ => {
            p.stroke_rr(rect(cx - 17.0, cy - 8.0, 34.0, 24.0), 3.0, w, c);
            p.stroke_rr(rect(cx - 7.0, cy - 15.0, 14.0, 7.0), 2.0, w, c);
            p.line(cx - 17.0, cy + 2.0, cx + 17.0, cy + 2.0, w, c);
            p.stroke_rr(rect(cx - 3.5, cy - 1.0, 7.0, 6.0), 1.5, 1.8, c);
        }
    }
    if badge {
        let (bx, by) = (cx + 13.0, cy + 13.0);
        p.circle(bx, by, 6.5, col(sp::ORANGE));
        let k = col(0x141414);
        p.circle(bx, by - 2.8, 1.0, k);
        p.line(bx, by - 0.8, bx, by + 3.2, 1.6, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::test_view;

    #[test]
    fn cards_and_buttons_sit_where_the_reference_has_them() {
        let v = test_view();
        let c = card(&v, 0);
        assert_eq!(
            (c.left, c.top, c.right, c.bottom),
            (172.0, 131.0, 547.0, 290.0)
        );
        let c = card(&v, 5);
        assert_eq!(
            (c.left, c.top, c.right, c.bottom),
            (558.0, 471.0, 933.0, 630.0)
        );
        let b = button(&card(&v, 1));
        assert_eq!(
            (b.left, b.top, b.right, b.bottom),
            (784.0, 245.0, 917.0, 275.0)
        );
        assert_eq!(hit(&v, 850.0, 260.0), Hit::Card(1));
        let t = toggle(&card(&v, 4), 1);
        assert_eq!(hit(&v, t.left + 10.0, t.top + 7.0), Hit::Toggle(1));
    }
}
