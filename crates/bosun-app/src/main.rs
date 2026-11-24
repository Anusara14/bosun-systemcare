//! Bosun SystemCare main window (`bosun.exe`). Runs on demand and exits when
//! closed; the always-on tray process lives in `bosun-tray`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backdrop;
mod chrome;
mod frame;
mod icons;
mod pages;
mod scan;
mod tray_ini;
mod window;

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetMessageW, IsIconic, MB_ICONERROR, MSG, MessageBoxW,
    SW_RESTORE, SetForegroundWindow, ShowWindow, TranslateMessage,
};
use windows::core::{HSTRING, Result, w};

use crate::pages::Page;

/// `--page speedup`, `--scan`, `--settings` (the tray opens the app this way).
fn request() -> window::Request {
    let args: Vec<String> = std::env::args().collect();
    window::Request {
        page: args
            .windows(2)
            .find(|w| w[0] == "--page")
            .and_then(|w| Page::from_arg(&w[1])),
        scan: args.iter().any(|a| a == "--scan"),
        settings: args.iter().any(|a| a == "--settings"),
    }
}

/// Brings an already-running main window to the front instead of opening a
/// second one, and hands it this launch's request.
fn activate_existing(r: window::Request) -> bool {
    // SAFETY: plain window lookup/activation by our own class name.
    unsafe {
        let Ok(h) = FindWindowW(window::CLASS, None) else {
            return false;
        };
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
            Some(h),
            window::WM_REQUEST,
            windows::Win32::Foundation::WPARAM(r.encode()),
            windows::Win32::Foundation::LPARAM(0),
        );
        true
    }
}

fn run() -> Result<()> {
    // SAFETY: standard Win32 start-up sequence on the UI thread.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\Bosun.SystemCare.App"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS && activate_existing(request()) {
            return Ok(());
        }
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        bosun_ui::init()?;
        window::create(request())?;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let text = HSTRING::from(format!("Bosun SystemCare could not start.\n\n{e}"));
        // SAFETY: modal error box with owned strings.
        unsafe { MessageBoxW(None, &text, w!("Bosun SystemCare"), MB_ICONERROR) };
    }
}
