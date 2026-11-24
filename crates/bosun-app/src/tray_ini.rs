//! Reads and writes the tray's `%APPDATA%\Bosun\tray.ini` (`key=value`
//! lines), so Speed Up's Real-Time TuneUp switches share the tray's settings.
//! ponytail: the running tray reads the file at start; live sync needs the
//! tray↔app pipe from the spec.
//!
//! The app's own settings go in `app.ini` beside it: the tray rewrites
//! `tray.ini` with only its keys.

use std::path::PathBuf;

const TRAY: &str = "tray.ini";
pub const APP: &str = "app.ini";

fn path(file: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Bosun").join(file))
}

/// `key`'s value in `text` as a switch; `default` when absent.
pub fn get_in(text: &str, key: &str, default: bool) -> bool {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .map_or(default, |(_, v)| v.trim() == "1")
}

/// `text` with `key` set to `on`, added if missing; other lines untouched.
pub fn set_in(text: &str, key: &str, on: bool) -> String {
    let line = format!("{key}={}", u8::from(on));
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, _)) if k.trim() == key => {
                found = true;
                line.clone()
            }
            _ => l.to_string(),
        })
        .collect();
    if !found {
        out.push(line);
    }
    out.join("\n") + "\n"
}

pub fn get(key: &str, default: bool) -> bool {
    get_from(TRAY, key, default)
}

pub fn set(key: &str, on: bool) -> std::io::Result<()> {
    set_to(TRAY, key, on)?;
    notify_tray();
    Ok(())
}

/// Asks a running tray to re-read `tray.ini` (its `Cmd::Reload`, posted as
/// `WM_APP + 3`). No tray running: nothing to do, it reads the file at start.
pub fn notify_tray() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};
    // SAFETY: finds the tray's host window by class and posts it a message.
    unsafe {
        if let Ok(h) = FindWindowW(windows::core::w!("BosunTrayHost"), None) {
            let _ = PostMessageW(Some(h), WM_APP + 3, WPARAM(11), LPARAM(0));
        }
    }
}

/// A number from `tray.ini` (the tray writes Auto Clean's last run there).
pub fn get_number(key: &str) -> Option<u64> {
    let text = std::fs::read_to_string(path(TRAY)?).ok()?;
    text.lines()
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .and_then(|(_, v)| v.trim().parse().ok())
}

pub fn get_from(file: &str, key: &str, default: bool) -> bool {
    path(file)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or(default, |t| get_in(&t, key, default))
}

pub fn set_to(file: &str, key: &str, on: bool) -> std::io::Result<()> {
    let p = path(file).ok_or_else(|| std::io::Error::other("APPDATA is unset"))?;
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, set_in(&text, key, on))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_rewrites_one_key() {
        let t = "widget_visible=1\nauto_ram_clean=0\nwidget_x=5\n";
        assert!(get_in(t, "widget_visible", false));
        assert!(!get_in(t, "auto_ram_clean", true));
        assert!(get_in(t, "missing", true));
        let t2 = set_in(t, "auto_ram_clean", true);
        assert_eq!(t2, "widget_visible=1\nauto_ram_clean=1\nwidget_x=5\n");
        assert!(set_in("", "a", true).ends_with("a=1\n"));
    }
}
