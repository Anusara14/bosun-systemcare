//! Notifications Bosun has shown, so the bell in the main window can list
//! them: `%LOCALAPPDATA%\Bosun\notifications.tsv`, one `ms<TAB>page<TAB>
//! title<TAB>text` line each, the newest 30 kept. The tray writes; the main
//! window reads, clears, and remembers what was seen.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const KEEP: usize = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// Unix milliseconds.
    pub at: u64,
    /// The main window page a click opens (`protect`, `rescue`, …).
    pub page: String,
    pub title: String,
    pub text: String,
}

fn dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Bosun"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Notices in `text`, newest first; malformed lines are skipped.
pub fn parse(text: &str) -> Vec<Notice> {
    let mut v: Vec<Notice> = text
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(4, '\t');
            Some(Notice {
                at: f.next()?.parse().ok()?,
                page: f.next()?.to_string(),
                title: f.next()?.to_string(),
                text: f.next().unwrap_or("").to_string(),
            })
        })
        .collect();
    v.sort_by_key(|n| std::cmp::Reverse(n.at));
    v
}

fn serialize(v: &[Notice]) -> String {
    let clean = |s: &str| s.replace(['\t', '\r', '\n'], " ");
    // stored oldest first, so appending stays cheap to reason about
    v.iter()
        .rev()
        .map(|n| {
            format!(
                "{}\t{}\t{}\t{}\n",
                n.at,
                clean(&n.page),
                clean(&n.title),
                clean(&n.text)
            )
        })
        .collect()
}

/// Newest first.
pub fn load() -> Vec<Notice> {
    dir()
        .and_then(|d| std::fs::read_to_string(d.join("notifications.tsv")).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// Records a notification shown now.
pub fn add(page: &str, title: &str, text: &str) -> std::io::Result<()> {
    let d = dir().ok_or_else(|| std::io::Error::other("LOCALAPPDATA is unset"))?;
    std::fs::create_dir_all(&d)?;
    let mut v = load();
    v.insert(
        0,
        Notice {
            at: now_ms(),
            page: page.into(),
            title: title.into(),
            text: text.into(),
        },
    );
    v.truncate(KEEP);
    std::fs::write(d.join("notifications.tsv"), serialize(&v))
}

/// Forgets every notification.
pub fn clear() -> std::io::Result<()> {
    match dir() {
        Some(d) => match std::fs::remove_file(d.join("notifications.tsv")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        None => Ok(()),
    }
}

fn seen_path() -> Option<PathBuf> {
    dir().map(|d| d.join("notifications.seen"))
}

/// Something arrived since the bell was last opened.
pub fn unread() -> bool {
    let seen: u64 = seen_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    load().first().is_some_and(|n| n.at > seen)
}

/// The bell was opened: everything so far is seen.
pub fn mark_seen() {
    if let (Some(p), Some(n)) = (seen_path(), load().first()) {
        let _ = std::fs::write(p, n.at.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_newest_first() {
        let v = vec![
            Notice {
                at: 20,
                page: "rescue".into(),
                title: "Auto Clean finished".into(),
                text: "Deleted 3 files".into(),
            },
            Notice {
                at: 10,
                page: "protect".into(),
                title: "Homepage\tchanged".into(),
                text: String::new(),
            },
        ];
        let back = parse(&serialize(&v));
        assert_eq!(back[0], v[0]);
        assert_eq!(back[1].title, "Homepage changed");
        assert!(parse("junk\nx\ty").is_empty());
    }
}
