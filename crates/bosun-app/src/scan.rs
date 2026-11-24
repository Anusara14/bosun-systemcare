//! The Care scan: a worker thread runs each module in turn and publishes its
//! progress through a mutex; the window polls it on a ~30 fps timer while
//! scanning and stops polling when the worker is done. Scanning is a dry run:
//! nothing is changed until the user presses Fix, which runs a second worker
//! over the ticked modules through `bosun_core::clean` (journal first).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bosun_core::clean::{self, Cleaned};
use bosun_core::junk::{self, Tally, Target};
use bosun_core::security::{self, Provider};
use bosun_core::{registry, shortcuts};

/// A module's row in the list.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Waiting,
    Running,
    Done(Tally),
    /// A status check (antivirus, firewall): healthy or not.
    Health(bool),
    Skipped,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Privacy,
    Junk,
    Shortcuts,
    Registry,
    Antivirus,
    Firewall,
}

impl Kind {
    /// Fix can act on what this module finds.
    pub fn fixable(self) -> bool {
        !matches!(self, Kind::Antivirus | Kind::Firewall)
    }
}

/// A Care module: its Manual-mode label, its scan-list name, and the end of
/// "Scanning for …".
pub struct Info {
    pub kind: Kind,
    pub pick: &'static str,
    pub name: &'static str,
    pub what: &'static str,
}

const fn info(kind: Kind, pick: &'static str, name: &'static str, what: &'static str) -> Info {
    Info {
        kind,
        pick,
        name,
        what,
    }
}

/// The modules that do real work (spec §2.1). IObit's Internet Boost,
/// Registry Defrag and Spyware Removal are deliberately absent (no effect on
/// Windows 10/11, or a malware engine Defender already provides).
pub const MODULES: [Info; 6] = [
    info(
        Kind::Privacy,
        "Privacy Sweep",
        "Privacy traces",
        "privacy traces",
    ),
    info(Kind::Junk, "Junk File Clean", "Junk files", "junk files"),
    info(
        Kind::Shortcuts,
        "Shortcut Fix",
        "Invalid shortcuts",
        "invalid shortcuts",
    ),
    info(
        Kind::Registry,
        "Registry Clean",
        "Registry entries",
        "registry entries",
    ),
    info(
        Kind::Antivirus,
        "Antivirus Protection",
        "Antivirus protection",
        "antivirus protection",
    ),
    info(Kind::Firewall, "Firewall", "Firewall", "firewall"),
];

/// Every module ticked.
pub const ALL: u16 = (1 << MODULES.len()) - 1;

#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub kind: Kind,
    pub name: &'static str,
    /// Finishes "Scanning for …".
    pub what: &'static str,
    pub status: Status,
    /// Ticked in the summary: Fix cleans it.
    pub checked: bool,
    /// What Fix removed and left behind.
    pub fixed: Option<Cleaned>,
}

impl Row {
    /// Files found, if any.
    pub fn found(&self) -> Option<Tally> {
        match self.status {
            Status::Done(t) if t.files > 0 && self.kind.fixable() => Some(t),
            _ => None,
        }
    }
}

/// What the screen shows; cloned into each frame's view.
#[derive(Clone, PartialEq, Debug)]
pub struct Progress {
    pub rows: Vec<Row>,
    pub current: usize,
    /// Current module's progress, 0..=1.
    pub pct: f32,
    pub path: String,
    /// `finished` with `fixing` false is the summary; with `fixing` true, the
    /// fix result.
    pub finished: bool,
    pub fixing: bool,
    /// Why Fix couldn't start (journal not writable).
    pub error: Option<String>,
}

impl Progress {
    /// Items and bytes found in the ticked modules.
    pub fn total(&self) -> Tally {
        let mut t = Tally::default();
        for r in self.rows.iter().filter(|r| r.checked) {
            if let Some(x) = r.found() {
                t.add(x);
            }
        }
        t
    }

    /// Totals of the fix result.
    pub fn cleaned(&self) -> Cleaned {
        let mut c = Cleaned::default();
        for f in self.rows.iter().filter_map(|r| r.fixed) {
            c.removed.add(f.removed);
            c.skipped.add(f.skipped);
        }
        c
    }

    pub fn is_summary(&self) -> bool {
        self.finished && !self.fixing
    }
}

/// A running or finished scan, owned by the window.
pub struct Scan {
    pub shared: Arc<Mutex<Progress>>,
    skip: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Scan {
    pub fn snapshot(&self) -> Option<Progress> {
        self.shared.lock().ok().map(|p| p.clone())
    }
    pub fn skip(&self) {
        self.skip.store(true, Ordering::Relaxed);
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Ticks or unticks a module in the summary.
    pub fn toggle(&self, i: usize) {
        update(&self.shared, |p| {
            if p.is_summary() && p.rows.get(i).is_some_and(|r| r.found().is_some()) {
                p.rows[i].checked = !p.rows[i].checked;
            }
        });
    }

    /// Starts cleaning the ticked modules (from the summary only). With
    /// `dry_run` files are walked and journaled but not deleted.
    pub fn fix(&self, dry_run: bool) -> bool {
        let Some(p) = self.snapshot().filter(|p| p.is_summary()) else {
            return false;
        };
        let picked: Vec<usize> = (0..p.rows.len())
            .filter(|&i| p.rows[i].checked && p.rows[i].found().is_some())
            .collect();
        if picked.is_empty() {
            return false;
        }
        self.skip.store(false, Ordering::Relaxed);
        self.stop.store(false, Ordering::Relaxed);
        update(&self.shared, |p| {
            p.fixing = true;
            p.finished = false;
            p.pct = 0.0;
            p.path.clear();
            p.error = None;
        });
        let (shared, skip, stop) = (self.shared.clone(), self.skip.clone(), self.stop.clone());
        std::thread::spawn(move || run_fix(picked, dry_run, &shared, &skip, &stop));
        true
    }
}

impl Drop for Scan {
    /// Leaving the scan (Back, closing the window) ends the worker promptly.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Each module stays on screen at least this long, as in the reference,
/// so a fast module still reads as a step rather than a flicker.
const MIN_MODULE: Duration = Duration::from_millis(700);
/// The last frame (100 %) holds briefly before the summary.
const END_HOLD: Duration = Duration::from_millis(400);

fn file_targets(kind: Kind) -> Vec<Target> {
    match kind {
        Kind::Privacy => junk::privacy_targets(),
        Kind::Junk => junk::targets(),
        _ => Vec::new(),
    }
}

/// Starts scanning the modules whose bit is set in `picks` (see [`MODULES`]).
pub fn start(picks: u16) -> Scan {
    let rows: Vec<Row> = MODULES
        .iter()
        .enumerate()
        .filter(|(i, _)| picks & (1 << i) != 0)
        .map(|(_, m)| Row {
            kind: m.kind,
            name: m.name,
            what: m.what,
            status: Status::Waiting,
            checked: true,
            fixed: None,
        })
        .collect();
    let kinds: Vec<Kind> = rows.iter().map(|r| r.kind).collect();
    let shared = Arc::new(Mutex::new(Progress {
        rows,
        current: 0,
        pct: 0.0,
        path: String::new(),
        finished: false,
        fixing: false,
        error: None,
    }));
    let scan = Scan {
        shared: shared.clone(),
        skip: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let (skip, stop) = (scan.skip.clone(), scan.stop.clone());
    std::thread::spawn(move || run(kinds, &shared, &skip, &stop));
    scan
}

fn update(shared: &Mutex<Progress>, f: impl FnOnce(&mut Progress)) {
    if let Ok(mut p) = shared.lock() {
        f(&mut p);
    }
}

/// Targets differ wildly in size (Temp dwarfs the rest), so progress is the
/// share of targets done plus a creep, per folder visited, over what remains.
fn creep(dirs: u32) -> f32 {
    // half way after ~1,400 folders: Temp-sized trees end in the 90s
    1.0 - 0.9995f32.powi(dirs as i32)
}

fn module_pct(done: usize, n: usize, dirs: u32) -> f32 {
    let base = done as f32 / n.max(1) as f32;
    // only a finished module may show 100 %
    (base + (1.0 - base) * creep(dirs)).min(0.99)
}

/// Scans one module; `report(item, fraction)` moves the screen and returns
/// `false` to cancel. `None` when cancelled.
fn scan_one(kind: Kind, report: &mut dyn FnMut(&str, f32) -> bool) -> Option<Status> {
    let mut n = 0u32;
    let mut tick = |item: &str| {
        n += 1;
        report(item, creep(n * 4).min(0.99))
    };
    let status = match kind {
        Kind::Privacy | Kind::Junk => {
            let targets = file_targets(kind);
            let now = SystemTime::now();
            let mut tally = Tally::default();
            let mut dirs = 0;
            for (j, t) in targets.iter().enumerate() {
                let mut go = true;
                tally.add(junk::measure(t, now, &mut |d: &Path| {
                    dirs += 1;
                    // ponytail: string per folder; fine at ~10k folders a module
                    go = report(&d.display().to_string(), module_pct(j, targets.len(), dirs));
                    go
                }));
                if !go {
                    return None;
                }
            }
            Status::Done(tally)
        }
        Kind::Shortcuts => {
            let mut go = true;
            let found = shortcuts::find(&mut |p| {
                go = tick(&p.display().to_string());
                go
            });
            Status::Done(Tally {
                files: found.len() as u64,
                bytes: 0,
            })
        }
        Kind::Registry => {
            let mut go = true;
            let found = registry::find(&mut |name| {
                go = tick(name);
                go
            });
            Status::Done(Tally {
                files: found.len() as u64,
                bytes: 0,
            })
        }
        Kind::Antivirus | Kind::Firewall => {
            let p = if kind == Kind::Antivirus {
                Provider::Antivirus
            } else {
                Provider::Firewall
            };
            if !report("Windows Security Center", 0.5) {
                return None;
            }
            security::healthy(p).map_or(Status::Skipped, Status::Health)
        }
    };
    Some(status)
}

fn run(kinds: Vec<Kind>, shared: &Mutex<Progress>, skip: &AtomicBool, stop: &AtomicBool) {
    for (i, kind) in kinds.into_iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            update(shared, |p| p.rows[i].status = Status::Skipped);
            continue;
        }
        skip.store(false, Ordering::Relaxed);
        let began = Instant::now();
        update(shared, |p| {
            p.current = i;
            p.pct = 0.0;
            p.path.clear();
            p.rows[i].status = Status::Running;
        });
        let cancelled = || skip.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed);
        let mut report = |item: &str, pct: f32| {
            update(shared, |p| {
                p.pct = pct;
                p.path.clear();
                p.path.push_str(item);
            });
            !cancelled()
        };
        let status = match scan_one(kind, &mut report) {
            Some(s) if !cancelled() => s,
            _ => Status::Skipped,
        };
        glide(shared, began, &cancelled);
        update(shared, |p| {
            p.pct = 1.0;
            p.rows[i].status = status;
        });
    }
    if !stop.load(Ordering::Relaxed) {
        std::thread::sleep(END_HOLD);
    }
    update(shared, |p| p.finished = true);
}

/// Fixes one module, journal first; `progress(item, fraction)`.
fn fix_one(
    kind: Kind,
    name: &str,
    dir: &Path,
    dry_run: bool,
    progress: &mut dyn FnMut(&str, f32) -> bool,
) -> std::io::Result<Cleaned> {
    let counts = |(done, left): (u64, u64)| Cleaned {
        removed: Tally {
            files: done,
            bytes: 0,
        },
        skipped: Tally {
            files: left,
            bytes: 0,
        },
    };
    match kind {
        Kind::Privacy | Kind::Junk => {
            let mut p = |f: &Path, x: f32| progress(&f.display().to_string(), x);
            clean::clean(&file_targets(kind), dir, name, dry_run, &mut p).map(|(_, o)| o)
        }
        Kind::Shortcuts => {
            let found = shortcuts::find(&mut |_| true);
            let mut p = |f: &Path, x: f32| progress(&f.display().to_string(), x);
            shortcuts::fix(&found, dir, dry_run, &mut p).map(counts)
        }
        Kind::Registry => {
            let found = registry::find(&mut |_| true);
            registry::fix(&found, dir, dry_run, progress).map(counts)
        }
        Kind::Antivirus | Kind::Firewall => Ok(Cleaned::default()),
    }
}

/// Glides the ring to 100 % over a module's minimum time, like the reference.
fn glide(shared: &Mutex<Progress>, began: Instant, cancelled: &dyn Fn() -> bool) {
    while !cancelled() {
        let t = began.elapsed().as_secs_f32() / MIN_MODULE.as_secs_f32();
        update(shared, |p| p.pct = p.pct.max(t.min(1.0)));
        if t >= 1.0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn run_fix(
    picked: Vec<usize>,
    dry_run: bool,
    shared: &Mutex<Progress>,
    skip: &AtomicBool,
    stop: &AtomicBool,
) {
    let Some(dir) = clean::journal_dir() else {
        update(shared, |p| {
            p.error = Some("No place to write the journal (LOCALAPPDATA is unset).".into());
            p.fixing = false;
            p.finished = true;
        });
        return;
    };
    for i in picked {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        skip.store(false, Ordering::Relaxed);
        let began = Instant::now();
        update(shared, |p| {
            p.current = i;
            p.pct = 0.0;
            p.path.clear();
        });
        let cancelled = || skip.load(Ordering::Relaxed) || stop.load(Ordering::Relaxed);
        let Some((kind, name)) = shared.lock().ok().map(|p| (p.rows[i].kind, p.rows[i].name))
        else {
            break;
        };
        let mut progress = |item: &str, done: f32| {
            update(shared, |p| {
                p.pct = done.min(0.99);
                p.path.clear();
                p.path.push_str(item);
            });
            !cancelled()
        };
        match fix_one(kind, name, &dir, dry_run, &mut progress) {
            Ok(out) => {
                glide(shared, began, &cancelled);
                update(shared, |p| {
                    p.pct = 1.0;
                    p.rows[i].fixed = Some(out);
                });
            }
            Err(e) => {
                // Journal not writable: nothing was deleted for this module.
                update(shared, |p| {
                    p.error = Some(format!("Couldn't write the journal: {e}"))
                });
                break;
            }
        }
    }
    std::thread::sleep(END_HOLD);
    update(shared, |p| p.finished = true);
}

/// "C:\a\very\long\path" → "C:\a\ver...\path": keeps the start and the last
/// component, as the reference does, so the line fits `fits(candidate)`.
pub fn middle_ellipsis(path: &str, fits: impl Fn(&str) -> bool) -> String {
    if fits(path) {
        return path.to_string();
    }
    let chars: Vec<char> = path.chars().collect();
    let tail_at = path
        .rfind('\\')
        .map_or(chars.len(), |b| path[..b].chars().count());
    let tail: String = chars[tail_at..].iter().collect();
    for keep in (0..tail_at).rev() {
        let s = format!("{}...{}", chars[..keep].iter().collect::<String>(), tail);
        if fits(&s) {
            return s;
        }
    }
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_progress_only_rises_and_never_finishes_early() {
        let mut last = 0.0;
        for (done, dirs) in [(0, 0), (0, 50), (0, 5000), (1, 5000), (12, 5001)] {
            let p = module_pct(done, 13, dirs);
            assert!(p >= last && p < 1.0, "{done}/{dirs}: {p}");
            last = p;
        }
    }

    #[test]
    fn creep_rises_but_never_finishes_a_target() {
        assert_eq!(creep(0), 0.0);
        assert!(creep(10) < creep(100));
        assert!(creep(100_000) <= 1.0);
    }

    #[test]
    fn middle_ellipsis_keeps_head_and_file_name() {
        let p = r"C:\Users\me\AppData\Local\Google\Chrome\User Data\Default\Cache\index";
        assert_eq!(middle_ellipsis(p, |_| true), p);
        let s = middle_ellipsis(p, |c| c.chars().count() <= 30);
        assert!(
            s.starts_with(r"C:\Users") && s.ends_with(r"...\index"),
            "{s}"
        );
        assert!(s.chars().count() <= 30);
    }

    #[test]
    fn total_sums_done_rows_only() {
        let t = |files, bytes| Tally { files, bytes };
        let row = |status| Row {
            kind: Kind::Junk,
            name: "x",
            what: "x",
            status,
            checked: true,
            fixed: None,
        };
        let p = Progress {
            rows: vec![
                row(Status::Done(t(2, 10))),
                row(Status::Skipped),
                row(Status::Done(t(3, 5))),
            ],
            current: 2,
            pct: 1.0,
            path: String::new(),
            finished: true,
            fixing: false,
            error: None,
        };
        assert_eq!(p.total(), t(5, 15));
        let mut p = p;
        p.rows[0].checked = false;
        assert_eq!(p.total(), t(3, 5), "unticked modules don't count");
    }

    #[test]
    fn stop_ends_the_worker_and_marks_rows() {
        let s = start(ALL);
        s.stop();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !s.snapshot().unwrap().finished {
            assert!(Instant::now() < deadline, "worker didn't stop");
            std::thread::sleep(Duration::from_millis(10));
        }
        let p = s.snapshot().unwrap();
        assert!(p.rows.iter().all(|r| r.status == Status::Skipped));
    }
}
