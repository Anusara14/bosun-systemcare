//! Protect page, measured from the reference (`.reanimate/phase5`, scaled
//! from 150 %): the Speed Up banner, then 2 × 2 cards of 375 × 245 from
//! (175, 132), 11 px apart. In a card: 41 px icon at +29, +32; title at +95
//! (cap centre +35.5, 16 px); two 13 px lines at +53.5 and +71; a divider at
//! +96.5; rows 24 px apart from +119.5 with the control centred 35.5 px in
//! from the right.
//!
//! The reference's paid features (Safe Folder, Anti-Tracking, VPN, ad
//! filtering) are replaced by Windows' own protections, read honestly.

use bosun_core::extensions::Browser;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::speedup::{badge, pill, text_at};
use crate::window::{Hit, View};

const CARD_X: f32 = 175.0;
const CARD_Y: f32 = 132.0;
const CARD_W: f32 = 375.0;
const CARD_H: f32 = 245.0;
const GAP: f32 = 11.0;

/// Everything the page shows, gathered when it opens.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ProtectInfo {
    pub antivirus: Option<bool>,
    pub firewall: Option<bool>,
    pub uac: bool,
    /// Privacy switches (see `bosun_core::privacy::SWITCHES`): protected?
    pub privacy: [bool; 4],
    pub browser: Option<Browser>,
    pub homepage: String,
    pub search: String,
    pub advisor: bool,
    pub changed: bool,
}

impl ProtectInfo {
    pub fn load() -> Self {
        use bosun_core::{homepage, privacy, security};
        let browser = homepage::default_browser();
        let s = browser
            .and_then(|b| homepage::current().into_iter().find(|(x, _)| *x == b))
            .map(|(_, s)| s)
            .unwrap_or_default();
        let advisor = homepage::advisor_on();
        ProtectInfo {
            antivirus: security::healthy(security::Provider::Antivirus),
            firewall: security::healthy(security::Provider::Firewall),
            uac: privacy::uac_on(),
            privacy: [0, 1, 2, 3].map(privacy::protected),
            browser,
            homepage: if s.homepage.is_empty() {
                s.startup.first().cloned().unwrap_or_default()
            } else {
                s.homepage
            },
            search: s.search,
            advisor,
            changed: advisor && homepage::changed(),
        }
    }

    /// The first thing that needs attention, for the banner.
    fn issue(&self) -> Option<&'static str> {
        if self.antivirus == Some(false) {
            Some("Antivirus protection is off. Turn it on in Windows Security.")
        } else if self.firewall == Some(false) {
            Some("The firewall is off. Turn it on in Windows Security.")
        } else if !self.uac {
            Some("User Account Control is off.")
        } else if self.changed {
            Some("Your browser's homepage or search engine has changed.")
        } else if !self.advisor {
            Some("Turn on Homepage Advisor to catch browser hijacks.")
        } else {
            None
        }
    }
}

fn card(v: &View, i: usize) -> D2D_RECT_F {
    let w = ((v.w - CARD_X - 24.0 - GAP) / 2.0).max(CARD_W);
    let (c, r) = (i % 2, i / 2);
    let h = ((v.h - CARD_Y - 27.0 - GAP) / 2.0).max(CARD_H);
    rect(
        CARD_X + c as f32 * (w + GAP),
        CARD_Y + r as f32 * (h + GAP),
        w,
        h,
    )
}

fn row_y(c: &D2D_RECT_F, k: usize) -> f32 {
    c.top + 119.5 + 24.0 * k as f32
}

fn toggle_rect(c: &D2D_RECT_F, k: usize) -> D2D_RECT_F {
    let cy = row_y(c, k);
    rect(c.right - 35.5 - 15.5, cy - 7.0, 31.0, 14.0)
}

fn security_button(c: &D2D_RECT_F) -> D2D_RECT_F {
    rect(c.left + 85.0, c.top + 157.0, 261.0, 29.0)
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    let privacy = card(v, 1);
    for k in 0..4 {
        let t = toggle_rect(&privacy, k);
        if contains(
            &rect(t.left - 6.0, t.top - 5.0, t.right - t.left + 12.0, 24.0),
            x,
            y,
        ) {
            return Hit::Privacy(k as u8);
        }
    }
    let browser = card(v, 2);
    let t = toggle_rect(&browser, 3);
    if contains(
        &rect(t.left - 6.0, t.top - 5.0, t.right - t.left + 12.0, 24.0),
        x,
        y,
    ) {
        return Hit::Advisor;
    }
    if contains(&security_button(&card(v, 3)), x, y) {
        return Hit::OpenSecurity;
    }
    Hit::None
}

/// The orange "!" ring (attention) or blue tick disc (all good) banner.
pub fn banner(p: &Painter, ok: bool, title: &str, count: Option<&str>, line: &str) {
    let f = &p.g.f;
    let (bx, by) = (197.5, 82.5);
    if ok {
        p.circle(bx, by, 24.0, col(0x61ADF8));
        let c = col(0x0A0A0A);
        p.line(bx - 10.0, by + 1.0, bx - 3.0, by + 8.0, 3.0, c);
        p.line(bx - 3.0, by + 8.0, bx + 11.0, by - 7.0, 3.0, c);
    } else {
        p.ring(bx, by, 23.0, 2.4, col(0xF5A56A));
        p.circle(bx, by - 9.0, 2.0, col(0xF5A56A));
        p.line(bx, by - 3.0, bx, by + 10.0, 3.0, col(0xF5A56A));
    }
    let mut x = 240.0;
    if let Some(n) = count {
        let w = p.g.measure(n, &f.h1);
        text_at(p, n, &f.h1, 20.0, x, w + 4.0, 71.5, 0xF5A56A);
        x += w + 5.0;
    }
    text_at(p, title, &f.h1, 20.0, x, 640.0, 71.5, 0xF2F2F2);
    text_at(p, line, &f.m13, 13.0, 241.0, 600.0, 96.0, 0xB8B8B8);
}

pub(crate) fn toggle(p: &Painter, r: D2D_RECT_F, on: bool, hover: bool) {
    let light = bosun_ui::gfx::is_light();
    // exact colours in both skins: the knob is always white
    p.untinted(|| {
        if on {
            p.fill_rr(r, 7.0, col(0x61ADF8));
            p.circle(r.right - 7.0, r.top + 7.0, 4.5, col(0xFFFFFF));
        } else {
            p.fill_rr(r, 7.0, col(if light { 0xC2C2C2 } else { 0x3A3A3A }));
            p.circle(
                r.left + 7.0,
                r.top + 7.0,
                4.5,
                col(if light { 0xFFFFFF } else { 0xE6E6E6 }),
            );
        }
    });
    if hover {
        p.stroke_rr(r, 7.0, 1.0, cola(0xFFFFFF, 0.35));
    }
}

/// A status mark: blue shield tick when good, orange "!" when not, a dash
/// when unknown.
fn status(p: &Painter, cx: f32, cy: f32, ok: Option<bool>) {
    match ok {
        Some(ok) => badge(p, cx, cy, ok),
        None => p.line(cx - 6.0, cy, cx + 6.0, cy, 2.0, col(0x5A5A5A)),
    }
}

pub fn draw(p: &Painter, v: &View) {
    let f = &p.g.f;
    let s = &v.protect;
    match s.issue() {
        Some(line) => banner(p, false, "Your PC is not fully protected.", None, line),
        None => banner(
            p,
            true,
            "Your PC is protected.",
            None,
            "Windows Security and Homepage Advisor are on.",
        ),
    }
    let cards: [(&str, [&str; 2]); 4] = [
        (
            "System Protection",
            [
                "Ensure your system is protected with all",
                "security protections on.",
            ],
        ),
        (
            "Privacy Protection",
            [
                "Turn off Windows features that track you",
                "or share your data for ads.",
            ],
        ),
        (
            "Browser Protection",
            [
                "Watch your browser's homepage and search",
                "engine for unwanted changes.",
            ],
        ),
        (
            "Windows Security",
            [
                "Your antivirus, firewall and account",
                "protection, managed by Windows.",
            ],
        ),
    ];
    for (i, (title, lines)) in cards.iter().enumerate() {
        let c = card(v, i);
        p.fill_rr(c, 6.0, col(0x1C1C1C));
        icon(p, i, c.left + 49.5, c.top + 52.5, s);
        let tx = c.left + 95.0;
        text_at(p, title, &f.m14sb, 16.0, tx, 300.0, c.top + 35.5, 0xF2F2F2);
        for (k, l) in lines.iter().enumerate() {
            text_at(
                p,
                l,
                &f.m13,
                13.0,
                tx,
                c.right - tx - 16.0,
                c.top + 53.5 + 17.5 * k as f32,
                0xB8B8B8,
            );
        }
        if i < 3 {
            p.line(
                tx,
                c.top + 96.5,
                c.right - 20.0,
                c.top + 96.5,
                1.0,
                col(0x2C2C2C),
            );
        }
        let rx = c.right - 35.5;
        let label =
            |k: usize, t: &str| text_at(p, t, &f.m13, 13.0, tx, 200.0, row_y(&c, k), 0xD0D0D0);
        match i {
            0 => {
                for (k, (t, ok)) in [
                    ("Antivirus", s.antivirus),
                    ("Firewall", s.firewall),
                    ("User Account Control", Some(s.uac)),
                ]
                .into_iter()
                .enumerate()
                {
                    label(k, t);
                    status(p, rx, row_y(&c, k), ok);
                }
            }
            1 => {
                for (k, (t, ..)) in bosun_core::privacy::SWITCHES.iter().enumerate() {
                    label(k, t);
                    toggle(
                        p,
                        toggle_rect(&c, k),
                        s.privacy[k],
                        v.hover == Hit::Privacy(k as u8),
                    );
                }
            }
            2 => {
                let browser = s.browser.map_or("Unknown", |b| b.label());
                let rows = [
                    ("Default Browser", browser.to_string()),
                    ("Homepage", s.homepage.clone()),
                    ("Search Engine", s.search.clone()),
                ];
                for (k, (t, val)) in rows.iter().enumerate() {
                    label(k, t);
                    let val = if val.is_empty() { "—" } else { val.as_str() };
                    let y = row_y(&c, k) - 0.064 * 13.0;
                    p.text(
                        val,
                        &f.m13,
                        rect(tx + 120.0, y - 13.0, c.right - 20.0 - tx - 120.0, 26.0),
                        col(0x9A9A9A),
                        Align::Right,
                    );
                }
                label(3, "Homepage Advisor");
                if s.changed {
                    let y = row_y(&c, 3) - 0.064 * 13.0;
                    p.text(
                        "Changed",
                        &f.m13,
                        rect(rx - 120.0, y - 13.0, 60.0, 26.0),
                        col(0xF5A56A),
                        Align::Right,
                    );
                }
                toggle(p, toggle_rect(&c, 3), s.advisor, v.hover == Hit::Advisor);
            }
            _ => {
                let ok = s.antivirus != Some(false) && s.firewall != Some(false) && s.uac;
                let msg = if ok {
                    "Everything Windows Security checks is on."
                } else {
                    "Windows Security needs your attention."
                };
                let c2 = if ok { 0xB8B8B8 } else { 0xF5A56A };
                text_at(p, msg, &f.m13, 13.0, tx, 300.0, c.top + 118.0, c2);
                pill(
                    p,
                    security_button(&c),
                    "Open Windows Security",
                    v.hover == Hit::OpenSecurity,
                );
            }
        }
    }
}

/// Line icons in the Speed Up style: monitor, document, browser window,
/// shield; badges follow state.
fn icon(p: &Painter, i: usize, cx: f32, cy: f32, s: &ProtectInfo) {
    let c = col(0xD8D8DC);
    let w = 1.9;
    match i {
        0 => {
            p.stroke_rr(rect(cx - 19.0, cy - 15.0, 34.0, 23.0), 2.5, w, c);
            p.line(cx - 7.0, cy + 15.0, cx + 4.0, cy + 15.0, w, c);
            p.line(cx - 1.5, cy + 8.0, cx - 1.5, cy + 15.0, w, c);
            let ok = s.antivirus != Some(false) && s.firewall != Some(false) && s.uac;
            badge(p, cx + 15.0, cy + 6.0, ok);
        }
        1 => {
            p.stroke_rr(rect(cx - 14.0, cy - 18.0, 26.0, 33.0), 2.5, w, c);
            for k in 0..3 {
                let y = cy - 9.0 + 7.0 * k as f32;
                p.line(cx - 8.0, y, cx + 4.0, y, 1.8, col(0x787880));
            }
            badge(p, cx + 12.0, cy + 12.0, s.privacy.iter().all(|x| *x));
        }
        2 => {
            p.stroke_rr(rect(cx - 19.0, cy - 15.0, 36.0, 28.0), 3.0, w, c);
            p.line(cx - 19.0, cy - 7.0, cx + 17.0, cy - 7.0, w, c);
            for k in 0..3 {
                p.circle(cx - 14.5 + 4.5 * k as f32, cy - 11.0, 1.1, c);
            }
            badge(p, cx + 14.0, cy + 10.0, s.advisor && !s.changed);
        }
        _ => {
            p.line(cx, cy - 18.0, cx - 14.0, cy - 12.0, w, c);
            p.line(cx, cy - 18.0, cx + 14.0, cy - 12.0, w, c);
            p.line(cx - 14.0, cy - 12.0, cx - 12.0, cy + 4.0, w, c);
            p.line(cx + 14.0, cy - 12.0, cx + 12.0, cy + 4.0, w, c);
            p.line(cx - 12.0, cy + 4.0, cx, cy + 16.0, w, c);
            p.line(cx + 12.0, cy + 4.0, cx, cy + 16.0, w, c);
            p.line(cx - 6.0, cy - 1.0, cx - 1.5, cy + 4.0, w, c);
            p.line(cx - 1.5, cy + 4.0, cx + 7.0, cy - 6.0, w, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cards_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        let c = card(&v, 0);
        assert_eq!(
            (c.left, c.top, c.right, c.bottom),
            (175.0, 132.0, 550.0, 377.0)
        );
        let c = card(&v, 3);
        assert_eq!((c.left, c.top), (561.0, 388.0));
        let t = toggle_rect(&card(&v, 1), 0);
        assert_eq!(hit(&v, t.left + 10.0, t.top + 7.0), Hit::Privacy(0));
    }
}
