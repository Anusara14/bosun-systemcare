//! The Ignore List (spec §2.9): things the user told Bosun to leave alone,
//! kept in `%APPDATA%\Bosun\ignore.txt` as `list<TAB>key<TAB>label` lines.
//! Startup items are keyed by their entry name, updates by winget package id.

use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    /// Startup Optimizer: don't suggest turning this off.
    Startup,
    /// Software Updater: don't offer this update.
    Update,
}

impl List {
    fn tag(self) -> &'static str {
        match self {
            List::Startup => "startup",
            List::Update => "update",
        }
    }

    fn from_tag(t: &str) -> Option<Self> {
        match t {
            "startup" => Some(List::Startup),
            "update" => Some(List::Update),
            _ => None,
        }
    }

    /// How the Settings page names it.
    pub fn label(self) -> &'static str {
        match self {
            List::Startup => "Startup item",
            List::Update => "Software update",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub list: List,
    pub key: String,
    pub label: String,
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Bosun\ignore.txt"))
}

/// Entries in `text`; unknown or malformed lines are skipped.
pub fn parse(text: &str) -> Vec<Entry> {
    // a byte-order mark (Notepad, PowerShell) would hide the first line
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let list = List::from_tag(f.next()?)?;
            let key = f.next()?.trim();
            (!key.is_empty()).then(|| Entry {
                list,
                key: key.to_string(),
                label: f.next().unwrap_or(key).trim().to_string(),
            })
        })
        .collect()
}

fn serialize(v: &[Entry]) -> String {
    // tabs and newlines in a label would break the line format
    let clean = |s: &str| s.replace(['\t', '\r', '\n'], " ");
    v.iter()
        .map(|e| format!("{}\t{}\t{}\n", e.list.tag(), clean(&e.key), clean(&e.label)))
        .collect()
}

pub fn load() -> Vec<Entry> {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

fn save(v: &[Entry]) -> io::Result<()> {
    let p = path().ok_or_else(|| io::Error::other("APPDATA is unset"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, serialize(v))
}

/// Keys on `list`.
pub fn keys(list: List) -> Vec<String> {
    load()
        .into_iter()
        .filter(|e| e.list == list)
        .map(|e| e.key)
        .collect()
}

/// Adds an entry (once).
pub fn add(list: List, key: &str, label: &str) -> io::Result<()> {
    let mut v = load();
    if !v.iter().any(|e| e.list == list && e.key == key) {
        v.push(Entry {
            list,
            key: key.to_string(),
            label: label.to_string(),
        });
    }
    save(&v)
}

/// Takes an entry off the list.
pub fn remove(list: List, key: &str) -> io::Result<()> {
    let mut v = load();
    v.retain(|e| !(e.list == list && e.key == key));
    save(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_skips_junk() {
        let v = vec![
            Entry {
                list: List::Startup,
                key: "OneDrive".into(),
                label: "Microsoft OneDrive".into(),
            },
            Entry {
                list: List::Update,
                key: "VideoLAN.VLC".into(),
                label: "VLC\tmedia player".into(),
            },
        ];
        let back = parse(&serialize(&v));
        assert_eq!(back[0], v[0]);
        assert_eq!(back[1].label, "VLC media player", "tabs can't split a line");
        assert!(parse("nonsense\nunknown\tx\tlabel\nstartup\t\tempty\n").is_empty());
        assert_eq!(
            parse("\u{feff}startup\tZebar\tZebar").len(),
            1,
            "a BOM is ignored"
        );
        assert_eq!(
            parse("update\tA.B")[0].label,
            "A.B",
            "label defaults to the key"
        );
    }
}
