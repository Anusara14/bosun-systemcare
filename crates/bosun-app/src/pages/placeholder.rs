//! Pages not built yet: an honest "coming next" card instead of fake content.

use bosun_ui::gfx::{Align, Painter, col, rect};

use crate::chrome::{SIDEBAR_W, TITLE_H};
use crate::pages::Page;
use crate::window::View;

fn blurb(page: Page) -> (&'static str, &'static str) {
    match page {
        Page::SpeedUp => (
            "Speed Up",
            "Turbo Boost, Startup Optimizer, driver updates and app cleanup. Arriving in Phase 3 and 5.",
        ),
        Page::Protect => (
            "Protect",
            "Windows Defender and Firewall status, plus Homepage Advisor. Arriving in Phase 5.",
        ),
        Page::Updater => (
            "Software Updater",
            "Outdated apps and one-click updates via winget. Arriving in Phase 5.",
        ),
        Page::About | Page::Care => ("Bosun SystemCare", "Lightweight PC care, built in Rust."),
    }
}

pub fn draw(p: &Painter, v: &View, page: Page) {
    let f = &p.g.f;
    let x = SIDEBAR_W + 24.0;
    let w = v.w - x - 24.0;
    let card = rect(x, TITLE_H + 28.0, w, 140.0);
    p.fill_rr(card, 8.0, col(0x1E1E1E));
    p.stroke_rr(card, 8.0, 1.0, col(0x2A2A2A));
    let (title, text) = blurb(page);
    p.text(
        title,
        &f.h1,
        rect(x + 24.0, card.top + 22.0, w - 48.0, 30.0),
        col(0xFFFFFF),
        Align::Left,
    );
    p.text(
        text,
        &f.m13,
        rect(x + 24.0, card.top + 60.0, w - 48.0, 22.0),
        col(0xB0B0B0),
        Align::Left,
    );
    if page == Page::About {
        p.logo(x + w - 88.0, card.top + 38.0, 64.0);
        let ver = format!("Version {}", env!("CARGO_PKG_VERSION"));
        p.text(
            &ver,
            &f.m12,
            rect(x + 24.0, card.top + 92.0, w - 48.0, 20.0),
            col(0x8C8C8C),
            Align::Left,
        );
    }
}
