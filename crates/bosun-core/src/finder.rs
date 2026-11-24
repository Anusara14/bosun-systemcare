//! Large File Finder and Empty Folder Finder (spec §2.2, More tools), over
//! the user's own folders. Reparse points are never followed and `AppData`
//! is skipped (apps own what's in there). Removing goes to the Recycle Bin.

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Files at least this big count as large.
pub const LARGE: u64 = 100 * 1024 * 1024;
/// At most this many large files are listed (biggest first).
const MAX_LARGE: usize = 300;

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub size: u64,
}

/// The user's profile folder (where the finders look).
pub fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// The user's own folders, where an empty folder is the user's to remove
/// (apps' data folders often keep empty folders on purpose).
pub fn user_folders() -> Vec<PathBuf> {
    let Some(h) = home() else { return Vec::new() };
    [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
    ]
    .iter()
    .map(|d| h.join(d))
    .filter(|p| p.is_dir())
    .collect()
}

/// A folder's readable entries.
type Entries = [(PathBuf, fs::Metadata)];

fn skip(dir: &Path) -> bool {
    // app data and hidden ".folders" belong to programs
    dir.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("AppData") || n.to_string_lossy().starts_with('.'))
}

/// Walks `root` depth-first; `f(dir, entries)` sees each folder's readable
/// entries (path, metadata). Returns `false` from `stop` to end early.
fn walk(root: &Path, stop: &dyn Fn() -> bool, f: &mut dyn FnMut(&Path, &Entries)) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if stop() {
            return;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let entries: Vec<(PathBuf, fs::Metadata)> = rd
            .flatten()
            .filter_map(|e| fs::symlink_metadata(e.path()).ok().map(|m| (e.path(), m)))
            .collect();
        for (p, m) in &entries {
            if m.is_dir() && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 && !skip(p) {
                stack.push(p.clone());
            }
        }
        f(&dir, &entries);
    }
}

/// Files of at least [`LARGE`] bytes under `root`, biggest first.
pub fn large_files(root: &Path, stop: &dyn Fn() -> bool) -> Vec<Found> {
    let mut out = Vec::new();
    walk(root, stop, &mut |_, entries| {
        for (p, m) in entries {
            if m.is_file()
                && m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                && m.len() >= LARGE
            {
                out.push(Found {
                    path: p.clone(),
                    size: m.len(),
                });
            }
        }
    });
    out.sort_by_key(|f| std::cmp::Reverse(f.size));
    out.truncate(MAX_LARGE);
    out
}

/// Folders under `root` (not `root` itself) with nothing in them at all.
pub fn empty_folders(root: &Path, stop: &dyn Fn() -> bool) -> Vec<Found> {
    let mut out = Vec::new();
    walk(root, stop, &mut |dir, entries| {
        if entries.is_empty() && dir != root {
            out.push(Found {
                path: dir.to_path_buf(),
                size: 0,
            });
        }
    });
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Moves a found file or folder to the Recycle Bin, journaled first.
pub fn recycle(found: &Found, dir: &Path, label: &str) -> std::io::Result<()> {
    use std::io::Write;
    let (_, mut f) = crate::clean::journal_file(dir, label)?;
    writeln!(f, "R	{}	{}", found.size, found.path.display())?;
    f.sync_all()?;
    crate::shortcuts::recycle(&found.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_empty_folders_and_skips_appdata() {
        let root = std::env::temp_dir().join(format!("bosun-finder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(r"a\empty")).unwrap();
        fs::create_dir_all(root.join(r"AppData\also-empty")).unwrap();
        fs::create_dir_all(root.join("full")).unwrap();
        fs::write(root.join(r"full\f.txt"), b"x").unwrap();
        let found: Vec<PathBuf> = empty_folders(&root, &|| false)
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(found, [root.join(r"a\empty")]);
        assert!(
            large_files(&root, &|| false).is_empty(),
            "nothing is 100 MB"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
