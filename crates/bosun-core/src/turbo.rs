//! Turbo Boost (spec §2.2): switch the power plan, stop chosen services and
//! close chosen background apps; Turn Off restores the plan and restarts the
//! services. Closing an app can lose unsaved work, so nothing is chosen for
//! the user: apps are closed only when ticked.
//!
//! State lives in `%APPDATA%\Bosun\turbo.ini` so Turn Off works after a
//! restart of the app; every action is journaled first.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerSetActiveScheme};
use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_START, SERVICE_STATUS, SERVICE_STOP,
    StartServiceW,
};
use windows::core::{GUID, HSTRING, PCWSTR};

/// Windows' "High performance" and "Power saver" plans.
pub const HIGH_PERFORMANCE: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
pub const POWER_SAVER: GUID = GUID::from_u128(0xa1841308_3541_4fab_bc81_f71556f20b4a);

/// Windows services that are safe to pause for a session (the reference's
/// System Services list): telemetry, offline maps, scanners, media sharing,
/// search indexing.
pub const SYSTEM_SERVICES: [&str; 5] = [
    "DiagTrack",
    "MapsBroker",
    "stisvc",
    "WMPNetworkSvc",
    "WSearch",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    #[default]
    Work,
    Game,
    Economy,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Work, Mode::Game, Mode::Economy];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Work => "Work Mode",
            Mode::Game => "Game Mode",
            Mode::Economy => "Economy Mode",
        }
    }

    /// The line under "Boost mode" (reference wording, per mode).
    pub fn blurb(self) -> &'static str {
        match self {
            Mode::Work => {
                "Boost your computer for a better work experience by terminating unnecessary services and applications."
            }
            Mode::Game => {
                "Boost your computer for smoother games by terminating unnecessary services and applications."
            }
            Mode::Economy => {
                "Save battery by terminating unnecessary services and applications and using the Power saver plan."
            }
        }
    }

    pub fn plan(self) -> GUID {
        if self == Mode::Economy {
            POWER_SAVER
        } else {
            HIGH_PERFORMANCE
        }
    }

    fn key(self) -> &'static str {
        match self {
            Mode::Work => "work",
            Mode::Game => "game",
            Mode::Economy => "economy",
        }
    }
}

/// What is on now, so Turn Off can undo it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub on: bool,
    pub mode: Mode,
    /// Plan active before boosting.
    pub prev_plan: Option<GUID>,
    /// Services we stopped (to start again).
    pub stopped: Vec<String>,
}

fn state_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Bosun\turbo.ini"))
}

impl State {
    pub fn parse(text: &str) -> Self {
        let mut s = State::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            match k.trim() {
                "on" => s.on = v.trim() == "1",
                "mode" => {
                    s.mode = Mode::ALL
                        .into_iter()
                        .find(|m| m.key() == v.trim())
                        .unwrap_or_default()
                }
                "prev_plan" => {
                    s.prev_plan = u128::from_str_radix(v.trim(), 16).ok().map(GUID::from_u128)
                }
                "stopped" => {
                    s.stopped = v
                        .split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(String::from)
                        .collect()
                }
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("on={}\nmode={}\n", u8::from(self.on), self.mode.key());
        if let Some(g) = self.prev_plan {
            out += &format!("prev_plan={:032x}\n", g.to_u128());
        }
        out += &format!("stopped={}\n", self.stopped.join(","));
        out
    }

    pub fn load() -> Self {
        state_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map_or_else(State::default, |t| State::parse(&t))
    }

    pub fn save(&self) -> io::Result<()> {
        let p = state_path().ok_or_else(|| io::Error::other("APPDATA is unset"))?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, self.serialize())
    }
}

pub fn active_plan() -> Option<GUID> {
    let mut p: *mut GUID = std::ptr::null_mut();
    // SAFETY: the system allocates the GUID; we copy it and LocalFree it.
    unsafe {
        if PowerGetActiveScheme(None, &mut p) != ERROR_SUCCESS || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = LocalFree(Some(HLOCAL(p.cast())));
        Some(g)
    }
}

fn set_plan(g: &GUID) -> bool {
    // SAFETY: activates an existing power scheme for this user.
    unsafe { PowerSetActiveScheme(None, Some(g)) == ERROR_SUCCESS }
}

struct H(SC_HANDLE);
impl Drop for H {
    fn drop(&mut self) {
        // SAFETY: closing a handle we opened.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn service(name: &str, access: u32) -> windows::core::Result<H> {
    // SAFETY: opens the service manager and one service by name.
    unsafe {
        let scm = H(OpenSCManagerW(
            PCWSTR::null(),
            PCWSTR::null(),
            SC_MANAGER_CONNECT,
        )?);
        Ok(H(OpenServiceW(scm.0, &HSTRING::from(name), access)?))
    }
}

/// Stops a service (needs admin).
pub fn stop_service(name: &str) -> windows::core::Result<()> {
    let s = service(name, SERVICE_STOP)?;
    let mut st = SERVICE_STATUS::default();
    // SAFETY: sends the stop control to a service we opened.
    unsafe { ControlService(s.0, SERVICE_CONTROL_STOP, &mut st) }
}

/// Starts a service (needs admin).
pub fn start_service(name: &str) -> windows::core::Result<()> {
    let s = service(name, SERVICE_START)?;
    // SAFETY: starts a service we opened, with no arguments.
    unsafe { StartServiceW(s.0, None) }
}

/// What the user picked in the Turbo Boost window.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub mode: Mode,
    pub services: Vec<String>,
    /// Background apps to close: (name, pids).
    pub apps: Vec<(String, Vec<u32>)>,
    /// Switch the power plan for this mode.
    pub power: bool,
}

/// The plan Turbo Boost's "Recommended" picks: Windows' optional system
/// services that are present, except search indexing; no other services or
/// apps; the mode's power plan. Shared by the main window and the tray.
pub fn recommended(mode: Mode) -> Plan {
    let all = crate::services::list();
    Plan {
        mode,
        services: SYSTEM_SERVICES
            .iter()
            .filter(|n| !n.eq_ignore_ascii_case("WSearch"))
            .filter_map(|n| all.iter().find(|s| s.name.eq_ignore_ascii_case(n)))
            .map(|s| s.name.clone())
            .collect(),
        apps: Vec::new(),
        power: true,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    pub services_done: usize,
    pub services_failed: usize,
    pub apps_closed: usize,
    pub apps_failed: usize,
    pub power: bool,
}

/// Turns Turbo Boost on, journaling what it is about to do first.
pub fn boost(plan: &Plan, dir: &Path) -> io::Result<Report> {
    let (_, mut f) = crate::clean::journal_file(dir, "Turbo Boost")?;
    let prev = active_plan();
    writeln!(f, "# bosun journal v1\tturbo\ton\t{}", plan.mode.label())?;
    if let Some(g) = prev {
        writeln!(f, "P\t{:032x}", g.to_u128())?;
    }
    for s in &plan.services {
        writeln!(f, "S\t{s}")?;
    }
    for (name, pids) in &plan.apps {
        writeln!(f, "A\t{name}\t{pids:?}")?;
    }
    f.sync_all()?;
    let mut r = Report::default();
    let mut state = State {
        on: true,
        mode: plan.mode,
        prev_plan: prev,
        stopped: Vec::new(),
    };
    if plan.power && Some(plan.mode.plan()) != prev {
        r.power = set_plan(&plan.mode.plan());
    }
    if !r.power {
        state.prev_plan = None;
    }
    for s in &plan.services {
        match stop_service(s) {
            Ok(()) => {
                r.services_done += 1;
                state.stopped.push(s.clone());
            }
            Err(_) => r.services_failed += 1,
        }
    }
    for (_, pids) in &plan.apps {
        let failed = crate::procs::end_task(pids);
        if failed < pids.len() {
            r.apps_closed += 1;
        } else {
            r.apps_failed += 1;
        }
    }
    state.save()?;
    Ok(r)
}

/// Turns Turbo Boost off: the previous power plan and stopped services come back.
pub fn restore(dir: &Path) -> io::Result<Report> {
    let state = State::load();
    let (_, mut f) = crate::clean::journal_file(dir, "Turbo Boost")?;
    writeln!(f, "# bosun journal v1\tturbo\toff\t{}", state.mode.label())?;
    f.sync_all()?;
    let mut r = Report::default();
    if let Some(g) = state.prev_plan {
        r.power = set_plan(&g);
    }
    for s in &state.stopped {
        if start_service(s).is_ok() {
            r.services_done += 1;
        } else {
            r.services_failed += 1;
        }
    }
    State {
        mode: state.mode,
        ..State::default()
    }
    .save()?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips() {
        let s = State {
            on: true,
            mode: Mode::Game,
            prev_plan: Some(HIGH_PERFORMANCE),
            stopped: vec!["WSearch".into(), "MapsBroker".into()],
        };
        assert_eq!(State::parse(&s.serialize()), s);
        assert_eq!(State::parse(""), State::default());
    }

    #[test]
    fn modes_pick_their_power_plan() {
        assert_eq!(Mode::Work.plan(), HIGH_PERFORMANCE);
        assert_eq!(Mode::Economy.plan(), POWER_SAVER);
        assert!(active_plan().is_some(), "every Windows has an active plan");
    }
}
