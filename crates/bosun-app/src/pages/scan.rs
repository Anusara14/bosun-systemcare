//! Scan and Summary screens, measured from the reference recording
//! (`.reanimate/scan-flow`, window-relative DIPs at 960 × 660). While a scan is
//! shown the sidebar is replaced by two panels:
//!
//! | element                | geometry                                         |
//! |------------------------|--------------------------------------------------|
//! | module list panel      | x 13, w 320, y 47 to h − 16                      |
//! | heading                | text x 67, cap centre y 76.5, 22 px semibold     |
//! | rows                   | from y 106, 40 px apart; current row x 21–312    |
//! | row status             | dash / spinner centred x 281, badge right at 291 |
//! | content panel          | x 341 to w − 15                                  |
//! | progress ring          | centre (content centre, 212), r 92, 6 px stroke  |
//! | percentage             | 72 px light digits + 20 px "%", 10 px apart      |
//! | "Scanning for …"       | cap centre y 361.5, 21 px; path line y 394       |
//! | Auto fix               | 15 px box, cap centre y 496                      |
//! | Skip / Stop, Back / Fix| 154 × 34 pills at y h − 132, 10 px apart         |
//! | summary card           | x 363, w 560, from y 102, 70 tall                |

use bosun_core::junk::Tally;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::scan::{Kind, Progress, Row, Status, middle_ellipsis};
use crate::window::{Hit, View};

const LIST_X: f32 = 13.0;
const LIST_W: f32 = 320.0;
const TOP: f32 = 47.0;
const BOTTOM_GAP: f32 = 16.0;
const CONTENT_X: f32 = 341.0;
const RIGHT_GAP: f32 = 15.0;
const ROW_Y: f32 = 106.0;
const ROW_H: f32 = 40.0;
const TEXT_X: f32 = 67.0;
const STATUS_X: f32 = 281.0;
const RING_Y: f32 = 212.0;
const RING_R: f32 = 92.0;
const BTN_W: f32 = 154.0;
const BTN_H: f32 = 34.0;
const BTN_FROM_BOTTOM: f32 = 132.0;
const CARD_INSET: f32 = 22.0;
const CARD_H: f32 = 70.0;
const CARD_GAP: f32 = 8.0;

mod sc {
    pub const PANEL: u32 = 0x1A1A1A;
    pub const CURRENT: u32 = 0x2E2E2E;
    pub const CARD: u32 = 0x222222;
    pub const TRACK: u32 = 0x2C2C2C;
    pub const ARC: u32 = 0x61ADF8;
    pub const TEXT: u32 = 0xEDEDED;
    pub const ROW: u32 = 0xC4C4C4;
    pub const DIM: u32 = 0xB4B4B4;
    pub const DASH: u32 = 0x4E4E4E;
    pub const ORANGE: u32 = 0xF5A56A;
}

/// Segoe UI's line box centres about 0.064 em below the cap-height centre, so
/// a text rect centred here puts the capitals' centre on `cap_y`.
#[allow(clippy::too_many_arguments)]
fn text_at(
    p: &Painter,
    s: &str,
    f: &IDWriteTextFormat,
    size: f32,
    (x, w): (f32, f32),
    cap_y: f32,
    c: u32,
    a: Align,
) {
    let cy = cap_y - 0.064 * size;
    p.text(s, f, rect(x, cy - size, w, 2.0 * size), col(c), a);
}

fn content_cx(v: &View) -> f32 {
    CONTENT_X + (v.w - RIGHT_GAP - CONTENT_X) / 2.0
}

/// (left, right) pill buttons: Skip / Stop while scanning, Back / Fix after.
fn buttons(v: &View) -> [D2D_RECT_F; 2] {
    let cx = content_cx(v);
    let y = v.h - BTN_FROM_BOTTOM;
    [
        rect(cx - 5.0 - BTN_W, y, BTN_W, BTN_H),
        rect(cx + 5.0, y, BTN_W, BTN_H),
    ]
}

fn auto_fix_box(v: &View) -> D2D_RECT_F {
    rect(content_cx(v) - 40.0, 488.5 + (v.h - 660.0), 15.0, 15.0)
}

pub fn hit(v: &View, s: &Progress, x: f32, y: f32) -> Hit {
    if s.finished && s.fixing {
        return if contains(&done_button(v), x, y) {
            Hit::ScanBack
        } else {
            Hit::None
        };
    }
    if s.is_summary()
        && let Some(i) = row_at(s, x, y)
    {
        return Hit::ScanRow(i);
    }
    let [l, r] = buttons(v);
    let (left, right) = if s.finished {
        (Hit::ScanBack, Hit::ScanFix)
    } else {
        (Hit::ScanSkip, Hit::ScanStop)
    };
    if contains(&l, x, y) {
        return left;
    }
    if contains(&r, x, y) {
        return right;
    }
    let b = auto_fix_box(v);
    // the box and its "Auto fix" label
    if !s.finished && !s.fixing && contains(&rect(b.left, b.top - 3.0, 95.0, 21.0), x, y) {
        return Hit::AutoFix;
    }
    Hit::None
}

/// The fix result's single button, centred under the content.
fn done_button(v: &View) -> D2D_RECT_F {
    rect(
        content_cx(v) - BTN_W / 2.0,
        v.h - BTN_FROM_BOTTOM,
        BTN_W,
        BTN_H,
    )
}

/// A summary row with findings (click toggles its tick).
fn row_at(s: &Progress, x: f32, y: f32) -> Option<usize> {
    (0..s.rows.len()).find(|&i| {
        s.rows[i].found().is_some()
            && contains(&rect(21.0, ROW_Y + i as f32 * ROW_H, 291.0, ROW_H), x, y)
    })
}

/// "1.3 GB", "512 MB", "40 KB": the reference's one-decimal style.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    let mut v = bytes as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i >= 2 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

pub fn draw(p: &Painter, v: &View, s: &Progress) {
    let bottom = v.h - BOTTOM_GAP;
    p.fill_rr(rect(LIST_X, TOP, LIST_W, bottom - TOP), 8.0, col(sc::PANEL));
    let right = rect(CONTENT_X, TOP, v.w - RIGHT_GAP - CONTENT_X, bottom - TOP);
    p.fill_rr(right, 8.0, col(sc::PANEL));
    draw_list(p, v, s);
    if s.finished && s.fixing {
        draw_result(p, v, s);
        let b = done_button(v);
        pill(p, b, "Done", (0x8DC3F8, 0x6DB4F7), v.hover == Hit::ScanBack);
        return;
    }
    if s.finished {
        draw_summary(p, v, s);
    } else {
        draw_progress(p, v, s);
    }
    draw_buttons(p, v, s.finished);
}

fn draw_list(p: &Painter, v: &View, s: &Progress) {
    let f = &p.g.f;
    let heading = match (s.finished, s.fixing) {
        (true, _) => "Summary",
        (false, true) => "Fixing...",
        (false, false) => "Scanning...",
    };
    if s.finished {
        p.fill_rr(rect(21.0, 55.0, 291.0, 42.0), 6.0, col(sc::CURRENT));
    }
    text_at(
        p,
        heading,
        &f.s_head,
        22.0,
        (TEXT_X, 240.0),
        76.5,
        sc::TEXT,
        Align::Left,
    );
    for (i, r) in s.rows.iter().enumerate() {
        let top = ROW_Y + i as f32 * ROW_H;
        let mid = top + ROW_H / 2.0;
        let current = !s.finished
            && (r.status == Status::Running || (s.fixing && i == s.current && r.checked));
        if current {
            p.fill_rr(rect(21.0, top, 291.0, ROW_H), 6.0, col(sc::CURRENT));
        } else if s.is_summary() && v.hover == Hit::ScanRow(i) {
            p.fill_rr(rect(21.0, top, 291.0, ROW_H), 6.0, cola(0xFFFFFF, 0.04));
        }
        let c = if current { sc::TEXT } else { sc::ROW };
        text_at(
            p,
            r.name,
            &f.pop_label,
            14.0,
            (TEXT_X, 180.0),
            mid - 0.5,
            c,
            Align::Left,
        );
        if current {
            spinner(p, STATUS_X - 1.0, mid, s.pct);
            continue;
        }
        if r.fixed.is_some() {
            done_mark(p, STATUS_X, mid);
            continue;
        }
        match r.status {
            Status::Running => spinner(p, STATUS_X - 1.0, mid, s.pct),
            Status::Done(t) if t.files > 0 => {
                if s.is_summary() {
                    checkbox(p, 49.0, mid, r.checked, 1.0);
                }
                badge(p, mid, t.files);
            }
            Status::Health(true) => done_mark(p, STATUS_X, mid),
            Status::Health(false) => info_mark(p, STATUS_X, mid),
            Status::Done(_) | Status::Skipped => {
                p.line(STATUS_X - 7.0, mid, STATUS_X + 7.0, mid, 2.0, col(sc::DASH));
            }
            Status::Waiting => {}
        }
    }
}

/// Blue circle with a tick: cleaned (the reference's "OK" mark).
fn done_mark(p: &Painter, cx: f32, cy: f32) {
    p.circle(cx, cy, 8.5, col(sc::ARC));
    let c = col(0x0A0A0A);
    p.line(cx - 4.0, cy + 0.2, cx - 1.2, cy + 3.0, 1.8, c);
    p.line(cx - 1.2, cy + 3.0, cx + 4.2, cy - 3.2, 1.8, c);
}

/// Orange ring with an "i": needs attention (the reference's info mark).
fn info_mark(p: &Painter, cx: f32, cy: f32) {
    p.ring(cx, cy, 8.0, 1.5, col(sc::ORANGE));
    p.circle(cx, cy - 3.6, 1.2, col(sc::ORANGE));
    p.line(cx, cy - 0.8, cx, cy + 4.2, 1.8, col(sc::ORANGE));
}

/// Small busy ring: dim track with a turning blue quarter.
fn spinner(p: &Painter, cx: f32, cy: f32, phase: f32) {
    p.ring(cx, cy, 9.0, 1.6, col(0x3A3A3A));
    p.arc_at(cx, cy, 9.0, 1.6, phase * 3.0, 0.3, col(sc::ARC));
}

/// Count pill in the warm "attention" colour, right edge at x 291.
fn badge(p: &Painter, cy: f32, n: u64) {
    let s = n.to_string();
    let w = (p.g.measure(&s, &p.g.f.m12) + 16.0).max(34.0);
    let r = rect(291.0 - w, cy - 10.5, w, 21.0);
    p.fill_rr(r, 10.5, cola(sc::ORANGE, 0.16));
    p.stroke_rr(r, 10.5, 1.0, cola(sc::ORANGE, 0.45));
    p.text(&s, &p.g.f.m12, r, col(sc::ORANGE), Align::Center);
}

/// 16 px tick box (blue when on), centred on (cx, cy); shared with Care.
pub fn checkbox(p: &Painter, cx: f32, cy: f32, on: bool, a: f32) {
    let b = rect(cx - 8.0, cy - 8.0, 16.0, 16.0);
    if on {
        p.fill_rr(b, 3.0, cola(sc::ARC, a));
        let c = cola(0x0A0A0A, a);
        p.line(cx - 4.0, cy + 0.2, cx - 1.2, cy + 3.0, 1.8, c);
        p.line(cx - 1.2, cy + 3.0, cx + 4.2, cy - 3.2, 1.8, c);
    } else {
        p.stroke_rr(b, 3.0, 1.0, cola(0x5A5A5A, a));
    }
}

fn draw_progress(p: &Painter, v: &View, s: &Progress) {
    let f = &p.g.f;
    let cx = content_cx(v);
    p.ring(cx, RING_Y, RING_R, 6.0, col(sc::TRACK));
    if s.pct >= 0.999 {
        // complete: full ring with a soft halo, as the reference's 100 % frame
        // halo only outside the ring; the inside stays dark
        let k = (RING_R + 3.0) / (RING_R + 14.0);
        p.radial(
            cx,
            RING_Y,
            RING_R + 14.0,
            &[
                (0.0, cola(sc::ARC, 0.0)),
                (k - 0.001, cola(sc::ARC, 0.0)),
                (k, cola(sc::ARC, 0.25)),
                (1.0, cola(sc::ARC, 0.0)),
            ],
        );
        p.ring(cx, RING_Y, RING_R, 6.0, col(sc::ARC));
    } else {
        p.arc(cx, RING_Y, RING_R, 6.0, s.pct, col(sc::ARC));
    }
    let digits = format!("{}", ((s.pct * 100.0).floor() as u32).clamp(1, 100));
    let wd = p.g.measure(&digits, &f.s_pct);
    let ws = p.g.measure("%", &f.s_sign);
    let x = cx - (wd + 10.0 + ws) / 2.0;
    text_at(
        p,
        &digits,
        &f.s_pct,
        72.0,
        (x, wd + 4.0),
        RING_Y,
        sc::TEXT,
        Align::Left,
    );
    text_at(
        p,
        "%",
        &f.s_sign,
        20.0,
        (x + wd + 10.0, ws + 4.0),
        227.0,
        sc::TEXT,
        Align::Left,
    );
    let Some(row) = s.rows.get(s.current) else {
        return;
    };
    let span = (CONTENT_X + 14.0, v.w - RIGHT_GAP - CONTENT_X - 28.0);
    let title = if s.fixing {
        format!("Cleaning {}", row.what)
    } else {
        format!("Scanning for {}", row.what)
    };
    text_at(
        p,
        &title,
        &f.s_title,
        21.0,
        span,
        361.5,
        sc::TEXT,
        Align::Center,
    );
    let path = middle_ellipsis(&s.path, |c| p.g.measure(c, &f.m13) <= span.1);
    text_at(p, &path, &f.m13, 13.0, span, 394.0, sc::DIM, Align::Center);
    if s.fixing {
        return;
    }
    let b = auto_fix_box(v);
    let on = v.auto_fix;
    checkbox(p, b.left + 7.5, b.top + 7.5, on, 1.0);
    let hover = v.hover == Hit::AutoFix;
    let c = if hover { sc::TEXT } else { sc::DIM };
    text_at(
        p,
        "Auto fix",
        &f.m13,
        13.0,
        (b.left + 24.0, 70.0),
        b.top + 7.5,
        c,
        Align::Left,
    );
    p.caret_down(b.left + 86.0, b.top + 8.0, 6.0, col(c));
}

/// Broom: a slanted handle over a fanned head.
fn broom(p: &Painter, cx: f32, cy: f32) {
    let c = col(0x8A8A8A);
    p.line(cx + 9.0, cy - 13.0, cx + 1.0, cy - 1.0, 2.4, c);
    p.fill_poly(
        &[
            (cx - 2.0, cy - 4.0),
            (cx + 5.0, cy + 1.0),
            (cx - 1.0, cy + 13.0),
            (cx - 12.0, cy + 6.0),
        ],
        c,
    );
}

fn draw_summary(p: &Painter, v: &View, s: &Progress) {
    let f = &p.g.f;
    let cx = content_cx(v);
    let span = (CONTENT_X + 14.0, v.w - RIGHT_GAP - CONTENT_X - 28.0);
    let total = s.total();
    let (head, c) = if total.files > 0 {
        let n = total.files;
        (
            format!("Fix {n} items to get a cleaner and faster PC."),
            sc::ORANGE,
        )
    } else if s.rows.iter().any(|r| r.status == Status::Health(false)) {
        ("Windows Security needs attention.".to_string(), sc::ORANGE)
    } else if s.rows.iter().any(|r| r.found().is_some()) {
        ("Tick a module to fix it.".to_string(), sc::TEXT)
    } else {
        ("Your PC is clean.".to_string(), sc::TEXT)
    };
    text_at(p, &head, &f.s_head, 22.0, span, 75.5, c, Align::Center);
    let card_w = v.w - RIGHT_GAP - CONTENT_X - 2.0 * CARD_INSET;
    let x = CONTENT_X + CARD_INSET;
    let mut y = 102.0;
    for r in &s.rows {
        let Some(card) = summary_card(r) else {
            continue;
        };
        draw_card(p, rect(x, y, card_w, CARD_H), &card);
        y += CARD_H + CARD_GAP;
    }
    let _ = cx;
}

fn draw_result(p: &Painter, v: &View, s: &Progress) {
    let f = &p.g.f;
    let span = (CONTENT_X + 14.0, v.w - RIGHT_GAP - CONTENT_X - 28.0);
    let total = s.cleaned();
    let (head, c) = match &s.error {
        Some(e) => (e.clone(), sc::ORANGE),
        None => (
            format!(
                "Cleaned {} items and freed {}.",
                total.removed.files,
                size(total.removed.bytes)
            ),
            sc::TEXT,
        ),
    };
    text_at(p, &head, &f.s_head, 22.0, span, 75.5, c, Align::Center);
    let card_w = v.w - RIGHT_GAP - CONTENT_X - 2.0 * CARD_INSET;
    let x = CONTENT_X + CARD_INSET;
    let mut y = 102.0;
    for r in &s.rows {
        let Some(card) = result_card(r) else { continue };
        draw_card(p, rect(x, y, card_w, CARD_H), &card);
        y += CARD_H + CARD_GAP;
    }
}

enum Icon {
    Broom,
    Done,
    Warn,
}

/// A summary or result card: icon, coloured headline, explanation.
struct Card {
    icon: Icon,
    head: String,
    color: u32,
    line: String,
}

fn draw_card(p: &Painter, r: D2D_RECT_F, c: &Card) {
    let f = &p.g.f;
    let (x, y) = (r.left, r.top);
    p.fill_rr(r, 6.0, col(sc::CARD));
    match c.icon {
        Icon::Broom => broom(p, x + 31.0, y + 35.0),
        Icon::Done => done_mark(p, x + 31.0, y + 35.0),
        Icon::Warn => info_mark(p, x + 31.0, y + 35.0),
    }
    text_at(
        p,
        &c.head,
        &f.s_size,
        17.0,
        (x + 74.0, 300.0),
        y + 24.0,
        c.color,
        Align::Left,
    );
    let w = r.right - r.left - 90.0;
    text_at(
        p,
        &c.line,
        &f.m13,
        13.0,
        (x + 75.0, w),
        y + 49.5,
        sc::DIM,
        Align::Left,
    );
}

fn summary_card(r: &Row) -> Option<Card> {
    if r.status == Status::Health(false) {
        return Some(Card {
            icon: Icon::Warn,
            head: "Needs attention".into(),
            color: sc::ORANGE,
            line: format!(
                "{}: off or out of date. Turn it on in Windows Security.",
                r.name
            ),
        });
    }
    let t = r.found()?;
    let (head, line) = match r.kind {
        Kind::Privacy | Kind::Junk => (size(t.bytes), card_line(r.name, t)),
        Kind::Shortcuts => (
            plural(t.files, "shortcut", "shortcuts"),
            format!(
                "{}: their targets no longer exist. Fix moves them to the Recycle Bin.",
                r.name
            ),
        ),
        Kind::Registry => (
            plural(t.files, "entry", "entries"),
            format!(
                "{}: they point to programs that are gone. Backed up first.",
                r.name
            ),
        ),
        Kind::Antivirus | Kind::Firewall => return None,
    };
    Some(Card {
        icon: Icon::Broom,
        head,
        color: sc::ORANGE,
        line,
    })
}

/// "1 shortcut", "3 shortcuts".
fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn result_card(r: &Row) -> Option<Card> {
    let out = r.fixed?;
    let head = match r.kind {
        Kind::Privacy | Kind::Junk => format!("{} freed", size(out.removed.bytes)),
        _ => format!("{} removed", out.removed.files),
    };
    Some(Card {
        icon: Icon::Done,
        head,
        color: sc::ARC,
        line: result_line(r.name, r.kind, out),
    })
}

/// "Junk files: 17,000 files removed. 1,092 were in use or need admin rights and were left."
fn result_line(module: &str, kind: Kind, c: bosun_core::clean::Cleaned) -> String {
    let n = c.removed.files;
    let mut s = match kind {
        Kind::Shortcuts => format!(
            "{module}: {} moved to the Recycle Bin.",
            plural(n, "shortcut", "shortcuts")
        ),
        Kind::Registry => format!(
            "{module}: {} removed (backup saved).",
            plural(n, "entry", "entries")
        ),
        _ => format!("{module}: {} removed.", plural(n, "file", "files")),
    };
    if c.skipped.files > 0 {
        s += &format!(
            " {} were in use or need administrator rights and were left.",
            c.skipped.files
        );
    }
    s
}

/// "15390 files can be cleaned to free up 1.3 GB of disk space." (reference),
/// named by module so two cards don't read the same.
fn card_line(module: &str, t: Tally) -> String {
    format!(
        "{}: {} files can be cleaned to free up {} of disk space.",
        module,
        t.files,
        size(t.bytes)
    )
}

fn draw_buttons(p: &Painter, v: &View, finished: bool) {
    let [l, r] = buttons(v);
    let (lt, rt, lh, rh) = if finished {
        ("Back", "Fix", Hit::ScanBack, Hit::ScanFix)
    } else {
        ("Skip", "Stop", Hit::ScanSkip, Hit::ScanStop)
    };
    pill(p, l, lt, (0xCFCFCF, 0x9C9C9C), v.hover == lh);
    pill(p, r, rt, (0x8DC3F8, 0x6DB4F7), v.hover == rh);
}

fn pill(p: &Painter, r: D2D_RECT_F, label: &str, (top, bottom): (u32, u32), hover: bool) {
    let x = (r.left + r.right) / 2.0;
    let rad = BTN_H / 2.0;
    p.fill_rr_linear(
        r,
        rad,
        (x, r.top),
        (x, r.bottom),
        &[(0.0, col(top)), (1.0, col(bottom))],
    );
    if hover {
        p.fill_rr(r, rad, cola(0xFFFFFF, 0.14));
    }
    // light top edge
    p.stroke_rr_linear(
        rect(
            r.left + 0.5,
            r.top + 0.5,
            r.right - r.left - 1.0,
            BTN_H - 1.0,
        ),
        rad,
        1.0,
        (x, r.top),
        (x, r.bottom),
        &[(0.0, cola(0xFFFFFF, 0.45)), (0.5, cola(0xFFFFFF, 0.0))],
    );
    p.text(label, &p.g.f.m14sb, r, col(0x141414), Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_line_mentions_left_files_only_when_there_are_some() {
        use bosun_core::clean::Cleaned;
        let t = |files, bytes| Tally { files, bytes };
        let all = Cleaned {
            removed: t(3, 9),
            skipped: t(0, 0),
        };
        assert_eq!(
            result_line("Junk files", Kind::Junk, all),
            "Junk files: 3 files removed."
        );
        let some = Cleaned {
            removed: t(3, 9),
            skipped: t(2, 4),
        };
        assert!(
            result_line("Junk files", Kind::Junk, some)
                .ends_with("2 were in use or need administrator rights and were left.")
        );
    }

    #[test]
    fn plural_counts() {
        assert_eq!(plural(1, "shortcut", "shortcuts"), "1 shortcut");
        assert_eq!(plural(10, "entry", "entries"), "10 entries");
    }

    #[test]
    fn size_matches_reference_style() {
        assert_eq!(size(1_395_864_371), "1.3 GB");
        assert_eq!(size(536_870_912), "512 MB");
        assert_eq!(size(40 * 1024), "40 KB");
    }

    #[test]
    fn buttons_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        let [l, r] = buttons(&v);
        assert_eq!(
            (l.left, l.top, l.right, l.bottom),
            (484.0, 528.0, 638.0, 562.0)
        );
        assert_eq!((r.left, r.right), (648.0, 802.0));
        assert_eq!(content_cx(&v), 643.0);
    }
}
