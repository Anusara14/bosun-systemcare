//! System Restore point before a Fix (spec §2.6, optional). Creating one
//! needs administrator rights and Bosun runs unelevated, so this asks once
//! through Windows' own prompt and runs PowerShell's `Checkpoint-Computer`.
//! Windows itself allows one new point per 24 hours by default; a request
//! inside that window succeeds without making a new one.

use std::io;

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, w};

/// Longest we wait for Windows to finish the point.
const WAIT_MS: u32 = 5 * 60 * 1000;

/// The PowerShell arguments for a point named `description`, reduced to
/// letters, digits, spaces and `:-` so it can't break out of the quotes.
pub fn command(description: &str) -> String {
    let safe: String = description
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " :-".contains(*c))
        .take(64)
        .collect();
    format!(
        "-NoProfile -NonInteractive -WindowStyle Hidden -Command \
         \"Checkpoint-Computer -Description '{safe}' -RestorePointType MODIFY_SETTINGS\""
    )
}

/// Creates a restore point, waiting for it. Errors when the prompt is
/// declined, System Protection is off, or PowerShell reports a failure.
pub fn create(description: &str) -> io::Result<()> {
    let args = HSTRING::from(command(description));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: w!("powershell.exe"),
        lpParameters: windows::core::PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` and the strings it points at outlive the call; the
    // process handle we get back is closed below.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| io::Error::other(format!("not started: {e}")))?;
        if info.hProcess.is_invalid() {
            return Err(io::Error::other("no process"));
        }
        let waited = WaitForSingleObject(info.hProcess, WAIT_MS);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if waited != WAIT_OBJECT_0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "still running"));
        }
        if code == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!("PowerShell exited with {code}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_cannot_break_out_of_the_command() {
        let c = command("Bosun: before Fix'; Remove-Item C:\\ -Recurse; '");
        assert!(c.contains("-Description 'Bosun: before Fix Remove-Item C: -Recurse '"));
        assert!(!c.contains(';'));
        assert_eq!(c.matches('\'').count(), 2);
        assert_eq!(c.matches('"').count(), 2);
    }
}
