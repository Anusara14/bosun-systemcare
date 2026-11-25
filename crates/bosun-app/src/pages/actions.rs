//! The bell's panel, measured from the reference's "activate all functions"
//! window (`.reanimate/phase6`, scaled from 150 %): a one-line header (cap
//! centre 30, x 17), section headings (cap 85.3 and 325.3, icon at 58.7, text
//! at 76), two columns of 418 px cards from x 50.7 and 489.3 (tops 105.3 and
//! 345.3), items 95 px apart (title at +24.7, two lines at +48.7 / +66, a
//! divider at +94), switches 19.5 px in from the card's right, and a footer
//! rule at 589.3 over a 202 × 33 button centred at y 620.
//!
//! IObit's list sells paid features. Ours are Bosun's own switches.

use bosun_ui::gfx::{Painter, col, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::protect::toggle;
use crate::pages::speedup::{pill, text_at};
use crate::window::{Hit, View};

/// The switches the panel shows, read when it opens.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Actions {
    pub on: [bool; 7],
}

/// (title, two lines). Index = switch id.
const ITEMS: [(&str, [&str; 2]); 7] = [
    (
        "Homepage Advisor",
        [
            "Warn when your browser's homepage or search",
            "engine changes behind your back.",
        ],
    ),
    (
        "Privacy Protection",
        [
            "Turn off Windows' advertising ID, tailored",
            "experiences and app launch tracking.",
        ],
    ),
    (
        "Auto Fix",
        [
            "Fix what a Care scan finds right away, without",
            "waiting on the summary.",
        ],
    ),
    (
        "Performance Monitor",
        [
            "Watch RAM and CPU use from a widget on the",
            "desktop. Click it for the full monitor.",
        ],
    ),
    (
        "Auto RAM Clean",
        [
            "Trim memory apps aren't using whenever RAM",
            "use passes 85%.",
        ],
    ),
    (
        "Start with Windows",
        [
            "Load the Bosun tray at Windows startup so the",
            "widget and Auto RAM Clean keep working.",
        ],
    ),
    (
        "Auto Clean",
        [
            "Clean junk files once a day in the background.",
            "Every run is listed in the Rescue Center.",
        ],
    ),
];

/// (heading, switch ids), left column then right, two rows.
const SECTIONS: [(&str, &[usize]); 4] = [
    ("Security", &[0, 1]),
    ("Clean", &[2]),
    ("Performance", &[3, 4]),
    ("Automatic", &[5, 6]),
];

impl Actions {
    pub fn load(auto_fix: bool) -> Self {
        let m = crate::pages::menu::MenuInfo::load();
        let privacy = (0..bosun_core::privacy::SWITCHES.len()).all(bosun_core::privacy::protected);
        Actions {
            on: [
                m.advisor,
                privacy,
                auto_fix,
                m.widget,
                m.auto_ram,
                m.tray_start,
                crate::tray_ini::get("auto_clean", false),
            ],
        }
    }
}

/// Sets switch `k`; Auto Fix (2) lives in the window, so the caller handles it.
pub fn set(k: usize, on: bool) -> Result<(), &'static str> {
    let m = crate::pages::menu::MenuInfo {
        widget: !on,
        tray_start: !on,
        auto_ram: !on,
        advisor: !on,
        auto_update: false,
    };
    match k {
        0 => crate::pages::menu::flip(&m, 3),
        1 => {
            let dir = bosun_core::clean::journal_dir().ok_or("No place to write the journal.")?;
            (0..bosun_core::privacy::SWITCHES.len())
                .try_for_each(|i| bosun_core::privacy::set(i, on, &dir))
                .map_err(|_| "Couldn't change the privacy settings.")
        }
        2 => crate::tray_ini::set_to(crate::tray_ini::APP, "auto_fix", on)
            .map_err(|_| "Couldn't save Auto Fix."),
        3 => crate::pages::menu::flip(&m, 0),
        4 => crate::pages::menu::flip(&m, 2),
        5 => crate::pages::menu::flip(&m, 1),
        _ => crate::tray_ini::set("auto_clean", on).map_err(|_| "Couldn't save Auto Clean."),
    }
}

fn card(s: usize) -> D2D_RECT_F {
    let x = if s.is_multiple_of(2) { 50.7 } else { 489.3 };
    let y = if s < 2 { 105.3 } else { 345.3 };
    rect(x, y, 418.0, 95.0 * SECTIONS[s].1.len() as f32 - 2.0)
}

fn switch(s: usize, i: usize) -> D2D_RECT_F {
    let c = card(s);
    rect(
        c.right - 19.5 - 31.0,
        c.top + 24.7 + 95.0 * i as f32 - 7.0,
        31.0,
        14.0,
    )
}

fn all_button(v: &View) -> D2D_RECT_F {
    rect(v.w / 2.0 - 101.0, 603.3 + (v.h - 660.0), 202.0, 33.0)
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    if y < 50.0 {
        if contains(&rect(v.w - 49.0, 12.0, 38.0, 36.0), x, y) {
            return Hit::BellClose;
        }
        if contains(&rect(v.w - 83.0, 12.0, 32.0, 36.0), x, y) {
            return Hit::Min;
        }
        return Hit::None;
    }
    if contains(&all_button(v), x, y) {
        return Hit::BellAll;
    }
    for (s, (_, ids)) in SECTIONS.iter().enumerate() {
        for (i, id) in ids.iter().enumerate() {
            let w = switch(s, i);
            // the item's title line toggles too
            let row = rect(
                card(s).left,
                w.top - 10.0,
                w.right - card(s).left + 8.0,
                34.0,
            );
            if contains(&row, x, y) {
                return Hit::BellToggle(*id as u8);
            }
        }
    }
    Hit::None
}

/// 16 px section glyphs: shield, broom, chart, circular arrows.
fn section_icon(p: &Painter, s: usize, cx: f32, cy: f32) {
    let c = col(0xC8C8C8);
    match s {
        0 => {
            p.fill_poly(
                &[
                    (cx - 7.0, cy - 6.0),
                    (cx, cy - 8.5),
                    (cx + 7.0, cy - 6.0),
                    (cx + 6.0, cy + 2.0),
                    (cx, cy + 8.0),
                    (cx - 6.0, cy + 2.0),
                ],
                c,
            );
            let k = col(0x161616);
            p.line(cx - 3.2, cy - 0.5, cx - 0.8, cy + 2.0, 1.6, k);
            p.line(cx - 0.8, cy + 2.0, cx + 3.6, cy - 2.6, 1.6, k);
        }
        1 => {
            p.line(cx, cy - 8.5, cx, cy - 2.0, 2.0, c);
            p.fill_poly(
                &[
                    (cx - 3.5, cy - 2.0),
                    (cx + 3.5, cy - 2.0),
                    (cx + 6.0, cy + 8.0),
                    (cx - 6.0, cy + 8.0),
                ],
                c,
            );
        }
        2 => {
            p.stroke_rr(rect(cx - 8.0, cy - 7.0, 16.0, 12.0), 1.0, 1.4, c);
            p.line(cx - 4.0, cy + 8.0, cx + 4.0, cy + 8.0, 1.4, c);
            let pts = [(-5.5, 1.5), (-2.5, -1.5), (0.0, 0.5), (4.5, -4.0)];
            for w in pts.windows(2) {
                p.line(cx + w[0].0, cy + w[0].1, cx + w[1].0, cy + w[1].1, 1.4, c);
            }
        }
        _ => {
            p.arc_at(cx, cy, 7.0, 1.8, -0.45, 0.38, c);
            p.arc_at(cx, cy, 7.0, 1.8, 0.05, 0.38, c);
            p.line(cx + 7.0, cy - 5.0, cx + 7.0, cy - 0.5, 1.8, c);
            p.line(cx - 7.0, cy + 5.0, cx - 7.0, cy + 0.5, 1.8, c);
        }
    }
}

pub fn draw(p: &Painter, v: &View, a: &Actions) {
    let f = &p.g.f;
    p.fill(rect(0.0, 0.0, v.w, v.h), col(0x161616));
    text_at(
        p,
        "Turn on the features below to keep your PC fast, tidy and protected.",
        &f.body,
        14.0,
        17.3,
        800.0,
        30.0,
        0xB8B8B8,
    );
    let (xx, xy) = (v.w - 31.3, 30.0);
    let xc = col(if v.hover == Hit::BellClose {
        0xFFFFFF
    } else {
        0xB0B0B0
    });
    p.line(xx - 5.5, xy - 5.5, xx + 5.5, xy + 5.5, 1.2, xc);
    p.line(xx + 5.5, xy - 5.5, xx - 5.5, xy + 5.5, 1.2, xc);
    let mc = col(if v.hover == Hit::Min {
        0xFFFFFF
    } else {
        0xB0B0B0
    });
    p.line(v.w - 72.5, xy, v.w - 59.5, xy, 1.2, mc);
    for (s, (head, ids)) in SECTIONS.iter().enumerate() {
        let c = card(s);
        let hy = if s < 2 { 85.3 } else { 325.3 };
        section_icon(p, s, c.left + 8.0, hy);
        text_at(p, head, &f.body, 15.0, c.left + 25.3, 300.0, hy, 0xD8D8D8);
        p.fill_rr(c, 6.0, col(0x1E1E1E));
        for (i, &id) in ids.iter().enumerate() {
            let top = c.top + 95.0 * i as f32;
            if i > 0 {
                p.line(
                    c.left + 18.6,
                    top - 1.0,
                    c.right - 18.6,
                    top - 1.0,
                    1.0,
                    col(0x2C2C2C),
                );
            }
            let (title, lines) = ITEMS[id];
            text_at(
                p,
                title,
                &f.title,
                16.0,
                c.left + 18.6,
                300.0,
                top + 24.7,
                0xF2F2F2,
            );
            for (k, l) in lines.iter().enumerate() {
                text_at(
                    p,
                    l,
                    &f.small,
                    13.0,
                    c.left + 18.6,
                    360.0,
                    top + 48.7 + 17.3 * k as f32,
                    0xB0B0B0,
                );
            }
            toggle(
                p,
                switch(s, i),
                a.on[id],
                v.hover == Hit::BellToggle(id as u8),
            );
        }
    }
    let fy = 589.3 + (v.h - 660.0);
    p.line(14.0, fy, v.w - 14.0, fy, 1.0, col(0x2C2C2C));
    let label = if a.on.iter().all(|&o| o) {
        "Everything is on"
    } else {
        "Turn everything on"
    };
    pill(p, all_button(v), label, v.hover == Hit::BellAll);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_switches_and_button_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        assert_eq!(
            (card(0).left, card(0).top, card(1).left),
            (50.7, 105.3, 489.3)
        );
        let s = switch(0, 0);
        assert_eq!((s.right, s.top + 7.0), (449.2, 130.0));
        assert_eq!(hit(&v, 440.0, 130.0), Hit::BellToggle(0));
        assert_eq!(hit(&v, 440.0, 225.0), Hit::BellToggle(1));
        assert_eq!(hit(&v, 880.0, 370.0), Hit::BellToggle(5));
        assert_eq!(hit(&v, 480.0, 620.0), Hit::BellAll);
        assert_eq!(hit(&v, 928.0, 30.0), Hit::BellClose);
    }
}
