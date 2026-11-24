//! Care page: the SCAN dial and the Recommended / Manual switch, laid out from
//! the reference (window-relative DIPs, 960 × 660 window):
//!
//! | radius  | dial layer                                         |
//! |---------|----------------------------------------------------|
//! | 127     | hairline light rim, soft glow outside it           |
//! | 116–125 | dark band                                          |
//! | 96–115  | six bright aperture blades with dark curved seams  |
//! | 88–95   | dark ring                                          |
//! | ≤ 88    | button with a 5 px bevel, "SCAN" bold 36 px        |
//!
//! Dial centre: 3 px left of the content centre, y 252. Switch: 265 × 45,
//! top 200 px below the dial centre; pill 121 × 38; "?" badge at its corner.

use std::f32::consts::TAU;

use bosun_ui::gfx::{Align, Painter, col, cola, contains, mix, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::chrome::SIDEBAR_W;
use crate::pages::scan::checkbox;
use crate::scan::{ALL, MODULES};
use crate::window::{DEFAULT_H, Hit, Mode, View};

const R_RIM: f32 = 127.0;
const R_BAND: f32 = 125.0;
const R_BLADES: f32 = 115.0;
const R_BLADES_IN: f32 = 96.0;
const R_BTN: f32 = 88.0;
const BEVEL: f32 = 5.0;
const R_GLOW: f32 = 150.0;
const BLADES: usize = 6;
const SWITCH_W: f32 = 265.0;
const SWITCH_H: f32 = 45.0;
const SWITCH_DY: f32 = 200.0;
const PILL_W: f32 = 121.0;
const PILL_H: f32 = 38.0;
/// Manual mode, measured from the reference recording (`.reanimate/manual`):
/// the dial rises 45 px and the switch closes up 40.5 px more; below it a
/// "Select All" row and a 4-column grid of 16 px tick boxes (columns 182 px
/// apart from 363.5 px left of the dial, rows 35 px apart), labels 26 px
/// right of the box, 13 px.
const MANUAL_RISE: f32 = 45.0;
const SWITCH_CLOSE_UP: f32 = 40.5;
const GRID_X: f32 = -363.5;
const GRID_COL: f32 = 182.0;
const GRID_ROW: f32 = 35.0;
const SELECT_ALL_DY: f32 = 217.5;
const BOX: f32 = 16.0;

/// Dial centre: 3 px left of the content centre (as measured); it drifts down
/// proportionally when the window is taller than the reference.
/// In Manual mode the dial rises 45 px (measured) to make room for the grid.
pub fn center(v: &View) -> (f32, f32) {
    let (cx, cy) = center_for(v.w, v.h);
    (cx, cy - MANUAL_RISE * v.manual_t)
}

/// Dial centre for a window of `w` × `h` DIPs.
pub fn center_for(w: f32, h: f32) -> (f32, f32) {
    let cx = SIDEBAR_W + (w - SIDEBAR_W) / 2.0 - 3.0;
    let cy = 252.0 + (h - DEFAULT_H) * 0.4;
    (cx, cy)
}

fn switch_rect(v: &View) -> D2D_RECT_F {
    let (cx, cy) = center(v);
    let dy = SWITCH_DY - SWITCH_CLOSE_UP * v.manual_t;
    rect(cx - SWITCH_W / 2.0, cy + dy, SWITCH_W, SWITCH_H)
}

fn halves(v: &View) -> [(Mode, D2D_RECT_F); 2] {
    let s = switch_rect(v);
    let top = s.top + (SWITCH_H - PILL_H) / 2.0;
    let left = rect(s.left + 4.0, top, PILL_W, PILL_H);
    let right = rect(
        left.right + 4.0,
        top,
        s.right - 4.0 - (left.right + 4.0),
        PILL_H,
    );
    [(Mode::Recommended, left), (Mode::Manual, right)]
}

/// The "?" badge overlapping the switch's top-left corner.
fn help_badge(v: &View) -> (f32, f32, f32) {
    let s = switch_rect(v);
    (s.left + 8.0, s.top + 6.0, 7.5)
}

/// Top-left of tick box `i` (`None` = Select All), relative to the dial.
fn pick_box(v: &View, i: Option<usize>) -> (f32, f32) {
    let (cx, cy) = center(v);
    let (col, row) = match i {
        None => (0, 0),
        Some(i) => (i % 4, i / 4 + 1),
    };
    (
        cx + GRID_X + GRID_COL * col as f32,
        cy + SELECT_ALL_DY + GRID_ROW * row as f32,
    )
}

fn pick_hit(v: &View, x: f32, y: f32) -> Option<Hit> {
    if v.manual_t < 0.99 {
        return None;
    }
    let over = |i| {
        let (bx, by) = pick_box(v, i);
        contains(&rect(bx - 2.0, by - 4.0, 160.0, BOX + 8.0), x, y)
    };
    if over(None) {
        return Some(Hit::PickAll);
    }
    (0..MODULES.len()).find(|&i| over(Some(i))).map(Hit::Pick)
}

fn draw_picks(p: &Painter, v: &View) {
    let a = v.manual_t;
    if a <= 0.0 {
        return;
    }
    let row = |label: &str, i: Option<usize>, on: bool, hot: bool| {
        let (bx, by) = pick_box(v, i);
        let (cx, cy) = (bx + BOX / 2.0, by + BOX / 2.0);
        if on {
            checkbox(p, cx, cy, true, a);
        } else {
            p.stroke_rr(
                rect(bx + 0.5, by + 0.5, BOX - 1.0, BOX - 1.0),
                2.5,
                1.0,
                cola(0x4C4C4C, a),
            );
        }
        let c = match (on, hot) {
            (_, true) => 0xFFFFFF,
            (true, false) => 0xD6D6D6,
            (false, false) => 0x8C8C8C,
        };
        p.text(
            label,
            &p.g.f.m13,
            rect(bx + 26.0, cy - 10.0, 150.0, 20.0),
            cola(c, a),
            Align::Left,
        );
    };
    row("Select All", None, v.picks == ALL, v.hover == Hit::PickAll);
    for (i, m) in MODULES.iter().enumerate() {
        row(
            m.pick,
            Some(i),
            v.picks & (1 << i) != 0,
            v.hover == Hit::Pick(i),
        );
    }
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    if let Some(h) = pick_hit(v, x, y) {
        return h;
    }
    let (cx, cy) = center(v);
    if (x - cx).hypot(y - cy) <= R_RIM {
        return Hit::Scan;
    }
    let (bx, by, br) = help_badge(v);
    if (x - bx).hypot(y - by) <= br + 1.0 {
        return Hit::ModeHelp;
    }
    halves(v)
        .into_iter()
        .find(|(_, r)| contains(r, x, y))
        .map_or(Hit::None, |(m, _)| Hit::Mode(m))
}

/// Six aperture blades: a bright band cut by curved seams that sweep inward.
/// `h` is the hover transition (0 rest → 1 hovered): the blades brighten.
fn draw_blades(p: &Painter, cx: f32, cy: f32, h: f32) {
    let inner = mix(0x4A9FF5, 0x5DB0FF, h);
    let outer = mix(0x86C4FF, 0x9DD2FF, h);
    let k = R_BLADES_IN / R_BLADES;
    p.radial(cx, cy, R_BLADES, &[(k, inner), (1.0, outer)]);
    let at = |r: f32, t: f32| (cx + r * t.sin(), cy - r * t.cos());
    for i in 0..BLADES {
        // The aperture turns one blade (60°) clockwise as the hover fades in.
        let t = (h + i as f32) / BLADES as f32 * TAU;
        // One clean, evenly wide gap per blade, slanting counter-clockwise as it
        // goes inward; same colour as the dark band so its ends blend in.
        let seam = [
            at(R_BLADES, t),
            at(R_BLADES - 6.0, t - 0.08),
            at(R_BLADES_IN + 6.0, t - 0.20),
            at(R_BLADES_IN, t - 0.28),
        ];
        p.curve(seam, 5.5, col(0x13213A));
    }
}

/// Under-dial glow strength for hover progress `h`: it vanishes in the first
/// quarter of the hover-in and only returns in the last quarter of hover-out.
pub fn under_glow(h: f32) -> f32 {
    let x = (h / 0.25).clamp(0.0, 1.0);
    1.0 - x * x * (3.0 - 2.0 * x)
}

fn smooth(edge0: f32, edge1: f32, h: f32) -> f32 {
    let x = ((h - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// (SCAN opacity, magnifier opacity) at linear progress `u` (0 rest, 1 hovered).
/// Not a mirror, as measured: entering, the word dims at once and is gone by
/// ~200 ms while the magnifier lands at ~130-200 ms; leaving, the magnifier
/// holds ~100 ms and the word returns at ~130-230 ms.
pub fn label_fades(u: f32, entering: bool) -> (f32, f32) {
    if entering {
        // a quick 40 % dip in the first frame, then the rest from 20 % to 60 %
        let dip = 0.4 * (u / 0.11).clamp(0.0, 1.0);
        (
            1.0 - dip - 0.6 * smooth(0.20, 0.60, u),
            smooth(0.33, 0.62, u),
        )
    } else {
        (1.0 - smooth(0.25, 0.55, u), smooth(0.45, 0.70, u))
    }
}

/// Magnifier: white ring with a handle at 45°, centred on (cx, cy).
fn magnifier(p: &Painter, cx: f32, cy: f32, a: f32) {
    let c = cola(0xFFFFFF, a);
    p.ring(cx - 4.0, cy - 4.0, 15.0, 4.2, c);
    p.line(cx + 7.5, cy + 7.5, cx + 18.0, cy + 18.0, 4.6, c);
}

fn draw_dial(p: &Painter, v: &View) {
    let (cx, cy) = center(v);
    let h = v.scan_hover;
    let pressed = v.hover == Hit::Scan && v.pressed == Hit::Scan;
    // Faint haze all the way round, a touch stronger below (as in the reference).
    let s = R_RIM / R_GLOW;
    let haze = [
        (s, cola(0x3F86D8, 0.16)),
        (s + 0.08, cola(0x3F86D8, 0.06)),
        (1.0, cola(0x3F86D8, 0.0)),
    ];
    p.radial(cx, cy, R_GLOW, &haze);
    // At rest, light pools under the dial: a glow centred a little below the
    // dial peeks out past the rim as a crescent, thick at the bottom and
    // thinning to nothing at the top. It fades away while hovered.
    let (dy, rg) = (7.0, R_RIM + 15.0);
    let a = 0.55 * under_glow(h);
    if a > 0.0 {
        let pool = [
            (0.80, cola(0x4A8FE0, a)),
            (0.90, cola(0x4A8FE0, a * 0.35)),
            (1.0, cola(0x4A8FE0, 0.0)),
        ];
        p.radial(cx, cy + dy, rg, &pool);
    }
    // Light-blue rim all the way round (brightest at the top), on a dark edge.
    p.circle(cx, cy, R_RIM + 1.0, col(0x0D1A30));
    let rim = [
        (0.0, col(0x5A8FCF)),
        (0.5, col(0x3F6FA5)),
        (1.0, col(0x34608F)),
    ];
    p.ring_linear(cx, cy, R_RIM - 1.0, 3.0, &rim);
    p.circle(cx, cy, R_BAND - 0.5, col(0x13213A));
    draw_blades(p, cx, cy, h);
    // dark ring between blades and button
    p.circle(cx, cy, R_BLADES_IN, col(0x0B111C));
    // button: bevel ring (light top → dark bottom), then the face
    let bevel = [
        (0.0, mix(0x6A6A6A, 0x7A7A7A, h)),
        (0.5, col(0x3A3A3A)),
        (1.0, col(0x202020)),
    ];
    p.ring_linear(cx, cy, R_BTN - BEVEL / 2.0, BEVEL, &bevel);
    let (top, bottom) = if pressed {
        (col(0x2C2C2C), col(0x181818))
    } else {
        (mix(0x3C3C3C, 0x545454, h), mix(0x1E1E1E, 0x303030, h))
    };
    let face = R_BTN - BEVEL;
    p.radial_at(
        (cx, cy, face),
        (cx, cy - face * 0.35, face * 1.35),
        &[(0.0, top), (1.0, bottom)],
    );
    // Label cross-fades into a magnifier on hover.
    let label = rect(cx - R_BTN, cy - 30.0, 2.0 * R_BTN, 60.0);
    let (text_a, icon_a) = v.scan_label;
    if text_a > 0.0 {
        p.text(
            "SCAN",
            &p.g.f.hero,
            label,
            cola(0xFFFFFF, text_a),
            Align::Center,
        );
    }
    if icon_a > 0.0 {
        magnifier(p, cx, cy, icon_a);
    }
}

fn draw_switch(p: &Painter, v: &View) {
    let s = switch_rect(v);
    p.fill_rr(s, SWITCH_H / 2.0, col(0x0E0E0E));
    p.stroke_rr(s, SWITCH_H / 2.0, 1.0, col(0x2A2A2A));
    for (mode, r) in halves(v) {
        let on = v.mode == mode;
        if on {
            let x = (r.left + r.right) / 2.0;
            let pill = [(0.0, col(0xA9DDFF)), (1.0, col(0x55B6FF))];
            p.fill_rr_linear(r, PILL_H / 2.0, (x, r.top), (x, r.bottom), &pill);
        } else if v.hover == Hit::Mode(mode) {
            p.fill_rr(r, PILL_H / 2.0, cola(0xFFFFFF, 0.06));
        }
        let label = match mode {
            Mode::Recommended => "Recommended",
            Mode::Manual => "Manual",
        };
        let c = col(if on { 0x0A0A0A } else { 0xFFFFFF });
        p.text(label, &p.g.f.m14sb, r, c, Align::Center);
    }
    let (bx, by, br) = help_badge(v);
    let c = if v.hover == Hit::ModeHelp {
        0x8A8A8A
    } else {
        0x6A6A6A
    };
    p.circle(bx, by, br, col(c));
    p.text(
        "?",
        &p.g.f.m12,
        rect(bx - br, by - br - 0.5, 2.0 * br, 2.0 * br),
        col(0xFFFFFF),
        Align::Center,
    );
}

pub fn draw(p: &Painter, v: &View) {
    draw_dial(p, v);
    draw_switch(p, v);
    draw_picks(p, v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::Page;

    fn view(w: f32, h: f32) -> View {
        View {
            page: Page::Care,
            w,
            h,
            hover: Hit::None,
            pressed: Hit::None,
            mode: Mode::Recommended,
            scan_hover: 0.0,
            scan_label: (1.0, 0.0),
            scan: None,
            auto_fix: false,
            picks: crate::scan::ALL,
            manual_t: 0.0,
            speed: Default::default(),
            opt: None,
            turbo: None,
            tools: false,
            protect: Default::default(),
            updater: None,
            note: None,
            maximized: false,
        }
    }

    #[test]
    fn label_timing_is_direction_aware() {
        for e in [true, false] {
            assert_eq!(label_fades(0.0, e), (1.0, 0.0));
            assert_eq!(label_fades(1.0, e), (0.0, 1.0));
        }
        // entering: word half gone after one frame (u ~0.11), icon not yet in
        let (t, i) = label_fades(0.11, true);
        assert!((t - 0.6).abs() < 1e-6 && i == 0.0);
        // leaving: magnifier still full ~100 ms in (u ~0.67), word not back
        let (t, i) = label_fades(0.67, false);
        assert!(t == 0.0 && i > 0.9);
    }

    #[test]
    fn under_glow_follows_the_reference() {
        assert_eq!(under_glow(0.0), 1.0);
        assert!(under_glow(0.10) < 0.75);
        assert_eq!(under_glow(0.25), 0.0);
        assert_eq!(under_glow(1.0), 0.0);
    }

    #[test]
    fn dial_sits_where_the_reference_has_it() {
        assert_eq!(center(&view(960.0, 660.0)), (553.0, 252.0));
    }

    #[test]
    fn manual_layout_matches_the_reference() {
        let mut v = view(960.0, 660.0);
        v.manual_t = 1.0;
        v.mode = Mode::Manual;
        assert_eq!(center(&v), (553.0, 207.0));
        let s = switch_rect(&v);
        // pill centre sits 182 px below the dial in Manual (vs 222.5 at rest)
        assert_eq!(s.top + SWITCH_H / 2.0, 207.0 + 182.0);
        assert_eq!(pick_box(&v, None), (553.0 - 363.5, 207.0 + 217.5));
        assert_eq!(
            pick_box(&v, Some(5)),
            (553.0 - 363.5 + 182.0, 207.0 + 217.5 + 70.0)
        );
        assert_eq!(hit(&v, 553.0 - 360.0, 207.0 + 225.0), Hit::PickAll);
        assert_eq!(
            hit(&v, 553.0 - 360.0 + 182.0 * 3.0, 207.0 + 260.0),
            Hit::Pick(3)
        );
        v.manual_t = 0.5;
        assert_eq!(
            hit(&v, 553.0 - 360.0, 207.0 + 225.0),
            Hit::None,
            "not while sliding"
        );
    }

    #[test]
    fn hits_dial_switch_and_help() {
        let v = view(960.0, 660.0);
        assert_eq!(hit(&v, 553.0, 252.0), Hit::Scan);
        assert_eq!(hit(&v, 553.0 + 126.0, 252.0), Hit::Scan);
        assert_eq!(
            hit(&v, 553.0 - 70.0, 252.0 + 222.0),
            Hit::Mode(Mode::Recommended)
        );
        assert_eq!(
            hit(&v, 553.0 + 70.0, 252.0 + 222.0),
            Hit::Mode(Mode::Manual)
        );
        assert_eq!(hit(&v, 553.0 - 132.5 + 8.0, 252.0 + 206.0), Hit::ModeHelp);
        assert_eq!(hit(&v, 300.0, 620.0), Hit::None);
    }
}
