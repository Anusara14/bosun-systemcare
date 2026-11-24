//! Filled sidebar icons drawn as vectors (26 DIP box), matching the
//! reference's solid-white style. Details are "cut" out with the colour
//! behind the icon so they read like knock-outs.

use std::f32::consts::TAU;

use bosun_ui::gfx::{Painter, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::pages::Page;

pub fn nav(p: &Painter, page: Page, cx: f32, cy: f32, fg: D2D1_COLOR_F, cut: D2D1_COLOR_F) {
    match page {
        Page::Care => care(p, cx, cy, fg),
        Page::SpeedUp => speed(p, cx, cy, fg, cut),
        Page::Protect => shield(p, cx, cy, fg, cut),
        Page::Updater => cloud(p, cx, cy, fg, cut),
        Page::Rescue => lifebuoy(p, cx, cy, fg, cut),
    }
}

/// Aperture: a ring with six blades sweeping toward the centre.
fn care(p: &Painter, cx: f32, cy: f32, fg: D2D1_COLOR_F) {
    p.ring(cx, cy, 11.0, 2.6, fg);
    let at = |r: f32, t: f32| (cx + r * t.sin(), cy - r * t.cos());
    for i in 0..6 {
        let t = i as f32 / 6.0 * TAU;
        p.curve(
            [
                at(11.0, t),
                at(9.0, t + 0.35),
                at(6.0, t + 0.8),
                at(3.6, t + 1.25),
            ],
            1.9,
            fg,
        );
    }
}

/// Gauge: solid dial with a knocked-out needle, hub and ticks.
fn speed(p: &Painter, cx: f32, cy: f32, fg: D2D1_COLOR_F, cut: D2D1_COLOR_F) {
    p.circle(cx, cy, 12.0, fg);
    for a in [-70.0f32, -35.0, 0.0, 35.0, 70.0] {
        let t = a.to_radians();
        p.line(
            cx + 9.4 * t.sin(),
            cy - 9.4 * t.cos(),
            cx + 7.2 * t.sin(),
            cy - 7.2 * t.cos(),
            1.6,
            cut,
        );
    }
    p.line(cx, cy + 2.0, cx + 5.4, cy - 5.0, 2.4, cut);
    p.circle(cx, cy + 2.0, 2.7, cut);
    p.fill(rect(cx - 5.0, cy + 7.0, 10.0, 1.8), cut);
}

/// Shield with a knocked-out check mark.
fn shield(p: &Painter, cx: f32, cy: f32, fg: D2D1_COLOR_F, cut: D2D1_COLOR_F) {
    let pts: Vec<(f32, f32)> = [
        (0.0, -12.5),
        (10.5, -9.0),
        (10.5, -1.0),
        (9.0, 4.0),
        (5.5, 8.5),
        (0.0, 12.5),
        (-5.5, 8.5),
        (-9.0, 4.0),
        (-10.5, -1.0),
        (-10.5, -9.0),
    ]
    .iter()
    .map(|&(x, y)| (cx + x, cy + y))
    .collect();
    p.fill_poly(&pts, fg);
    p.line(cx - 4.6, cy + 0.2, cx - 1.2, cy + 3.6, 2.4, cut);
    p.line(cx - 1.2, cy + 3.6, cx + 4.8, cy - 3.2, 2.4, cut);
}

/// Cloud outline with an up-arrow rising through its base.
fn cloud(p: &Painter, cx: f32, cy: f32, fg: D2D1_COLOR_F, cut: D2D1_COLOR_F) {
    let parts = [(-5.5, 1.5, 5.5), (0.5, -2.5, 7.5), (6.5, 2.0, 5.0)];
    for (dx, dy, r) in parts {
        p.circle(cx + dx, cy + dy, r, fg);
    }
    p.fill_rr(rect(cx - 5.5, cy + 1.5, 12.0, 5.5), 2.0, fg);
    for (dx, dy, r) in parts {
        p.circle(cx + dx, cy + dy, r - 2.2, cut);
    }
    p.fill(rect(cx - 5.5, cy + 1.5, 12.0, 3.3), cut);
    p.fill(rect(cx - 2.8, cy + 4.0, 5.6, 4.0), cut);
    p.line(cx + 0.3, cy + 11.0, cx + 0.3, cy + 0.5, 2.2, fg);
    p.line(cx + 0.3, cy + 0.0, cx - 3.4, cy + 3.7, 2.2, fg);
    p.line(cx + 0.3, cy + 0.0, cx + 4.0, cy + 3.7, 2.2, fg);
}

/// Lifebuoy: a thick ring with four diagonal bands knocked out.
fn lifebuoy(p: &Painter, cx: f32, cy: f32, fg: D2D1_COLOR_F, cut: D2D1_COLOR_F) {
    p.ring(cx, cy, 8.6, 6.8, fg);
    for k in 0..4 {
        let t = std::f32::consts::FRAC_PI_4 + k as f32 * std::f32::consts::FRAC_PI_2;
        let (s, c) = t.sin_cos();
        p.line(
            cx + 5.0 * c,
            cy + 5.0 * s,
            cx + 12.2 * c,
            cy + 12.2 * s,
            2.0,
            cut,
        );
    }
}
