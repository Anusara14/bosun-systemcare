//! The bell's drop-down: notifications Bosun has shown (homepage changes,
//! Auto Clean results), newest first, each opening the page it is about.
//! A 360 px card under the bell, like the system's own notification flyout.

use bosun_core::notices::Notice;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::speedup::text_at;
use crate::window::{Hit, View};

const W: f32 = 360.0;
const HEAD: f32 = 48.0;
const ITEM: f32 = 66.0;
/// Shown at once (the log keeps more).
pub const SHOWN: usize = 5;

fn panel(v: &View, n: usize) -> D2D_RECT_F {
    let bell = v.w - 201.0;
    let body = if n == 0 {
        96.0
    } else {
        ITEM * n.min(SHOWN) as f32 + 8.0
    };
    rect((bell + 60.0 - W).max(8.0), 40.0, W, HEAD + body)
}

fn item(v: &View, n: usize, k: usize) -> D2D_RECT_F {
    let p = panel(v, n);
    rect(
        p.left + 6.0,
        p.top + HEAD + 4.0 + ITEM * k as f32,
        W - 12.0,
        ITEM,
    )
}

fn clear_link(v: &View, n: usize) -> D2D_RECT_F {
    let p = panel(v, n);
    rect(p.right - 18.0 - 60.0, p.top + 14.0, 60.0, 20.0)
}

pub fn hit(v: &View, list: &[Notice], x: f32, y: f32) -> Hit {
    let n = list.len();
    if !contains(&panel(v, n), x, y) {
        return Hit::NoticeOutside;
    }
    if n > 0 && contains(&clear_link(v, n), x, y) {
        return Hit::NoticeClear;
    }
    (0..n.min(SHOWN))
        .find(|&k| contains(&item(v, n, k), x, y))
        .map_or(Hit::None, |k| Hit::NoticeItem(k as u8))
}

/// "just now", "5 min ago", "3 h ago", else the date.
fn ago(at_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    match now.saturating_sub(at_ms) / 1000 {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        _ => crate::pages::rescue::when(at_ms),
    }
}

/// `text` broken into at most two lines that fit `width` (the second ends
/// with "…" if it still runs over).
fn two_lines(p: &Painter, text: &str, width: f32) -> Vec<String> {
    let f = &p.g.f.m12;
    let mut lines: Vec<String> = vec![String::new()];
    for word in text.split_whitespace() {
        let n = lines.len();
        let last = lines.last_mut().expect("never empty");
        let tried = if last.is_empty() {
            word.to_string()
        } else {
            format!("{last} {word}")
        };
        if p.g.measure(&tried, f) <= width || last.is_empty() {
            *last = tried;
        } else if n == 2 {
            last.push('…');
            break;
        } else {
            lines.push(word.to_string());
        }
    }
    lines
}

pub fn draw(p: &Painter, v: &View, list: &[Notice]) {
    let f = &p.g.f;
    let n = list.len();
    let r = panel(v, n);
    p.untinted(|| {
        for (grow, a) in [(6.0, 0.10), (2.0, 0.18)] {
            let s = rect(
                r.left - grow,
                r.top - grow + 3.0,
                W + 2.0 * grow,
                r.bottom - r.top + 2.0 * grow,
            );
            p.fill_rr(s, 8.0 + grow, cola(0x000000, a));
        }
    });
    p.fill_rr(r, 8.0, col(0x232323));
    p.stroke_rr(r, 8.0, 1.0, col(0x333333));
    text_at(
        p,
        "Notifications",
        &f.m14sb,
        15.0,
        r.left + 18.0,
        200.0,
        r.top + 24.0,
        0xF2F2F2,
    );
    if n > 0 {
        let c = if v.hover == Hit::NoticeClear {
            0x8CC4FF
        } else {
            0x61ADF8
        };
        let w = p.g.measure("Clear all", &f.m13);
        let x = r.right - 18.0 - w;
        text_at(p, "Clear all", &f.m13, 13.0, x, w + 4.0, r.top + 24.0, c);
    }
    p.line(
        r.left + 1.0,
        r.top + HEAD - 0.5,
        r.right - 1.0,
        r.top + HEAD - 0.5,
        1.0,
        col(0x2E2E2E),
    );
    if n == 0 {
        let cy = r.top + HEAD + 48.0;
        p.text(
            "You're all caught up.",
            &f.m13,
            rect(r.left, cy - 10.0, W, 20.0),
            col(0x8A8A8A),
            Align::Center,
        );
        return;
    }
    for (k, note) in list.iter().take(SHOWN).enumerate() {
        let it = item(v, n, k);
        if v.hover == Hit::NoticeItem(k as u8) {
            p.fill_rr(it, 6.0, col(0x2C2C2C));
        }
        let (ix, iy) = (it.left + 24.0, it.top + 22.0);
        p.ring(ix, iy, 10.0, 1.6, col(0x61ADF8));
        p.circle(ix, iy - 4.2, 1.2, col(0x61ADF8));
        p.line(ix, iy - 1.2, ix, iy + 4.8, 1.8, col(0x61ADF8));
        let when = ago(note.at);
        let ww = p.g.measure(&when, &f.m12);
        text_at(
            p,
            &when,
            &f.m12,
            12.0,
            it.right - 12.0 - ww,
            ww + 4.0,
            it.top + 20.0,
            0x8A8A8A,
        );
        let tw = it.right - 12.0 - ww - 10.0 - (it.left + 48.0);
        text_at(
            p,
            &note.title,
            &f.m13sb,
            13.0,
            it.left + 48.0,
            tw,
            it.top + 20.0,
            0xF2F2F2,
        );
        for (l, line) in two_lines(p, &note.text, W - 72.0).iter().enumerate() {
            text_at(
                p,
                line,
                &f.m12,
                12.0,
                it.left + 48.0,
                W - 66.0,
                it.top + 39.0 + 15.0 * l as f32,
                0xB0B0B0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_hangs_under_the_bell() {
        let v = crate::window::test_view();
        let list = vec![Notice {
            at: 0,
            page: "rescue".into(),
            title: "Auto Clean finished".into(),
            text: "Deleted 3 files".into(),
        }];
        let r = panel(&v, 1);
        assert!(r.left < v.w - 201.0 && r.right > v.w - 201.0 && r.top == 40.0);
        assert_eq!(hit(&v, &list, 100.0, 300.0), Hit::NoticeOutside);
        let it = item(&v, 1, 0);
        assert_eq!(
            hit(&v, &list, it.left + 20.0, it.top + 20.0),
            Hit::NoticeItem(0)
        );
        let c = clear_link(&v, 1);
        assert_eq!(hit(&v, &list, c.left + 5.0, c.top + 5.0), Hit::NoticeClear);
    }
}
