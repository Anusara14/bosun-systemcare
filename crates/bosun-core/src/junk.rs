//! Junk File Clean and Privacy Sweep (browser caches), scan side: measures each junk category without touching
//! anything (dry run, spec §4.3). Fixing re-walks the same targets later, so
//! nothing here holds per-file lists in memory.

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Which files under a root count as junk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    /// Everything below the root.
    All,
    /// Files whose name ends with this suffix (case-insensitive).
    Suffix(&'static str),
    /// Files whose name starts with this prefix (case-insensitive).
    Prefix(&'static str),
    /// Files not modified for this many days (skips what is in use right now).
    OlderThanDays(u64),
}

#[derive(Clone, Debug)]
pub struct Target {
    pub category: &'static str,
    pub root: PathBuf,
    pub filter: Filter,
    /// Walk into subfolders.
    pub deep: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    pub files: u64,
    pub bytes: u64,
}

impl Tally {
    pub fn add(&mut self, o: Tally) {
        self.files += o.files;
        self.bytes += o.bytes;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub category: &'static str,
    pub tally: Tally,
}

fn env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// Chromium-family browsers under `%LOCALAPPDATA%`, by their user-data folder.
const CHROMIUM: [&str; 6] = [
    r"Google\Chrome\User Data",
    r"Microsoft\Edge\User Data",
    r"BraveSoftware\Brave-Browser\User Data",
    r"Vivaldi\User Data",
    r"Chromium\User Data",
    r"Opera Software\Opera Stable",
];

/// Browser caches (Privacy Sweep's default-on part, spec §2.1). Cookies and
/// history stay out: they are opt-in and not plain files.
pub fn privacy_targets() -> Vec<Target> {
    let Some(la) = env("LOCALAPPDATA") else {
        return Vec::new();
    };
    let cache = |root: PathBuf| Target {
        category: "Browser cache",
        root,
        filter: Filter::All,
        deep: true,
    };
    let mut v = Vec::new();
    for b in CHROMIUM {
        let data = la.join(b);
        // Profiles are "Default", "Profile N", or the folder itself (Opera).
        let mut profiles: Vec<PathBuf> = fs::read_dir(&data)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let n = lower_name(p);
                        n == "default" || n.starts_with("profile ")
                    })
                    .collect()
            })
            .unwrap_or_default();
        if profiles.is_empty() {
            profiles.push(data);
        }
        for p in profiles {
            for c in ["Cache", "Code Cache", "GPUCache"] {
                v.push(cache(p.join(c)));
            }
        }
    }
    let ff = la.join(r"Mozilla\Firefox\Profiles");
    for p in fs::read_dir(ff).into_iter().flatten().flatten() {
        v.push(cache(p.path().join("cache2")));
    }
    v.retain(|t| t.root.is_dir());
    v
}

/// The junk locations from the spec (§2.1), resolved for this user.
pub fn targets() -> Vec<Target> {
    let t = |category, root: PathBuf, filter, deep| Target {
        category,
        root,
        filter,
        deep,
    };
    let mut v = Vec::new();
    if let Some(tmp) = env("TEMP") {
        // ponytail: one day old keeps installers and running apps' temp files safe
        v.push(t("Temporary files", tmp, Filter::OlderThanDays(1), true));
    }
    if let Some(win) = env("WINDIR") {
        v.push(t(
            "Temporary files",
            win.join("Temp"),
            Filter::OlderThanDays(1),
            true,
        ));
        v.push(t(
            "Windows Update cache",
            win.join(r"SoftwareDistribution\Download"),
            Filter::All,
            true,
        ));
        v.push(t(
            "Delivery Optimization files",
            win.join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
            Filter::All,
            true,
        ));
        v.push(t(
            "Memory dumps",
            win.join("Minidump"),
            Filter::Suffix(".dmp"),
            false,
        ));
        v.push(t(
            "Memory dumps",
            win.clone(),
            Filter::Suffix("memory.dmp"),
            false,
        ));
        v.push(t(
            "Log files",
            win.join("Logs"),
            Filter::Suffix(".log"),
            true,
        ));
    }
    if let Some(pd) = env("ProgramData") {
        for q in ["ReportArchive", "ReportQueue"] {
            v.push(t(
                "Windows error reports",
                pd.join(r"Microsoft\Windows\WER").join(q),
                Filter::All,
                true,
            ));
        }
    }
    if let Some(la) = env("LOCALAPPDATA") {
        v.push(t(
            "Memory dumps",
            la.join("CrashDumps"),
            Filter::Suffix(".dmp"),
            false,
        ));
        v.push(t(
            "Windows error reports",
            la.join(r"Microsoft\Windows\WER"),
            Filter::All,
            true,
        ));
        v.push(t(
            "Thumbnail cache",
            la.join(r"Microsoft\Windows\Explorer"),
            Filter::Prefix("thumbcache_"),
            false,
        ));
        v.push(t(
            "DirectX shader cache",
            la.join("D3DSCache"),
            Filter::All,
            true,
        ));
    }
    v
}

fn lower_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn matches(filter: Filter, path: &Path, md: &fs::Metadata, now: SystemTime) -> bool {
    match filter {
        Filter::All => true,
        Filter::Suffix(s) => lower_name(path).ends_with(s),
        Filter::Prefix(s) => lower_name(path).starts_with(s),
        Filter::OlderThanDays(d) => md
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= Duration::from_secs(d * 86_400)),
    }
}

/// Walks one target, calling `on_file(path, bytes)` for every matching file.
/// Unreadable folders count as empty; reparse points (symlinks, junctions)
/// are never followed, so a walk can't leave its root. `visit` sees each
/// folder before it is read; returning `false` stops the walk.
pub fn walk(
    t: &Target,
    now: SystemTime,
    visit: &mut dyn FnMut(&Path) -> bool,
    on_file: &mut dyn FnMut(&Path, u64),
) {
    let mut stack = vec![t.root.clone()];
    while let Some(dir) = stack.pop() {
        if !visit(&dir) {
            break;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(md) = fs::symlink_metadata(&path) else {
                continue;
            };
            if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                continue;
            }
            if md.is_dir() {
                if t.deep {
                    stack.push(path);
                }
            } else if matches(t.filter, &path, &md, now) {
                on_file(&path, md.len());
            }
        }
    }
}

/// Sizes up one target (see [`walk`]).
pub fn measure(t: &Target, now: SystemTime, visit: &mut dyn FnMut(&Path) -> bool) -> Tally {
    let mut tally = Tally::default();
    walk(t, now, visit, &mut |_, bytes| {
        tally.add(Tally { files: 1, bytes })
    });
    tally
}

/// Scans every target, merging targets of the same category in first-seen order.
/// `progress` gets (done, total) after each target so a UI can move a bar.
pub fn scan(targets: &[Target], mut progress: impl FnMut(usize, usize)) -> Vec<Found> {
    let now = SystemTime::now();
    let mut out: Vec<Found> = Vec::new();
    // ponytail: the same root can appear twice (e.g. WER under two parents);
    // skip exact duplicates rather than de-overlapping nested roots.
    let mut seen: Vec<(PathBuf, Filter)> = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        if !seen.iter().any(|(r, f)| *r == t.root && *f == t.filter) {
            seen.push((t.root.clone(), t.filter));
            let tally = measure(t, now, &mut |_| true);
            match out.iter_mut().find(|f| f.category == t.category) {
                Some(f) => f.tally.add(tally),
                None => out.push(Found {
                    category: t.category,
                    tally,
                }),
            }
        }
        progress(i + 1, targets.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bosun-junk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.dmp"), [0u8; 10]).unwrap();
        fs::write(root.join("b.txt"), [0u8; 5]).unwrap();
        fs::write(root.join(r"sub\c.dmp"), [0u8; 7]).unwrap();
        root
    }

    fn target(root: &Path, filter: Filter, deep: bool) -> Target {
        Target {
            category: "x",
            root: root.to_path_buf(),
            filter,
            deep,
        }
    }

    #[test]
    fn measures_by_filter_and_depth() {
        let root = tree("measure");
        let now = SystemTime::now();
        let all = measure(&target(&root, Filter::All, true), now, &mut |_| true);
        assert_eq!(
            all,
            Tally {
                files: 3,
                bytes: 22
            }
        );
        let shallow = measure(
            &target(&root, Filter::Suffix(".dmp"), false),
            now,
            &mut |_| true,
        );
        assert_eq!(
            shallow,
            Tally {
                files: 1,
                bytes: 10
            }
        );
        let deep = measure(
            &target(&root, Filter::Suffix(".dmp"), true),
            now,
            &mut |_| true,
        );
        assert_eq!(
            deep,
            Tally {
                files: 2,
                bytes: 17
            }
        );
        // just-written files are not "older than a day"
        let old = measure(
            &target(&root, Filter::OlderThanDays(1), true),
            now,
            &mut |_| true,
        );
        assert_eq!(old.files, 0);
        let later = now + Duration::from_secs(2 * 86_400);
        assert_eq!(
            measure(
                &target(&root, Filter::OlderThanDays(1), true),
                later,
                &mut |_| true
            )
            .files,
            3
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn scan_merges_categories_skips_duplicates_and_reports_progress() {
        let root = tree("scan");
        let t = target(&root, Filter::All, true);
        let mut calls = Vec::new();
        let found = scan(&[t.clone(), t], |d, n| calls.push((d, n)));
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].tally,
            Tally {
                files: 3,
                bytes: 22
            }
        );
        assert_eq!(calls, [(1, 2), (2, 2)]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_root_is_empty_not_an_error() {
        let t = target(Path::new(r"Z:\no\such\dir"), Filter::All, true);
        assert_eq!(
            measure(&t, SystemTime::now(), &mut |_| true),
            Tally::default()
        );
    }

    /// Read-only scan of this machine: `cargo test -p bosun-core real_scan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_scan() {
        let at = std::time::Instant::now();
        let all: Vec<Target> = privacy_targets().into_iter().chain(targets()).collect();
        for f in scan(&all, |_, _| {}) {
            println!(
                "{:<30} {:>7} files {:>10.1} MB",
                f.category,
                f.tally.files,
                f.tally.bytes as f64 / 1048576.0
            );
        }
        println!("took {:?}", at.elapsed());
    }
}
