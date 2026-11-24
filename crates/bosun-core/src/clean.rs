//! Journaled cleaning (spec §4.1: nothing destructive without a journal entry
//! written first).
//!
//! For each run a journal file `<unix ms>-<label>.tsv` is written:
//! 1. `P\t<bytes>\t<path>`: the plan, every file the targets match, flushed
//!    to disk before anything is touched;
//! 2. files are then deleted *from the plan*, so a file that appears after
//!    the plan was written is never removed;
//! 3. `F\t<bytes>\t<path>\t<reason>` for each file that couldn't go (in use,
//!    access denied), and a closing `E` line with the totals.
//!
//! Paths can't contain tabs or newlines on Windows, so plain TSV is safe.
//! Caches and temp files are deleted outright (spec §4.2), so these entries
//! are a record, not an undo.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::junk::{Tally, Target, walk};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Share of the progress ring spent writing the plan.
const PLAN_SHARE: f32 = 0.3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cleaned {
    pub removed: Tally,
    /// Files that stayed: in use, access denied, or left when stopped.
    pub skipped: Tally,
}

/// `%LOCALAPPDATA%\Bosun\journal` (the app runs unelevated).
pub fn journal_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Bosun\journal"))
}

/// A new journal file `<unix ms>-<label>.tsv` in `dir`, opened for append.
pub(crate) fn journal_file(dir: &Path, label: &str) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let name: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let path = dir.join(format!("{ms}-{name}.tsv"));
    let f = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&path)?;
    Ok((path, f))
}

/// Removes the files `targets` match, journal first. `progress(path, done)`
/// reports each folder while planning (the first 30 %) and each file while
/// deleting (the rest); returning `false` stops (the rest stay and count as
/// skipped). With `dry_run` nothing is deleted but the
/// journal and totals are produced as if it had been.
pub fn clean(
    targets: &[Target],
    dir: &Path,
    label: &str,
    dry_run: bool,
    progress: &mut dyn FnMut(&Path, f32) -> bool,
) -> io::Result<(PathBuf, Cleaned)> {
    let (path, file) = journal_file(dir, label)?;
    let mut w = BufWriter::new(file);
    writeln!(w, "# bosun journal v1\tclean\t{label}\tdry_run={dry_run}")?;
    let now = SystemTime::now();
    let mut planned = 0u64;
    let mut err = None;
    let mut dirs = 0u32;
    // Stopping while planning leaves every file in place.
    let mut stopped = false;
    for t in targets {
        // ponytail: a write error stops writing, not the walk; it's reported after
        walk(
            t,
            now,
            &mut |d| {
                dirs += 1;
                // planning share creeps towards 30 % with every folder
                let f = PLAN_SHARE * (1.0 - 0.9995f32.powi(dirs as i32));
                !stopped && {
                    stopped = !progress(d, f);
                    !stopped
                }
            },
            &mut |p, bytes| {
                if err.is_none() {
                    match writeln!(w, "P\t{bytes}\t{}", p.display()) {
                        Ok(()) => planned += 1,
                        Err(e) => err = Some(e),
                    }
                }
            },
        );
    }
    if let Some(e) = err {
        return Err(e);
    }
    // The plan must be on disk before the first delete.
    w.flush()?;
    w.get_ref().sync_all()?;

    let mut out = Cleaned::default();
    let reader = BufReader::new(File::open(&path)?);
    let mut done = 0u64;
    for line in reader.lines() {
        if done == planned {
            break; // the rest are our own F lines
        }
        let line = line?;
        let Some(rest) = line.strip_prefix("P\t") else {
            continue;
        };
        let Some((bytes, file)) = rest.split_once('\t') else {
            continue;
        };
        let bytes: u64 = bytes.parse().unwrap_or(0);
        let one = Tally { files: 1, bytes };
        done += 1;
        let file = Path::new(file);
        let f = PLAN_SHARE + (1.0 - PLAN_SHARE) * done as f32 / planned.max(1) as f32;
        if stopped || !progress(file, f) {
            stopped = true;
            out.skipped.add(one);
            continue;
        }
        match remove(file, dry_run) {
            Ok(()) => out.removed.add(one),
            Err(e) => {
                out.skipped.add(one);
                writeln!(w, "F\t{bytes}\t{}\t{}", file.display(), e.kind())?;
            }
        }
    }
    writeln!(
        w,
        "E\tremoved={}/{}\tskipped={}/{}\tstopped={stopped}",
        out.removed.files, out.removed.bytes, out.skipped.files, out.skipped.bytes
    )?;
    w.flush()?;
    Ok((path, out))
}

/// Deletes one planned file, re-checking it is still a plain file (not
/// swapped for a link or folder since the plan was written).
fn remove(file: &Path, dry_run: bool) -> io::Result<()> {
    let md = fs::symlink_metadata(file)?;
    if !md.is_file() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a plain file",
        ));
    }
    if dry_run {
        Ok(())
    } else {
        fs::remove_file(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::junk::Filter;
    use std::os::windows::fs::OpenOptionsExt;

    fn setup(name: &str) -> (PathBuf, PathBuf, Target) {
        let base = std::env::temp_dir().join(format!("bosun-clean-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("cache");
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.bin"), [0u8; 10]).unwrap();
        fs::write(root.join(r"sub\b.bin"), [0u8; 5]).unwrap();
        let t = Target {
            category: "x",
            root: root.clone(),
            filter: Filter::All,
            deep: true,
        };
        (base, root, t)
    }

    fn journal(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    #[test]
    fn deletes_planned_files_and_journals_them_first() {
        let (base, root, t) = setup("del");
        let (j, out) = clean(&[t], &base.join("j"), "Junk files", false, &mut |_, _| true).unwrap();
        assert_eq!(
            out.removed,
            Tally {
                files: 2,
                bytes: 15
            }
        );
        assert_eq!(out.skipped, Tally::default());
        assert!(!root.join("a.bin").exists() && !root.join(r"sub\b.bin").exists());
        assert!(root.join("sub").is_dir(), "folders stay");
        let text = journal(&j);
        assert!(text.starts_with("# bosun journal v1\tclean\tJunk files\tdry_run=false"));
        assert_eq!(text.lines().filter(|l| l.starts_with("P\t")).count(), 2);
        assert!(text.lines().last().unwrap().starts_with("E\tremoved=2/15"));
        assert!(
            j.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("-junk-files.tsv")
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn locked_files_are_skipped_and_logged() {
        let (base, root, t) = setup("lock");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("a.bin"))
            .unwrap();
        let (j, out) = clean(&[t], &base.join("j"), "x", false, &mut |_, _| true).unwrap();
        assert_eq!(out.removed, Tally { files: 1, bytes: 5 });
        assert_eq!(
            out.skipped,
            Tally {
                files: 1,
                bytes: 10
            }
        );
        assert!(
            journal(&j)
                .lines()
                .any(|l| l.starts_with("F\t10\t") && l.contains("a.bin"))
        );
        drop(held);
        assert!(root.join("a.bin").exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn dry_run_and_stop_leave_files_alone() {
        let (base, root, t) = setup("dry");
        let (_, out) = clean(
            std::slice::from_ref(&t),
            &base.join("j"),
            "x",
            true,
            &mut |_, _| true,
        )
        .unwrap();
        assert_eq!(out.removed.files, 2);
        assert!(root.join("a.bin").exists() && root.join(r"sub\b.bin").exists());
        let (_, out) = clean(&[t], &base.join("j"), "x", false, &mut |_, _| false).unwrap();
        assert_eq!(out.removed, Tally::default());
        // stopped while planning: nothing listed, nothing touched
        assert_eq!(out.skipped.files, 0);
        assert!(root.join("a.bin").exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn stop_while_deleting_keeps_the_rest_and_counts_it() {
        let (base, root, t) = setup("stopdel");
        // allow planning (< 30 %) and the first delete, then stop
        let mut files = 0;
        let (_, out) = clean(&[t], &base.join("j"), "x", false, &mut |_, f| {
            if f >= PLAN_SHARE {
                files += 1;
            }
            files <= 1
        })
        .unwrap();
        assert_eq!((out.removed.files, out.skipped.files), (1, 1));
        let left = [root.join("a.bin"), root.join("sub").join("b.bin")];
        assert_eq!(left.iter().filter(|p| p.exists()).count(), 1);
        fs::remove_dir_all(base).unwrap();
    }
}
