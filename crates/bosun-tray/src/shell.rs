//! Small OS integration helpers: launching the main app, dark menus, work areas.

use std::path::PathBuf;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCSTR, w};

/// `bosun.exe` next to the tray binary, if it has been installed.
pub fn main_app_path() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("bosun.exe");
    p.exists().then_some(p)
}

pub fn open_main_app(page: &str) {
    open_main_args(&format!("--page {page}"));
}

/// Starts (or, if open, re-targets) the main window with `args`.
pub fn open_main_args(args: &str) {
    let Some(p) = main_app_path() else { return };
    // SAFETY: plain ShellExecute with owned wide strings.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(p.as_os_str()),
            &HSTRING::from(args),
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Makes Win32 popup menus follow the dark theme. Uses uxtheme's
/// `SetPreferredAppMode` (ordinal 135), which every dark-mode Win32 app relies on;
/// silently does nothing if Windows ever removes it.
pub fn enable_dark_menus() {
    type SetMode = unsafe extern "system" fn(i32) -> i32;
    type Flush = unsafe extern "system" fn();
    // SAFETY: ordinals are resolved from uxtheme.dll and called with the documented
    // community signatures; failures are ignored.
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        if let Some(f) = GetProcAddress(lib, PCSTR(135 as *const u8)) {
            let set: SetMode = std::mem::transmute(f);
            set(2); // ForceDark
        }
        if let Some(f) = GetProcAddress(lib, PCSTR(136 as *const u8)) {
            let flush: Flush = std::mem::transmute(f);
            flush();
        }
    }
}

/// Work area (screen minus taskbar) of the monitor nearest `pt`.
pub fn work_area(pt: POINT) -> RECT {
    // SAFETY: MONITORINFO is sized correctly.
    unsafe {
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}
