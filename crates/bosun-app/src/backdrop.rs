//! Window backdrop: flat charcoal, matching the reference's flat background.
//! `overlay` is a cached static layer drawn above the base (empty for now).

use bosun_ui::gfx::Painter;

/// Charcoal base everything sits on.
pub const BASE: u32 = 0x151515;

/// Static layer over the base; nothing in this design.
pub fn overlay(_p: &Painter, _w: f32, _h: f32) {}
