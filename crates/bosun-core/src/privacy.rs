//! Privacy Protection (Protect page): Windows' own per-user privacy switches,
//! the same ones Settings → Privacy shows. Each is one HKCU DWORD, so turning
//! protection on or off is reversible; the old value is journaled first.

use std::io::{self, Write};
use std::path::Path;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, REG_DWORD,
    REG_OPTION_NON_VOLATILE, RegCreateKeyExW, RegSetValueExW,
};
use windows::core::HSTRING;

use crate::registry::{Key, dword, open_in};

/// (label, key under HKCU, value, value when protected, value when not)
pub const SWITCHES: [(&str, &str, &str, u32, u32); 4] = [
    (
        "Advertising ID",
        r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
        "Enabled",
        0,
        1,
    ),
    (
        "Tailored Experiences",
        r"Software\Microsoft\Windows\CurrentVersion\Privacy",
        "TailoredExperiencesWithDiagnosticDataEnabled",
        0,
        1,
    ),
    (
        "Website Language Access",
        r"Control Panel\International\User Profile",
        "HttpAcceptLanguageOptOut",
        1,
        0,
    ),
    (
        "App Launch Tracking",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
        "Start_TrackProgs",
        0,
        1,
    ),
];

/// Whether switch `i` is in its protected state (a missing value means
/// Windows' default, which is unprotected for all four).
pub fn protected(i: usize) -> bool {
    let (_, key, name, on, _) = SWITCHES[i];
    open_in(HKEY_CURRENT_USER, key, false)
        .and_then(|k| dword(&k, None, name))
        .is_some_and(|v| v == on)
}

fn set_dword(root: HKEY, key: &str, name: &str, v: u32) -> io::Result<()> {
    let mut h = HKEY::default();
    // SAFETY: creates/opens our target key and writes one DWORD.
    unsafe {
        RegCreateKeyExW(
            root,
            &HSTRING::from(key),
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
            &HSTRING::from(name),
            None,
            REG_DWORD,
            Some(&v.to_le_bytes()),
        )
        .ok()
        .map_err(|e| io::Error::from_raw_os_error(e.code().0))
    }
}

/// Turns protection for switch `i` on or off, journaling the old value.
pub fn set(i: usize, protect: bool, dir: &Path) -> io::Result<()> {
    let (label, key, name, on, off) = SWITCHES[i];
    let old = open_in(HKEY_CURRENT_USER, key, false).and_then(|k| dword(&k, None, name));
    let (_, mut f) = crate::clean::journal_file(dir, "Privacy")?;
    writeln!(
        f,
        "D\tHKCU\\{key}\t{name}\t{}\t{}\t{label}",
        old.map_or_else(|| "none".into(), |v| v.to_string()),
        if protect { on } else { off }
    )?;
    f.sync_all()?;
    set_dword(HKEY_CURRENT_USER, key, name, if protect { on } else { off })
}

/// User Account Control is on (`EnableLUA`, read-only for us).
pub fn uac_on() -> bool {
    open_in(
        HKEY_LOCAL_MACHINE,
        r"Software\Microsoft\Windows\CurrentVersion\Policies\System",
        false,
    )
    .and_then(|k| dword(&k, None, "EnableLUA"))
    .is_none_or(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_switch_and_uac() {
        for i in 0..SWITCHES.len() {
            let _ = protected(i); // must not panic on a fresh profile
        }
        assert!(uac_on(), "UAC is on on a normal desktop");
    }

    #[test]
    fn set_dword_round_trips_in_a_test_key() {
        set_dword(HKEY_CURRENT_USER, r"Software\BosunTestPrivacy", "X", 7).unwrap();
        let k = open_in(HKEY_CURRENT_USER, r"Software\BosunTestPrivacy", false).unwrap();
        assert_eq!(dword(&k, None, "X"), Some(7));
        drop(k);
        use windows::Win32::System::Registry::RegDeleteTreeW;
        unsafe {
            let _ = RegDeleteTreeW(
                HKEY_CURRENT_USER,
                &HSTRING::from(r"Software\BosunTestPrivacy"),
            );
        }
    }
}
