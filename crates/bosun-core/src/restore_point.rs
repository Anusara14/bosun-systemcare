//! System Restore point before a Fix (spec §2.6, optional). Creating one
//! needs administrator rights and Bosun runs unelevated, so this asks once
//! through Windows' own prompt and runs PowerShell's `Checkpoint-Computer`.
//! Windows itself allows one new point per 24 hours by default; a request
//! inside that window succeeds without making a new one.

use std::io;

/// Longest we wait for Windows to finish the point.
const WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

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
    let began = std::time::Instant::now();
    match crate::elevate::run("powershell.exe", &command(description), &|| {
        began.elapsed() > WAIT
    })? {
        crate::elevate::Outcome::Exited(0) => Ok(()),
        crate::elevate::Outcome::Exited(code) => {
            Err(io::Error::other(format!("PowerShell exited with {code}")))
        }
        crate::elevate::Outcome::LeftRunning => {
            Err(io::Error::new(io::ErrorKind::TimedOut, "still running"))
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
