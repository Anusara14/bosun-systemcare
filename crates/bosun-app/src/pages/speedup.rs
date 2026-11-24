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
    /// tray.ini: Auto Clean (junk files once a day).
    pub auto_clean: bool,
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
    let tc = card(v, 5);
    for k in 0..5 {
        let cx = tc.left + 95.0 + 22.5 + 43.0 * k as f32;
        if (x - cx).hypot(y - (tc.top + 117.0)) <= 17.0 {
            // Disk Cleaner, Large File Finder, Empty Folder Finder, Shortcut Fixer, more
            return [
                Hit::Tool(12),
                Hit::Tool(15),
                Hit::Tool(13),
                Hit::Tool(14),
                Hit::ToolsOpen,
            ][k];
        }
    }
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
    for row in 0..3 {
        let t = toggle(&rt, row);
        if contains(&rect(t.left - 4.0, t.top - 4.0, 40.0, 22.0), x, y) {
            return Hit::Toggle(row as u8);
        }
    }
    Hit::None
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn text_at(
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
            (i == 0 && !s.turbo_on) || (i == 1 && opt > 0),
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
    // hover ring on the tools row
    let tc = card(v, 5);
    for (k, h) in [
        Hit::Tool(12),
        Hit::Tool(15),
        Hit::Tool(13),
        Hit::Tool(14),
        Hit::ToolsOpen,
    ]
    .into_iter()
    .enumerate()
    {
        if v.hover == h {
            let cx = tc.left + 95.0 + 22.5 + 43.0 * k as f32;
            p.circle(cx, tc.top + 117.0, 16.0, cola(0xFFFFFF, 0.08));
        }
    }
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

pub(crate) fn pill(p: &Painter, r: D2D_RECT_F, label: &str, hover: bool) {
    let x = (r.left + r.right) / 2.0;
    // half the button's own height: a true pill at any size, never an oval
    let rad = (r.bottom - r.top) / 2.0;
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
        ("Auto Clean (daily)", v.speed.auto_clean),
    ];
    for (i, (label, on)) in rows.iter().enumerate() {
        let y = c.top + 103.5 + 22.0 * i as f32;
        text_at(p, label, &p.g.f.m13, 13.0, tx, 200.0, y, sp::TEXT);
        // the shared switch: white knob in both skins
        crate::pages::protect::toggle(p, toggle(c, i), *on, v.hover == Hit::Toggle(i as u8));
    }
}

/// The More Optimization Tools row: blue rings with blue glyphs, as in the
/// reference (its PRO tags are dropped). Ours: Disk Cleaner, Large File
/// Finder, Empty Folder Finder, Shortcut Fixer, then "more".
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
    let blue = col(sp::BLUE);
    let w = 1.8;
    for k in 0..5 {
        let cx = tx + 22.5 + 43.0 * k as f32;
        let cy = c.top + 117.0;
        if k == 4 {
            p.ring(cx, cy, 16.0, 1.8, col(0x6E6E6E));
            for d in [-6.0, 0.0, 6.0] {
                p.circle(cx + d, cy, 1.9, col(0xC8C8C8));
            }
            continue;
        }
        p.ring(cx, cy, 16.0, 1.8, blue);
        match k {
            // Disk Cleaner: a drive with a light
            0 => {
                p.stroke_rr(rect(cx - 9.0, cy - 6.0, 18.0, 12.0), 2.0, w, blue);
                p.line(cx - 9.0, cy + 1.5, cx + 9.0, cy + 1.5, w, blue);
                p.circle(cx + 5.0, cy + 3.8, 1.2, blue);
            }
            // Large File Finder: a page and a magnifier
            1 => {
                p.stroke_rr(rect(cx - 8.0, cy - 9.0, 11.0, 15.0), 1.5, w, blue);
                p.ring(cx + 3.5, cy + 2.5, 4.0, w, blue);
                p.line(cx + 6.4, cy + 5.4, cx + 9.0, cy + 8.0, 2.0, blue);
            }
            // Empty Folder Finder: a folder
            2 => {
                p.fill_poly(
                    &[
                        (cx - 9.0, cy - 6.0),
                        (cx - 3.0, cy - 6.0),
                        (cx - 1.0, cy - 4.0),
                        (cx - 9.0, cy - 4.0),
                    ],
                    blue,
                );
                p.stroke_rr(rect(cx - 9.0, cy - 4.0, 18.0, 11.0), 1.5, w, blue);
            }
            // Shortcut Fixer: a window with a tick (reference glyph)
            _ => {
                p.stroke_rr(rect(cx - 9.0, cy - 7.0, 16.0, 13.0), 1.5, w, blue);
                p.line(cx - 9.0, cy - 3.5, cx + 7.0, cy - 3.5, w, blue);
                p.line(cx + 1.0, cy + 4.0, cx + 4.0, cy + 7.0, 2.0, blue);
                p.line(cx + 4.0, cy + 7.0, cx + 10.0, cy + 0.5, 2.0, blue);
            }
        }
    }
}

/// Orange "i" badge (attention) or blue tick badge (all good), as in the
/// reference: 7 px radius, bottom-right of the icon.
pub(crate) fn badge(p: &Painter, x: f32, y: f32, ok: bool) {
    let k = col(0x141414);
    if ok {
        p.circle(x, y, 7.0, col(sp::BLUE));
        p.line(x - 3.2, y + 0.2, x - 1.0, y + 2.6, 1.8, k);
        p.line(x - 1.0, y + 2.6, x + 3.4, y - 2.4, 1.8, k);
    } else {
        p.circle(x, y, 7.0, col(sp::ORANGE));
        p.circle(x, y - 3.0, 1.1, k);
        p.line(x, y - 0.6, x, y + 3.6, 1.9, k);
    }
}

/// Card icons, drawn in the reference's 60 × 60 box (origin card + (20, 12)),
/// measured at 5× from `.reanimate/speedup/ref-icons.png`: a light main
/// stroke with darker accents. `cx, cy` is the box centre.
fn icon(p: &Painter, i: usize, cx: f32, cy: f32, attention: bool) {
    let (ox, oy) = (cx - 29.5, cy - 30.5);
    let at = |x: f32, y: f32| (ox + x, oy + y);
    let c = col(0xD8D8DC);
    let dim = col(0x787880);
    let w = 1.9;
    let line = |a: (f32, f32), b: (f32, f32), c, w| p.line(a.0, a.1, b.0, b.1, w, c);
    let poly = |pts: &[(f32, f32)], c| {
        for seg in pts.windows(2) {
            line(at(seg[0].0, seg[0].1), at(seg[1].0, seg[1].1), c, w);
        }
    };
    match i {
        // Turbo: gauge open at the bottom, dim inner arc, needle into a hub, base
        0 => {
            let (gx, gy) = at(28.0, 29.0);
            p.arc_at(gx, gy, 20.0, w, 0.67, 0.60, c);
            p.arc_at(gx, gy, 13.0, 1.8, 0.75, 0.25, dim);
            line(at(13.0, 29.0), at(24.5, 29.0), c, 3.6);
            p.ring(gx, gy, 3.3, 1.8, c);
            let (bx, by) = at(21.0, 41.0);
            p.fill_rr(rect(bx, by, 13.0, 3.2), 1.6, dim);
            if attention {
                let (x, y) = at(45.0, 43.5);
                badge(p, x, y, false);
            }
        }
        // Startup: rocket pointing up-right, grey porthole, fins, motion dashes
        1 => {
            let (rx, ry) = at(31.0, 29.0);
            let k = std::f32::consts::FRAC_1_SQRT_2;
            // (u along the nose, v across) → screen
            // 1.22× the first cut, to the reference's span (nose (52, 12) to fins)
            let r = |u: f32, v: f32| (rx + 1.22 * (u + v) * k, ry + 1.22 * (-u + v) * k);
            let body = [
                r(21.0, 0.0),
                r(13.0, 5.5),
                r(-8.0, 6.0),
                r(-11.0, 0.0),
                r(-8.0, -6.0),
                r(13.0, -5.5),
                r(21.0, 0.0),
            ];
            for seg in body.windows(2) {
                p.line(seg[0].0, seg[0].1, seg[1].0, seg[1].1, w, c);
            }
            for side in [1.0, -1.0] {
                let f = [
                    r(-2.0, 6.0 * side),
                    r(-11.0, 13.0 * side),
                    r(-9.0, 6.0 * side),
                ];
                for seg in f.windows(2) {
                    p.line(seg[0].0, seg[0].1, seg[1].0, seg[1].1, w, c);
                }
            }
            let (wx, wy) = r(7.0, 0.0);
            p.ring(wx, wy, 3.4, 1.8, dim);
            for (v, len) in [(-4.0, 3.5), (0.0, 5.0), (4.0, 3.5)] {
                let a = r(-15.0, v);
                let b = r(-15.0 - len, v);
                p.line(a.0, a.1, b.0, b.1, 1.8, dim);
            }
            if attention {
                let (x, y) = at(45.0, 43.5);
                badge(p, x, y, false);
            }
        }
        // Drivers / hardware: rounded box, four screws, double ring
        2 => {
            let (x0, y0) = at(11.5, 9.5);
            p.stroke_rr(rect(x0, y0, 34.0, 38.0), 3.0, w, c);
            for (sx, sy) in [(16.5, 14.5), (40.5, 14.5), (16.5, 42.5), (40.5, 42.5)] {
                let (x, y) = at(sx, sy);
                p.circle(x, y, 1.2, c);
            }
            let (rx, ry) = at(28.5, 28.5);
            p.ring(rx, ry, 10.0, w, c);
            p.ring(rx, ry, 4.0, 1.8, dim);
        }
        // App cleaner: brush with stem, ferrule and a flared, notched head
        3 => {
            let (sx, sy) = at(23.5, 10.0);
            p.stroke_rr(rect(sx, sy, 9.0, 14.0), 4.0, w, c);
            let (fx, fy) = at(15.0, 23.5);
            p.stroke_rr(rect(fx, fy, 26.0, 5.0), 2.5, w, c);
            poly(
                &[
                    (18.5, 28.5),
                    (11.5, 42.5),
                    (11.5, 47.0),
                    (23.5, 47.0),
                    (26.0, 42.0),
                    (30.0, 42.0),
                    (32.5, 47.0),
                    (44.5, 47.0),
                    (44.5, 42.5),
                    (37.5, 28.5),
                ],
                c,
            );
            line(at(17.5, 36.0), at(16.5, 40.0), dim, 1.8);
        }
        // Real-time: clock with a gap for the blue tick badge
        4 => {
            let (gx, gy) = at(28.0, 29.0);
            p.arc_at(gx, gy, 18.5, w, 0.47, 0.85, c);
            line(at(28.0, 29.0), at(28.0, 18.0), c, w);
            line(at(28.0, 29.0), at(22.0, 33.0), c, w);
            let (x, y) = at(44.0, 44.0);
            badge(p, x, y, true);
        }
        // More tools: toolbox with handle, divider and latch
        _ => {
            let (bx, by) = at(10.0, 20.0);
            p.stroke_rr(rect(bx, by, 37.0, 25.0), 3.0, w, c);
            poly(
                &[
                    (20.5, 20.0),
                    (20.5, 15.0),
                    (22.5, 12.5),
                    (34.5, 12.5),
                    (36.5, 15.0),
                    (36.5, 20.0),
                ],
                c,
            );
            line(at(10.0, 30.5), at(24.0, 30.5), dim, 1.8);
            line(at(33.0, 30.5), at(47.0, 30.5), dim, 1.8);
            let (lx, ly) = at(24.0, 26.0);
            p.stroke_rr(rect(lx, ly, 9.0, 9.0), 2.0, w, c);
        }
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
