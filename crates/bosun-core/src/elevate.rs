//! Running one Windows tool with administrator rights from our unelevated
//! process: Windows shows its own prompt, we wait for the tool to finish.
//! Used by the restore point and Disk Optimization.

use std::io;

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR, w};

/// How it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Finished with this exit code.
    Exited(u32),
    /// `stop` asked us to stop waiting; the tool keeps running on its own.
    LeftRunning,
}

/// Runs `file args` elevated and hidden, waiting until it ends or `stop()`
/// returns true (checked twice a second). Errors when the prompt is declined.
pub fn run(file: &str, args: &str, stop: &dyn Fn() -> bool) -> io::Result<Outcome> {
    let (file, args) = (HSTRING::from(file), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` and its strings outlive the call; the process handle we
    // get back is closed on every path below.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| io::Error::other(format!("not started: {e}")))?;
        if info.hProcess.is_invalid() {
            return Err(io::Error::other("no process"));
        }
        let outcome = loop {
            match WaitForSingleObject(info.hProcess, 500) {
                WAIT_OBJECT_0 => {
                    let mut code = 1u32;
                    let _ = GetExitCodeProcess(info.hProcess, &mut code);
                    break Outcome::Exited(code);
                }
                WAIT_TIMEOUT if !stop() => continue,
                WAIT_TIMEOUT => break Outcome::LeftRunning,
                _ => {
                    let _ = CloseHandle(info.hProcess);
                    return Err(io::Error::last_os_error());
                }
            }
        };
        let _ = CloseHandle(info.hProcess);
        Ok(outcome)
    }
}
