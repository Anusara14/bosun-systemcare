//! Startup Optimizer, opened from Speed Up. Measured from the reference
//! (`.reanimate/speedup`, frames 290 and 361; window-relative DIPs):
//!
//! | element            | geometry                                                |
//! |--------------------|---------------------------------------------------------|
//! | back arrow         | centred (26.5, 21.5)                                    |
//! | tabs               | 5 × 184 px from x 17, y 45–90; label cap centre y 69     |
//! | panel              | x 17–936, from y 90 to h − 23                           |
//! | table              | box x 37–916, y 165 to h − 74; header cap centre 178     |
//! | rows               | 61 px apart, first centre y 224; icons 31 px             |
//! | status pill        | 70 × 22 at x 744 in a 104 px track; caret at 854         |
//! | footer links       | cap centre y 606: "Rescue Center" x 37, "Refresh" → 916  |
//! | Quick Optimization | info ring (53.5, 126.5) d 33; Optimize 141 × 30 at (775, 111) |
//!
//! IObit's "Boot time" and "Disable rate" come from an admin-only event log
//! and their crowd data; we show where an entry starts from and its scope.

use bosun_core::apps::App;
use bosun_core::drivers::Driver;
use bosun_core::extensions::{Browser, Extension};
use bosun_core::finder::Found;
use bosun_core::services::{Service, Start};
use bosun_core::startup::{Advice, Item, Source, advice};
use bosun_core::tasks::Task;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::window::{Hit, View};

pub const TABS: [&str; 5] = [
    "Quick Optimization",
    "Startup Items",
    "Services",
    "Scheduled Tasks",
    "Browser Startups",
];
const TAB_X: f32 = 17.0;
const TAB_W: f32 = 184.0;
const TAB_TOP: f32 = 45.0;
const PANEL_TOP: f32 = 90.0;
const TABLE_X: f32 = 37.0;
const TABLE_TOP: f32 = 165.0;
const HEADER_BOTTOM: f32 = 193.0;
const ROW_H: f32 = 61.0;
const FIRST_ROW: f32 = 224.0;

mod oc {
    pub const PANEL: u32 = 0x1C1C1C;
    pub const TABLE: u32 = 0x141414;
    pub const LINE: u32 = 0x2E2E2E;
    pub const TEXT: u32 = 0xE6E6E6;
    pub const DIM: u32 = 0xA8A8A8;
    pub const ORANGE: u32 = 0xF5A56A;
}

/// One table row, as drawn.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    /// Index into the optimizer's item list.
    pub idx: usize,
    pub name: String,
    /// The two info columns (e.g. "Registry" / "This user").
    pub a: String,
    pub b: String,
    pub advice: Advice,
    pub pill: Pill,
    pub checked: bool,
    /// Raw HICON (owned by the window's optimizer state).
    pub icon: isize,
    /// A browser's group header row (Browser Startups) rather than an entry.
    pub group: bool,
    /// Third info column (Programs: install date).
    pub c: String,
}

/// Status pill: on (blue), manual-start service (grey) or off (grey).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pill {
    On,
    Manual,
    Off,
}

/// What the optimizer shows this frame.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct OptView {
    pub tab: u8,
    pub rows: Vec<Row>,
    /// Entries on this tab before filtering (Startup Items: all of them).
    pub total: usize,
    pub scroll: f32,
    pub show_system: bool,
    /// Opened as the App/Extension Cleaner (Programs + Browser Extensions).
    pub apps_mode: bool,
    /// Programs sort: 0 name, 1 size, 2 install date.
    pub sort: u8,
    /// Opened as Driver Updates (one tab).
    pub drivers_mode: bool,
    /// Opened as the file finder (Large Files, Empty Folders).
    pub finder_mode: bool,
    /// Still asking Windows Update.
    pub loading: bool,
    /// Startup Items: Boot time / Impact measurement state.
    pub measure: Measure,
}

/// The Startup Items tab's measurement (Boot time, Impact).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Measure {
    #[default]
    Never,
    /// Unix seconds of the saved measurement.
    At(u64),
    Running,
    Failed,
}

/// The window's optimizer state.
pub struct Optimizer {
    pub tab: u8,
    pub items: Vec<Item>,
    pub checked: Vec<bool>,
    pub ignored: Vec<bool>,
    pub scroll: f32,
    icons: Vec<HICON>,
    /// Loaded when their tab is first opened.
    pub services: Option<Vec<Service>>,
    svc_icons: Vec<HICON>,
    pub tasks: Option<Vec<Task>>,
    pub extensions: Option<Vec<Extension>>,
    browser_icons: Vec<(Browser, HICON)>,
    pub apps_mode: bool,
    pub programs: Option<Vec<App>>,
    prog_icons: Vec<HICON>,
    pub sort: u8,
    pub drivers_mode: bool,
    pub drivers: Option<Vec<Driver>>,
    /// Background Windows Update search in flight.
    driver_rx: Option<std::sync::mpsc::Receiver<Vec<Driver>>>,
    pub finder_mode: bool,
    pub large: Option<Vec<Found>>,
    pub empty: Option<Vec<Found>>,
    finder_rx: Option<std::sync::mpsc::Receiver<(u8, Vec<Found>)>>,
    /// Services tab also lists Windows' own services.
    pub show_system: bool,
    /// Windows' startup measurements (when, programs), once measured.
    impact: Option<(u64, Vec<bosun_core::startup_impact::Measured>)>,
    /// The elevated measuring run in flight (true = it succeeded).
    measure_rx: Option<std::sync::mpsc::Receiver<bool>>,
    measure_failed: bool,
}

fn item_icon(item: &Item) -> HICON {
    file_icon(item.exe.as_deref())
}

fn file_icon(exe: Option<&std::path::Path>) -> HICON {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    use windows::core::HSTRING;
    let Some(exe) = exe.filter(|e| e.exists()) else {
        return HICON::default();
    };
    let mut info = SHFILEINFOW::default();
    // SAFETY: fills `info`; the icon is destroyed when the optimizer closes.
    unsafe {
        SHGetFileInfoW(
            &HSTRING::from(exe.as_os_str()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
    }
    info.hIcon
}

impl Optimizer {
    pub fn open(tab: u8) -> Self {
        let mut o = Optimizer {
            tab,
            items: Vec::new(),
            checked: Vec::new(),
            ignored: Vec::new(),
            scroll: 0.0,
            icons: Vec::new(),
            services: None,
            svc_icons: Vec::new(),
            tasks: None,
            extensions: None,
            browser_icons: Vec::new(),
            apps_mode: false,
            programs: None,
            prog_icons: Vec::new(),
            sort: 0,
            drivers_mode: false,
            drivers: None,
            driver_rx: None,
            finder_mode: false,
            large: None,
            empty: None,
            finder_rx: None,
            show_system: false,
            impact: bosun_core::startup_impact::load(),
            measure_rx: None,
            measure_failed: false,
        };
        o.reload();
        o
    }

    /// Driver Updates (tab 6): Windows Update is asked in the background.
    pub fn open_drivers() -> Self {
        let mut o = Self::open(6);
        o.drivers_mode = true;
        o.search_drivers();
        o
    }

    /// Starts (or restarts) the Windows Update search for drivers.
    pub fn search_drivers(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // ponytail: a failed search shows as "none found"; the error isn't actionable here
            let _ = tx.send(bosun_core::drivers::search().unwrap_or_default());
        });
        self.drivers = None;
        self.driver_rx = Some(rx);
    }

    /// The file finder on Large Files (7) or Empty Folders (8).
    pub fn open_finder(tab: u8) -> Self {
        let mut o = Self::open(tab);
        o.finder_mode = true;
        o.search_finder();
        o
    }

    /// Searches the user's folders for the current finder tab, off the UI thread.
    pub fn search_finder(&mut self) {
        let tab = self.tab;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Some(home) = bosun_core::finder::home() else {
                let _ = tx.send((tab, Vec::new()));
                return;
            };
            let found = if tab == 7 {
                bosun_core::finder::large_files(&home, &|| false)
            } else {
                let mut v: Vec<_> = bosun_core::finder::user_folders()
                    .iter()
                    .flat_map(|d| bosun_core::finder::empty_folders(d, &|| false))
                    .collect();
                v.sort_by(|a, b| a.path.cmp(&b.path));
                v
            };
            let _ = tx.send((tab, found));
        });
        if tab == 7 {
            self.large = None;
        } else {
            self.empty = None;
        }
        self.finder_rx = Some(rx);
    }

    /// Picks up finished searches; `true` while any is still running.
    /// Runs `bosun.exe --measure-startup` through the administrator prompt;
    /// `poll_drivers` picks up the result.
    pub fn measure(&mut self) {
        if self.measure_rx.is_some() {
            return;
        }
        let (Ok(exe), Some(out)) = (
            std::env::current_exe(),
            bosun_core::startup_impact::cache_path(),
        ) else {
            self.measure_failed = true;
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let args = format!("--measure-startup \"{}\"", out.display());
            let ok = matches!(
                bosun_core::elevate::run(&exe.to_string_lossy(), &args, &|| false),
                Ok(bosun_core::elevate::Outcome::Exited(0))
            );
            let _ = tx.send(ok);
        });
        self.measure_failed = false;
        self.measure_rx = Some(rx);
    }

    fn boot(&self, it: &Item) -> Option<&bosun_core::startup_impact::Measured> {
        let exe = it
            .exe
            .as_ref()?
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        self.impact.as_ref()?.1.iter().find(|m| m.exe == exe)
    }

    pub fn poll_drivers(&mut self) -> bool {
        if let Some(rx) = &self.measure_rx {
            match rx.try_recv() {
                Ok(ok) => {
                    self.measure_failed = !ok;
                    if ok {
                        self.impact = bosun_core::startup_impact::load();
                    }
                    self.measure_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.measure_failed = true;
                    self.measure_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.finder_rx {
            match rx.try_recv() {
                Ok((tab, v)) => {
                    if tab == 7 {
                        self.large = Some(v);
                    } else {
                        self.empty = Some(v);
                    }
                    self.finder_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.finder_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.finder_mode
            && self.finder_rx.is_none()
            && ((self.tab == 7 && self.large.is_none()) || (self.tab == 8 && self.empty.is_none()))
        {
            self.search_finder();
        }
        if let Some(rx) = &self.driver_rx {
            match rx.try_recv() {
                Ok(v) => {
                    self.drivers = Some(v);
                    self.driver_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.drivers = Some(Vec::new());
                    self.driver_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.driver_rx.is_some() || self.finder_rx.is_some() || self.measure_rx.is_some()
    }

    /// The App/Extension Cleaner: Programs (tab 5) and Browser Extensions (tab 4).
    pub fn open_apps() -> Self {
        let mut o = Self::open(5);
        o.apps_mode = true;
        o
    }

    /// Loads the current tab's entries if it hasn't been opened yet.
    pub fn ensure(&mut self) {
        if self.tab == 5 && self.programs.is_none() {
            let list = bosun_core::apps::list();
            self.prog_icons = list.iter().map(|a| file_icon(a.icon.as_deref())).collect();
            self.programs = Some(list);
        }
        if self.tab == 2 && self.services.is_none() {
            let list = bosun_core::services::list();
            self.svc_icons = list.iter().map(|s| file_icon(s.exe.as_deref())).collect();
            self.services = Some(list);
        }
        if self.tab == 3 && self.tasks.is_none() {
            self.tasks = Some(bosun_core::tasks::list());
        }
        if self.tab == 4 && self.extensions.is_none() {
            let list = bosun_core::extensions::list();
            let mut seen: Vec<Browser> = list.iter().map(|e| e.browser).collect();
            seen.dedup();
            self.browser_icons = seen
                .into_iter()
                .map(|b| (b, file_icon(b.exe_path().as_deref())))
                .collect();
            self.extensions = Some(list);
        }
    }

    /// Re-reads the startup entries (after a change or Refresh).
    pub fn reload(&mut self) {
        self.free_icons();
        self.items = bosun_core::startup::list();
        self.checked = vec![false; self.items.len()];
        // the Ignore List (Settings) survives restarts
        let ignored = bosun_core::ignore::keys(bosun_core::ignore::List::Startup);
        self.ignored = self
            .items
            .iter()
            .map(|it| ignored.contains(&it.name))
            .collect();
        self.icons = self.items.iter().map(item_icon).collect();
        self.services = None;
        self.tasks = None;
        self.extensions = None;
        self.programs = None;
        self.scroll = 0.0;
        self.ensure();
    }

    fn free_icons(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        let all: Vec<HICON> = self
            .icons
            .drain(..)
            .chain(self.svc_icons.drain(..))
            .chain(self.browser_icons.drain(..).map(|(_, i)| i))
            .chain(self.prog_icons.drain(..))
            .collect();
        for i in all {
            if !i.is_invalid() {
                // SAFETY: icons we got from SHGetFileInfoW and own.
                unsafe {
                    let _ = DestroyIcon(i);
                }
            }
        }
    }

    /// Item indices listed on the current tab.
    pub fn listed(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| match self.tab {
                0 => {
                    self.items[i].enabled
                        && advice(&self.items[i]) != Advice::Keep
                        && !self.ignored[i]
                }
                1 => true,
                _ => false,
            })
            .collect()
    }

    /// Indices listed on the Services or Scheduled Tasks tab.
    fn listed_other(&self) -> Vec<usize> {
        match self.tab {
            2 => self.services.as_ref().map_or_else(Vec::new, |v| {
                (0..v.len())
                    .filter(|&i| self.show_system || !v[i].system)
                    .collect()
            }),
            3 => self
                .tasks
                .as_ref()
                .map_or_else(Vec::new, |v| (0..v.len()).collect()),
            4 => self
                .extensions
                .as_ref()
                .map_or_else(Vec::new, |v| (0..v.len()).collect()),
            6 => self
                .drivers
                .as_ref()
                .map_or_else(Vec::new, |v| (0..v.len()).collect()),
            7 => self
                .large
                .as_ref()
                .map_or_else(Vec::new, |v| (0..v.len()).collect()),
            8 => self
                .empty
                .as_ref()
                .map_or_else(Vec::new, |v| (0..v.len()).collect()),
            5 => self.programs.as_ref().map_or_else(Vec::new, |v| {
                let mut idx: Vec<usize> = (0..v.len()).collect();
                match self.sort {
                    1 => idx.sort_by_key(|&i| std::cmp::Reverse(v[i].size)),
                    2 => idx.sort_by(|&a, &b| v[b].installed.cmp(&v[a].installed)),
                    _ => {}
                }
                idx
            }),
            _ => Vec::new(),
        }
    }

    /// Rows on the current tab, whatever it lists.
    pub fn row_count(&self) -> usize {
        if self.tab >= 2 {
            return self.view_other().rows.len();
        }
        self.listed().len()
    }

    pub fn view(&self) -> OptView {
        if self.tab >= 2 {
            return self.view_other();
        }
        let rows = self
            .listed()
            .into_iter()
            .map(|i| {
                let it = &self.items[i];
                Row {
                    idx: i,
                    name: it.name.trim_end_matches(".lnk").to_string(),
                    a: if self.tab == 1 {
                        // Boot time: CPU it used at the measured sign-in
                        self.boot(it).map_or("—".into(), |m| boot_time(m.cpu_ms))
                    } else {
                        match it.source {
                            Source::UserFolder | Source::CommonFolder => "Startup folder",
                            _ => "Registry",
                        }
                        .into()
                    },
                    b: if self.tab == 1 {
                        self.boot(it).map_or("—".into(), |m| {
                            bosun_core::startup_impact::Impact::of(m)
                                .label()
                                .to_string()
                        })
                    } else if it.source.machine_wide() {
                        "All users".into()
                    } else {
                        "This user".into()
                    },
                    advice: advice(it),
                    pill: if it.enabled { Pill::On } else { Pill::Off },
                    checked: self.checked[i],
                    icon: self.icons[i].0 as isize,
                    group: false,
                    c: String::new(),
                }
            })
            .collect();
        OptView {
            tab: self.tab,
            rows,
            total: self.items.len(),
            scroll: self.scroll,
            show_system: self.show_system,
            apps_mode: self.apps_mode,
            sort: self.sort,
            drivers_mode: self.drivers_mode,
            loading: self.driver_rx.is_some() || self.finder_rx.is_some(),
            measure: if self.measure_rx.is_some() {
                Measure::Running
            } else if self.measure_failed {
                Measure::Failed
            } else {
                self.impact
                    .as_ref()
                    .map_or(Measure::Never, |(at, _)| Measure::At(*at))
            },
            finder_mode: self.finder_mode,
        }
    }

    fn view_other(&self) -> OptView {
        let idx = self.listed_other();
        let rows: Vec<Row> = match self.tab {
            7 | 8 => {
                let v = if self.tab == 7 {
                    self.large.as_deref()
                } else {
                    self.empty.as_deref()
                };
                let v = v.unwrap_or_default();
                idx.iter()
                    .map(|&i| {
                        let p = &v[i].path;
                        Row {
                            idx: i,
                            name: p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            a: p.parent()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default(),
                            b: if self.tab == 7 {
                                bosun_core::format::bytes(v[i].size)
                            } else {
                                String::new()
                            },
                            advice: Advice::Optional,
                            pill: Pill::On,
                            checked: false,
                            icon: 0,
                            group: false,
                            c: String::new(),
                        }
                    })
                    .collect()
            }
            6 => {
                let v = self.drivers.as_deref().unwrap_or_default();
                idx.iter()
                    .map(|&i| Row {
                        idx: i,
                        name: v[i].title.clone(),
                        a: v[i].class.clone(),
                        b: v[i].date.clone(),
                        advice: Advice::Optional,
                        pill: Pill::On,
                        checked: false,
                        icon: 0,
                        group: false,
                        c: String::new(),
                    })
                    .collect()
            }
            5 => {
                let v = self.programs.as_deref().unwrap_or_default();
                idx.iter()
                    .map(|&i| {
                        let a = &v[i];
                        Row {
                            idx: i,
                            name: a.name.clone(),
                            a: a.publisher.clone(),
                            b: if a.size > 0 {
                                bosun_core::format::bytes(a.size)
                            } else {
                                String::new()
                            },
                            advice: Advice::Optional,
                            pill: Pill::On,
                            checked: false,
                            icon: self.prog_icons[i].0 as isize,
                            group: false,
                            c: a.date(),
                        }
                    })
                    .collect()
            }
            2 => {
                let v = self.services.as_deref().unwrap_or_default();
                idx.iter()
                    .map(|&i| {
                        let s = &v[i];
                        Row {
                            idx: i,
                            name: s.display.clone(),
                            a: s.start.label().into(),
                            b: if s.running { "Running" } else { "Stopped" }.into(),
                            advice: if s.system {
                                Advice::Keep
                            } else {
                                Advice::Optional
                            },
                            pill: match s.start {
                                Start::Automatic => Pill::On,
                                Start::Manual => Pill::Manual,
                                _ => Pill::Off,
                            },
                            checked: false,
                            icon: self.svc_icons[i].0 as isize,
                            group: false,
                            c: String::new(),
                        }
                    })
                    .collect()
            }
            4 => {
                let v = self.extensions.as_deref().unwrap_or_default();
                let icon = |b: Browser| {
                    self.browser_icons
                        .iter()
                        .find(|(x, _)| *x == b)
                        .map_or(0, |(_, i)| i.0 as isize)
                };
                let mut rows = Vec::new();
                let mut last: Option<Browser> = None;
                for &i in &idx {
                    let e = &v[i];
                    if last != Some(e.browser) {
                        last = Some(e.browser);
                        rows.push(Row {
                            idx: usize::MAX,
                            name: e.browser.label().into(),
                            a: String::new(),
                            b: String::new(),
                            advice: Advice::Optional,
                            pill: Pill::On,
                            checked: false,
                            icon: 0,
                            group: true,
                            c: String::new(),
                        });
                    }
                    rows.push(Row {
                        idx: i,
                        name: e.name.clone(),
                        a: e.version.clone(),
                        b: e.profile.clone(),
                        advice: Advice::Optional,
                        pill: Pill::On,
                        checked: false,
                        icon: icon(e.browser),
                        group: false,
                        c: String::new(),
                    });
                }
                rows
            }
            _ => {
                let v = self.tasks.as_deref().unwrap_or_default();
                idx.iter()
                    .map(|&i| {
                        let t = &v[i];
                        // folder path without per-account SID segments
                        let folder = t
                            .path
                            .rsplit_once('\\')
                            .map_or("", |(d, _)| d)
                            .split('\\')
                            .filter(|seg| !seg.starts_with("S-1-5-"))
                            .collect::<Vec<_>>()
                            .join("\\");
                        Row {
                            idx: i,
                            name: task_name(&t.name),
                            a: if folder.is_empty() {
                                "\\".into()
                            } else {
                                folder
                            },
                            b: String::new(),
                            advice: Advice::Optional,
                            pill: if t.enabled { Pill::On } else { Pill::Off },
                            checked: false,
                            icon: 0,
                            group: false,
                            c: String::new(),
                        }
                    })
                    .collect()
            }
        };
        let total = match self.tab {
            4 => self.extensions.as_ref().map_or(0, Vec::len),
            5 => self.programs.as_ref().map_or(0, Vec::len),
            6 => self.drivers.as_ref().map_or(0, Vec::len),
            7 => self.large.as_ref().map_or(0, Vec::len),
            8 => self.empty.as_ref().map_or(0, Vec::len),
            2 => self.services.as_ref().map_or(0, |v| {
                v.iter().filter(|s| self.show_system || !s.system).count()
            }),
            _ => rows.len(),
        };
        OptView {
            tab: self.tab,
            rows,
            total,
            scroll: self.scroll,
            show_system: self.show_system,
            apps_mode: self.apps_mode,
            sort: self.sort,
            drivers_mode: self.drivers_mode,
            loading: self.driver_rx.is_some() || self.finder_rx.is_some(),
            measure: if self.measure_rx.is_some() {
                Measure::Running
            } else if self.measure_failed {
                Measure::Failed
            } else {
                self.impact
                    .as_ref()
                    .map_or(Measure::Never, |(at, _)| Measure::At(*at))
            },
            finder_mode: self.finder_mode,
        }
    }

    /// Scrolls by `dy` DIPs, kept within the list for a window `h` tall.
    pub fn scroll_by(&mut self, dy: f32, h: f32) {
        let n = self.row_count() as f32;
        let visible = table_bottom(h) - HEADER_BOTTOM;
        let max = (n * ROW_H - visible).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
    }
}

impl Drop for Optimizer {
    fn drop(&mut self) {
        self.free_icons();
    }
}

/// "OneDrive Startup Task-S-1-5-21-…" → "OneDrive Startup Task": per-user
/// tasks carry the account's SID, which means nothing to a reader.
pub fn task_name(name: &str) -> String {
    match name.find("-S-1-5-") {
        Some(i) => name[..i].to_string(),
        None => name.to_string(),
    }
}

fn table_bottom(h: f32) -> f32 {
    h - 74.0
}

/// The tabs shown: (tab id, label) by position.
fn tab_list(apps_mode: bool, drivers_mode: bool, finder_mode: bool) -> Vec<(u8, &'static str)> {
    if finder_mode {
        vec![(7, "Large Files"), (8, "Empty Folders")]
    } else if drivers_mode {
        vec![(6, "Driver Updates")]
    } else if apps_mode {
        vec![(5, "Programs"), (4, "Browser Extensions")]
    } else {
        TABS.iter()
            .enumerate()
            .map(|(i, t)| (i as u8, *t))
            .collect()
    }
}

fn tab_rect(i: usize) -> D2D_RECT_F {
    rect(
        TAB_X + TAB_W * i as f32,
        TAB_TOP,
        TAB_W,
        PANEL_TOP - TAB_TOP,
    )
}

fn optimize_button() -> D2D_RECT_F {
    rect(775.0, 111.0, 141.0, 30.0)
}

fn row_center(o: &OptView, k: usize) -> f32 {
    FIRST_ROW + ROW_H * k as f32 - o.scroll
}

/// Columns: (header, x) for each tab's table.
fn columns(tab: u8) -> [(&'static str, f32); 5] {
    match tab {
        2 => {
            return [
                ("Name", 49.0),
                ("Starts", 357.0),
                ("Now", 486.0),
                ("Recommendation", 599.0),
                ("Status & Operation", 743.0),
            ];
        }
        7 | 8 => {
            return [
                ("Name", 49.0),
                ("Location", 357.0),
                (if tab == 7 { "Size" } else { "" }, 660.0),
                ("", 740.0),
                ("Operation", 778.0),
            ];
        }
        6 => {
            return [
                ("Name", 49.0),
                ("Type", 560.0),
                ("Date", 660.0),
                ("", 740.0),
                ("Operation", 778.0),
            ];
        }
        5 => {
            return [
                ("Name", 49.0),
                ("Publisher", 357.0),
                ("Size", 560.0),
                ("Installed", 660.0),
                ("Operation", 778.0),
            ];
        }
        4 => {
            return [
                ("Name", 49.0),
                ("Version", 380.0),
                ("Profile", 525.0),
                ("", 600.0),
                ("Operation", 732.0),
            ];
        }
        3 => {
            return [
                ("Name", 49.0),
                ("Folder", 357.0),
                ("", 486.0),
                ("Recommendation", 599.0),
                ("Status & Operation", 743.0),
            ];
        }
        _ => {}
    }
    if tab == 0 {
        [
            ("Name", 79.0),
            ("Starts from", 408.0),
            ("Scope", 521.0),
            ("Recommendation", 634.0),
            ("Operation", 778.0),
        ]
    } else {
        // the reference's Startup Items columns: Boot time, then our Impact
        // (Task Manager's grading) in place of IObit's crowd "Disable rate"
        [
            ("Name", 49.0),
            ("Boot time", 357.0),
            ("Impact", 486.0),
            ("Recommendation", 599.0),
            ("Status & Operation", 743.0),
        ]
    }
}

pub fn hit(v: &View, o: &OptView, x: f32, y: f32) -> Hit {
    if (x - 26.0).hypot(y - 23.5) <= 14.0 {
        return Hit::OptBack;
    }
    if o.tab == 1 && o.measure != Measure::Running && contains(&measure_link(), x, y) {
        return Hit::OptMeasure;
    }
    let tabs = tab_list(o.apps_mode, o.drivers_mode, o.finder_mode);
    if let Some(i) = (0..tabs.len()).find(|&i| contains(&tab_rect(i), x, y)) {
        return Hit::OptTab(tabs[i].0);
    }
    if o.tab == 5 && (170.0..188.0).contains(&y) {
        for (k, x0) in [(0u8, 49.0), (1, 560.0), (2, 660.0)] {
            if (x0..x0 + 80.0).contains(&x) {
                return Hit::OptSort(k);
            }
        }
    }
    let foot = v.h - 54.0;
    if o.tab < 4 && contains(&rect(TABLE_X, foot - 10.0, 90.0, 20.0), x, y) {
        return Hit::OptRescue;
    }
    if contains(&rect(866.0, foot - 10.0, 52.0, 20.0), x, y) {
        return Hit::OptRefresh;
    }
    if o.tab == 2 && contains(&rect(700.0, foot - 10.0, 150.0, 20.0), x, y) {
        return Hit::OptSystem;
    }
    if o.tab == 0 && contains(&optimize_button(), x, y) {
        return Hit::OptGo;
    }
    if o.tab == 0 && contains(&rect(50.0, 170.0, 20.0, 18.0), x, y) {
        return Hit::OptCheckAll;
    }
    if !(HEADER_BOTTOM..table_bottom(v.h)).contains(&y) {
        return Hit::None;
    }
    for (k, r) in o.rows.iter().enumerate() {
        let cy = row_center(o, k);
        if (y - cy).abs() > ROW_H / 2.0 {
            continue;
        }
        return match o.tab {
            0 if contains(&rect(775.0, cy - 10.0, 50.0, 20.0), x, y) => Hit::OptIgnore(r.idx),
            0 if x < 360.0 => Hit::OptCheck(r.idx),
            1..=3 if contains(&rect(744.0, cy - 11.0, 125.0, 22.0), x, y) => Hit::OptStatus(r.idx),
            5..=8 if contains(&rect(776.0, cy - 10.0, 70.0, 20.0), x, y) => Hit::OptRemove(r.idx),
            4 if !r.group && contains(&rect(735.0, cy - 10.0, 60.0, 20.0), x, y) => {
                Hit::OptRemove(r.idx)
            }
            _ => Hit::None,
        };
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

fn link(p: &Painter, s: &str, x: f32, cap_y: f32, hover: bool, right: bool) {
    let f = &p.g.f.m13;
    let w = p.g.measure(s, f);
    let x = if right { x - w } else { x };
    let c = if hover { 0xFFFFFF } else { oc::TEXT };
    text_at(p, s, f, 13.0, x, w + 4.0, cap_y, c);
    p.line(x, cap_y + 7.0, x + w, cap_y + 7.0, 1.0, col(c));
}

pub fn draw(p: &Painter, v: &View, o: &OptView) {
    let f = &p.g.f;
    // back arrow ↶
    let c = col(if v.hover == Hit::OptBack {
        0xFFFFFF
    } else {
        0xB0B0B0
    });
    // measured: arrowhead tip (19.5, 18.5) pointing left, a line into a U-turn
    // of radius 5 on the right, and a bottom stroke back to x 23
    p.line(19.5, 18.5, 30.0, 18.5, 1.6, c);
    p.line(19.5, 18.5, 23.0, 15.0, 1.6, c);
    p.line(19.5, 18.5, 23.0, 22.0, 1.6, c);
    p.arc_at(30.0, 23.5, 5.0, 1.6, 0.0, 0.5, c);
    p.line(30.0, 28.5, 23.0, 28.5, 1.6, c);
    // tabs and panel
    let bottom = v.h - 23.0;
    p.fill_rr(
        rect(TAB_X, PANEL_TOP, 919.0, bottom - PANEL_TOP),
        8.0,
        col(oc::PANEL),
    );
    for (i, (id, name)) in tab_list(o.apps_mode, o.drivers_mode, o.finder_mode)
        .into_iter()
        .enumerate()
    {
        let r = tab_rect(i);
        let on = o.tab == id;
        if on {
            p.fill_rr(
                rect(r.left, r.top, r.right - r.left, 60.0),
                8.0,
                col(oc::PANEL),
            );
        }
        let c = if on || v.hover == Hit::OptTab(id) {
            0xFFFFFF
        } else {
            0xC8C8C8
        };
        let fnt = &f.m14sb;
        let cy = 69.0 - 0.064 * 14.0;
        p.text(
            name,
            fnt,
            rect(r.left, cy - 14.0, TAB_W, 28.0),
            col(c),
            Align::Center,
        );
    }
    match o.tab {
        0 => draw_quick_head(p, v, o),
        7 | 8 if o.loading => text_at(
            p,
            "Searching your folders…",
            &f.pop_label,
            15.0,
            38.0,
            860.0,
            123.5,
            oc::TEXT,
        ),
        6 if o.loading => text_at(
            p,
            "Checking Windows Update for driver updates…",
            &f.pop_label,
            15.0,
            38.0,
            860.0,
            123.5,
            oc::TEXT,
        ),
        1..=8 => {
            let n = o.total.to_string();
            let wn = p.g.measure(&n, &f.pop_label);
            text_at(p, &n, &f.pop_label, 15.0, 38.0, wn + 2.0, 123.5, oc::ORANGE);
            let rest = match o.tab {
                1 => " startup items are found. Unneeded ones slow down sign-in.",
                2 => {
                    " services are found. System startup may be slowed down by the unnecessary ones."
                }
                4 => " extensions are found. You can remove the unwanted ones as per your needs.",
                5 => " programs are installed. Uninstall the ones you no longer use.",
                6 => " driver updates are offered by Windows Update for this PC.",
                7 => " large files (100 MB or more) are in your folders.",
                8 => " empty folders are in your folders.",
                _ => {
                    " scheduled tasks are found. System startup may be slowed down by the unnecessary ones."
                }
            };
            text_at(
                p,
                rest,
                &f.pop_label,
                15.0,
                38.0 + wn,
                860.0,
                123.5,
                oc::TEXT,
            );
        }
        _ => {}
    }
    if o.tab == 1 {
        let t = measure_text(o.measure);
        let link = o.measure != Measure::Running;
        let c = match (link, v.hover == Hit::OptMeasure) {
            (false, _) => oc::TEXT,
            (true, true) => 0x8CC4FF,
            (true, false) => 0x61ADF8,
        };
        let w = p.g.measure(&t, &f.m13);
        let r = measure_link();
        text_at(p, &t, &f.m13, 13.0, r.right - w, w + 4.0, 123.5, c);
        if link {
            p.line(r.right - w, 131.0, r.right, 131.0, 1.0, col(c));
        }
    }
    let tb = table_bottom(v.h);
    let table = rect(TABLE_X, TABLE_TOP, 879.0, tb - TABLE_TOP);
    p.fill(table, col(oc::TABLE));
    p.stroke_rr(table, 0.0, 1.0, col(oc::LINE));
    {
        let cols = columns(o.tab);
        for (k, (label, x)) in cols.iter().enumerate() {
            text_at(p, label, &f.m12, 12.0, *x, 160.0, 178.0, oc::DIM);
            // thin divider before the next column, and after the last (reference)
            let next = cols.get(k + 1).map_or(888.0, |c| c.1 - 10.0);
            if k > 0 || o.tab >= 1 {
                p.line(next, 172.0, next, 185.0, 1.0, col(oc::LINE));
            }
        }
        if o.tab == 0 {
            let all = !o.rows.is_empty() && o.rows.iter().all(|r| r.checked);
            crate::pages::scan::checkbox(p, 59.5, 178.5, all, 1.0);
        }
        p.line(
            TABLE_X,
            HEADER_BOTTOM - 0.5,
            TABLE_X + 879.0,
            HEADER_BOTTOM - 0.5,
            1.0,
            col(oc::LINE),
        );
        p.clip(rect(TABLE_X, HEADER_BOTTOM, 879.0, tb - HEADER_BOTTOM));
        for (k, r) in o.rows.iter().enumerate() {
            let cy = row_center(o, k);
            if cy < HEADER_BOTTOM - ROW_H || cy > tb + ROW_H {
                continue;
            }
            draw_row(p, v, o, r, cy);
        }
        p.unclip();
        draw_scrollbar(p, o, tb);
        if o.rows.is_empty() {
            let msg = match o.tab {
                0 => "Nothing to optimize: every startup item is either needed or already off.",
                1 => "No startup items found.",
                2 => "No non-Windows services found.",
                4 => "No browser extensions found.",
                5 => "No installed programs found.",
                6 if o.loading => "Asking Windows Update… this can take a minute.",
                7 | 8 if o.loading => "Searching your folders… this can take a minute.",
                7 => "No files of 100 MB or more in your folders.",
                8 => "No empty folders in your folders.",
                6 => "Your drivers are up to date: Windows Update offers no driver updates.",
                _ => "No scheduled tasks found outside Windows' own.",
            };
            p.text(
                msg,
                &f.m13,
                rect(TABLE_X, 300.0, 879.0, 20.0),
                col(oc::DIM),
                Align::Center,
            );
        }
    }
    let foot = v.h - 54.0;
    // Browser Startups has no Rescue Center link in the reference
    if o.tab < 4 {
        link(
            p,
            "Rescue Center",
            TABLE_X,
            foot,
            v.hover == Hit::OptRescue,
            false,
        );
    }
    link(p, "Refresh", 916.0, foot, v.hover == Hit::OptRefresh, true);
    if o.tab == 2 {
        let label = if o.show_system {
            "Hide System Services"
        } else {
            "Show System Services"
        };
        link(p, label, 848.0, foot, v.hover == Hit::OptSystem, true);
    }
}

/// "1.2 s", "340 ms".
fn boot_time(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

/// Startup Items: the Measure link, right-aligned on the banner line.
fn measure_link() -> D2D_RECT_F {
    rect(916.0 - 260.0, 113.0, 260.0, 21.0)
}

fn measure_text(m: Measure) -> String {
    match m {
        Measure::Never => "Measure startup impact".into(),
        Measure::Running => "Measuring… (approve Windows' prompt)".into(),
        Measure::Failed => match bosun_core::startup_impact::last_error() {
            Some(e) => format!("Couldn't measure ({e}) · Try again"),
            None => "Not measured (prompt declined?) · Try again".into(),
        },
        Measure::At(t) => format!("Measured {} · Again", crate::pages::rescue::when(t * 1000)),
    }
}

fn draw_quick_head(p: &Painter, v: &View, o: &OptView) {
    let f = &p.g.f;
    let (cx, cy) = (53.5, 126.5);
    p.ring(cx, cy, 15.5, 1.8, col(oc::ORANGE));
    p.circle(cx, cy - 6.0, 1.6, col(oc::ORANGE));
    p.line(cx, cy - 2.0, cx, cy + 7.0, 2.2, col(oc::ORANGE));
    let n = o.rows.len();
    text_at(
        p,
        &format!("{n} startup items can be turned off"),
        &f.pop_label,
        15.0,
        83.0,
        600.0,
        115.0,
        oc::TEXT,
    );
    text_at(
        p,
        "Fewer startup items mean a faster sign-in. You can turn any of them back on later.",
        &f.m13,
        13.0,
        82.0,
        640.0,
        134.0,
        oc::DIM,
    );
    let b = optimize_button();
    let x = (b.left + b.right) / 2.0;
    let stops = [(0.0, col(0x8DC3F8)), (1.0, col(0x6DB4F7))];
    p.fill_rr_linear(b, 15.0, (x, b.top), (x, b.bottom), &stops);
    if v.hover == Hit::OptGo {
        p.fill_rr(b, 15.0, cola(0xFFFFFF, 0.14));
    }
    p.text("Optimize", &f.m14sb, b, col(0x141414), Align::Center);
}

fn draw_row(p: &Painter, v: &View, o: &OptView, r: &Row, cy: f32) {
    let f = &p.g.f;
    if r.group {
        // "Microsoft Edge ───────── ▾" (reference)
        let w = p.g.measure(&r.name, &f.pop_label);
        text_at(p, &r.name, &f.pop_label, 15.0, 53.0, w + 4.0, cy, oc::TEXT);
        p.line(53.0 + w + 16.0, cy, 874.0, cy, 1.0, col(oc::LINE));
        p.caret_down(886.0, cy + 0.5, 8.0, col(0xC8C8C8));
        return;
    }
    let hot = match v.hover {
        Hit::OptCheck(i) | Hit::OptStatus(i) | Hit::OptIgnore(i) => i == r.idx,
        _ => false,
    };
    if hot {
        p.fill(
            rect(TABLE_X + 1.0, cy - ROW_H / 2.0, 877.0, ROW_H),
            cola(0xFFFFFF, 0.05),
        );
    }
    let cols = columns(o.tab);
    let (icon_x, name_x) = if o.tab == 0 {
        (77.0, 118.0)
    } else {
        (49.0, 91.0)
    };
    if o.tab == 0 {
        crate::pages::scan::checkbox(p, 59.5, cy, r.checked, 1.0);
    }
    draw_icon(p, r.icon, icon_x, cy);
    text_at(
        p,
        &r.name,
        &f.pop_label,
        14.0,
        name_x,
        cols[1].1 - name_x - 16.0,
        cy - 0.5,
        oc::TEXT,
    );
    // each info column stops 10 px short of the next one
    let a_w = if r.b.is_empty() {
        cols[3].1 - cols[1].1 - 16.0
    } else {
        cols[2].1 - cols[1].1 - 10.0
    };
    text_at(p, &r.a, &f.m13, 13.0, cols[1].1, a_w, cy, oc::TEXT);
    text_at(p, &r.b, &f.m13, 13.0, cols[2].1, 110.0, cy, oc::TEXT);
    if o.tab == 7 || o.tab == 8 {
        link(
            p,
            "Recycle",
            778.0,
            cy,
            v.hover == Hit::OptRemove(r.idx),
            false,
        );
        return;
    }
    if o.tab == 6 {
        link(
            p,
            "Install",
            778.0,
            cy,
            v.hover == Hit::OptRemove(r.idx),
            false,
        );
        return;
    }
    if o.tab == 5 {
        text_at(p, &r.c, &f.m13, 13.0, cols[3].1, 110.0, cy, oc::TEXT);
        link(
            p,
            "Uninstall",
            778.0,
            cy,
            v.hover == Hit::OptRemove(r.idx),
            false,
        );
        return;
    }
    if o.tab == 4 {
        link(
            p,
            "Remove",
            737.0,
            cy,
            v.hover == Hit::OptRemove(r.idx),
            false,
        );
        return;
    }
    let (adv, ac) = match r.advice {
        Advice::Keep => ("Enable", oc::TEXT),
        Advice::Optional => ("Optional", oc::TEXT),
        Advice::Disable => ("Disable", oc::ORANGE),
    };
    text_at(p, adv, &f.m13, 13.0, cols[3].1, 130.0, cy, ac);
    if o.tab == 0 {
        link(
            p,
            "Ignore",
            cols[4].1,
            cy,
            v.hover == Hit::OptIgnore(r.idx),
            false,
        );
    } else {
        status_pill(p, r.pill, cy, v.hover == Hit::OptStatus(r.idx));
    }
}

/// "Enabled" (blue, left) or "Disabled" (grey, right) in a dark track, with
/// the menu caret beside it.
fn status_pill(p: &Painter, pill: Pill, cy: f32, hover: bool) {
    let track = rect(744.0, cy - 11.0, 104.0, 22.0);
    p.fill_rr(track, 11.0, col(0x2A2A2A));
    let (r, label, top, bottom) = if pill == Pill::Manual {
        (
            rect(778.0, cy - 11.0, 70.0, 22.0),
            "Manual",
            0xB4B4B4,
            0x949494,
        )
    } else if pill == Pill::On {
        (
            rect(744.0, cy - 11.0, 70.0, 22.0),
            "Enabled",
            0x8DC3F8,
            0x6DB4F7,
        )
    } else {
        (
            rect(778.0, cy - 11.0, 70.0, 22.0),
            "Disabled",
            0x9A9A9A,
            0x7A7A7A,
        )
    };
    let x = (r.left + r.right) / 2.0;
    p.fill_rr_linear(
        r,
        11.0,
        (x, r.top),
        (x, r.bottom),
        &[(0.0, col(top)), (1.0, col(bottom))],
    );
    p.text(label, &p.g.f.m12, r, col(0x141414), Align::Center);
    let c = col(if hover { 0xFFFFFF } else { 0xC8C8C8 });
    p.caret_down(864.0, cy + 0.5, 8.0, c);
}

fn draw_icon(p: &Painter, icon: isize, x: f32, cy: f32) {
    let r = rect(x, cy - 15.5, 31.0, 31.0);
    let bmp = (icon != 0)
        .then(|| p.g.icon_bitmap(p.rt, HICON(icon as _)))
        .flatten();
    match bmp {
        Some(b) => p.bitmap(&b, r, 1.0),
        None => {
            p.stroke_rr(
                rect(x + 3.0, cy - 12.5, 25.0, 25.0),
                4.0,
                1.6,
                col(0x5A5A5A),
            );
            p.fill(rect(x + 9.0, cy - 6.5, 13.0, 13.0), col(0x4A4A4A));
        }
    }
}

fn draw_scrollbar(p: &Painter, o: &OptView, tb: f32) {
    let visible = tb - HEADER_BOTTOM;
    let content = o.rows.len() as f32 * ROW_H;
    if content <= visible {
        return;
    }
    let h = (visible * visible / content).max(30.0);
    let y = HEADER_BOTTOM + (visible - h) * (o.scroll / (content - visible));
    p.fill_rr(rect(907.0, y + 4.0, 5.0, h - 8.0), 2.5, col(0x4A4A4A));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_names_lose_the_account_sid() {
        assert_eq!(
            task_name("OneDrive Startup Task-S-1-5-21-1665643400-1983033961-3832323990-1001"),
            "OneDrive Startup Task"
        );
        assert_eq!(task_name("WindhawkRunUITask"), "WindhawkRunUITask");
    }

    #[test]
    fn tabs_and_controls_sit_where_the_reference_has_them() {
        let t = tab_rect(1);
        assert_eq!(
            (t.left, t.top, t.right, t.bottom),
            (201.0, 45.0, 385.0, 90.0)
        );
        assert_eq!(optimize_button().right, 916.0);
        let v = crate::window::test_view();
        let o = OptView {
            tab: 1,
            ..Default::default()
        };
        assert_eq!(hit(&v, &o, 300.0, 68.0), Hit::OptTab(1));
        assert_eq!(hit(&v, &o, 26.0, 21.0), Hit::OptBack);
        assert_eq!(hit(&v, &o, 900.0, 606.0), Hit::OptRefresh);
        assert_eq!(hit(&v, &o, 60.0, 606.0), Hit::OptRescue);
    }
}
