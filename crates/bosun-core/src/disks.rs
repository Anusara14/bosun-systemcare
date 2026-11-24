//! Disk Optimization (spec §2.1): Windows' own `defrag /O` on every fixed
//! drive (retrim on SSDs, defragment on hard drives). It needs administrator
//! rights, so it runs through Windows' prompt. Nothing to undo; the run is
//! still journaled so the Rescue Center lists it.

use std::io::{self, Write};
use std::path::Path;

use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDriveStringsW};
use windows::core::PCWSTR;

/// `DRIVE_FIXED` from `GetDriveTypeW`.
const DRIVE_FIXED: u32 = 3;

/// Fixed drives, like `C:\`.
pub fn fixed() -> Vec<String> {
    let mut buf = [0u16; 512];
    // SAFETY: fills our buffer with NUL-separated root paths.
    let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
    buf[..n.min(buf.len())]
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
        .filter(|root| {
            let z: Vec<u16> = root.iter().copied().chain([0]).collect();
            // SAFETY: a NUL-terminated root path.
            unsafe { GetDriveTypeW(PCWSTR(z.as_ptr())) == DRIVE_FIXED }
        })
        .map(String::from_utf16_lossy)
        .collect()
}

/// What happened when optimizing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Optimized {
    Done,
    /// Still running when we stopped waiting; Windows finishes it.
    StillRunning,
}

/// Optimizes every fixed drive, journal first. `stop()` ends the wait (the
/// optimization itself carries on). Errors when the prompt is declined or
/// `defrag` fails.
pub fn optimize(dir: &Path, dry_run: bool, stop: &dyn Fn() -> bool) -> io::Result<Optimized> {
    let drives = fixed();
    let (_, mut f) = crate::clean::journal_file(dir, "Disk Optimization")?;
    writeln!(
        f,
        "# bosun journal v1\toptimize\tDisk Optimization\tdry_run={dry_run}"
    )?;
    writeln!(f, "O\t{}", drives.join(" "))?;
    f.sync_all()?;
    if dry_run {
        writeln!(f, "E\toptimized={}", drives.len())?;
        return Ok(Optimized::Done);
    }
    // /C every volume, /O the right operation per media type, /H normal priority
    match crate::elevate::run("defrag.exe", "/C /O /H", stop)? {
        crate::elevate::Outcome::Exited(0) => {
            writeln!(f, "E\toptimized={}", drives.len())?;
            Ok(Optimized::Done)
        }
        crate::elevate::Outcome::Exited(code) => {
            writeln!(f, "F\tdefrag exited with {code}")?;
            Err(io::Error::other(format!("defrag exited with {code}")))
        }
        crate::elevate::Outcome::LeftRunning => {
            writeln!(f, "E\toptimized={}\tstill_running=true", drives.len())?;
            Ok(Optimized::StillRunning)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_system_drive() {
        let v = fixed();
        assert!(v.iter().any(|d| d.eq_ignore_ascii_case("C:\\")), "{v:?}");
    }
}
