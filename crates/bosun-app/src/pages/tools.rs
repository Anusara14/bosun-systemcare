//! More Optimization Tools, measured from the reference (`.reanimate/speedup`
//! frame 520; window-relative DIPs): four 205 px columns from x 38, 20 px
//! apart; headers at cap centre y 73 (15 px) with an ⓘ; panels from y 90 to
//! h − 43; tiles 66 px apart from y 125: a 16 px blue ring at x 68, label at
//! x 93; hover box 195 × 56.
//!
//! The reference's columns are mostly adverts for other products; ours are
//! all tools that do something here, built in or Windows' own.

use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::window::{Hit, View};

const COL_X: f32 = 38.0;
const COL_W: f32 = 205.0;
const COL_GAP: f32 = 20.0;
const TILE_Y: f32 = 125.0;
const TILE_H: f32 = 66.0;

/// Tool ids, in column order. `Hit::Tool(id)` carries one.
pub const COLUMNS: [(&str, [(u8, &str); 4]); 4] = [
    (
        "Speed Up",
        [
            (0, "Startup Optimizer"),
            (1, "Turbo Boost"),
            (2, "Driver Updates"),
            (3, "App Cleaner"),
        ],
    ),
    (
        "System Optimize",
        [
            (4, "Performance Monitor"),
            (5, "System Information"),
            (6, "Power Options"),
            (7, "Default Apps"),
        ],
    ),
    (
        "Security & Repair",
        [
            (8, "Windows Security"),
            (9, "Rescue Center"),
            (10, "Registry Clean"),
            (11, "Storage"),
        ],
    ),
    (
        "System Clean",
        [
            (12, "Disk Cleaner"),
            (13, "Empty Folder Scanner"),
            (14, "Shortcut Fixer"),
            (15, "Large File Finder"),
        ],
    ),
];

fn tile(c: usize, r: usize) -> D2D_RECT_F {
    let x = COL_X + (COL_W + COL_GAP) * c as f32;
    rect(
        x + 5.0,
        TILE_Y + TILE_H * r as f32 - 28.0,
        COL_W - 10.0,
        56.0,
    )
}

pub fn hit(_v: &View, x: f32, y: f32) -> Hit {
    if (x - 26.0).hypot(y - 23.5) <= 14.0 {
        return Hit::OptBack;
    }
    for (c, (_, tiles)) in COLUMNS.iter().enumerate() {
        for (r, (id, _)) in tiles.iter().enumerate() {
            if contains(&tile(c, r), x, y) {
                return Hit::Tool(*id);
            }
        }
    }
    Hit::None
}

pub fn draw(p: &Painter, v: &View) {
    let f = &p.g.f;
    let c = col(if v.hover == Hit::OptBack {
        0xFFFFFF
    } else {
        0xB0B0B0
    });
    p.line(19.5, 18.5, 30.0, 18.5, 1.6, c);
    p.line(19.5, 18.5, 23.0, 15.0, 1.6, c);
    p.line(19.5, 18.5, 23.0, 22.0, 1.6, c);
    p.arc_at(30.0, 23.5, 5.0, 1.6, 0.0, 0.5, c);
    p.line(30.0, 28.5, 23.0, 28.5, 1.6, c);
    for (ci, (head, tiles)) in COLUMNS.iter().enumerate() {
        let x = COL_X + (COL_W + COL_GAP) * ci as f32;
        let cy = 73.0 - 0.064 * 15.0;
        p.text(
            head,
            &f.pop_label,
            rect(x, cy - 15.0, COL_W, 30.0),
            col(0xD8D8D8),
            Align::Left,
        );
        let w = p.g.measure(head, &f.pop_label);
        info_dot(p, x + w + 12.0, 73.5);
        p.fill_rr(rect(x, 90.0, COL_W, v.h - 43.0 - 90.0), 6.0, col(0x1C1C1C));
        for (ri, (id, label)) in tiles.iter().enumerate() {
            let t = tile(ci, ri);
            if v.hover == Hit::Tool(*id) {
                p.fill_rr(t, 6.0, cola(0xFFFFFF, 0.08));
            }
            let my = TILE_Y + TILE_H * ri as f32;
            glyph(p, *id, x + 30.0, my);
            let ty = my - 0.064 * 13.0;
            p.text(
                label,
                &f.m13,
                rect(x + 55.0, ty - 13.0, COL_W - 60.0, 26.0),
                col(0xE6E6E6),
                Align::Left,
            );
        }
    }
}

fn info_dot(p: &Painter, cx: f32, cy: f32) {
    p.circle(cx, cy, 6.0, col(0x8A8A8A));
    let k = col(0x1C1C1C);
    p.circle(cx, cy - 2.5, 0.9, k);
    p.line(cx, cy - 0.5, cx, cy + 3.0, 1.4, k);
}

/// A tool's glyph inside a 16 px blue ring, centred on (cx, cy).
pub fn glyph(p: &Painter, id: u8, cx: f32, cy: f32) {
    let b = col(0x61ADF8);
    let w = 1.7;
    p.ring(cx, cy, 16.0, 1.8, b);
    match id {
        // rocket
        0 => {
            p.line(cx - 6.0, cy + 6.0, cx + 6.0, cy - 6.0, w, b);
            p.ring(cx + 1.5, cy - 1.5, 5.5, w, b);
            p.line(cx - 7.0, cy + 2.0, cx - 3.0, cy + 2.0, w, b);
            p.line(cx - 2.0, cy + 3.0, cx - 2.0, cy + 7.0, w, b);
        }
        // gauge
        1 => {
            p.arc_at(cx, cy + 2.0, 8.0, w, 0.7, 0.6, b);
            p.line(cx, cy + 2.0, cx + 5.0, cy - 3.0, w, b);
        }
        // chip
        2 => {
            p.stroke_rr(rect(cx - 6.0, cy - 7.0, 12.0, 14.0), 2.0, w, b);
            for d in [-3.5, 0.0, 3.5] {
                p.line(cx - 9.0, cy + d, cx - 6.0, cy + d, w, b);
                p.line(cx + 6.0, cy + d, cx + 9.0, cy + d, w, b);
            }
        }
        // grid of apps with a bin
        3 => {
            for (dx, dy) in [(-7.0, -7.0), (1.0, -7.0), (-7.0, 1.0)] {
                p.stroke_rr(rect(cx + dx, cy + dy, 6.0, 6.0), 1.0, w, b);
            }
            p.stroke_rr(rect(cx + 1.5, cy + 2.0, 5.0, 5.0), 1.0, w, b);
        }
        // monitor with a pulse
        4 => {
            p.stroke_rr(rect(cx - 8.0, cy - 6.0, 16.0, 11.0), 1.5, w, b);
            p.line(cx - 6.0, cy, cx - 2.0, cy, w, b);
            p.line(cx - 2.0, cy, cx, cy - 3.0, w, b);
            p.line(cx, cy - 3.0, cx + 2.0, cy + 2.0, w, b);
            p.line(cx + 2.0, cy + 2.0, cx + 6.0, cy + 2.0, w, b);
            p.line(cx - 3.0, cy + 8.0, cx + 3.0, cy + 8.0, w, b);
        }
        // info
        5 => {
            p.circle(cx, cy - 5.0, 1.4, b);
            p.line(cx, cy - 1.5, cx, cy + 6.5, 2.2, b);
        }
        // power
        6 => {
            p.arc_at(cx, cy + 1.0, 7.0, w, 0.58, 0.84, b);
            p.line(cx, cy - 8.0, cx, cy, w, b);
        }
        // window with a check (default apps)
        7 => {
            p.stroke_rr(rect(cx - 8.0, cy - 7.0, 16.0, 13.0), 1.5, w, b);
            p.line(cx - 8.0, cy - 3.5, cx + 8.0, cy - 3.5, w, b);
            p.line(cx - 3.0, cy + 1.5, cx - 0.5, cy + 4.0, w, b);
            p.line(cx - 0.5, cy + 4.0, cx + 4.5, cy - 1.0, w, b);
        }
        // shield
        8 => {
            p.line(cx, cy - 8.0, cx - 7.0, cy - 5.0, w, b);
            p.line(cx, cy - 8.0, cx + 7.0, cy - 5.0, w, b);
            p.line(cx - 7.0, cy - 5.0, cx - 6.0, cy + 2.0, w, b);
            p.line(cx + 7.0, cy - 5.0, cx + 6.0, cy + 2.0, w, b);
            p.line(cx - 6.0, cy + 2.0, cx, cy + 8.0, w, b);
            p.line(cx + 6.0, cy + 2.0, cx, cy + 8.0, w, b);
        }
        // rescue: circular arrow
        9 => {
            p.arc_at(cx, cy, 7.0, w, 0.15, 0.75, b);
            p.line(cx + 5.0, cy - 5.0, cx + 6.5, cy - 9.0, w, b);
            p.line(cx + 5.0, cy - 5.0, cx + 9.0, cy - 4.0, w, b);
        }
        // registry: stacked blocks
        10 => {
            p.stroke_rr(rect(cx - 7.0, cy - 7.0, 6.0, 6.0), 1.0, w, b);
            p.stroke_rr(rect(cx + 1.0, cy - 7.0, 6.0, 6.0), 1.0, w, b);
            p.stroke_rr(rect(cx - 7.0, cy + 1.0, 14.0, 6.0), 1.0, w, b);
        }
        // storage: a drive
        11 | 12 => {
            p.stroke_rr(rect(cx - 9.0, cy - 6.0, 18.0, 12.0), 2.0, w, b);
            p.line(cx - 9.0, cy + 1.5, cx + 9.0, cy + 1.5, w, b);
            p.circle(cx + 5.0, cy + 3.8, 1.2, b);
            if id == 12 {
                // disk cleaner: a sparkle
                p.line(cx - 4.0, cy - 9.0, cx - 4.0, cy - 6.0, w, b);
            }
        }
        // empty folder
        13 => {
            p.fill_poly(
                &[
                    (cx - 9.0, cy - 6.0),
                    (cx - 3.0, cy - 6.0),
                    (cx - 1.0, cy - 4.0),
                    (cx - 9.0, cy - 4.0),
                ],
                b,
            );
            p.stroke_rr(rect(cx - 9.0, cy - 4.0, 18.0, 11.0), 1.5, w, b);
        }
        // shortcut: arrow out of a box
        14 => {
            p.stroke_rr(rect(cx - 7.0, cy - 7.0, 14.0, 14.0), 2.0, w, b);
            p.line(cx - 2.0, cy + 2.0, cx + 4.0, cy - 4.0, w, b);
            p.line(cx + 4.0, cy - 4.0, cx, cy - 4.0, w, b);
            p.line(cx + 4.0, cy - 4.0, cx + 4.0, cy, w, b);
        }
        // large file: page and magnifier
        _ => {
            p.stroke_rr(rect(cx - 8.0, cy - 9.0, 11.0, 15.0), 1.5, w, b);
            p.ring(cx + 3.5, cy + 2.5, 4.0, w, b);
            p.line(cx + 6.4, cy + 5.4, cx + 9.0, cy + 8.0, 2.0, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        // first tile of column 1 ("Smart RAM" in the reference) centre ≈ (330, 125)
        assert_eq!(hit(&v, 330.0, 125.0), Hit::Tool(4));
        assert_eq!(hit(&v, 780.0, 125.0 + 66.0 * 3.0), Hit::Tool(15));
        assert_eq!(hit(&v, 26.0, 23.0), Hit::OptBack);
        let ids: Vec<u8> = COLUMNS
            .iter()
            .flat_map(|(_, t)| t.iter().map(|x| x.0))
            .collect();
        assert_eq!(ids, (0..16).collect::<Vec<u8>>(), "ids run 0..16 in order");
    }
}
