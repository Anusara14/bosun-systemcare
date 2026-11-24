//! Rescue Center (spec §2.6): every journal in `%LOCALAPPDATA%\Bosun\journal`,
//! newest first, with a one-line summary and, where the change can be
//! reversed, a restore. Restoring goes through the same journaled setters, so
//! a restore is itself in the journal; the original file gets an `X` line.
//!
//! What can't be reversed is said plainly: junk cleaning deletes for good,
//! and shortcuts or folders sent to the Recycle Bin come back from there.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Clean,
    Registry,
    Recycle,
    Startup,
    Services,
    Tasks,
    Privacy,
    Turbo,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undo {
    /// We can put it back.
    Restore,
    /// It's in the Recycle Bin.
    RecycleBin,
    /// Deleted for good, a test run, or already undone.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    /// Unix milliseconds, from the file name.
    pub when: u64,
    pub title: String,
    pub summary: String,
    pub kind: Kind,
    pub undo: Undo,
    pub dry_run: bool,
    pub restored: bool,
}

fn titled(slug: &str) -> String {
    let s = slug.replace('-', " ");
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn field(line: &str, i: usize) -> &str {
    line.split('\t').nth(i).unwrap_or("")
}

/// `removed=12/3456` → (12, 3456); `removed=12` → (12, 0).
fn tally(e: &str, key: &str) -> (u64, u64) {
    e.split('\t')
        .find_map(|f| f.strip_prefix(key))
        .map(|v| {
            let (n, b) = v.split_once('/').unwrap_or((v, "0"));
            (n.parse().unwrap_or(0), b.parse().unwrap_or(0))
        })
        .unwrap_or((0, 0))
}

/// `18286` → `18,286`.
fn grouped(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: u64, one: &str) -> String {
    format!("{} {one}{}", grouped(n), if n == 1 { "" } else { "s" })
}

/// "Deleted 3 files" → "Test run: would delete 3 files".
fn would(summary: &str) -> String {
    let (verb, rest) = summary.split_once(' ').unwrap_or((summary, ""));
    let verb = match verb {
        "Deleted" => "would delete",
        "Removed" => "would remove",
        "Moved" => "would move",
        v => v,
    };
    format!("Test run: {verb} {rest}")
}

/// Reads one journal (`name` is its file name, `text` its contents).
pub fn parse(path: &Path, text: &str) -> Option<Entry> {
    let name = path.file_stem()?.to_string_lossy();
    let (ms, slug) = name.split_once('-')?;
    let when = ms.parse().ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let header = lines.first().filter(|l| l.starts_with("# bosun journal"));
    let dry_run = header.is_some_and(|h| h.contains("dry_run=true"));
    let restored = lines.iter().any(|l| l.starts_with("X\t"));
    let end = lines.iter().rev().find(|l| l.starts_with("E\t")).copied();
    let body = |p: &'static str| lines.iter().filter(move |l| l.starts_with(p));
    let first = |p: &'static str| body(p).next().copied().unwrap_or("");
    let mut title = header
        .map(|h| field(h, 2))
        .filter(|t| !t.is_empty())
        .map_or_else(|| titled(slug), String::from);
    let (kind, summary) = match header.map_or("", |h| field(h, 1)) {
        "clean" => {
            let (n, b) = end.map_or((body("P\t").count() as u64, 0), |e| tally(e, "removed="));
            (
                Kind::Clean,
                format!(
                    "Deleted {} ({})",
                    plural(n, "file"),
                    crate::format::bytes(b)
                ),
            )
        }
        "registry" => {
            let n = end.map_or(body("D\t").count() as u64, |e| tally(e, "removed=").0);
            (
                Kind::Registry,
                format!(
                    "Removed {}",
                    plural(n, "registry entry").replace("entrys", "entries")
                ),
            )
        }
        "recycle" => {
            let n = end.map_or(body("R\t").count() as u64, |e| tally(e, "recycled=").0);
            (
                Kind::Recycle,
                format!("Moved {} to the Recycle Bin", plural(n, "item")),
            )
        }
        "turbo" => {
            let on = field(header.copied().unwrap_or(""), 2) == "on";
            title = "Turbo Boost".into();
            let mode = field(header.copied().unwrap_or(""), 3);
            (
                Kind::Turbo,
                format!("Turned {} ({mode})", if on { "on" } else { "off" }),
            )
        }
        _ if !first("S\t").is_empty() => {
            let l = first("S\t");
            let on = field(l, 4).starts_with("02") || field(l, 4).starts_with("06");
            (
                Kind::Startup,
                format!(
                    "Turned {} {} at startup",
                    field(l, 2),
                    if on { "on" } else { "off" }
                ),
            )
        }
        _ if !first("V\t").is_empty() => {
            let l = first("V\t");
            (
                Kind::Services,
                format!("{}: {} → {}", field(l, 1), field(l, 2), field(l, 3)),
            )
        }
        _ if !first("T\t").is_empty() => {
            let l = first("T\t");
            let on = field(l, 3) == "true";
            (
                Kind::Tasks,
                format!(
                    "{} {}",
                    if on { "Enabled" } else { "Disabled" },
                    field(l, 1).trim_start_matches('\\')
                ),
            )
        }
        _ if !first("D\t").is_empty() => {
            let l = first("D\t");
            let p = crate::privacy::SWITCHES
                .iter()
                .find(|s| field(l, 1) == format!(r"HKCU\{}", s.1) && field(l, 2) == s.2);
            let protect = p.is_some_and(|s| field(l, 4) == s.3.to_string());
            (
                Kind::Privacy,
                format!(
                    "{} {}",
                    field(l, 5),
                    if protect { "protected" } else { "allowed" }
                ),
            )
        }
        _ if !first("R\t").is_empty() => {
            let l = first("R\t");
            let p = field(l, 2);
            (Kind::Recycle, format!("Moved {p} to the Recycle Bin"))
        }
        _ => (Kind::Other, String::from("A change by Bosun")),
    };
    let undo = if dry_run || restored {
        Undo::None
    } else {
        match kind {
            Kind::Clean | Kind::Other => Undo::None,
            Kind::Recycle => Undo::RecycleBin,
            Kind::Registry if body("B\t").next().is_none() => Undo::None,
            Kind::Turbo if field(header.copied().unwrap_or(""), 2) != "on" => Undo::None,
            _ => Undo::Restore,
        }
    };
    Some(Entry {
        path: path.to_path_buf(),
        when,
        title,
        summary: if dry_run { would(&summary) } else { summary },
        kind,
        undo,
        dry_run,
        restored,
    })
}

/// Every journal in `dir`, newest first. Only the newest Turbo Boost "on"
/// can be undone, and only while Turbo Boost is still on.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "tsv"))
        .filter_map(|p| fs::read_to_string(&p).ok().and_then(|t| parse(&p, &t)))
        .collect();
    v.sort_by(|a, b| b.when.cmp(&a.when).then(b.path.cmp(&a.path)));
    let mut turbo_seen = !crate::turbo::State::load().on;
    for e in v.iter_mut().filter(|e| e.kind == Kind::Turbo) {
        if turbo_seen {
            e.undo = Undo::None;
        }
        turbo_seen = true;
    }
    v
}

/// Journal files (with their `.reg` backups) older than `cutoff_ms`, by the
/// time in their names. Other files in the folder are never touched.
fn journal_files(dir: &Path, cutoff_ms: u64) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| {
            let p = e.path();
            let ext_ok = p.extension().is_some_and(|x| x == "tsv" || x == "reg");
            let ms = p
                .file_stem()
                .and_then(|s| s.to_str()?.split('-').next()?.parse::<u64>().ok());
            ext_ok && ms.is_some_and(|m| m < cutoff_ms)
        })
        .map(|e| (e.path(), e.metadata().map_or(0, |m| m.len())))
        .collect()
}

/// (journals, bytes on disk including backups).
pub fn usage(dir: &Path) -> (usize, u64) {
    let all = journal_files(dir, u64::MAX);
    let n = all
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|x| x == "tsv"))
        .count();
    (n, all.iter().map(|(_, b)| b).sum())
}

/// Deletes journals (and their backups) older than `days`; returns how many
/// journals went. Their changes can no longer be restored.
pub fn prune(dir: &Path, days: u64) -> io::Result<usize> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let cutoff = now.saturating_sub(days * 86_400_000);
    let mut n = 0;
    for (p, _) in journal_files(dir, cutoff) {
        fs::remove_file(&p)?;
        n += usize::from(p.extension().is_some_and(|x| x == "tsv"));
    }
    Ok(n)
}

fn lines(e: &Entry, prefix: &str) -> io::Result<Vec<String>> {
    Ok(fs::read_to_string(&e.path)?
        .lines()
        .filter(|l| l.starts_with(prefix))
        .map(String::from)
        .collect())
}

fn mark(e: &Entry) -> io::Result<()> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let mut f = OpenOptions::new().append(true).open(&e.path)?;
    writeln!(f, "X\t{ms}\trestored")?;
    f.sync_all()
}

fn not_found(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{what} is no longer there"),
    )
}

/// Puts a change back. Fails without marking the entry if any part fails.
pub fn restore(e: &Entry, dir: &Path) -> io::Result<()> {
    if e.undo != Undo::Restore {
        return Err(io::Error::other("this change can't be restored here"));
    }
    match e.kind {
        Kind::Registry => {
            for l in lines(e, "B\t")? {
                let ok = Command::new("reg")
                    .args(["import", field(&l, 2)])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status()?
                    .success();
                if !ok {
                    return Err(io::Error::other("reg import failed"));
                }
            }
        }
        Kind::Startup => {
            let items = crate::startup::list();
            for l in lines(e, "S\t")? {
                let machine = field(&l, 1).starts_with("HKLM");
                let old = field(&l, 3);
                // no flag, or an even first byte, means it was on
                let on = old == "none"
                    || u8::from_str_radix(old.get(..2).unwrap_or("02"), 16)
                        .is_ok_and(|b| b.is_multiple_of(2));
                let item = items
                    .iter()
                    .find(|i| i.name == field(&l, 2) && i.source.machine_wide() == machine)
                    .ok_or_else(|| not_found(field(&l, 2)))?;
                crate::startup::set_enabled(item, on, dir)?;
            }
        }
        Kind::Services => {
            use crate::services::Start;
            let all = crate::services::list();
            for l in lines(e, "V\t")? {
                let to = match field(&l, 2) {
                    "Automatic" => Start::Automatic,
                    "Manual" => Start::Manual,
                    "Disabled" => Start::Disabled,
                    _ => return Err(io::Error::other("unknown start type")),
                };
                let s = all
                    .iter()
                    .find(|s| s.name == field(&l, 1))
                    .ok_or_else(|| not_found(field(&l, 1)))?;
                crate::services::set_start(s, to, dir)?;
            }
        }
        Kind::Tasks => {
            let all = crate::tasks::list();
            for l in lines(e, "T\t")? {
                let t = all
                    .iter()
                    .find(|t| t.path == field(&l, 1))
                    .ok_or_else(|| not_found(field(&l, 1)))?;
                crate::tasks::set_enabled(t, field(&l, 2) == "true", dir)?;
            }
        }
        Kind::Privacy => {
            for l in lines(e, "D\t")? {
                let (i, s) = crate::privacy::SWITCHES
                    .iter()
                    .enumerate()
                    .find(|(_, s)| field(&l, 1) == format!(r"HKCU\{}", s.1) && field(&l, 2) == s.2)
                    .ok_or_else(|| io::Error::other("unknown privacy switch"))?;
                crate::privacy::set(i, field(&l, 3) == s.3.to_string(), dir)?;
            }
        }
        Kind::Turbo => {
            crate::turbo::restore(dir)?;
        }
        Kind::Clean | Kind::Recycle | Kind::Other => unreachable!("undo is None for these"),
    }
    mark(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!(r"C:\j\{name}"))
    }

    #[test]
    fn summarises_each_kind() {
        let e = parse(
            &p("100-junk-files.tsv"),
            "# bosun journal v1\tclean\tJunk files\tdry_run=false\nP\t5\ta\nE\tremoved=2/2048\tskipped=0/0\tstopped=false\n",
        )
        .unwrap();
        assert_eq!(
            (e.when, e.title.as_str(), e.kind, e.undo),
            (100, "Junk files", Kind::Clean, Undo::None)
        );
        assert_eq!(e.summary, "Deleted 2 files (2.00 KB)");

        let e = parse(
            &p("200-registry-clean.tsv"),
            "# bosun journal v1\tregistry\tRegistry Clean\tdry_run=false\nB\tHKCU\\X\tC:\\j\\200-registry-clean-0.reg\nE\tremoved=1\tleft=0\n",
        )
        .unwrap();
        assert_eq!(
            (e.summary.as_str(), e.undo),
            ("Removed 1 registry entry", Undo::Restore)
        );

        let e = parse(
            &p("300-startup.tsv"),
            "S\tHKCU\\K\\Run\tOneDrive\t020000000000000000000000\t030000000000000000000000\n",
        )
        .unwrap();
        assert_eq!(
            (e.kind, e.summary.as_str(), e.title.as_str()),
            (Kind::Startup, "Turned OneDrive off at startup", "Startup")
        );

        let e = parse(&p("400-services.tsv"), "V\tSysMain\tAutomatic\tManual\n").unwrap();
        assert_eq!(e.summary, "SysMain: Automatic → Manual");

        let e = parse(
            &p("500-scheduled-tasks.tsv"),
            "T\t\\Foo\\Bar\ttrue\tfalse\n",
        )
        .unwrap();
        assert_eq!(
            (e.summary.as_str(), e.title.as_str()),
            (r"Disabled Foo\Bar", "Scheduled tasks")
        );

        let key = crate::privacy::SWITCHES[0].1;
        let e = parse(
            &p("600-privacy.tsv"),
            &format!("D\tHKCU\\{key}\tEnabled\t1\t0\tAdvertising ID\n"),
        )
        .unwrap();
        assert_eq!(
            (e.kind, e.summary.as_str()),
            (Kind::Privacy, "Advertising ID protected")
        );

        let e = parse(
            &p("700-shortcut-fix.tsv"),
            "# bosun journal v1\trecycle\tShortcut Fix\tdry_run=false\nR\ta.lnk\tb\nE\trecycled=1\tleft=0\n",
        )
        .unwrap();
        assert_eq!(
            (e.undo, e.summary.as_str()),
            (Undo::RecycleBin, "Moved 1 item to the Recycle Bin")
        );
    }

    #[test]
    fn test_runs_and_restored_entries_cannot_be_undone() {
        let e = parse(
            &p("1-registry-clean.tsv"),
            "# bosun journal v1\tregistry\tRegistry Clean\tdry_run=true\nB\tHKCU\\X\tx.reg\nD\tHKCU\\X\tvalue\tv\tm\n",
        )
        .unwrap();
        assert_eq!(e.summary, "Test run: would remove 1 registry entry");
        assert!(e.dry_run);
        assert_eq!(grouped(18_286), "18,286");
        assert_eq!(e.undo, Undo::None);
        let e = parse(
            &p("2-services.tsv"),
            "V\tA\tManual\tDisabled\nX\t3\trestored\n",
        )
        .unwrap();
        assert!(e.restored);
        assert_eq!(e.undo, Undo::None);
        assert!(parse(&p("garbage.tsv"), "").is_none());
    }

    #[test]
    fn prunes_only_old_journal_files() {
        let d = std::env::temp_dir().join(format!("bosun-prune-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        for f in [
            "1000-old.tsv",
            "1000-old-0.reg",
            "notes.txt",
            "99999999999999-new.tsv",
        ] {
            fs::write(d.join(f), "x").unwrap();
        }
        assert_eq!(usage(&d), (2, 3));
        assert_eq!(prune(&d, 30).unwrap(), 1);
        assert!(d.join("notes.txt").exists() && d.join("99999999999999-new.tsv").exists());
        assert!(!d.join("1000-old-0.reg").exists());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn restores_a_registry_clean_from_its_backup() {
        use windows::Win32::System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RegCreateKeyW, RegDeleteTreeW, RegDeleteValueW,
            RegSetValueExW,
        };
        use windows::core::HSTRING;
        const KEY: &str = r"Software\BosunTestRescue";
        let d = std::env::temp_dir().join(format!("bosun-rescue-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let mut h = windows::Win32::System::Registry::HKEY::default();
        let v: Vec<u8> = "x\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
        // SAFETY: a throwaway test key under HKCU.
        unsafe {
            RegCreateKeyW(HKEY_CURRENT_USER, &HSTRING::from(KEY), &mut h)
                .ok()
                .unwrap();
            RegSetValueExW(h, &HSTRING::from("Gone"), None, REG_SZ, Some(&v))
                .ok()
                .unwrap();
        }
        let backup = d.join("5-registry-clean-0.reg");
        let ok = Command::new("reg")
            .args([
                "export",
                &format!(r"HKCU\{KEY}"),
                &backup.to_string_lossy(),
                "/y",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        // SAFETY: as above.
        unsafe { RegDeleteValueW(h, &HSTRING::from("Gone")).ok().unwrap() };
        let journal = d.join("5-registry-clean.tsv");
        fs::write(
            &journal,
            format!(
                "# bosun journal v1\tregistry\tRegistry Clean\tdry_run=false\nB\tHKCU\\{KEY}\t{}\nE\tremoved=1\tleft=0\n",
                backup.display()
            ),
        )
        .unwrap();
        let e = parse(&journal, &fs::read_to_string(&journal).unwrap()).unwrap();
        restore(&e, &d).unwrap();
        let back = crate::registry::open_in(HKEY_CURRENT_USER, KEY, false)
            .and_then(|k| crate::registry::string(&k, None, "Gone"));
        assert_eq!(back.as_deref(), Some("x"));
        let again = parse(&journal, &fs::read_to_string(&journal).unwrap()).unwrap();
        assert!(again.restored && again.undo == Undo::None);
        // SAFETY: as above.
        unsafe {
            let _ = windows::Win32::System::Registry::RegCloseKey(h);
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(KEY));
        }
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn lists_the_real_journal() {
        if let Some(d) = crate::clean::journal_dir() {
            let v = list(&d);
            assert!(v.windows(2).all(|w| w[0].when >= w[1].when));
        }
    }
}
