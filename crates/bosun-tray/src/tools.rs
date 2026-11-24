//! What the monitor footer and tray menu buttons actually do: Turbo Boost
//! (the same journaled Turbo Boost as the main window, Recommended plan),
//! and screenshots via Windows' own Snipping Tool.

use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

pub fn turbo_on() -> bool {
    bosun_core::turbo::State::load().on
}

/// Toggles Turbo Boost. Returns the new state, or an error message.
pub fn toggle_turbo() -> Result<bool, &'static str> {
    let dir = bosun_core::clean::journal_dir().ok_or("No place to write the journal.")?;
    let state = bosun_core::turbo::State::load();
    if state.on {
        bosun_core::turbo::restore(&dir).map_err(|_| "Couldn't turn Turbo Boost off.")?;
        return Ok(false);
    }
    let plan = bosun_core::turbo::recommended(state.mode);
    bosun_core::turbo::boost(&plan, &dir).map_err(|_| "Couldn't turn Turbo Boost on.")?;
    Ok(true)
}

fn open(target: &str) {
    // SAFETY: plain ShellExecute with owned strings.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// Region screenshot with the Windows snipping overlay.
pub fn screenshot() {
    open("ms-screenclip:");
}

pub fn snipping_tool() {
    open("snippingtool.exe");
}
