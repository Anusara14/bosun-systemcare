//! Registry Clean, current-user entries only (the app runs unelevated) and
//! only *provably* orphaned ones (spec §2.1):
//!
//! | where (under HKCU)                                   | orphaned when            |
//! |------------------------------------------------------|--------------------------|
//! | `Software\Microsoft\Windows\CurrentVersion\Run`      | the program it runs is gone |
//! | `…\Shell\MuiCache`                                   | the program it names is gone |
//! | `Software\Microsoft\Windows\CurrentVersion\App Paths\<x>` | its default path is gone |
//!
//! Before anything is removed, every affected key is exported with
//! `reg export` into the journal folder; double-clicking it restores it.

use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::core::{HSTRING, PWSTR};

use crate::paths;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const MUI: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache";
const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A value under `key`.
    Value(String),
    /// A whole subkey of `key`.
    Key(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Orphan {
    /// Parent key under HKCU.
    pub key: &'static str,
    pub item: Item,
    /// The missing file that makes it an orphan.
    pub missing: PathBuf,
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: closing a key we opened.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(sub: &str, write: bool) -> Option<Key> {
    let mut h = HKEY::default();
    let sam = if write {
        KEY_READ | KEY_SET_VALUE
    } else {
        KEY_READ
    };
    // SAFETY: opens a key under HKCU into `h`.
    let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(sub), None, sam, &mut h) };
    (r == ERROR_SUCCESS).then_some(Key(h))
}

fn value_names(k: &Key) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 16_384];
        let mut len = buf.len() as u32;
        // SAFETY: enumerates names only into our buffer.
        let r = unsafe {
            RegEnumValueW(
                k.0,
                i,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

fn subkey_names(k: &Key) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        // SAFETY: enumerates subkey names into our buffer.
        let r = unsafe {
            RegEnumKeyExW(
                k.0,
                i,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    out
}

/// A string value (`""` for the default value), environment-expanded.
fn string(k: &Key, sub: Option<&str>, name: &str) -> Option<String> {
    let sub = sub.map(HSTRING::from).unwrap_or_default();
    let name = HSTRING::from(name);
    let mut buf = vec![0u16; 4096];
    let mut cb = (buf.len() * 2) as u32;
    // SAFETY: reads one string value into our buffer of `cb` bytes.
    let r = unsafe {
        RegGetValueW(
            k.0,
            &sub,
            &name,
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut cb),
        )
    };
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (cb as usize / 2).saturating_sub(1);
    buf.truncate(n);
    Some(String::from_utf16_lossy(&buf))
}

/// `C:\x\app.exe.FriendlyAppName` → `C:\x\app.exe`.
pub fn mui_exe(name: &str) -> Option<PathBuf> {
    let at = name.to_ascii_lowercase().rfind(".exe.")?;
    Some(PathBuf::from(&name[..at + 4]))
}

/// Every provably orphaned entry. `visit` sees each entry checked; `false` stops.
pub fn find(visit: &mut dyn FnMut(&str) -> bool) -> Vec<Orphan> {
    let mut out = Vec::new();
    if let Some(k) = open(RUN, false) {
        for name in value_names(&k) {
            if !visit(&name) {
                return out;
            }
            let exe = string(&k, None, &name).and_then(|c| paths::exe_from_command(&c));
            if let Some(exe) = exe.filter(|e| paths::is_missing(e)) {
                out.push(Orphan {
                    key: RUN,
                    item: Item::Value(name),
                    missing: exe,
                });
            }
        }
    }
    if let Some(k) = open(MUI, false) {
        for name in value_names(&k) {
            if !visit(&name) {
                return out;
            }
            if let Some(exe) = mui_exe(&name).filter(|e| paths::is_missing(e)) {
                out.push(Orphan {
                    key: MUI,
                    item: Item::Value(name),
                    missing: exe,
                });
            }
        }
    }
    if let Some(k) = open(APP_PATHS, false) {
        for sub in subkey_names(&k) {
            if !visit(&sub) {
                return out;
            }
            let path = string(&k, Some(&sub), "").map(|p| PathBuf::from(p.trim_matches('"')));
            if let Some(p) = path.filter(|p| paths::is_missing(p)) {
                out.push(Orphan {
                    key: APP_PATHS,
                    item: Item::Key(sub),
                    missing: p,
                });
            }
        }
    }
    out
}

/// Exports `HKCU\<key>` with `reg export` to `file`.
fn export(key: &str, file: &Path) -> io::Result<()> {
    let ok = Command::new("reg.exe")
        .args(["export", &format!(r"HKCU\{key}")])
        .arg(file)
        .arg("/y")
        .creation_flags(CREATE_NO_WINDOW)
        .status()?
        .success();
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(format!("reg export of {key} failed")))
    }
}

fn remove(o: &Orphan) -> io::Result<()> {
    let k = open(o.key, true).ok_or_else(|| io::Error::other("can't open key"))?;
    // SAFETY: deletes one value or subtree under a key we opened for writing.
    let r = unsafe {
        match &o.item {
            Item::Value(n) => RegDeleteValueW(k.0, &HSTRING::from(n.as_str())),
            Item::Key(n) => RegDeleteTreeW(k.0, &HSTRING::from(n.as_str())),
        }
    };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r.0 as i32))
    }
}

/// Backs up every affected key, journals the entries, then removes them.
/// Nothing is removed if a backup fails. Returns (removed, left).
pub fn fix(
    orphans: &[Orphan],
    dir: &Path,
    dry_run: bool,
    progress: &mut dyn FnMut(&str, f32) -> bool,
) -> io::Result<(u64, u64)> {
    let (jpath, mut f) = crate::clean::journal_file(dir, "Registry Clean")?;
    writeln!(
        f,
        "# bosun journal v1\tregistry\tRegistry Clean\tdry_run={dry_run}"
    )?;
    let mut keys: Vec<&str> = orphans.iter().map(|o| o.key).collect();
    keys.dedup();
    let stem = jpath.with_extension("");
    for (i, key) in keys.iter().enumerate() {
        let backup = PathBuf::from(format!("{}-{i}.reg", stem.display()));
        export(key, &backup)?;
        writeln!(f, "B\tHKCU\\{key}\t{}", backup.display())?;
    }
    for o in orphans {
        let what = match &o.item {
            Item::Value(n) => format!("value\t{n}"),
            Item::Key(n) => format!("key\t{n}"),
        };
        writeln!(f, "D\tHKCU\\{}\t{what}\t{}", o.key, o.missing.display())?;
    }
    f.sync_all()?;
    let (mut done, mut left) = (0u64, 0u64);
    for (i, o) in orphans.iter().enumerate() {
        let label = match &o.item {
            Item::Value(n) | Item::Key(n) => n.as_str(),
        };
        if !progress(label, (i + 1) as f32 / orphans.len().max(1) as f32) {
            left += (orphans.len() - i) as u64;
            break;
        }
        let r = if dry_run { Ok(()) } else { remove(o) };
        match r {
            Ok(()) => done += 1,
            Err(e) => {
                left += 1;
                writeln!(f, "F\t{label}\t{e}")?;
            }
        }
    }
    writeln!(f, "E\tremoved={done}\tleft={left}")?;
    Ok((done, left))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mui_names_map_to_their_program() {
        assert_eq!(
            mui_exe(r"C:\Apps\Tool.exe.FriendlyAppName"),
            Some(PathBuf::from(r"C:\Apps\Tool.exe"))
        );
        assert_eq!(
            mui_exe(r"C:\x\a.EXE.ApplicationCompany"),
            Some(PathBuf::from(r"C:\x\a.EXE"))
        );
        assert_eq!(mui_exe("LangID"), None);
    }

    /// Creates a throwaway HKCU key with one orphaned and one live value, then
    /// backs up and removes the orphan through the same code paths.
    #[test]
    fn removes_only_the_orphan_after_backing_up() {
        use windows::Win32::System::Registry::{
            KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RegCreateKeyExW, RegSetValueExW,
        };
        const TEST: &str = r"Software\BosunTest\Run";
        let mut h = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(TEST),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut h,
                None,
            )
            .ok()
            .unwrap();
            let set = |name: &str, data: &str| {
                let w: Vec<u16> = data.encode_utf16().chain([0]).collect();
                let bytes = std::slice::from_raw_parts(w.as_ptr().cast::<u8>(), w.len() * 2);
                RegSetValueExW(h, &HSTRING::from(name), None, REG_SZ, Some(bytes))
                    .ok()
                    .unwrap();
            };
            let win = std::env::var("WINDIR").unwrap();
            set("gone", &format!(r"{win}\no-such-bosun.exe /x"));
            set("live", &format!(r"{win}\notepad.exe"));
            let _ = RegCloseKey(h);
        }
        let k = open(TEST, false).unwrap();
        let orphans: Vec<Orphan> = value_names(&k)
            .into_iter()
            .filter_map(|n| {
                let exe = string(&k, None, &n).and_then(|c| paths::exe_from_command(&c))?;
                paths::is_missing(&exe).then_some(Orphan {
                    key: TEST,
                    item: Item::Value(n),
                    missing: exe,
                })
            })
            .collect();
        drop(k);
        assert_eq!(orphans.len(), 1);
        let dir = std::env::temp_dir().join(format!("bosun-reg-{}", std::process::id()));
        let (n, left) = fix(&orphans, &dir, false, &mut |_, _| true).unwrap();
        assert_eq!((n, left), (1, 0));
        let k = open(TEST, false).unwrap();
        assert_eq!(value_names(&k), ["live"]);
        drop(k);
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "reg"))
            .collect();
        assert_eq!(backups.len(), 1, "one .reg backup per affected key");
        unsafe {
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(r"Software\BosunTest"));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Read-only: `cargo test -p bosun-core real_registry -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_registry() {
        let mut n = 0;
        for o in find(&mut |_| {
            n += 1;
            true
        }) {
            println!("{} {:?} -> {}", o.key, o.item, o.missing.display());
        }
        println!("checked {n}");
    }
}
