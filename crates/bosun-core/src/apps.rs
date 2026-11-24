//! Installed programs (spec §2.2, App/Extension Cleaner): the Windows
//! uninstall list, sortable by size and install date. Uninstalling runs the
//! program's own uninstaller; we never delete program files ourselves.

use std::path::PathBuf;

use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

use crate::registry::{dword, open_in, string, subkeys};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// Estimated size in bytes (0 when the installer didn't say).
    pub size: u64,
    /// `YYYYMMDD`, when the installer recorded it.
    pub installed: String,
    pub uninstall: String,
    /// Program whose icon represents it.
    pub icon: Option<PathBuf>,
}

impl App {
    /// "2026-11-23" from "20261123".
    pub fn date(&self) -> String {
        let d = &self.installed;
        if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
            format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..])
        } else {
            String::new()
        }
    }
}

/// `"C:\x\app.exe",0` → `C:\x\app.exe`.
pub fn icon_path(display_icon: &str) -> Option<PathBuf> {
    let s = display_icon.trim();
    let s = s.split_once(',').map_or(s, |(p, _)| p).trim_matches('"');
    (!s.is_empty()).then(|| PathBuf::from(crate::paths::expand(s)))
}

fn read(root: HKEY, key: &str, out: &mut Vec<App>) {
    let Some(k) = open_in(root, key, false) else {
        return;
    };
    for sub in subkeys(&k) {
        let s = |name: &str| string(&k, Some(&sub), name).unwrap_or_default();
        let name = s("DisplayName");
        let uninstall = s("UninstallString");
        // Updates, components and entries without an uninstaller aren't programs
        // people manage.
        if name.is_empty()
            || uninstall.is_empty()
            || dword(&k, Some(&sub), "SystemComponent") == Some(1)
            || !s("ParentKeyName").is_empty()
            || !s("ReleaseType").is_empty()
        {
            continue;
        }
        let icon = icon_path(&s("DisplayIcon"));
        out.push(App {
            publisher: s("Publisher"),
            version: s("DisplayVersion"),
            size: u64::from(dword(&k, Some(&sub), "EstimatedSize").unwrap_or(0)) * 1024,
            installed: s("InstallDate"),
            uninstall,
            icon,
            name,
        });
    }
}

/// Every installed program (64-bit, 32-bit and per-user), by name, without
/// duplicates.
pub fn list() -> Vec<App> {
    let mut out = Vec::new();
    read(HKEY_LOCAL_MACHINE, UNINSTALL, &mut out);
    read(HKEY_LOCAL_MACHINE, UNINSTALL32, &mut out);
    read(HKEY_CURRENT_USER, UNINSTALL, &mut out);
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.name == b.name && a.version == b.version);
    out
}

/// Starts the program's own uninstaller (it may ask for admin rights or
/// show its own window). Returns whether Windows started it.
pub fn uninstall(app: &App) -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let cmd = crate::paths::expand(&app.uninstall);
    // Split "program" from its arguments the same way the shell would.
    let (exe, args) = match cmd.strip_prefix('"').and_then(|r| r.split_once('"')) {
        Some((e, rest)) => (e.to_string(), rest.trim().to_string()),
        None => match crate::paths::exe_from_command(&cmd) {
            Some(e) => {
                let e = e.display().to_string();
                let rest = cmd[e.len().min(cmd.len())..].trim().to_string();
                (e, rest)
            }
            None => (cmd.clone(), String::new()),
        },
    };
    // SAFETY: runs the uninstaller the program registered for itself.
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(exe),
            &HSTRING::from(args),
            None,
            SW_SHOWNORMAL,
        )
    };
    r.0 as isize > 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_and_dates_parse() {
        assert_eq!(
            icon_path(r#""C:\A\b.exe",0"#),
            Some(PathBuf::from(r"C:\A\b.exe"))
        );
        assert_eq!(icon_path(""), None);
        let a = App {
            name: "x".into(),
            publisher: String::new(),
            version: String::new(),
            size: 0,
            installed: "20261123".into(),
            uninstall: String::new(),
            icon: None,
        };
        assert_eq!(a.date(), "2026-11-23");
    }

    #[test]
    fn lists_installed_programs() {
        // Any Windows with a browser or runtime installed has entries.
        let all = list();
        assert!(
            all.iter()
                .all(|a| !a.name.is_empty() && !a.uninstall.is_empty())
        );
    }

    /// Read-only: `cargo test -p bosun-core real_apps -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_apps() {
        let all = list();
        for a in all.iter().take(15) {
            println!(
                "{} | {} | {} | {}",
                a.name,
                a.publisher,
                crate::format::bytes(a.size),
                a.date()
            );
        }
        println!("{} programs", all.len());
    }
}
