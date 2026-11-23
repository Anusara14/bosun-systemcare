//! Pages shown in the content area to the right of the sidebar.

pub mod care;
mod placeholder;

use bosun_ui::gfx::Painter;

use crate::window::{Hit, View};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Care,
    SpeedUp,
    Protect,
    Updater,
    About,
}

pub const ALL: [Page; 5] = [
    Page::Care,
    Page::SpeedUp,
    Page::Protect,
    Page::Updater,
    Page::About,
];

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::Care => "Care",
            Page::SpeedUp => "Speed Up",
            Page::Protect => "Protect",
            Page::Updater => "Software Updater",
            Page::About => "About",
        }
    }

    pub fn from_arg(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "care" => Page::Care,
            "speedup" | "speed-up" => Page::SpeedUp,
            "protect" => Page::Protect,
            "updater" | "software-updater" => Page::Updater,
            "about" => Page::About,
            _ => return None,
        })
    }
}

pub fn draw(p: &Painter, v: &View) {
    match v.page {
        Page::Care => care::draw(p, v),
        other => placeholder::draw(p, v, other),
    }
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    match v.page {
        Page::Care => care::hit(v, x, y),
        _ => Hit::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_args_parse() {
        assert_eq!(Page::from_arg("SpeedUp"), Some(Page::SpeedUp));
        assert_eq!(Page::from_arg("software-updater"), Some(Page::Updater));
        assert_eq!(Page::from_arg("nope"), None);
        assert_eq!(ALL.len(), 5);
    }
}
