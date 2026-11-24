//! Shared drawing layer for Bosun SystemCare: Direct2D/DirectWrite helpers,
//! design tokens, vector icons (logo, rocket) and per-pixel-alpha surfaces.
//! Used by both the tray process and the main window.

pub mod gfx;
pub mod layered;
pub mod snapshot;

use std::cell::Cell;

use gfx::Gfx;

thread_local! {
    static GFX: Cell<Option<&'static Gfx>> = const { Cell::new(None) };
}

/// Creates the factories and fonts for this (UI) thread. Call once, after COM init.
pub fn init() -> windows::core::Result<()> {
    if GFX.with(Cell::get).is_none() {
        let g: &'static Gfx = Box::leak(Box::new(Gfx::new()?));
        GFX.with(|c| c.set(Some(g)));
    }
    Ok(())
}

/// Factories and fonts created by [`init`].
pub fn graphics() -> &'static Gfx {
    GFX.with(Cell::get)
        .expect("bosun_ui::init must run before drawing")
}
