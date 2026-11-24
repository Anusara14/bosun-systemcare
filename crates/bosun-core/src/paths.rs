//! "Does this point at a file that is gone?", answered conservatively: only a
//! full path on a fixed local drive that exists counts. Relative names (found
//! via PATH), network paths and removable or unplugged drives never do.

use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::core::HSTRING;

const DRIVE_FIXED: u32 = 3;

/// Expands `%VAR%` references from the environment; unknown ones stay as-is.
pub fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The program a command line runs: the quoted part, or everything up to and
/// including the first ".exe". `None` when there is no recognisable program.
pub fn exe_from_command(cmd: &str) -> Option<PathBuf> {
    let cmd = expand(cmd.trim());
    let exe = if let Some(q) = cmd.strip_prefix('"') {
        q.split('"').next()?.to_string()
    } else {
        let end = cmd.to_ascii_lowercase().find(".exe")? + 4;
        cmd[..end].to_string()
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// `C:\...` style path whose drive is a fixed local disk.
fn on_fixed_drive(p: &Path) -> bool {
    let s = p.to_string_lossy();
    let b = s.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return false;
    }
    let root = HSTRING::from(&s[..3]);
    // SAFETY: plain query with a valid root path string.
    unsafe { GetDriveTypeW(&root) == DRIVE_FIXED }
}

/// True only when `p` is a full path on a fixed drive and nothing is there.
pub fn is_missing(p: &Path) -> bool {
    on_fixed_drive(p) && !p.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_is_taken_from_quotes_or_up_to_dot_exe() {
        assert_eq!(
            exe_from_command(r#""C:\Program Files\A b\app.exe" --min"#),
            Some(PathBuf::from(r"C:\Program Files\A b\app.exe"))
        );
        assert_eq!(
            exe_from_command(r"C:\Tools\x.EXE /quiet"),
            Some(PathBuf::from(r"C:\Tools\x.EXE"))
        );
        assert_eq!(exe_from_command("just words"), None);
    }

    #[test]
    fn env_vars_expand_and_unknown_ones_stay() {
        // SAFETY: test-only; no other thread reads this variable.
        unsafe { std::env::set_var("BOSUN_TEST_DIR", r"C:\T") };
        assert_eq!(expand(r"%BOSUN_TEST_DIR%\a.exe"), r"C:\T\a.exe");
        assert_eq!(expand(r"%NO_SUCH_VAR_BOSUN%\a"), r"%NO_SUCH_VAR_BOSUN%\a");
        assert_eq!(expand("100%"), "100%");
    }

    #[test]
    fn only_full_paths_on_fixed_drives_count_as_missing() {
        let sys = std::env::var("WINDIR").unwrap();
        assert!(!is_missing(&PathBuf::from(&sys)), "exists");
        assert!(is_missing(&PathBuf::from(format!(
            r"{sys}\no-such-bosun-file.exe"
        ))));
        assert!(
            !is_missing(Path::new("notepad.exe")),
            "relative: found via PATH"
        );
        assert!(!is_missing(Path::new(r"\\server\share\x.exe")), "network");
    }
}
