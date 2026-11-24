//! Software Updater (spec §2.4): outdated apps and updates through winget.
//! We read `winget upgrade`'s table and update one package at a time with
//! winget itself; installers may still ask for admin rights.

use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub name: String,
    pub id: String,
    pub current: String,
    pub available: String,
    pub source: String,
}

/// Column start of `name` in the header line (by characters).
fn col(header: &[char], name: &str) -> Option<usize> {
    let n: Vec<char> = name.chars().collect();
    (0..header.len().saturating_sub(n.len() - 1)).find(|&i| {
        header[i..i + n.len()] == n[..]
            && (i == 0 || header[i - 1] == ' ')
            && header.get(i + n.len()).is_none_or(|c| *c == ' ')
    })
}

fn cell(line: &[char], from: usize, to: Option<usize>) -> String {
    let end = to.unwrap_or(line.len()).min(line.len());
    if from >= end {
        return String::new();
    }
    line[from..end]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parses `winget upgrade` output: the table between the dashed rule and the
/// "N upgrades available." line. Progress spinners before it are ignored.
pub fn parse(out: &str) -> Vec<Update> {
    let lines: Vec<&str> = out
        .lines()
        .map(|l| l.rsplit('\r').next().unwrap_or(l))
        .collect();
    let Some(rule) = lines.iter().position(|l| l.trim_start().starts_with("---")) else {
        return Vec::new();
    };
    let Some(header) = rule
        .checked_sub(1)
        .map(|i| lines[i].chars().collect::<Vec<char>>())
    else {
        return Vec::new();
    };
    let (Some(id), Some(ver), Some(avail)) = (
        col(&header, "Id"),
        col(&header, "Version"),
        col(&header, "Available"),
    ) else {
        return Vec::new();
    };
    let src = col(&header, "Source");
    let mut v = Vec::new();
    for l in &lines[rule + 1..] {
        let c: Vec<char> = l.chars().collect();
        if c.len() <= avail || l.trim().is_empty() {
            break; // the summary line or a second table follows
        }
        let u = Update {
            name: cell(&c, 0, Some(id)),
            id: cell(&c, id, Some(ver)),
            current: cell(&c, ver, Some(avail)),
            available: cell(&c, avail, src),
            source: src.map_or_else(String::new, |s| cell(&c, s, None)),
        };
        if !u.id.is_empty() {
            v.push(u);
        }
    }
    v
}

/// Packages winget can upgrade from its community source (a few seconds).
pub fn list() -> std::io::Result<Vec<Update>> {
    let out = Command::new("winget")
        .args([
            "upgrade",
            "--include-unknown",
            // ponytail: the msstore source makes the check ~2 min instead of ~5 s;
            // Store apps update themselves through the Store anyway
            "--source",
            "winget",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Updates one package silently; `Ok(false)` when winget reports a failure.
pub fn update(id: &str) -> std::io::Result<bool> {
    let status = Command::new("winget")
        .args([
            "upgrade",
            "--id",
            id,
            "--exact",
            "--source",
            "winget",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "   - \r   \\ \r\
Name                       Id                       Version      Available    Source\n\
------------------------------------------------------------------------------------\n\
VLC media player           VideoLAN.VLC             3.0.23       3.0.24       winget\n\
Zoom Workplace             Zoom.Zoom.EXE            7.1.0 (41345) 7.2.5 (51704) winget\n\
Python Launcher            Python.Launcher          < 3.14.7     3.15.2       winget\n\
3 upgrades available.\n";

    #[test]
    fn parses_the_upgrade_table() {
        let v = parse(OUT);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "VLC media player");
        assert_eq!(v[0].id, "VideoLAN.VLC");
        assert_eq!(
            (v[0].current.as_str(), v[0].available.as_str()),
            ("3.0.23", "3.0.24")
        );
        assert_eq!(v[2].current, "< 3.14.7");
        assert_eq!(v[2].source, "winget");
        assert!(parse("No installed package found.").is_empty());
    }

    /// Asks winget (slow): `cargo test -p bosun-core real_updates -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_updates() {
        let v = list().unwrap();
        for u in v.iter().take(8) {
            println!(
                "{} | {} | {} -> {} | {}",
                u.name, u.id, u.current, u.available, u.source
            );
        }
        println!("{} updates", v.len());
    }
}
