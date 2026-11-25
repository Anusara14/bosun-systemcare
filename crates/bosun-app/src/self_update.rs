//! "Check for updates" in the About dialog: the check and the download run
//! on a worker thread, the window polls for the result.

use std::sync::mpsc::{Receiver, channel};

use bosun_core::self_update::{self as su, Release};

#[derive(Clone, Debug, Default)]
enum State {
    #[default]
    Idle,
    Checking,
    Current,
    Available(Release),
    Downloading,
    /// The installer is running; it closes this window.
    Installing,
    Failed(String),
}

#[derive(Default)]
pub struct SelfUpdate {
    state: State,
    rx: Option<Receiver<State>>,
}

impl SelfUpdate {
    /// What the About dialog shows, and whether clicking it does something.
    pub fn line(&self) -> (String, bool) {
        match &self.state {
            State::Idle => ("Check for updates".into(), true),
            State::Checking => ("Checking for updates…".into(), false),
            State::Current => ("Bosun is up to date".into(), false),
            State::Available(r) => (format!("Update to version {}", r.label()), true),
            State::Downloading => ("Downloading the update…".into(), false),
            State::Installing => ("Installing the update…".into(), false),
            State::Failed(e) => (format!("{e}. Try again"), true),
        }
    }

    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    fn run(&mut self, now: State, work: impl FnOnce() -> State + Send + 'static) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.state = now;
        self.rx = Some(rx);
    }

    /// Opening About checks once, unless it already knows the answer.
    pub fn check_if_idle(&mut self) {
        if matches!(self.state, State::Idle) {
            self.check();
        }
    }

    fn check(&mut self) {
        self.run(State::Checking, || match su::latest() {
            Ok(r) if r.is_newer() => State::Available(r),
            Ok(_) => State::Current,
            Err(e) => State::Failed(short("Couldn't check", &e)),
        });
    }

    /// The link: check again, or download and install the newer version.
    pub fn click(&mut self) {
        match self.state.clone() {
            State::Available(r) => self.run(State::Downloading, move || {
                match su::download(&r).and_then(|p| su::install(&p, true)) {
                    Ok(()) => State::Installing,
                    Err(e) => State::Failed(short("Update failed", &e)),
                }
            }),
            State::Idle | State::Failed(_) => self.check(),
            _ => {}
        }
    }

    /// Takes the worker's result; true while it's still working.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        match rx.try_recv() {
            Ok(s) => {
                self.state = s;
                self.rx = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => {
                self.state = State::Failed("Update stopped".into());
                self.rx = None;
                false
            }
        }
    }
}

/// "Couldn't check (reason)", with curl's long messages cut down.
fn short(what: &str, e: &std::io::Error) -> String {
    let msg = e.to_string();
    let msg = msg
        .trim_start_matches("curl: ")
        .lines()
        .next()
        .unwrap_or("");
    let msg: String = msg.chars().take(48).collect();
    if msg.is_empty() {
        what.into()
    } else {
        format!("{what} ({msg})")
    }
}

/// `bosun.exe --check-update` (the tray runs it once a day). Exit codes:
/// 0 up to date or updating, 10 a newer version is out (its number is
/// printed), 1 the check failed.
pub fn headless() -> i32 {
    let r = match su::latest() {
        Ok(r) if r.is_newer() => r,
        Ok(_) => return 0,
        Err(_) => return 1,
    };
    if crate::tray_ini::get("auto_update", false) {
        // the installer closes the tray, updates and starts the tray again
        match su::download(&r).and_then(|p| su::install(&p, false)) {
            Ok(()) => 0,
            Err(_) => 1,
        }
    } else {
        print!("{}", r.label());
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_only_where_a_click_does_something() {
        let mut u = SelfUpdate::default();
        assert!(u.line().1);
        u.state = State::Checking;
        assert!(!u.line().1);
        u.state = State::Available(Release {
            version: (9, 0, 0),
            setup_url: String::new(),
            sha_url: String::new(),
        });
        assert_eq!(u.line(), ("Update to version 9.0.0".into(), true));
        let e = std::io::Error::other("curl: (6) Could not resolve host: api.github.com");
        assert_eq!(
            short("Couldn't check", &e),
            "Couldn't check ((6) Could not resolve host: api.github.com)"
        );
    }
}
