//! Settings, measured from the reference dialog (`.reanimate/phase6`, scaled
//! from 150 %): a 39 px title bar, a 233 px tree (13 px rows 20 px apart from
//! cap centre 58.7, children indented to 43), the page from x 265 (15 px bold
//! heading at 62, checkbox rows with the box at 265 and text at 280), and a
//! 60 px footer from y 600: "Quick Settings" at 19 and three 132 × 34 pills
//! at 534, 675 and 817 (Recommended, Cancel, OK).
//!
//! Only settings Bosun really has are here. Changes wait for OK; Cancel and
//! the close button drop them; Recommended puts the defaults back.

use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

use crate::pages::speedup::text_at;
use crate::scan::MODULES;
use crate::window::{Hit, View};

const TITLE_H: f32 = 39.0;
const TREE_W: f32 = 233.0;
const FOOT_Y: f32 = 600.0;
const PAGE_X: f32 = 265.0;

/// Tree rows: (label, indented). Rows 2..=10 are `MODULES`.
const TREE: [(&str, bool); 14] = [
    ("General Settings", false),
    ("Clean & Optimize", false),
    ("Privacy Sweep", true),
    ("Junk File Clean", true),
    ("Shortcut Fix", true),
    ("Registry Clean", true),
    ("Antivirus Protection", true),
    ("Firewall", true),
    ("System Optimization", true),
    ("Disk Optimization", true),
    ("Windows Update", true),
    ("Auto RAM Clean", false),
    ("Auto Clean", false),
    ("Logs & Backup", false),
];

/// What each Care module really does, for its page.
const ABOUT_MODULE: [[&str; 2]; 9] = [
    [
        "Clears browser caches and the usage traces apps leave behind.",
        "Files are deleted for good; each run is listed in the Rescue Center.",
    ],
    [
        "Removes temporary files, logs and caches Windows and apps no longer need.",
        "Files are deleted for good; each run is listed in the Rescue Center.",
    ],
    [
        "Finds shortcuts whose program is gone and moves them to the Recycle Bin,",
        "so you can still get them back from there.",
    ],
    [
        "Removes registry entries that point to programs or files that no longer exist.",
        "Every key is backed up first and can be restored from the Rescue Center.",
    ],
    [
        "Checks that Microsoft Defender or another antivirus is on and up to date.",
        "Bosun doesn't scan files itself.",
    ],
    [
        "Checks that Windows Firewall is on.",
        "Turn it on in Windows Security if a scan finds it off.",
    ],
    [
        "Five real, reversible tweaks in your own registry: frozen apps end at sign-out,",
        "shorter waits, faster menus, no startup-app delay. Undo any from the Rescue Center.",
    ],
    [
        "Runs Windows' own optimizer on your fixed drives: trim on SSDs, defragment on",
        "hard drives. Windows asks for administrator rights; it isn't ticked by default.",
    ],
    [
        "Asks Windows Update which updates are waiting and tells you. Installing them",
        "stays with Windows Update in Settings.",
    ],
];

pub const DEFAULT_PICKS: u16 = crate::scan::ALL;

/// The values the dialog edits.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Values {
    pub tray_start: bool,
    pub widget: bool,
    pub auto_ram: bool,
    /// Junk files cleaned once a day by the tray.
    pub auto_clean: bool,
    /// A System Restore point before each Fix.
    pub restore_point: bool,
    /// Care scan modules (bit i = `MODULES[i]`).
    pub picks: u16,
}

impl Values {
    pub const RECOMMENDED: Values = Values {
        tray_start: true,
        widget: true,
        auto_ram: false,
        auto_clean: false,
        restore_point: false,
        picks: DEFAULT_PICKS,
    };

    pub fn load() -> Self {
        let m = crate::pages::menu::MenuInfo::load();
        Values {
            tray_start: m.tray_start,
            widget: m.widget,
            auto_ram: m.auto_ram,
            auto_clean: crate::tray_ini::get("auto_clean", false),
            restore_point: crate::tray_ini::get_from(crate::tray_ini::APP, "restore_point", false),
            picks: saved_picks(),
        }
    }
}

/// The Care scan modules saved in `app.ini` (all of them by default).
pub fn saved_picks() -> u16 {
    (0..MODULES.len())
        .filter(|&i| crate::tray_ini::get_from(crate::tray_ini::APP, &format!("care_{i}"), true))
        .fold(0, |m, i| m | 1 << i)
}

#[derive(Clone, PartialEq, Debug)]
pub struct SettingsView {
    pub sel: u8,
    pub saved: Values,
    pub draft: Values,
    /// (journals, bytes) in the journal folder.
    pub logs: (usize, u64),
}

impl SettingsView {
    pub fn open() -> Self {
        let v = Values::load();
        SettingsView {
            sel: 0,
            saved: v,
            draft: v,
            logs: logs(),
        }
    }

    /// Flips checkbox `k` (see `Hit::SetCheck`).
    pub fn check(&mut self, k: u8) {
        let d = &mut self.draft;
        match k {
            0 => d.tray_start = !d.tray_start,
            1 => d.widget = !d.widget,
            2 => d.auto_ram = !d.auto_ram,
            3 => d.restore_point = !d.restore_point,
            4 => d.auto_clean = !d.auto_clean,
            k => d.picks ^= 1 << (k - 10),
        }
    }

    /// Writes what changed; `Err` names what failed.
    pub fn apply(&self) -> Result<(), &'static str> {
        let (s, d) = (self.saved, self.draft);
        if d.picks == 0 {
            return Err("Pick at least one module for Care scans.");
        }
        let info = crate::pages::menu::MenuInfo {
            widget: s.widget,
            tray_start: s.tray_start,
            auto_ram: s.auto_ram,
            advisor: false,
        };
        if d.widget != s.widget {
            crate::pages::menu::flip(&info, 0)?;
        }
        if d.tray_start != s.tray_start {
            crate::pages::menu::flip(&info, 1)?;
        }
        if d.auto_ram != s.auto_ram {
            crate::pages::menu::flip(&info, 2)?;
        }
        if d.auto_clean != s.auto_clean {
            crate::tray_ini::set("auto_clean", d.auto_clean)
                .map_err(|_| "Couldn't save Auto Clean.")?;
        }
        if d.restore_point != s.restore_point {
            crate::tray_ini::set_to(crate::tray_ini::APP, "restore_point", d.restore_point)
                .map_err(|_| "Couldn't save the restore point setting.")?;
        }
        for i in 0..MODULES.len() {
            let on = d.picks & 1 << i != 0;
            if on != (s.picks & 1 << i != 0) {
                crate::tray_ini::set_to(crate::tray_ini::APP, &format!("care_{i}"), on)
                    .map_err(|_| "Couldn't save the Care scan modules.")?;
            }
        }
        Ok(())
    }
}

pub fn logs() -> (usize, u64) {
    bosun_core::clean::journal_dir().map_or((0, 0), |d| bosun_core::rescue::usage(&d))
}

fn tree_row(k: usize) -> D2D_RECT_F {
    rect(1.3, 58.7 + 20.0 * k as f32 - 9.7, TREE_W - 1.3, 19.4)
}

/// (checkbox id, cap centre y) on the current page.
fn checks(sel: u8) -> Vec<(u8, f32)> {
    match sel {
        0 => vec![(0, 95.3), (1, 123.3), (3, 195.3)],
        1 => (0..MODULES.len())
            .map(|i| (10 + i as u8, 117.0 + 21.0 * i as f32))
            .collect(),
        2..=10 => vec![(10 + sel - 2, 142.0)],
        11 => vec![(2, 142.0)],
        12 => vec![(4, 142.0)],
        _ => Vec::new(),
    }
}

fn check_row(v: &View, y: f32) -> D2D_RECT_F {
    rect(
        PAGE_X - 3.0,
        y - 10.0,
        (v.w - PAGE_X - 40.0).min(420.0),
        20.0,
    )
}

fn pills(v: &View) -> [D2D_RECT_F; 3] {
    let r = v.w - 960.0;
    [
        rect(r + 534.0, FOOT_Y + 12.0, 132.0, 34.0),
        rect(r + 675.3, FOOT_Y + 12.0, 132.0, 34.0),
        rect(r + 816.7, FOOT_Y + 12.0, 132.0, 34.0),
    ]
}

const OPEN_BTN: D2D_RECT_F = D2D_RECT_F {
    left: PAGE_X,
    top: 150.0,
    right: PAGE_X + 120.0,
    bottom: 180.0,
};
const PRUNE_BTN: D2D_RECT_F = D2D_RECT_F {
    left: PAGE_X + 132.0,
    top: 150.0,
    right: PAGE_X + 352.0,
    bottom: 180.0,
};

fn foot_y(v: &View) -> f32 {
    FOOT_Y + (v.h - 660.0)
}

pub fn hit(v: &View, s: &SettingsView, x: f32, y: f32) -> Hit {
    let fy = foot_y(v);
    if y < TITLE_H {
        return if x > v.w - 46.0 {
            Hit::SetCancel
        } else {
            Hit::None
        };
    }
    if y >= fy {
        let dy = fy - FOOT_Y;
        if let Some(k) = pills(v)
            .iter()
            .position(|r| contains(&rect(r.left, r.top + dy, r.right - r.left, 34.0), x, y))
        {
            return [Hit::SetRecommended, Hit::SetCancel, Hit::SetOk][k];
        }
        if contains(&rect(19.0, fy + 18.0, 100.0, 20.0), x, y) {
            return Hit::SetQuick;
        }
        return Hit::None;
    }
    if x < TREE_W {
        return (0..TREE.len())
            .find(|&k| contains(&tree_row(k), x, y))
            .map_or(Hit::None, |k| Hit::SetNav(k as u8));
    }
    if s.sel == 13 {
        if contains(&OPEN_BTN, x, y) {
            return Hit::SetOpenLogs;
        }
        if contains(&PRUNE_BTN, x, y) {
            return Hit::SetPrune;
        }
    }
    checks(s.sel)
        .into_iter()
        .find(|(_, cy)| contains(&check_row(v, *cy), x, y))
        .map_or(Hit::None, |(k, _)| Hit::SetCheck(k))
}

fn checkbox(p: &Painter, x: f32, cy: f32, on: bool, hot: bool) {
    let r = rect(x, cy - 6.0, 12.0, 12.0);
    if on {
        p.fill_rr(r, 2.0, col(if hot { 0x7DBCFA } else { 0x61ADF8 }));
        let c = col(0x101010);
        p.line(x + 2.6, cy + 0.2, x + 5.0, cy + 2.6, 1.6, c);
        p.line(x + 5.0, cy + 2.6, x + 9.6, cy - 2.4, 1.6, c);
    } else {
        p.stroke_rr(r, 2.0, 1.2, col(if hot { 0xBDBDBD } else { 0x7A7A7A }));
    }
}

fn check_line(p: &Painter, v: &View, s: &SettingsView, k: u8, cy: f32, label: &str) {
    let on = match k {
        0 => s.draft.tray_start,
        1 => s.draft.widget,
        2 => s.draft.auto_ram,
        3 => s.draft.restore_point,
        4 => s.draft.auto_clean,
        k => s.draft.picks & 1 << (k - 10) != 0,
    };
    let hot = v.hover == Hit::SetCheck(k);
    checkbox(p, PAGE_X, cy, on, hot);
    text_at(
        p,
        label,
        &p.g.f.m13,
        13.0,
        PAGE_X + 15.0,
        560.0,
        cy,
        0xE6E6E6,
    );
}

fn para(p: &Painter, lines: &[&str], x: f32, y: f32) {
    for (i, l) in lines.iter().enumerate() {
        text_at(
            p,
            l,
            &p.g.f.m13,
            13.0,
            x,
            640.0,
            y + 17.3 * i as f32,
            0xB0B0B0,
        );
    }
}

/// The reference's footer buttons: a soft vertical gradient pill with a
/// hairline highlight along the top, dark label.
fn button(p: &Painter, r: D2D_RECT_F, label: &str, hot: bool) {
    let rad = (r.bottom - r.top) / 2.0;
    let x = (r.left + r.right) / 2.0;
    let (top, bottom) = if hot {
        (0xE4E4E4, 0xCFCFCF)
    } else {
        (0xCDCDCD, 0xB4B4B4)
    };
    p.untinted(|| {
        p.fill_rr_linear(
            r,
            rad,
            (x, r.top),
            (x, r.bottom),
            &[(0.0, col(top)), (1.0, col(bottom))],
        );
        p.stroke_rr(
            rect(
                r.left + 0.5,
                r.top + 0.5,
                r.right - r.left - 1.0,
                r.bottom - r.top - 1.0,
            ),
            rad - 0.5,
            1.0,
            cola(0xFFFFFF, 0.35),
        );
        p.text(label, &p.g.f.body, r, col(0x1A1A1A), Align::Center);
    });
}

fn page(p: &Painter, v: &View, s: &SettingsView) {
    let f = &p.g.f;
    let head = |t: &str| text_at(p, t, &f.m14sb, 15.0, PAGE_X, 600.0, 62.0, 0xF2F2F2);
    match s.sel {
        0 => {
            head("General Settings");
            check_line(p, v, s, 0, 95.3, "Load the Bosun tray at Windows startup");
            check_line(
                p,
                v,
                s,
                1,
                123.3,
                "Show the Performance Monitor widget on the desktop",
            );
            para(
                p,
                &[
                    "Shows RAM and CPU use at a glance. Click it for the Performance Monitor, or",
                    "right-click it for Clean RAM, power options and its own settings.",
                ],
                PAGE_X + 15.0,
                146.0,
            );
            check_line(
                p,
                v,
                s,
                3,
                195.3,
                "Create a System Restore point before each Fix",
            );
            para(
                p,
                &[
                    "Windows asks for administrator rights each time. Bosun's journal keeps",
                    "every change restorable from the Rescue Center either way.",
                ],
                PAGE_X + 15.0,
                218.0,
            );
        }
        1 => {
            head("Clean & Optimize");
            text_at(
                p,
                "Modules a Care scan includes:",
                &f.m13sb,
                13.0,
                PAGE_X,
                400.0,
                92.0,
                0xE6E6E6,
            );
            let b = rect(
                PAGE_X - 0.3,
                104.0,
                337.3,
                MODULES.len() as f32 * 21.0 + 14.0,
            );
            p.stroke_rr(b, 1.0, 1.0, col(0x3A3A3A));
            for (i, m) in MODULES.iter().enumerate() {
                check_line(p, v, s, 10 + i as u8, 117.0 + 21.0 * i as f32, m.pick);
            }
            para(
                p,
                &["Manual mode on the Care page can still pick modules for one scan."],
                PAGE_X,
                104.0 + MODULES.len() as f32 * 21.0 + 14.0 + 24.0,
            );
        }
        k @ 2..=10 => {
            let i = (k - 2) as usize;
            head(MODULES[i].pick);
            para(p, &ABOUT_MODULE[i], PAGE_X, 95.3);
            check_line(p, v, s, k + 8, 142.0, "Include in Care scans");
            if MODULES[i].kind == crate::scan::Kind::Tweaks {
                // the spec: every tweak shows the exact value it writes
                for (n, t) in bosun_core::tweaks::TWEAKS.iter().enumerate() {
                    let y = 184.0 + 38.0 * n as f32;
                    text_at(p, t.label, &f.m13, 13.0, PAGE_X, 640.0, y, 0xE6E6E6);
                    let value = match t.value {
                        bosun_core::tweaks::Value::Sz(x) => format!("\"{x}\""),
                        bosun_core::tweaks::Value::Dword(d) => format!("{d} (DWORD)"),
                    };
                    let line = format!(r"HKCU\{}\{} = {value}", t.key, t.name);
                    text_at(p, &line, &f.m12, 12.0, PAGE_X, 640.0, y + 17.0, 0x8A8A8A);
                }
            }
        }
        11 => {
            head("Auto RAM Clean");
            para(
                p,
                &[
                    "While the Bosun tray runs, it trims the memory apps have set aside but aren't",
                    "using whenever RAM use passes 85%. Nothing is closed.",
                ],
                PAGE_X,
                95.3,
            );
            check_line(p, v, s, 2, 142.0, "Enable Auto RAM Clean");
        }
        12 => {
            head("Auto Clean");
            para(
                p,
                &[
                    "Once a day the Bosun tray cleans junk files (temporary files, caches, logs),",
                    "the safe module. Browser data is never touched. Each run is in the Rescue Center.",
                ],
                PAGE_X,
                95.3,
            );
            check_line(
                p,
                v,
                s,
                4,
                142.0,
                "Clean junk files automatically every day",
            );
            let last = crate::tray_ini::get_number("auto_clean_last")
                .filter(|&t| t > 0)
                .map_or_else(
                    || "Last run: never".to_string(),
                    |t| format!("Last run: {}", crate::pages::rescue::when(t * 1000)),
                );
            text_at(
                p,
                &last,
                &f.m13,
                13.0,
                PAGE_X + 15.0,
                400.0,
                166.0,
                0x8A8A8A,
            );
        }
        _ => {
            head("Logs & Backup");
            para(
                p,
                &[
                    "Bosun writes every change to a journal before making it, with registry",
                    "backups. The Rescue Center restores from them.",
                ],
                PAGE_X,
                95.3,
            );
            let (n, b) = s.logs;
            let line = format!(
                "{n} journal{} · {} on disk",
                if n == 1 { "" } else { "s" },
                bosun_core::format::bytes(b)
            );
            text_at(p, &line, &f.m13, 13.0, PAGE_X, 400.0, 136.0, 0xE6E6E6);
            button(p, OPEN_BTN, "Open folder", v.hover == Hit::SetOpenLogs);
            button(
                p,
                PRUNE_BTN,
                "Delete logs older than 30 days",
                v.hover == Hit::SetPrune,
            );
            para(
                p,
                &["Changes in deleted logs can't be restored any more."],
                PAGE_X,
                200.0,
            );
        }
    }
}

pub fn draw(p: &Painter, v: &View, s: &SettingsView) {
    let f = &p.g.f;
    let fy = foot_y(v);
    p.fill(rect(0.0, 0.0, v.w, v.h), col(0x1E1E1E));
    // title bar and footer: the darker frame around the dialog
    p.fill(rect(0.0, 0.0, v.w, TITLE_H), col(0x141414));
    p.fill(rect(0.0, fy, v.w, v.h - fy), col(0x141414));
    text_at(
        p,
        "Bosun SystemCare - Settings",
        &f.body,
        15.0,
        15.3,
        400.0,
        20.7,
        0xA8A8A8,
    );
    let (xx, xy) = (v.w - 23.0, 20.0);
    let xc = col(if v.hover == Hit::SetCancel {
        0xE6E6E6
    } else {
        0x9A9A9A
    });
    p.line(xx - 5.0, xy - 5.0, xx + 5.0, xy + 5.0, 1.2, xc);
    p.line(xx + 5.0, xy - 5.0, xx - 5.0, xy + 5.0, 1.2, xc);
    p.line(TREE_W, TITLE_H, TREE_W, fy, 1.0, col(0x2E2E2E));
    for (k, (label, indent)) in TREE.iter().enumerate() {
        let r = tree_row(k);
        if s.sel == k as u8 {
            p.fill(r, col(0x2F3B48));
            p.stroke_rr(r, 0.0, 1.0, col(0x61ADF8));
        } else if v.hover == Hit::SetNav(k as u8) {
            p.fill(r, col(0x262626));
        }
        let x = if *indent { 43.3 } else { 24.0 };
        text_at(
            p,
            label,
            &f.m13,
            13.0,
            x,
            200.0,
            58.7 + 20.0 * k as f32,
            0xE6E6E6,
        );
    }
    // the Clean & Optimize group's open chevron
    p.caret_down(11.3, 58.7 + 20.0, 7.0, col(0xB0B0B0));
    page(p, v, s);
    let qc = if v.hover == Hit::SetQuick {
        0x8CC4FF
    } else {
        0x61ADF8
    };
    let w = p.g.measure("Quick Settings", &f.body);
    text_at(
        p,
        "Quick Settings",
        &f.body,
        14.0,
        19.3,
        w + 4.0,
        fy + 28.0,
        qc,
    );
    p.line(19.3, fy + 36.0, 19.3 + w, fy + 36.0, 1.0, col(qc));
    let dy = fy - FOOT_Y;
    for (k, (r, label)) in pills(v)
        .iter()
        .zip(["Recommended", "Cancel", "OK"])
        .enumerate()
    {
        let hot = v.hover == [Hit::SetRecommended, Hit::SetCancel, Hit::SetOk][k];
        button(p, rect(r.left, r.top + dy, 132.0, 34.0), label, hot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_drafts() {
        let v = crate::window::test_view();
        let mut s = SettingsView {
            sel: 0,
            saved: Values::RECOMMENDED,
            draft: Values::RECOMMENDED,
            logs: (0, 0),
        };
        assert_eq!(hit(&v, &s, 100.0, 58.7), Hit::SetNav(0));
        assert_eq!(hit(&v, &s, 100.0, 58.7 + 20.0 * 9.0), Hit::SetNav(9));
        assert_eq!(hit(&v, &s, 300.0, 95.3), Hit::SetCheck(0));
        assert_eq!(hit(&v, &s, 900.0, 628.0), Hit::SetOk);
        assert_eq!(hit(&v, &s, 600.0, 628.0), Hit::SetRecommended);
        assert_eq!(hit(&v, &s, 50.0, 628.0), Hit::SetQuick);
        s.check(12);
        assert_eq!(s.draft.picks, DEFAULT_PICKS & !(1 << 2));
        s.check(0);
        assert!(!s.draft.tray_start);
        assert_eq!(hit(&v, &s, 300.0, 195.3), Hit::SetCheck(3));
        s.check(3);
        assert!(s.draft.restore_point);
        assert_ne!(s.draft, s.saved);
    }
}
