//! Shortcut Fix: `.lnk` files on the Desktop and in the Start menu whose
//! target is gone (spec §2.1). Fixing moves them to the Recycle Bin (§4.2),
//! journaled first, so every one can be put back.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FO_DELETE, FOF_ALLOWUNDO, FOF_NO_UI, FOLDERID_CommonPrograms, FOLDERID_Desktop,
    FOLDERID_Programs, FOLDERID_PublicDesktop, IShellLinkW, KF_FLAG_DEFAULT, SHFILEOPSTRUCTW,
    SHFileOperationW, SHGetKnownFolderPath, SLGP_RAWPATH, ShellLink,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR};

use crate::paths;

#[derive(Clone, Debug, PartialEq)]
pub struct Broken {
    pub lnk: PathBuf,
    pub target: PathBuf,
}

fn known(id: &GUID) -> Option<PathBuf> {
    // SAFETY: the returned buffer is copied, then freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

/// (folder, walk subfolders): the user's and the shared Desktop and Start menu.
fn folders() -> Vec<(PathBuf, bool)> {
    [
        (FOLDERID_Desktop, false),
        (FOLDERID_PublicDesktop, false),
        (FOLDERID_Programs, true),
        (FOLDERID_CommonPrograms, true),
    ]
    .iter()
    .filter_map(|(id, deep)| known(id).map(|p| (p, *deep)))
    .collect()
}

/// The path a shortcut points at, or `None` for shell-item shortcuts
/// (Control Panel, Store apps) that have no file target.
pub fn target(lnk: &Path) -> Option<PathBuf> {
    // SAFETY: COM calls on objects we create; the buffer is ours.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>()
            .ok()?
            .Load(&HSTRING::from(lnk.as_os_str()), STGM_READ)
            .ok()?;
        let mut buf = [0u16; 1024];
        let mut fd = WIN32_FIND_DATAW::default();
        link.GetPath(&mut buf, &mut fd, SLGP_RAWPATH.0 as u32)
            .ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let raw = String::from_utf16_lossy(&buf[..n]);
        (!raw.is_empty()).then(|| PathBuf::from(paths::expand(&raw)))
    }
}

fn lnks(dir: &Path, deep: bool, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() && !t.is_symlink() && deep => lnks(&p, deep, out),
            Ok(t)
                if t.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) =>
            {
                out.push(p)
            }
            _ => {}
        }
    }
}

/// Every shortcut whose target is provably gone. `visit` sees each shortcut
/// checked; returning `false` stops.
pub fn find(visit: &mut dyn FnMut(&Path) -> bool) -> Vec<Broken> {
    let mut all = Vec::new();
    for (dir, deep) in folders() {
        lnks(&dir, deep, &mut all);
    }
    let mut out = Vec::new();
    for lnk in all {
        if !visit(&lnk) {
            break;
        }
        if let Some(t) = target(&lnk).filter(|t| paths::is_missing(t)) {
            out.push(Broken { lnk, target: t });
        }
    }
    out
}

/// Moves one file to the Recycle Bin, silently.
pub fn recycle(p: &Path) -> io::Result<()> {
    // double-NUL-terminated list of one path
    let from: Vec<u16> = p.as_os_str().encode_wide_nul2();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NO_UI.0) as u16,
        ..Default::default()
    };
    // SAFETY: `from` outlives the call; no window, no UI.
    let r = unsafe { SHFileOperationW(&mut op) };
    if r == 0 && !op.fAnyOperationsAborted.as_bool() {
        Ok(())
    } else {
        Err(io::Error::other(format!("recycle failed (code {r:#x})")))
    }
}

trait Wide2 {
    fn encode_wide_nul2(&self) -> Vec<u16>;
}

impl Wide2 for std::ffi::OsStr {
    fn encode_wide_nul2(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0, 0]).collect()
    }
}

/// Journals then recycles each broken shortcut: `R\t<shortcut>\t<target>`
/// lines first (flushed), then `F` lines for any that stay.
/// Returns (recycled, left).
pub fn fix(
    broken: &[Broken],
    dir: &Path,
    dry_run: bool,
    progress: &mut dyn FnMut(&Path, f32) -> bool,
) -> io::Result<(u64, u64)> {
    let (_, mut f) = crate::clean::journal_file(dir, "Shortcut Fix")?;
    writeln!(
        f,
        "# bosun journal v1\trecycle\tShortcut Fix\tdry_run={dry_run}"
    )?;
    for b in broken {
        writeln!(f, "R\t{}\t{}", b.lnk.display(), b.target.display())?;
    }
    f.sync_all()?;
    let (mut done, mut left) = (0u64, 0u64);
    for (i, b) in broken.iter().enumerate() {
        if !progress(&b.lnk, (i + 1) as f32 / broken.len().max(1) as f32) {
            left += (broken.len() - i) as u64;
            break;
        }
        let r = if dry_run { Ok(()) } else { recycle(&b.lnk) };
        match r {
            Ok(()) => done += 1,
            Err(e) => {
                left += 1;
                writeln!(f, "F\t{}\t{e}", b.lnk.display())?;
            }
        }
    }
    writeln!(f, "E\trecycled={done}\tleft={left}")?;
    Ok((done, left))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a .lnk pointing at `to` through IShellLink + IPersistFile::Save.
    fn make_lnk(at: &Path, to: &Path) {
        use windows::Win32::System::Com::IPersistFile;
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(to.as_os_str())).unwrap();
            link.cast::<IPersistFile>()
                .unwrap()
                .Save(&HSTRING::from(at.as_os_str()), true)
                .unwrap();
        }
    }

    #[test]
    fn reads_targets_and_spots_missing_ones() {
        let dir = std::env::temp_dir().join(format!("bosun-lnk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.txt");
        fs::write(&real, b"x").unwrap();
        let gone = dir.join("gone.exe");
        make_lnk(&dir.join("ok.lnk"), &real);
        make_lnk(&dir.join("bad.lnk"), &gone);
        assert_eq!(target(&dir.join("ok.lnk")), Some(real.clone()));
        let bad = target(&dir.join("bad.lnk")).unwrap();
        assert!(paths::is_missing(&bad) && !paths::is_missing(&real));

        let broken = [Broken {
            lnk: dir.join("bad.lnk"),
            target: bad,
        }];
        let (n, left) = fix(&broken, &dir.join("j"), false, &mut |_, _| true).unwrap();
        assert_eq!((n, left), (1, 0));
        assert!(!dir.join("bad.lnk").exists() && dir.join("ok.lnk").exists());
        // ponytail: the test's shortcut now sits in the user's Recycle Bin
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Read-only: `cargo test -p bosun-core real_shortcuts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_shortcuts() {
        let mut n = 0;
        for b in find(&mut |_| {
            n += 1;
            true
        }) {
            println!("{} -> {}", b.lnk.display(), b.target.display());
        }
        println!("checked {n}");
        println!(
            "antivirus {:?} firewall {:?}",
            crate::security::healthy(crate::security::Provider::Antivirus),
            crate::security::healthy(crate::security::Provider::Firewall)
        );
    }
}
