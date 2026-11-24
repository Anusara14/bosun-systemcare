//! Bosun SystemCare tray process: tray icon, floating widget and Performance Monitor.
//! Runs unelevated and stays small; the main window is a separate, on-demand process.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod monitor;
mod popup;
mod settings;
mod shell;
mod tools;
mod tray;
mod widget;
mod wmenu;

use std::cell::{Cell, RefCell};
use std::time::Instant;

use bosun_core::metrics::{Adapter, Vitals, VitalsSampler, per_second, read_adapters};
use bosun_core::procs::{Group, ProcessTable};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW,
    IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_ICONWARNING, MB_YESNO, MSG, MessageBoxW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetTimer, TranslateMessage,
    WINDOW_EX_STYLE, WM_APP, WM_COMMAND, WM_DESTROY, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_POPUP,
};
use windows::core::{HSTRING, Result, w};

use crate::settings::Settings;
pub use bosun_ui::{gfx, graphics, layered};

pub const WM_TRAY: u32 = WM_APP + 1;
const WM_OTHER_INSTANCE: u32 = WM_APP + 2;
const WM_CMD: u32 = WM_APP + 3;
/// Background RAM clean finished; lParam = bytes freed.
const WM_CLEANED: u32 = WM_APP + 4;
/// Auto RAM Clean: trigger at this load, at most this often.
const AUTO_CLEAN_PCT: u32 = 85;
const AUTO_CLEAN_GAP: std::time::Duration = std::time::Duration::from_secs(600);
const TICK: usize = 1;

/// Everything that changes windows is posted as a command and run from the host
/// window procedure, so no window state is borrowed while Windows re-enters us.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(usize)]
pub enum Cmd {
    ToggleMonitor = 1,
    ShowMonitor,
    HideMonitor,
    OpenMain,
    ToggleWidget,
    Menu,
    Exit,
    Refresh,
    CleanRam,
    /// Monitor's close button: hide the monitor and the floating widget.
    Dismiss,
    /// The main window changed `tray.ini`: read it again and apply it.
    /// Fixed at 11: the app posts this number.
    Reload = 11,
    /// Tray menu: start a Care scan in the main window.
    Scan = 12,
    /// Tray menu: Turbo Boost on or off (the journaled one).
    Turbo = 13,
    /// Tray menu: the main window's Settings dialog.
    OpenSettings = 14,
}

impl Cmd {
    fn from_usize(v: usize) -> Option<Self> {
        use Cmd::*;
        [
            ToggleMonitor,
            ShowMonitor,
            HideMonitor,
            OpenMain,
            ToggleWidget,
            Menu,
            Exit,
            Refresh,
            CleanRam,
            Reload,
            Scan,
            Turbo,
            OpenSettings,
        ]
        .into_iter()
        .find(|c| *c as usize == v)
    }
}

pub struct AdapterRate {
    pub name: String,
    pub rx_bps: u64,
    pub tx_bps: u64,
    pub link_bps: u64,
}

#[derive(Default)]
pub struct Data {
    pub vitals: Vitals,
    pub groups: Vec<Group>,
    pub adapters: Vec<AdapterRate>,
}

pub struct State {
    sampler: VitalsSampler,
    table: ProcessTable,
    prev_adapters: Vec<Adapter>,
    prev_at: Instant,
    pub data: Data,
    pub settings: Settings,
    last_clean: Option<Instant>,
}

impl State {
    fn tick(&mut self, full: bool) {
        self.data.vitals = self.sampler.tick(full);
        if full {
            self.data.groups = self.table.refresh();
            self.sample_adapters();
        } else if !self.data.groups.is_empty() {
            // Monitor closed: give the memory back.
            self.data = Data {
                vitals: self.data.vitals,
                ..Data::default()
            };
            self.prev_adapters = Vec::new();
            self.table.release();
        }
    }

    fn sample_adapters(&mut self) {
        let now = Instant::now();
        let secs = now.duration_since(self.prev_at).as_secs_f64();
        let cur = read_adapters();
        self.data.adapters = cur
            .iter()
            .map(|a| {
                let prev = self.prev_adapters.iter().find(|p| p.name == a.name);
                AdapterRate {
                    name: a.name.clone(),
                    rx_bps: prev.map_or(0, |p| per_second(p.rx, a.rx, secs)),
                    tx_bps: prev.map_or(0, |p| per_second(p.tx, a.tx, secs)),
                    link_bps: a.link_bps,
                }
            })
            .collect();
        self.prev_adapters = cur;
        self.prev_at = now;
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static HOST: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    static HINST: Cell<HINSTANCE> = const { Cell::new(HINSTANCE(std::ptr::null_mut())) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Runs `f` on the shared state unless it is already borrowed (re-entrancy).
pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Read-only view of the latest data (used while painting).
pub fn with_data<R>(f: impl FnOnce(&Data) -> R) -> Option<R> {
    STATE.with(|s| {
        s.try_borrow()
            .ok()
            .and_then(|g| g.as_ref().map(|st| f(&st.data)))
    })
}

pub fn hinstance() -> HINSTANCE {
    HINST.with(Cell::get)
}

pub fn host() -> HWND {
    HOST.with(Cell::get)
}

pub fn post(cmd: Cmd) {
    // SAFETY: posting to our own host window.
    unsafe {
        let _ = PostMessageW(Some(host()), WM_CMD, WPARAM(cmd as usize), LPARAM(0));
    }
}

fn set_rate(monitor_open: bool) {
    // SAFETY: host window is alive for the whole message loop.
    unsafe {
        SetTimer(
            Some(host()),
            TICK,
            if monitor_open { 1000 } else { 2000 },
            None,
        )
    };
}

fn tick() {
    let open = monitor::is_open();
    let auto = with_state(|s| {
        s.tick(open);
        let due = s.last_clean.is_none_or(|t| t.elapsed() >= AUTO_CLEAN_GAP);
        s.settings.auto_ram_clean && due && s.data.vitals.mem.load_pct >= AUTO_CLEAN_PCT
    });
    if auto == Some(true) {
        post(Cmd::CleanRam);
    }
    widget::render();
    if open {
        monitor::invalidate();
    }
    tray::update_tip();
}

fn exec(cmd: Cmd) {
    match cmd {
        Cmd::ToggleMonitor if monitor::is_open() => exec(Cmd::HideMonitor),
        Cmd::ToggleMonitor | Cmd::ShowMonitor => {
            // The widget expands into the monitor: the monitor opens in its place.
            if monitor::show() {
                widget::hide();
                set_rate(true);
                tick();
            }
        }
        Cmd::HideMonitor => {
            if monitor::is_open() {
                monitor::hide();
                if with_state(|s| s.settings.widget_visible) == Some(true) {
                    widget::show();
                }
                set_rate(false);
                tick();
            }
        }
        Cmd::Dismiss => {
            let text = w!("Hide the floating widget?

You can bring it back any time from the Bosun icon in the system tray.");
            // SAFETY: modal confirmation owned by the host window.
            let yes = unsafe {
                MessageBoxW(
                    Some(host()),
                    text,
                    w!("Bosun SystemCare"),
                    MB_YESNO | MB_ICONQUESTION,
                )
            } == IDYES;
            if yes {
                with_state(|s| {
                    s.settings.widget_visible = false;
                    s.settings.save();
                });
            }
            exec(Cmd::HideMonitor);
        }
        Cmd::OpenMain => shell::open_main_app("care"),
        Cmd::Scan => shell::open_main_args("--scan"),
        Cmd::OpenSettings => shell::open_main_args("--settings"),
        Cmd::Turbo => {
            if let Err(e) = tools::toggle_turbo() {
                // SAFETY: modal message owned by the host window.
                unsafe {
                    MessageBoxW(
                        Some(host()),
                        &HSTRING::from(e),
                        w!("Bosun SystemCare"),
                        MB_ICONWARNING,
                    );
                }
            }
            monitor::invalidate();
        }
        Cmd::ToggleWidget => {
            let visible = with_state(|s| {
                s.settings.widget_visible = !s.settings.widget_visible;
                s.settings.save();
                s.settings.widget_visible
            });
            match visible {
                Some(true) => widget::show(),
                Some(false) => widget::hide(),
                None => {}
            }
        }
        Cmd::Menu => tray::show_menu(),
        Cmd::Reload => {
            bosun_ui::gfx::set_light(settings::light_skin());
            monitor::invalidate();
            // keep the widget where the user dragged it; take everything else
            let fresh = Settings::load();
            let shown = with_state(|s| {
                let pos = s.settings.widget_pos;
                s.settings = Settings {
                    widget_pos: pos,
                    ..fresh
                };
                s.settings.widget_visible
            });
            match shown {
                Some(true) => {
                    widget::show();
                    widget::apply_pin();
                    widget::refresh();
                }
                Some(false) => widget::hide(),
                None => {}
            }
        }
        Cmd::Refresh => tick(),
        Cmd::CleanRam => clean_ram(),
        // SAFETY: destroying our own host window ends the message loop.
        Cmd::Exit => unsafe {
            let _ = DestroyWindow(host());
        },
    }
}

/// Animates the widget and trims working sets on a worker thread, so the
/// rocket keeps flying while Windows does the work.
fn clean_ram() {
    if widget::is_cleaning() {
        return;
    }
    with_state(|s| s.last_clean = Some(Instant::now()));
    widget::start_clean();
    let host = host().0 as isize;
    std::thread::spawn(move || {
        let freed = bosun_core::procs::trim_working_sets();
        // SAFETY: posting to the host window owned by the UI thread.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(host as _)),
                WM_CLEANED,
                WPARAM(0),
                LPARAM(freed as isize),
            );
        }
    });
}

extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => tick(),
        WM_TRAY => tray::on_message(wp, lp),
        WM_CMD => {
            if let Some(c) = Cmd::from_usize(wp.0) {
                exec(c);
            }
        }
        WM_COMMAND => {
            if let Some(c) = tray::menu_command(wp.0 & 0xFFFF) {
                exec(c);
            }
        }
        WM_OTHER_INSTANCE => exec(Cmd::ShowMonitor),
        WM_CLEANED => {
            let ram = bosun_core::metrics::read_memory().load_pct;
            tick();
            widget::clean_done(lp.0 as u64, ram);
        }
        WM_DESTROY => {
            tray::remove();
            widget::hide();
            // SAFETY: ends the message loop.
            unsafe { PostQuitMessage(0) };
        }
        m if m != 0 && m == TASKBAR_CREATED.with(Cell::get) => tray::add(),
        // SAFETY: default handling for everything else.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

fn run() -> Result<()> {
    // SAFETY: standard Win32 start-up sequence on the UI thread.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\Bosun.SystemCare.Tray"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // `--clean-ram` lets shortcuts/hotkeys trigger Clean RAM in the running tray.
            if let Ok(h) = FindWindowW(w!("BosunTrayHost"), None) {
                let (msg, wp) = if std::env::args().any(|a| a == "--clean-ram") {
                    (WM_CMD, Cmd::CleanRam as usize)
                } else {
                    (WM_OTHER_INSTANCE, 0)
                };
                let _ = PostMessageW(Some(h), msg, WPARAM(wp), LPARAM(0));
            }
            return Ok(());
        }
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        shell::enable_dark_menus();
        bosun_ui::init()?;
        bosun_ui::gfx::set_light(settings::light_skin());
        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        HINST.with(|h| h.set(hinst));

        let class = w!("BosunTrayHost");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            class,
            w!("Bosun SystemCare"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )?;
        HOST.with(|h| h.set(hwnd));
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(w!("TaskbarCreated"))));

        let settings = Settings::load();
        let widget_visible = settings.widget_visible;
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                sampler: VitalsSampler::new(),
                table: ProcessTable::new(),
                prev_adapters: Vec::new(),
                prev_at: Instant::now(),
                data: Data::default(),
                settings,
                last_clean: None,
            })
        });

        widget::register_class();
        popup::register_class();
        wmenu::register_class();
        monitor::register_class();
        tray::add();
        if widget_visible {
            widget::show();
        }
        set_rate(false);
        tick();

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

#[cfg(test)]
mod tests {
    use super::Cmd;

    #[test]
    fn commands_round_trip_through_wparam() {
        for c in [Cmd::ToggleMonitor, Cmd::Exit, Cmd::Refresh, Cmd::Menu] {
            assert_eq!(Cmd::from_usize(c as usize), Some(c));
        }
        assert_eq!(Cmd::from_usize(0), None);
    }

    #[test]
    fn reload_is_the_number_the_app_posts() {
        // bosun-app's tray_ini::notify_tray posts WM_APP + 3 with 11
        assert_eq!(
            super::WM_CMD,
            windows::Win32::UI::WindowsAndMessaging::WM_APP + 3
        );
        assert_eq!(Cmd::from_usize(11), Some(Cmd::Reload));
    }
}
