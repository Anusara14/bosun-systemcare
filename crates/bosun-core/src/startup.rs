//! Startup items (spec §2.2, Startup Optimizer → Startup Items): the Run keys
//! and Startup folders, and whether each is enabled.
//!
//! Enabling and disabling works like Task Manager's Startup tab: the entry
//! itself is never touched; a 12-byte flag under
//! `…\Explorer\StartupApproved\{Run,Run32,StartupFolder}` says on (`02`) or
//! off (`03` + the time it was turned off). That makes every change
//! reversible, and the old flag is journaled before it is written.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_BINARY,
    REG_OPTION_NON_VOLATILE, RRF_RT_REG_BINARY, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{FOLDERID_CommonStartup, FOLDERID_Startup};
use windows::core::{GUID, HSTRING};

use crate::paths;
use crate::registry::{Key, open_in, string, value_names};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// `HKCU\…\Run`: this user only.
    UserRun,
    /// `HKLM\…\Run`: every user (changing it needs admin).
    MachineRun,
    /// `HKLM\WOW6432Node\…\Run`: 32-bit programs, every user.
    MachineRun32,
    /// This user's Startup folder.
    UserFolder,
    /// The all-users Startup folder.
    CommonFolder,
}

impl Source {
    /// Where its on/off flag lives.
    fn approved(self) -> (HKEY, &'static str) {
        match self {
            Source::UserRun => (HKEY_CURRENT_USER, "Run"),
            Source::MachineRun => (HKEY_LOCAL_MACHINE, "Run"),
            Source::MachineRun32 => (HKEY_LOCAL_MACHINE, "Run32"),
            Source::UserFolder => (HKEY_CURRENT_USER, "StartupFolder"),
            Source::CommonFolder => (HKEY_LOCAL_MACHINE, "StartupFolder"),
        }
    }

    /// Changing it needs administrator rights.
    pub fn machine_wide(self) -> bool {
        matches!(
            self,
            Source::MachineRun | Source::MachineRun32 | Source::CommonFolder
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Value name, or the shortcut's file name for Startup folders.
    pub name: String,
    pub command: String,
    pub source: Source,
    pub enabled: bool,
    /// The program it starts, when one can be recognised.
    pub exe: Option<PathBuf>,
}

impl Item {
    /// The program is gone, so the entry does nothing at sign-in.
    pub fn broken(&self) -> bool {
        self.exe.as_deref().is_some_and(paths::is_missing)
    }
}

/// What to do with a startup entry (our own rules; spec §2.2 replaces
/// IObit's crowd "disable rate" with these).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Advice {
    /// Windows, security or driver helper: leave it on.
    Keep,
    /// Fine to disable or delay.
    Optional,
    /// Its program is gone; it does nothing.
    Disable,
}

/// Keep Windows' own and driver helpers (programs under `%WINDIR%`, e.g.
/// DriverStore) and Windows Security; anything with a missing program can
/// go; the rest is optional.
pub fn advice(item: &Item) -> Advice {
    if item.broken() {
        return Advice::Disable;
    }
    let win = std::env::var("WINDIR")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let in_windows = item.exe.as_ref().is_some_and(|e| {
        !win.is_empty() && e.to_string_lossy().to_ascii_lowercase().starts_with(&win)
    });
    if in_windows || item.name.eq_ignore_ascii_case("SecurityHealth") {
        Advice::Keep
    } else {
        Advice::Optional
    }
}

/// Enabled entries we would turn off: the "N startup items can be
/// optimized" count.
pub fn optimizable(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|i| i.enabled && advice(i) != Advice::Keep)
        .count()
}

/// Task Manager's flag: the low bit of the first byte set means "off";
/// no flag at all means on.
pub fn flag_enabled(bytes: &[u8]) -> bool {
    bytes.first().is_none_or(|b| b & 1 == 0)
}

/// The 12-byte flag for on, or off since `when`.
pub fn flag_bytes(on: bool, when: SystemTime) -> [u8; 12] {
    let mut b = [0u8; 12];
    if on {
        b[0] = 0x02;
    } else {
        b[0] = 0x03;
        // FILETIME: 100 ns ticks since 1601-01-01
        let unix = when
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() / 100);
        let ft = (unix + 116_444_736_000_000_000) as u64;
        b[4..].copy_from_slice(&ft.to_le_bytes());
    }
    b
}

fn read_flag(source: Source, name: &str) -> Option<Vec<u8>> {
    let (root, sub) = source.approved();
    let k = open_in(root, &format!(r"{APPROVED}\{sub}"), false)?;
    let mut buf = [0u8; 64];
    let mut cb = buf.len() as u32;
    // SAFETY: reads one binary value into our buffer.
    let r = unsafe {
        RegGetValueW(
            k.0,
            None,
            &HSTRING::from(name),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut cb),
        )
    };
    (r == ERROR_SUCCESS).then(|| buf[..cb as usize].to_vec())
}

fn known(id: &GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned buffer is copied, then freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

fn run_items(root: HKEY, key: &str, source: Source, out: &mut Vec<Item>) {
    let Some(k): Option<Key> = open_in(root, key, false) else {
        return;
    };
    for name in value_names(&k) {
        let command = string(&k, None, &name).unwrap_or_default();
        let enabled = read_flag(source, &name).is_none_or(|b| flag_enabled(&b));
        let exe = paths::exe_from_command(&command);
        out.push(Item {
            name,
            command,
            source,
            enabled,
            exe,
        });
    }
}

fn folder_items(dir: &Path, source: Source, out: &mut Vec<Item>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !p.is_file() || name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let is_lnk = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"));
        let exe = if is_lnk {
            crate::shortcuts::target(&p)
        } else {
            Some(p.clone())
        };
        let enabled = read_flag(source, &name).is_none_or(|b| flag_enabled(&b));
        out.push(Item {
            name,
            command: exe
                .as_ref()
                .map_or_else(String::new, |e| e.display().to_string()),
            source,
            enabled,
            exe,
        });
    }
}

/// Every startup entry from the Run keys and Startup folders.
pub fn list() -> Vec<Item> {
    let mut v = Vec::new();
    run_items(HKEY_CURRENT_USER, RUN, Source::UserRun, &mut v);
    run_items(HKEY_LOCAL_MACHINE, RUN, Source::MachineRun, &mut v);
    run_items(HKEY_LOCAL_MACHINE, RUN32, Source::MachineRun32, &mut v);
    if let Some(d) = known(&FOLDERID_Startup) {
        folder_items(&d, Source::UserFolder, &mut v);
    }
    if let Some(d) = known(&FOLDERID_CommonStartup) {
        folder_items(&d, Source::CommonFolder, &mut v);
    }
    v
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Turns an entry on or off. The old flag goes into a journal in `dir`
/// first; restoring it means writing those bytes back.
pub fn set_enabled(item: &Item, on: bool, dir: &Path) -> io::Result<()> {
    let (root, sub) = item.source.approved();
    let key = format!(r"{APPROVED}\{sub}");
    let old = read_flag(item.source, &item.name);
    let new = flag_bytes(on, SystemTime::now());
    let (_, mut f) = crate::clean::journal_file(dir, "Startup")?;
    let hive = if item.source.machine_wide() {
        "HKLM"
    } else {
        "HKCU"
    };
    writeln!(
        f,
        "S\t{hive}\\{key}\t{}\t{}\t{}",
        item.name,
        old.as_deref().map_or_else(|| "none".into(), hex),
        hex(&new)
    )?;
    f.sync_all()?;
    let mut h = HKEY::default();
    // SAFETY: creates/opens the flag key and writes one binary value.
    unsafe {
        RegCreateKeyExW(
            root,
            &HSTRING::from(key.as_str()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut h,
            None,
        )
        .ok()
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
        let k = Key(h);
        RegSetValueExW(
            k.0,
            &HSTRING::from(item.name.as_str()),
            None,
            REG_BINARY,
            Some(&new),
        )
        .ok()
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))
    }
}

/// The name of Bosun's own Run entry (it starts the tray).
pub const AUTOSTART_NAME: &str = "Bosun SystemCare";

/// Our own entry in this user's Run key is there.
pub fn autostart(name: &str) -> bool {
    open_in(HKEY_CURRENT_USER, RUN, false).is_some_and(|k| string(&k, None, name).is_some())
}

/// Adds (`Some(command)`) or removes our own Run entry. Ours only, so no journal.
pub fn set_autostart(name: &str, command: Option<&str>) -> io::Result<()> {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{REG_SZ, RegDeleteValueW};
    let mut h = HKEY::default();
    // SAFETY: opens this user's Run key and writes or deletes one value of ours.
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut h,
            None,
        )
        .ok()
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))?;
        let k = Key(h);
        let r = match command {
            Some(c) => {
                let bytes: Vec<u8> = c
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                RegSetValueExW(k.0, &HSTRING::from(name), None, REG_SZ, Some(&bytes))
            }
            None => match RegDeleteValueW(k.0, &HSTRING::from(name)) {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                r => r,
            },
        };
        r.ok().map_err(|e| io::Error::from_raw_os_error(e.code().0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn flags_read_like_task_manager() {
        assert!(flag_enabled(&[0x02, 0, 0, 0]));
        assert!(flag_enabled(&[0x06, 0, 0, 0]));
        assert!(!flag_enabled(&[0x03, 0, 0, 0]));
        assert!(!flag_enabled(&[0x07]));
        assert!(flag_enabled(&[]), "no flag: on");
        assert_eq!(
            flag_bytes(true, SystemTime::now()),
            [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        let off = flag_bytes(false, UNIX_EPOCH + Duration::from_secs(1));
        assert_eq!(off[0], 3);
        // 1970-01-01T00:00:01Z as FILETIME
        assert_eq!(
            u64::from_le_bytes(off[4..].try_into().unwrap()),
            116_444_736_010_000_000
        );
    }

    #[test]
    fn toggling_writes_the_flag_and_journals_the_old_one() {
        let item = Item {
            name: format!("BosunTest-{}", std::process::id()),
            command: String::new(),
            source: Source::UserRun,
            enabled: true,
            exe: None,
        };
        let dir = std::env::temp_dir().join(format!("bosun-startup-{}", std::process::id()));
        set_enabled(&item, false, &dir).unwrap();
        assert_eq!(
            read_flag(item.source, &item.name).map(|b| flag_enabled(&b)),
            Some(false)
        );
        set_enabled(&item, true, &dir).unwrap();
        assert_eq!(
            read_flag(item.source, &item.name).map(|b| flag_enabled(&b)),
            Some(true)
        );
        let journals: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect();
        assert!(
            journals.iter().any(|j| j.contains("\tnone\t03")),
            "first change: no old flag"
        );
        // clean up the test flag
        use windows::Win32::System::Registry::RegDeleteValueW;
        let k = open_in(HKEY_CURRENT_USER, &format!(r"{APPROVED}\Run"), true).unwrap();
        unsafe {
            let _ = RegDeleteValueW(k.0, &HSTRING::from(item.name.as_str()));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn advice_keeps_windows_helpers_and_drops_broken_ones() {
        let win = std::env::var("WINDIR").unwrap();
        let item = |name: &str, exe: String| Item {
            name: name.into(),
            command: exe.clone(),
            source: Source::UserRun,
            enabled: true,
            exe: Some(PathBuf::from(exe)),
        };
        let audio = item("RtkAudUService", format!(r"{win}\System32\notepad.exe"));
        assert_eq!(advice(&audio), Advice::Keep);
        let app = item("Zebar", r"D:\Software\Zebar\zebar.exe".into());
        assert_eq!(advice(&app), Advice::Optional);
        let gone = item("Old", format!(r"{win}\no-such-bosun-app.exe"));
        assert_eq!(advice(&gone), Advice::Disable);
        let mut off = item("Spotify", r"C:\x\spotify.exe".into());
        off.enabled = false;
        assert_eq!(optimizable(&[app, off, audio]), 1);
    }

    /// Read-only: `cargo test -p bosun-core real_startup -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_startup() {
        for i in list() {
            println!(
                "{:?} on={} broken={} {} | {}",
                i.source,
                i.enabled,
                i.broken(),
                i.name,
                i.command
            );
        }
    }
}
