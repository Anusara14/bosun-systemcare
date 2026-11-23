//! What the monitor footer buttons actually do: Turbo Boost (power plan),
//! and screenshots via Windows' own Snipping Tool.

use std::cell::Cell;

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerSetActiveScheme};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{GUID, HSTRING, w};

/// Windows' built-in "High performance" plan.
const HIGH_PERFORMANCE: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);

thread_local! {
    /// Plan to return to when Turbo Boost is switched off.
    static PREVIOUS_PLAN: Cell<Option<GUID>> = const { Cell::new(None) };
}

fn active_plan() -> Option<GUID> {
    let mut p: *mut GUID = std::ptr::null_mut();
    // SAFETY: Windows allocates the GUID; we copy it out and LocalFree it.
    unsafe {
        if PowerGetActiveScheme(None, &mut p).is_err() || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = LocalFree(Some(HLOCAL(p.cast())));
        Some(g)
    }
}

pub fn turbo_on() -> bool {
    active_plan() == Some(HIGH_PERFORMANCE)
}

/// Toggles Turbo Boost. Returns the new state, or an error message.
pub fn toggle_turbo() -> Result<bool, &'static str> {
    if turbo_on() {
        let back = PREVIOUS_PLAN.with(Cell::take).unwrap_or(BALANCED);
        // SAFETY: switching to a power scheme GUID.
        unsafe { PowerSetActiveScheme(None, Some(&back)) }
            .ok()
            .map_err(|_| "Couldn't restore the previous power plan")?;
        return Ok(false);
    }
    let prev = active_plan();
    // SAFETY: switching to a power scheme GUID.
    unsafe { PowerSetActiveScheme(None, Some(&HIGH_PERFORMANCE)) }
        .ok()
        .map_err(|_| "This PC has no High performance power plan")?;
    PREVIOUS_PLAN.with(|c| c.set(prev));
    Ok(true)
}

/// Windows' "Balanced" plan, the fallback when we don't know what to restore.
const BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_guids_are_the_windows_defaults() {
        assert_eq!(
            format!("{HIGH_PERFORMANCE:?}").to_lowercase(),
            "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"
        );
        assert_eq!(
            format!("{BALANCED:?}").to_lowercase(),
            "381b4222-f694-41f0-9685-ff5bb260df2e"
        );
    }
}
