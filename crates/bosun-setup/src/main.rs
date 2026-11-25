//! Bosun SystemCare setup: installs for the current user (no administrator
//! rights) to `%LOCALAPPDATA%\Programs\Bosun SystemCare`, with a Start menu
//! shortcut, an "Apps & features" entry and the tray at sign-in. The same
//! file, copied in as `uninstall.exe`, removes it all again with
//! `--uninstall`.
//!
//! Flags: `--quiet` (no questions; for scripted tests), `--dir <path>`
//! (install somewhere else), `--files-only` (copy files, touch nothing else).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO,
    MESSAGEBOX_STYLE, MessageBoxW, PostMessageW, WM_CLOSE,
};
use windows::core::{HSTRING, Interface, PCWSTR, w};

const APP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bosun.exe"));
const TRAY: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bosun-tray.exe"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = "Bosun SystemCare";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\BosunSystemCare";

struct Opts {
    quiet: bool,
    files_only: bool,
    dir: PathBuf,
}

fn default_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join(NAME))
}

fn ask(o: &Opts, text: &str) -> bool {
    o.quiet || message(text, MB_YESNO | MB_ICONQUESTION) == IDYES
}

fn message(
    text: &str,
    style: MESSAGEBOX_STYLE,
) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    let title = HSTRING::from(format!("{NAME} {VERSION} Setup"));
    // SAFETY: modal box with owned strings.
    unsafe { MessageBoxW(None, &HSTRING::from(text), &title, style) }
}

fn tell(o: &Opts, text: &str, style: MESSAGEBOX_STYLE) {
    if !o.quiet {
        message(text, style);
    }
}

/// Asks a running main window and tray to close (they're ours: closing is
/// the same as the user clicking Close / Exit).
fn close_running() {
    for class in [w!("BosunMain"), w!("BosunTrayHost")] {
        // SAFETY: finds our own windows by class and posts them WM_CLOSE.
        unsafe {
            if let Ok(h) = FindWindowW(class, PCWSTR::null()) {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Writes `bytes` to `path`, retrying while a closing program still holds it.
fn write_retry(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut last = None;
    for _ in 0..20 {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(last.unwrap_or_else(|| io::Error::other("couldn't write")))
}

fn reg_err(e: windows::Win32::Foundation::WIN32_ERROR) -> io::Result<()> {
    e.ok().map_err(|e| io::Error::from_raw_os_error(e.code().0))
}

/// HKCU uninstall entry ("Apps & features").
fn register(dir: &Path, size_kb: u32) -> io::Result<()> {
    let mut h = HKEY::default();
    // SAFETY: creates our own key under HKCU and writes plain values to it.
    unsafe {
        reg_err(RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(UNINSTALL_KEY),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            None,
            &mut h,
            None,
        ))?;
        let sz = |name: &str, v: &str| {
            let b: Vec<u8> = v
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect();
            RegSetValueExW(h, &HSTRING::from(name), None, REG_SZ, Some(&b))
        };
        let uninstall = format!("\"{}\" --uninstall", dir.join("uninstall.exe").display());
        let dir_s = dir.display().to_string();
        let icon = dir.join("bosun.exe").display().to_string();
        let r = [
            sz("DisplayName", NAME),
            sz("DisplayVersion", VERSION),
            sz("Publisher", "Bosun"),
            sz("InstallLocation", &dir_s),
            sz("DisplayIcon", &icon),
            sz("UninstallString", &uninstall),
        ];
        for name in ["NoModify", "NoRepair"] {
            let _ = RegSetValueExW(
                h,
                &HSTRING::from(name),
                None,
                REG_DWORD,
                Some(&1u32.to_le_bytes()),
            );
        }
        let _ = RegSetValueExW(
            h,
            &HSTRING::from("EstimatedSize"),
            None,
            REG_DWORD,
            Some(&size_kb.to_le_bytes()),
        );
        let _ = RegCloseKey(h);
        r.into_iter().try_for_each(reg_err)
    }
}

fn start_menu_link() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| {
        PathBuf::from(a)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(format!("{NAME}.lnk"))
    })
}

fn shortcut(target: &Path, link: &Path) -> windows::core::Result<()> {
    // SAFETY: COM on this thread; the shell link object is released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        sl.SetPath(&HSTRING::from(target.as_os_str()))?;
        sl.SetDescription(w!("Keep your PC clean, fast and protected"))?;
        sl.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        if let Some(d) = target.parent() {
            sl.SetWorkingDirectory(&HSTRING::from(d.as_os_str()))?;
        }
        sl.cast::<IPersistFile>()?
            .Save(&HSTRING::from(link.as_os_str()), true)
    }
}

fn spawn(exe: &Path) {
    let _ = std::process::Command::new(exe).spawn();
}

fn install(o: &Opts) -> Result<(), String> {
    if APP.is_empty() || TRAY.is_empty() {
        return Err(
            "This setup was built without the app inside. Build it with scripts\\release.ps1."
                .into(),
        );
    }
    let question = format!(
        "Install {NAME} {VERSION} for you?\n\nIt goes to {}\nand needs no administrator rights. The Bosun tray will start when you sign in; you can turn that off in Settings.",
        o.dir.display()
    );
    if !ask(o, &question) {
        return Ok(());
    }
    close_running();
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("Couldn't create the folder: {e}"))?;
    let (app, tray) = (o.dir.join("bosun.exe"), o.dir.join("bosun-tray.exe"));
    write_retry(&app, APP).map_err(|e| format!("Couldn't write bosun.exe: {e}"))?;
    write_retry(&tray, TRAY).map_err(|e| format!("Couldn't write bosun-tray.exe: {e}"))?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let uninstaller = o.dir.join("uninstall.exe");
    if me != uninstaller {
        std::fs::copy(&me, &uninstaller)
            .map_err(|e| format!("Couldn't copy the uninstaller: {e}"))?;
    }
    if o.files_only {
        return Ok(());
    }
    let size_kb = ((APP.len() + TRAY.len()) as u64 * 2 / 1024) as u32;
    register(&o.dir, size_kb).map_err(|e| format!("Couldn't add the uninstall entry: {e}"))?;
    if let Some(link) = start_menu_link() {
        shortcut(&app, &link)
            .map_err(|e| format!("Couldn't create the Start menu shortcut: {e}"))?;
    }
    let cmd = format!("\"{}\"", tray.display());
    bosun_core::startup::set_autostart(bosun_core::startup::AUTOSTART_NAME, Some(&cmd))
        .map_err(|e| format!("Couldn't add the tray to startup: {e}"))?;
    if !o.quiet {
        spawn(&tray);
        spawn(&app);
    }
    Ok(())
}

/// Deletes `dir` after this process exits (it is running from inside it).
fn remove_later(dir: &Path) {
    use std::os::windows::process::CommandExt;
    // only ever our own install folder
    if !dir.ends_with(NAME) {
        return;
    }
    let script = format!(
        "ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"",
        dir.display()
    );
    let _ = std::process::Command::new("cmd")
        .args(["/c", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
}

fn uninstall(o: &Opts) -> Result<(), String> {
    if !ask(o, &format!("Remove {NAME} from this PC?")) {
        return Ok(());
    }
    // the default is to keep them: they're small and hold the Rescue Center
    let wipe = !o.quiet
        && message(
            "Also delete your Bosun settings and the Rescue Center journal?\n\nChoose No to keep them (for a later reinstall).",
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES;
    close_running();
    std::thread::sleep(Duration::from_millis(800));
    let _ = bosun_core::startup::set_autostart(bosun_core::startup::AUTOSTART_NAME, None);
    if let Some(link) = start_menu_link() {
        let _ = std::fs::remove_file(link);
    }
    // SAFETY: deletes our own uninstall key.
    let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY)) };
    for f in ["bosun.exe", "bosun-tray.exe"] {
        let _ = std::fs::remove_file(o.dir.join(f));
    }
    if wipe {
        for var in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(var) {
                let _ = std::fs::remove_dir_all(PathBuf::from(d).join("Bosun"));
            }
        }
    }
    remove_later(&o.dir);
    tell(o, &format!("{NAME} was removed."), MB_ICONINFORMATION);
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .windows(2)
        .find(|w| w[0] == "--dir")
        .map(|w| PathBuf::from(&w[1]))
        .or_else(|| {
            // the uninstaller removes the folder it lives in
            args.iter().any(|a| a == "--uninstall").then(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
            })?
        })
        .or_else(default_dir);
    let Some(dir) = dir else {
        message("LOCALAPPDATA is not set.", MB_ICONERROR);
        std::process::exit(1);
    };
    let o = Opts {
        quiet: args.iter().any(|a| a == "--quiet"),
        files_only: args.iter().any(|a| a == "--files-only"),
        dir,
    };
    let r = if args.iter().any(|a| a == "--uninstall") {
        uninstall(&o)
    } else {
        install(&o)
    };
    if let Err(e) = r {
        tell(&o, &e, MB_ICONERROR);
        std::process::exit(1);
    }
}
