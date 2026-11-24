//! System Optimization (spec §2.1, 🔁): only real, reversible tweaks, each
//! one value in this user's registry, shown with its exact value. The old
//! value is journaled first, so the Rescue Center can put it back.
//! Placebo tweaks (prefetch "boosts", network "accelerators") are left out.

use std::io::{self, Write};
use std::path::Path;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
};
use windows::core::HSTRING;

use crate::registry::{Key, dword, open_in, string};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Sz(&'static str),
    Dword(u32),
}

impl Value {
    fn kind(self) -> &'static str {
        match self {
            Value::Sz(_) => "sz",
            Value::Dword(_) => "dword",
        }
    }

    fn text(self) -> String {
        match self {
            Value::Sz(s) => s.to_string(),
            Value::Dword(d) => d.to_string(),
        }
    }
}

pub struct Tweak {
    pub label: &'static str,
    pub key: &'static str,
    pub name: &'static str,
    pub value: Value,
}

const DESKTOP: &str = r"Control Panel\Desktop";

pub const TWEAKS: [Tweak; 5] = [
    Tweak {
        label: "End frozen apps automatically at sign-out",
        key: DESKTOP,
        name: "AutoEndTasks",
        value: Value::Sz("1"),
    },
    Tweak {
        label: "Close hung apps after 3 s instead of 5 s",
        key: DESKTOP,
        name: "HungAppTimeout",
        value: Value::Sz("3000"),
    },
    Tweak {
        label: "Wait 5 s instead of 20 s for apps at sign-out",
        key: DESKTOP,
        name: "WaitToKillAppTimeout",
        value: Value::Sz("5000"),
    },
    Tweak {
        label: "Open menus after 200 ms instead of 400 ms",
        key: DESKTOP,
        name: "MenuShowDelay",
        value: Value::Sz("200"),
    },
    Tweak {
        label: "Start your startup apps without Windows' delay",
        key: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize",
        name: "StartupDelayInMSec",
        value: Value::Dword(0),
    },
];

/// The value now, as text (`None` when it isn't set).
fn current(t: &Tweak) -> Option<String> {
    let k = open_in(HKEY_CURRENT_USER, t.key, false)?;
    match t.value {
        Value::Sz(_) => string(&k, None, t.name),
        Value::Dword(_) => dword(&k, None, t.name).map(|d| d.to_string()),
    }
}

/// Whether a tweak would improve on `now` (the value as set, if any). The
/// timeouts and delays are "lower is faster": a value already at or below
/// ours is left alone (someone chose it; ours would be slower). Not set means
/// Windows' slower default. On/off switches just compare.
fn improves(t: &Tweak, now: Option<&str>) -> bool {
    let target = t.value.text();
    let Some(now) = now else { return true };
    let numeric = t.name != "AutoEndTasks";
    match (numeric, now.trim().parse::<u64>(), target.parse::<u64>()) {
        (true, Ok(n), Ok(want)) => n > want,
        _ => now.trim() != target,
    }
}

/// Indices of the tweaks that would make things faster than they are now.
pub fn pending() -> Vec<usize> {
    (0..TWEAKS.len())
        .filter(|&i| improves(&TWEAKS[i], current(&TWEAKS[i]).as_deref()))
        .collect()
}

fn win(r: windows::Win32::Foundation::WIN32_ERROR) -> io::Result<()> {
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(r.0 as i32))
    }
}

/// A value to write, owned (journals hand back old values as text).
enum Raw {
    Sz(String),
    Dword(u32),
}

impl From<Value> for Raw {
    fn from(v: Value) -> Self {
        match v {
            Value::Sz(s) => Raw::Sz(s.into()),
            Value::Dword(d) => Raw::Dword(d),
        }
    }
}

/// Writes `v` (or deletes the value for `None`) under HKCU\`key`.
fn write(key: &str, name: &str, v: Option<Raw>) -> io::Result<()> {
    let mut h = HKEY::default();
    // SAFETY: opens/creates one HKCU key and writes or deletes one value.
    unsafe {
        win(RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut h,
            None,
        ))?;
        let k = Key(h);
        let name = HSTRING::from(name);
        match v {
            Some(Raw::Sz(s)) => {
                let b: Vec<u8> = s
                    .encode_utf16()
                    .chain([0])
                    .flat_map(u16::to_le_bytes)
                    .collect();
                win(RegSetValueExW(k.0, &name, None, REG_SZ, Some(&b)))
            }
            Some(Raw::Dword(d)) => win(RegSetValueExW(
                k.0,
                &name,
                None,
                REG_DWORD,
                Some(&d.to_le_bytes()),
            )),
            None => match RegDeleteValueW(k.0, &name) {
                ERROR_FILE_NOT_FOUND => Ok(()),
                r => win(r),
            },
        }
    }
}

/// Applies tweaks `which`, journal first. Returns (applied, left).
pub fn apply(
    which: &[usize],
    dir: &Path,
    dry_run: bool,
    progress: &mut dyn FnMut(&str, f32) -> bool,
) -> io::Result<(u64, u64)> {
    let (_, mut f) = crate::clean::journal_file(dir, "System Optimization")?;
    writeln!(
        f,
        "# bosun journal v1\ttweaks\tSystem Optimization\tdry_run={dry_run}"
    )?;
    for &i in which {
        let t = &TWEAKS[i];
        let old = current(t).unwrap_or_else(|| "none".into());
        writeln!(
            f,
            "W\tHKCU\\{}\t{}\t{}\t{old}\t{}\t{}",
            t.key,
            t.name,
            t.value.kind(),
            t.value.text(),
            t.label
        )?;
    }
    f.sync_all()?;
    let (mut done, mut left) = (0u64, 0u64);
    for (n, &i) in which.iter().enumerate() {
        let t = &TWEAKS[i];
        if !progress(t.label, (n + 1) as f32 / which.len().max(1) as f32) {
            left += (which.len() - n) as u64;
            break;
        }
        match if dry_run {
            Ok(())
        } else {
            write(t.key, t.name, Some(t.value.into()))
        } {
            Ok(()) => done += 1,
            Err(e) => {
                left += 1;
                writeln!(f, "F\t{}\t{e}", t.name)?;
            }
        }
    }
    writeln!(f, "E\tapplied={done}\tleft={left}")?;
    Ok((done, left))
}

/// Puts one journaled `W` line's old value back.
pub(crate) fn restore_line(line: &str) -> io::Result<()> {
    let f: Vec<&str> = line.split('\t').collect();
    let (Some(key), Some(name), Some(kind), Some(old)) = (f.get(1), f.get(2), f.get(3), f.get(4))
    else {
        return Err(io::Error::other("malformed journal line"));
    };
    let key = key
        .strip_prefix(r"HKCU\")
        .ok_or_else(|| io::Error::other("not an HKCU key"))?;
    // only the values this module writes may be touched
    if !TWEAKS.iter().any(|t| t.key == key && t.name == *name) {
        return Err(io::Error::other("not one of Bosun's tweaks"));
    }
    let v = match (*kind, *old) {
        (_, "none") => None,
        ("dword", o) => Some(Raw::Dword(
            o.parse().map_err(|_| io::Error::other("bad dword"))?,
        )),
        (_, o) => Some(Raw::Sz(o.to_string())),
    };
    write(key, name, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tweak_is_a_per_user_value() {
        for t in &TWEAKS {
            assert!(!t.key.starts_with("HKLM") && !t.key.is_empty());
            assert!(!t.label.is_empty() && !t.name.is_empty());
        }
        // reading never fails on a normal profile
        let _ = pending();
    }

    #[test]
    fn never_makes_a_faster_setting_slower() {
        let menu = &TWEAKS[3]; // MenuShowDelay = 200
        assert!(improves(menu, None), "Windows' default (400) is slower");
        assert!(improves(menu, Some("400")));
        assert!(!improves(menu, Some("0")), "0 is already faster than 200");
        assert!(!improves(menu, Some("200")));
        let delay = &TWEAKS[4]; // StartupDelayInMSec = 0
        assert!(improves(delay, Some("10000")) && !improves(delay, Some("0")));
        let end = &TWEAKS[0]; // AutoEndTasks = "1"
        assert!(improves(end, Some("0")) && !improves(end, Some("1")));
    }

    #[test]
    fn restore_refuses_values_that_are_not_ours() {
        let bad =
            "W\tHKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\tEvil\tsz\tnone\tx\tlabel";
        assert!(restore_line(bad).is_err());
        assert!(restore_line("W\tbroken").is_err());
    }

    /// Read-only look at this PC: `cargo test -p bosun-core real_care_extras -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_care_extras() {
        for i in pending() {
            println!(
                "pending tweak: {} (now {:?})",
                TWEAKS[i].label,
                current(&TWEAKS[i])
            );
        }
        println!("fixed drives: {:?}", crate::disks::fixed());
        let t = std::time::Instant::now();
        let u = crate::drivers::pending_updates();
        println!(
            "windows updates: {:?} in {:.1}s",
            u.map(|v| v.len()),
            t.elapsed().as_secs_f32()
        );
    }
}
