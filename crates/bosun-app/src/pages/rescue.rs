//! Rescue Center, in the reference's Action Center layout (`.reanimate/phase6`,
//! scaled from 150 %): a 20 px heading at x 175 (cap centre 66.7) over a 13 px
//! line (90); rows 757 × 71, 78 px apart, centred from y 169; a 32 px round
//! icon centred at x 216; the title at x 251 (cy − 5.7) over a 13 px line
//! (cy + 10.3); a 113 × 30 button at x 798; a link 24 px under the last row.
//!
//! IObit's rows sell other software. Ours are the changes Bosun made, newest
//! first, each with Restore where it can be put back.

use bosun_core::rescue::{Entry, Kind, Undo};
use bosun_ui::gfx::{Align, Painter, col, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::speedup::{pill, text_at};
use crate::window::{Hit, View};

const ROW_X: f32 = 175.0;
const ROW_H: f32 = 71.0;
const PITCH: f32 = 78.0;
const FIRST: f32 = 169.0;
const LIST_TOP: f32 = 128.0;

/// The journal as shown, and how far it is scrolled.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RescueView {
    pub entries: Vec<Entry>,
    pub scroll: f32,
}

impl RescueView {
    pub fn load() -> Self {
        RescueView {
            entries: bosun_core::clean::journal_dir()
                .map(|d| bosun_core::rescue::list(&d))
                .unwrap_or_default(),
            scroll: 0.0,
        }
    }

    pub fn scroll_by(&mut self, dy: f32, h: f32) {
        let n = self.entries.len() as f32;
        let max = (n * PITCH + 40.0 - (h - LIST_TOP - 10.0)).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
    }
}

fn row_rect(v: &View, r: &RescueView, k: usize) -> D2D_RECT_F {
    let cy = FIRST + PITCH * k as f32 - r.scroll;
    rect(ROW_X, cy - ROW_H / 2.0, v.w - 28.0 - ROW_X, ROW_H)
}

fn button(r: &D2D_RECT_F) -> D2D_RECT_F {
    let cy = (r.top + r.bottom) / 2.0;
    rect(r.right - 21.5 - 113.0, cy - 15.0, 113.0, 30.0)
}

fn folder_link(v: &View, r: &RescueView) -> D2D_RECT_F {
    let last = row_rect(v, r, r.entries.len().max(1) - 1);
    rect(last.right - 120.0, last.bottom + 16.0, 120.0, 16.0)
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    let Some(r) = &v.rescue else { return Hit::None };
    if y < LIST_TOP {
        return Hit::None;
    }
    if contains(&folder_link(v, r), x, y) {
        return Hit::RescueFolder;
    }
    for (k, e) in r.entries.iter().enumerate() {
        let rr = row_rect(v, r, k);
        if !contains(&rr, x, y) {
            continue;
        }
        if contains(&button(&rr), x, y) {
            match e.undo {
                Undo::Restore => return Hit::RescueRestore(k),
                Undo::RecycleBin => return Hit::RescueBin(k),
                Undo::None => {}
            }
        }
        return Hit::RescueRow(k);
    }
    Hit::None
}

/// `Nov 24, 13:37` in local time.
fn when(ms: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // 100 ns ticks since 1601
    let t = (ms + 11_644_473_600_000) * 10_000;
    let ft = FILETIME {
        dwLowDateTime: t as u32,
        dwHighDateTime: (t >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: plain conversions into our own structs.
    let ok = unsafe {
        FileTimeToSystemTime(&ft, &mut utc).is_ok()
            && SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok()
    };
    if !ok {
        return String::new();
    }
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {}, {:02}:{:02}",
        MON[(local.wMonth.clamp(1, 12) - 1) as usize],
        local.wDay,
        local.wHour,
        local.wMinute
    )
}

/// Ring icons in the reference's style: a 1.6 px circle with a glyph inside.
fn icon(p: &Painter, kind: Kind, cx: f32, cy: f32) {
    let c = col(match kind {
        Kind::Clean | Kind::Recycle => 0x8FD18F,
        Kind::Registry | Kind::Privacy => 0x61ADF8,
        Kind::Startup | Kind::Services | Kind::Tasks | Kind::Turbo => 0xF5A56A,
        Kind::Other => 0xB0B0B0,
    });
    p.ring(cx, cy, 15.0, 1.6, c);
    match kind {
        Kind::Clean | Kind::Recycle => {
            // a bin
            p.stroke_rr(rect(cx - 5.0, cy - 4.0, 10.0, 11.0), 1.5, 1.6, c);
            p.line(cx - 7.5, cy - 6.0, cx + 7.5, cy - 6.0, 1.6, c);
            p.line(cx - 2.0, cy - 8.5, cx + 2.0, cy - 8.5, 1.6, c);
            p.line(cx - 1.8, cy - 1.0, cx - 1.8, cy + 4.0, 1.4, c);
            p.line(cx + 1.8, cy - 1.0, cx + 1.8, cy + 4.0, 1.4, c);
        }
        Kind::Registry => {
            // four cubes
            for (dx, dy) in [(-4.5, -4.5), (0.5, -4.5), (-4.5, 0.5), (0.5, 0.5)] {
                p.fill_rr(rect(cx + dx, cy + dy, 4.0, 4.0), 0.8, c);
            }
        }
        Kind::Privacy => {
            // an eye
            p.curve(
                [
                    (cx - 8.0, cy),
                    (cx - 3.0, cy - 6.0),
                    (cx + 3.0, cy - 6.0),
                    (cx + 8.0, cy),
                ],
                1.6,
                c,
            );
            p.curve(
                [
                    (cx - 8.0, cy),
                    (cx - 3.0, cy + 6.0),
                    (cx + 3.0, cy + 6.0),
                    (cx + 8.0, cy),
                ],
                1.6,
                c,
            );
            p.circle(cx, cy, 2.4, c);
        }
        Kind::Turbo => {
            // a bolt
            p.line(cx + 2.5, cy - 8.0, cx - 3.5, cy + 1.0, 1.8, c);
            p.line(cx - 3.5, cy + 1.0, cx + 3.5, cy - 1.0, 1.8, c);
            p.line(cx + 3.5, cy - 1.0, cx - 2.5, cy + 8.0, 1.8, c);
        }
        _ => {
            // a power switch: startup items, services and tasks
            p.line(cx, cy - 8.0, cx, cy - 1.0, 1.8, c);
            p.curve(
                [
                    (cx - 4.5, cy - 5.5),
                    (cx - 9.0, cy + 1.0),
                    (cx - 4.0, cy + 8.0),
                    (cx, cy + 8.0),
                ],
                1.6,
                c,
            );
            p.curve(
                [
                    (cx + 4.5, cy - 5.5),
                    (cx + 9.0, cy + 1.0),
                    (cx + 4.0, cy + 8.0),
                    (cx, cy + 8.0),
                ],
                1.6,
                c,
            );
        }
    }
}

fn link(p: &Painter, s: &str, r: D2D_RECT_F, hot: bool) {
    let f = &p.g.f;
    let c = if hot { 0x8CC4FF } else { 0x61ADF8 };
    let w = p.g.measure(s, &f.m13);
    let x = (r.left + r.right) / 2.0 - w / 2.0;
    let cy = (r.top + r.bottom) / 2.0;
    text_at(p, s, &f.m13, 13.0, x, w + 4.0, cy, c);
    p.line(x, cy + 7.5, x + w, cy + 7.5, 1.0, col(c));
}

pub fn draw(p: &Painter, v: &View) {
    let f = &p.g.f;
    let Some(r) = &v.rescue else { return };
    text_at(
        p,
        "Rescue Center",
        &f.h1,
        20.0,
        ROW_X,
        500.0,
        66.7,
        0xF2F2F2,
    );
    text_at(
        p,
        "Bosun saves every change before making it. Put back what you need.",
        &f.m13,
        13.0,
        ROW_X,
        700.0,
        90.0,
        0xB8B8B8,
    );
    p.clip(rect(ROW_X, LIST_TOP, v.w - ROW_X, v.h - LIST_TOP));
    for (k, e) in r.entries.iter().enumerate() {
        let rr = row_rect(v, r, k);
        if rr.bottom < LIST_TOP || rr.top > v.h {
            continue;
        }
        let hot = matches!(v.hover, Hit::RescueRow(i) | Hit::RescueRestore(i) | Hit::RescueBin(i) if i == k);
        p.fill_rr(rr, 6.0, col(if hot { 0x222222 } else { 0x1C1C1C }));
        let cy = (rr.top + rr.bottom) / 2.0;
        icon(p, e.kind, 216.0, cy);
        text_at(
            p,
            &e.title,
            &f.m14sb,
            15.0,
            251.0,
            520.0,
            cy - 5.7,
            0xF2F2F2,
        );
        let line = format!("{}  ·  {}", when(e.when), e.summary);
        text_at(p, &line, &f.m13, 13.0, 251.0, 520.0, cy + 10.3, 0xB0B0B0);
        let b = button(&rr);
        match e.undo {
            Undo::Restore => pill(p, b, "Restore", v.hover == Hit::RescueRestore(k)),
            Undo::RecycleBin => link(p, "Recycle Bin", b, v.hover == Hit::RescueBin(k)),
            Undo::None => {
                let t = if e.restored {
                    "Restored"
                } else if e.dry_run {
                    "Nothing changed"
                } else {
                    "Can't be undone"
                };
                p.text(t, &f.m13, b, col(0x8A8A8A), Align::Center);
            }
        }
    }
    if r.entries.is_empty() {
        p.text(
            "Nothing yet: changes Bosun makes will be listed here.",
            &f.m13,
            rect(ROW_X, 220.0, v.w - ROW_X - 28.0, 20.0),
            col(0x8A8A8A),
            Align::Center,
        );
    } else {
        let l = folder_link(v, r);
        let c = if v.hover == Hit::RescueFolder {
            0xFFFFFF
        } else {
            0xD0D0D0
        };
        let w = p.g.measure("Open journal folder", &f.m13);
        text_at(
            p,
            "Open journal folder",
            &f.m13,
            13.0,
            l.right - w,
            w + 4.0,
            l.top + 8.0,
            c,
        );
        p.line(
            l.right - w,
            l.top + 15.0,
            l.right,
            l.top + 15.0,
            1.0,
            col(c),
        );
    }
    p.unclip();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_buttons_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        let r = RescueView::default();
        let rr = row_rect(&v, &r, 0);
        assert_eq!(
            (rr.left, rr.right, (rr.top + rr.bottom) / 2.0),
            (175.0, 932.0, 169.0)
        );
        let b = button(&rr);
        assert_eq!((b.left, b.top, b.right), (797.5, 154.0, 910.5));
        assert!(!when(1_795_498_417_555).is_empty());
    }
}
