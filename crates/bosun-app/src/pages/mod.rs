//! Pages shown in the content area to the right of the sidebar.

pub mod actions;
pub mod care;
pub mod menu;
pub mod optimizer;

pub mod protect;
pub mod rescue;
pub mod scan;
pub mod settings;
pub mod speedup;
pub mod tools;
pub mod turbo;
pub mod updater;

use bosun_ui::gfx::Painter;

use crate::window::{Hit, View};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Care,
    SpeedUp,
    Protect,
    Updater,
    Rescue,
}

pub const ALL: [Page; 5] = [
    Page::Care,
    Page::SpeedUp,
    Page::Protect,
    Page::Updater,
    Page::Rescue,
];

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::Care => "Care",
            Page::SpeedUp => "Speed Up",
            Page::Protect => "Protect",
            Page::Updater => "Software Updater",
            Page::Rescue => "Rescue Center",
        }
    }

    pub fn from_arg(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "care" => Page::Care,
            "speedup" | "speed-up" => Page::SpeedUp,
            "protect" => Page::Protect,
            "updater" | "software-updater" => Page::Updater,
            "rescue" | "rescue-center" => Page::Rescue,
            _ => return None,
        })
    }
}

pub fn draw(p: &Painter, v: &View) {
    content(p, v);
    if let Some(m) = &v.menu {
        menu::draw_menu(p, v, m);
    }
    if v.about {
        menu::draw_about(p, v);
    }
}

fn content(p: &Painter, v: &View) {
    if let Some(s) = &v.settings {
        return settings::draw(p, v, s);
    }
    if let Some(b) = &v.bell {
        return actions::draw(p, v, b);
    }
    if v.tools {
        return tools::draw(p, v);
    }
    if let Some(t) = &v.turbo {
        return turbo::draw(p, v, t);
    }
    if let Some(o) = &v.opt {
        return optimizer::draw(p, v, o);
    }
    if let Some(s) = &v.scan {
        return scan::draw(p, v, s);
    }
    match v.page {
        Page::Care => care::draw(p, v),
        Page::SpeedUp => speedup::draw(p, v),
        Page::Protect => protect::draw(p, v),
        Page::Updater => updater::draw(p, v),
        Page::Rescue => rescue::draw(p, v),
    }
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    if v.opt.is_some() || v.turbo.is_some() {
        return Hit::None;
    }
    if let Some(s) = &v.scan {
        return scan::hit(v, s, x, y);
    }
    match v.page {
        Page::Care => care::hit(v, x, y),
        Page::SpeedUp => speedup::hit(v, x, y),
        Page::Protect => protect::hit(v, x, y),
        Page::Updater => updater::hit(v, x, y),
        Page::Rescue => rescue::hit(v, x, y),
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
