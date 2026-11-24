//! Turbo Boost settings, opened from Speed Up. Measured from the reference
//! (`.reanimate/speedup` frames 70 and 110; window-relative DIPs):
//!
//! | element         | geometry                                                    |
//! |-----------------|-------------------------------------------------------------|
//! | "Boost mode:"   | x 39, cap centre 58, 15 px; dropdown 152 × 23 at (131, 46)  |
//! | mode blurb      | x 38, cap centre 82, 13 px; "Select the items…" at 144      |
//! | tabs            | 3 × 212 px from x 37, y 164–201; label cap centre 184       |
//! | table           | x 57–896, y 223–563; header cap centre 237; rows 40 apart from 272 |
//! | Recommended     | link x 72, cap centre 549.5                                 |
//! | power plan box  | box centre (65.5, 592), text x 83                           |
//! | Boost           | 141 × 30 at (755, 586)                                      |

use bosun_core::turbo::Mode;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

use crate::window::{Hit, View};

pub const TABS: [&str; 3] = ["System Services", "Non-Windows Services", "Background Apps"];
const TAB_X: f32 = 37.0;
const TAB_W: f32 = 212.0;
const TABLE: (f32, f32, f32, f32) = (57.0, 223.0, 896.0, 563.0);
const HEADER_BOTTOM: f32 = 252.0;
const ROW_H: f32 = 40.0;
const FIRST_ROW: f32 = 272.0;
const LIST_BOTTOM: f32 = 533.0;

mod tc {
    pub const PANEL: u32 = 0x1C1C1C;
    pub const TABLE: u32 = 0x141414;
    pub const LINE: u32 = 0x2E2E2E;
    pub const TEXT: u32 = 0xE6E6E6;
    pub const DIM: u32 = 0xA8A8A8;
    pub const BLUE: u32 = 0x61ADF8;
}

/// One selectable row.
#[derive(Clone, PartialEq, Debug)]
pub struct TRow {
    pub name: String,
    /// Description (services) or details (apps).
    pub a: String,
    pub b: String,
    pub c: String,
    pub checked: bool,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct TurboView {
    pub mode: Mode,
    pub tab: u8,
    pub rows: Vec<TRow>,
    pub power: bool,
    pub scroll: f32,
}

/// The window's Turbo Boost settings state.
pub struct Turbo {
    pub mode: Mode,
    pub tab: u8,
    pub power: bool,
    pub scroll: f32,
    /// (internal name, display name, description, ticked)
    pub system: Vec<(String, String, String, bool)>,
    pub other: Vec<(String, String, String, bool)>,
    /// (name, pids, memory text, cpu text, ticked)
    pub apps: Vec<(String, Vec<u32>, String, String, bool)>,
}

impl Turbo {
    pub fn open(mode: Mode) -> Self {
        use bosun_core::services::list;
        use bosun_core::turbo::SYSTEM_SERVICES;
        let all = list();
        let system = SYSTEM_SERVICES
            .iter()
            .filter_map(|n| all.iter().find(|s| s.name.eq_ignore_ascii_case(n)))
            .map(|s| {
                // Recommended: everything but search indexing
                let tick = !s.name.eq_ignore_ascii_case("WSearch");
                (
                    s.name.clone(),
                    s.display.clone(),
                    s.description.clone(),
                    tick,
                )
            })
            .collect();
        let other = all
            .iter()
            .filter(|s| !s.system && s.running)
            .map(|s| {
                (
                    s.name.clone(),
                    s.display.clone(),
                    s.description.clone(),
                    false,
                )
            })
            .collect();
        Turbo {
            mode,
            tab: 0,
            power: true,
            scroll: 0.0,
            system,
            other,
            apps: background_apps(),
        }
    }

    pub fn view(&self) -> TurboView {
        let rows = match self.tab {
            0 | 1 => {
                let v = if self.tab == 0 {
                    &self.system
                } else {
                    &self.other
                };
                v.iter()
                    .map(|(_, d, desc, t)| TRow {
                        name: d.clone(),
                        a: desc.clone(),
                        b: String::new(),
                        c: String::new(),
                        checked: *t,
                    })
                    .collect()
            }
            _ => self
                .apps
                .iter()
                .map(|(n, pids, mem, cpu, t)| TRow {
                    name: n.clone(),
                    a: format!(
                        "{} process{}",
                        pids.len(),
                        if pids.len() == 1 { "" } else { "es" }
                    ),
                    b: mem.clone(),
                    c: cpu.clone(),
                    checked: *t,
                })
                .collect(),
        };
        TurboView {
            mode: self.mode,
            tab: self.tab,
            rows,
            power: self.power,
            scroll: self.scroll,
        }
    }

    /// Ticks or unticks row `i` on the current tab (`None` = all of them).
    pub fn toggle(&mut self, i: Option<usize>) {
        let set = |ticks: &mut dyn Iterator<Item = &mut bool>, i: Option<usize>| {
            let mut v: Vec<&mut bool> = ticks.collect();
            match i {
                Some(i) => {
                    if let Some(t) = v.get_mut(i) {
                        **t = !**t;
                    }
                }
                None => {
                    let all = v.iter().all(|t| **t);
                    for t in v {
                        *t = !all;
                    }
                }
            }
        };
        match self.tab {
            0 => set(&mut self.system.iter_mut().map(|x| &mut x.3), i),
            1 => set(&mut self.other.iter_mut().map(|x| &mut x.3), i),
            _ => set(&mut self.apps.iter_mut().map(|x| &mut x.4), i),
        }
    }

    /// "Recommended": the safe Windows services only; nothing else.
    pub fn recommend(&mut self) {
        for s in &mut self.system {
            s.3 = !s.0.eq_ignore_ascii_case("WSearch");
        }
        for s in &mut self.other {
            s.3 = false;
        }
        for a in &mut self.apps {
            a.4 = false;
        }
    }

    pub fn plan(&self) -> bosun_core::turbo::Plan {
        bosun_core::turbo::Plan {
            mode: self.mode,
            services: self
                .system
                .iter()
                .chain(&self.other)
                .filter(|s| s.3)
                .map(|s| s.0.clone())
                .collect(),
            apps: self
                .apps
                .iter()
                .filter(|a| a.4)
                .map(|a| (a.0.clone(), a.1.clone()))
                .collect(),
            power: self.power,
        }
    }

    pub fn scroll_by(&mut self, dy: f32) {
        let n = self.view().rows.len() as f32;
        let max = (n * ROW_H - (LIST_BOTTOM - HEADER_BOTTOM)).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
    }
}

/// Programs running in the background (no window), not part of Windows and
/// not protected, grouped by program.
fn background_apps() -> Vec<(String, Vec<u32>, String, String, bool)> {
    let mut snap = bosun_core::procs::ProcessTable::new();
    let _ = snap.refresh();
    std::thread::sleep(std::time::Duration::from_millis(150));
    let groups = snap.refresh();
    let win = std::env::var("WINDIR")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut out: Vec<_> = groups
        .into_iter()
        .filter(|g| !g.has_window && !g.protected)
        .filter(|g| {
            g.path
                .as_deref()
                .is_some_and(|p| !p.to_ascii_lowercase().starts_with(&win))
        })
        .map(|g| {
            (
                g.name,
                g.pids,
                bosun_core::format::bytes(g.private_bytes),
                format!("{:.1} %", g.cpu_pct),
                false,
            )
        })
        .collect();
    out.sort_by_key(|a| a.0.to_lowercase());
    out
}

fn tab_rect(i: usize) -> D2D_RECT_F {
    rect(TAB_X + TAB_W * i as f32, 164.0, TAB_W, 37.0)
}

fn dropdown() -> D2D_RECT_F {
    rect(131.0, 46.0, 152.0, 23.0)
}

fn boost_button() -> D2D_RECT_F {
    rect(755.0, 586.0, 141.0, 30.0)
}

fn row_center(t: &TurboView, k: usize) -> f32 {
    FIRST_ROW + ROW_H * k as f32 - t.scroll
}

pub fn hit(_v: &View, t: &TurboView, x: f32, y: f32) -> Hit {
    if (x - 26.0).hypot(y - 23.5) <= 14.0 {
        return Hit::OptBack;
    }
    if contains(&dropdown(), x, y) {
        return Hit::TurboMode;
    }
    if let Some(i) = (0..TABS.len()).find(|&i| contains(&tab_rect(i), x, y)) {
        return Hit::TurboTab(i as u8);
    }
    if contains(&boost_button(), x, y) {
        return Hit::TurboGo;
    }
    if contains(&rect(57.0, 581.0, 460.0, 22.0), x, y) {
        return Hit::TurboPower;
    }
    if contains(&rect(70.0, 539.0, 100.0, 20.0), x, y) {
        return Hit::TurboRecommend;
    }
    if contains(&rect(70.0, 228.0, 22.0, 20.0), x, y) {
        return Hit::TurboTick(None);
    }
    if (HEADER_BOTTOM..LIST_BOTTOM).contains(&y) && (TABLE.0..TABLE.2).contains(&x) {
        let k = ((y + t.scroll - FIRST_ROW + ROW_H / 2.0) / ROW_H).floor();
        if k >= 0.0 && (k as usize) < t.rows.len() {
            return Hit::TurboTick(Some(k as usize));
        }
    }
    Hit::None
}

#[allow(clippy::too_many_arguments)]
fn text_at(
    p: &Painter,
    s: &str,
    f: &IDWriteTextFormat,
    size: f32,
    x: f32,
    w: f32,
    cap_y: f32,
    c: u32,
) {
    let cy = cap_y - 0.064 * size;
    p.text(s, f, rect(x, cy - size, w, 2.0 * size), col(c), Align::Left);
}

pub fn draw(p: &Painter, v: &View, t: &TurboView) {
    let f = &p.g.f;
    // back arrow (same as the Startup Optimizer)
    let c = col(if v.hover == Hit::OptBack {
        0xFFFFFF
    } else {
        0xB0B0B0
    });
    p.line(19.5, 18.5, 30.0, 18.5, 1.6, c);
    p.line(19.5, 18.5, 23.0, 15.0, 1.6, c);
    p.line(19.5, 18.5, 23.0, 22.0, 1.6, c);
    p.arc_at(30.0, 23.5, 5.0, 1.6, 0.0, 0.5, c);
    p.line(30.0, 28.5, 23.0, 28.5, 1.6, c);

    text_at(
        p,
        "Boost mode:",
        &f.pop_label,
        15.0,
        39.0,
        100.0,
        58.0,
        tc::TEXT,
    );
    let d = dropdown();
    p.fill_rr(d, 3.0, col(0x1A1A1A));
    let hot = v.hover == Hit::TurboMode;
    p.stroke_rr(d, 3.0, 1.0, col(if hot { 0x5A5A5A } else { 0x3A3A3A }));
    text_at(
        p,
        t.mode.label(),
        &f.m13,
        13.0,
        141.0,
        120.0,
        57.5,
        tc::TEXT,
    );
    p.caret_down(269.0, 58.0, 7.0, col(0xC8C8C8));
    text_at(p, t.mode.blurb(), &f.m13, 13.0, 38.0, 840.0, 82.0, tc::TEXT);
    text_at(
        p,
        "Select the items you want to stop to boost your computer.",
        &f.m13,
        13.0,
        38.0,
        600.0,
        144.0,
        tc::TEXT,
    );
    // panel and tabs
    p.fill_rr(
        rect(37.0, 201.0, 879.0, v.h - 23.0 - 201.0),
        8.0,
        col(tc::PANEL),
    );
    for (i, name) in TABS.iter().enumerate() {
        let r = tab_rect(i);
        let on = t.tab as usize == i;
        if on {
            p.fill_rr(rect(r.left, r.top, TAB_W, 50.0), 8.0, col(tc::PANEL));
        }
        let c = if on || v.hover == Hit::TurboTab(i as u8) {
            0xFFFFFF
        } else {
            0xC8C8C8
        };
        let cy = 184.0 - 0.064 * 15.0;
        p.text(
            name,
            &f.pop_label,
            rect(r.left, cy - 15.0, TAB_W, 30.0),
            col(c),
            Align::Center,
        );
    }
    // table
    let (x0, y0, x1, y1) = TABLE;
    p.fill(rect(x0, y0, x1 - x0, y1 - y0), col(tc::TABLE));
    p.stroke_rr(rect(x0, y0, x1 - x0, y1 - y0), 0.0, 1.0, col(tc::LINE));
    let heads: &[(&str, f32)] = if t.tab == 2 {
        &[
            ("Item Name", 99.0),
            ("Processes", 352.0),
            ("Memory Usage", 589.0),
            ("CPU", 731.0),
        ]
    } else {
        &[("Item Name", 99.0), ("Description", 339.0)]
    };
    for (k, (label, x)) in heads.iter().enumerate() {
        text_at(p, label, &f.m12, 12.0, *x, 200.0, 237.0, tc::DIM);
        if k + 1 < heads.len() {
            let nx = heads[k + 1].1 - 10.0;
            p.line(nx, 231.0, nx, 244.0, 1.0, col(tc::LINE));
        }
    }
    p.line(851.0, 231.0, 851.0, 244.0, 1.0, col(tc::LINE));
    let all = !t.rows.is_empty() && t.rows.iter().all(|r| r.checked);
    crate::pages::scan::checkbox(p, 80.5, 237.5, all, 1.0);
    p.line(
        x0,
        HEADER_BOTTOM - 0.5,
        x1,
        HEADER_BOTTOM - 0.5,
        1.0,
        col(tc::LINE),
    );
    p.clip(rect(
        x0,
        HEADER_BOTTOM,
        x1 - x0,
        LIST_BOTTOM - HEADER_BOTTOM,
    ));
    for (k, r) in t.rows.iter().enumerate() {
        let cy = row_center(t, k);
        if cy < HEADER_BOTTOM - ROW_H || cy > LIST_BOTTOM + ROW_H {
            continue;
        }
        if v.hover == Hit::TurboTick(Some(k)) {
            p.fill(
                rect(x0 + 1.0, cy - ROW_H / 2.0, x1 - x0 - 2.0, ROW_H),
                cola(0xFFFFFF, 0.06),
            );
        }
        crate::pages::scan::checkbox(p, 80.5, cy, r.checked, 1.0);
        let nw = if t.tab == 2 { 240.0 } else { 230.0 };
        text_at(p, &r.name, &f.m13, 13.0, 99.0, nw, cy, tc::TEXT);
        if t.tab == 2 {
            text_at(p, &r.a, &f.m13, 13.0, 352.0, 220.0, cy, tc::TEXT);
            text_at(p, &r.b, &f.m13, 13.0, 589.0, 130.0, cy, tc::TEXT);
            text_at(p, &r.c, &f.m13, 13.0, 731.0, 110.0, cy, tc::TEXT);
        } else {
            text_at(p, &r.a, &f.m13, 13.0, 339.0, 500.0, cy, tc::TEXT);
        }
    }
    p.unclip();
    if t.rows.is_empty() {
        let msg = match t.tab {
            0 => "None of these Windows services are running.",
            1 => "No non-Windows services are running.",
            _ => "No background apps are running.",
        };
        p.text(
            msg,
            &f.m13,
            rect(x0, 380.0, x1 - x0, 20.0),
            col(tc::DIM),
            Align::Center,
        );
    }
    // Recommended strip
    p.fill(
        rect(x0 + 1.0, LIST_BOTTOM, x1 - x0 - 2.0, y1 - LIST_BOTTOM - 1.0),
        col(0x1A1A1A),
    );
    let rc = if v.hover == Hit::TurboRecommend {
        0x9CCBFA
    } else {
        tc::BLUE
    };
    let w = p.g.measure("Recommended", &f.m13);
    text_at(p, "Recommended", &f.m13, 13.0, 72.0, w + 4.0, 549.5, rc);
    p.line(72.0, 556.5, 72.0 + w, 556.5, 1.0, col(rc));
    // power plan and Boost
    crate::pages::scan::checkbox(p, 65.5, 592.0, t.power, 1.0);
    let plan = if t.mode == Mode::Economy {
        "Power saver"
    } else {
        "High performance"
    };
    let label = format!("Switch to the {plan} power plan while boosting");
    let lc = if v.hover == Hit::TurboPower {
        0xFFFFFF
    } else {
        tc::TEXT
    };
    text_at(p, &label, &f.m13, 13.0, 83.0, 460.0, 592.0, lc);
    let b = boost_button();
    let x = (b.left + b.right) / 2.0;
    let stops = [(0.0, col(0x8DC3F8)), (1.0, col(0x6DB4F7))];
    p.fill_rr_linear(b, 15.0, (x, b.top), (x, b.bottom), &stops);
    if v.hover == Hit::TurboGo {
        p.fill_rr(b, 15.0, cola(0xFFFFFF, 0.14));
    }
    p.text("Boost", &f.m14sb, b, col(0x141414), Align::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        let t = TurboView {
            rows: vec![
                TRow {
                    name: "a".into(),
                    a: String::new(),
                    b: String::new(),
                    c: String::new(),
                    checked: false
                };
                3
            ],
            ..Default::default()
        };
        assert_eq!(hit(&v, &t, 800.0, 600.0), Hit::TurboGo);
        assert_eq!(hit(&v, &t, 200.0, 57.0), Hit::TurboMode);
        assert_eq!(hit(&v, &t, 355.0, 184.0), Hit::TurboTab(1));
        assert_eq!(hit(&v, &t, 300.0, 312.0), Hit::TurboTick(Some(1)));
        assert_eq!(hit(&v, &t, 100.0, 549.0), Hit::TurboRecommend);
        let b = boost_button();
        assert_eq!((b.right, b.bottom), (896.0, 616.0));
    }
}
