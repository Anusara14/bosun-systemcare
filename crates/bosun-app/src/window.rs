//! The main window: state, painting, input and the window procedure.

use std::cell::RefCell;
use std::time::Instant;

use bosun_ui::gfx::{self, Painter, col, rect};
use bosun_ui::graphics;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_SIZE_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_SOFTWARE, ID2D1Bitmap, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    IDC_ARROW, IDC_HAND, IsZoomed, KillTimer, LoadCursorW, LoadIconW, PostQuitMessage,
    RegisterClassExW, SW_MINIMIZE, SW_SHOWNORMAL, SetCursor, SetTimer, ShowWindow, WM_DESTROY,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSEMOVE, WM_PAINT,
    WM_SETCURSOR, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, Result, w};

use crate::pages::{self, Page};
use crate::{backdrop, chrome, frame};

pub const CLASS: PCWSTR = w!("BosunMain");

/// A second launch hands its `Request` to the running window (wParam).
pub const WM_REQUEST: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 20;

/// What a launch asks for: a page, a Care scan, the Settings dialog.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Request {
    pub page: Option<Page>,
    pub scan: bool,
    pub settings: bool,
}

impl Request {
    /// Low 4 bits: page index + 1 (0 = none); bit 4 scan; bit 5 settings.
    pub fn encode(self) -> usize {
        let page = self
            .page
            .and_then(|p| pages::ALL.iter().position(|&q| q == p))
            .map_or(0, |i| i + 1);
        page | usize::from(self.scan) << 4 | usize::from(self.settings) << 5
    }

    pub fn decode(v: usize) -> Self {
        Request {
            page: (v & 0xF)
                .checked_sub(1)
                .and_then(|i| pages::ALL.get(i).copied()),
            scan: v & 1 << 4 != 0,
            settings: v & 1 << 5 != 0,
        }
    }
}

/// Carries out a launch request in an open window: a scan starts on Care
/// (unless one is already running), a page opens with its data loaded.
fn apply_request(hwnd: HWND, r: Request) {
    with_app(|a| {
        a.menu = None;
        a.about = false;
        a.bell = None;
        if r.page.is_some() || r.scan {
            a.opt = None;
            a.turbo = None;
            a.tools = false;
            a.settings = None;
        }
    });
    if r.scan {
        click(hwnd, Hit::Nav(Page::Care));
        if with_app(|a| a.scan.is_none()) == Some(true) {
            // SAFETY: one-shot timer on our own window (presses SCAN).
            unsafe { SetTimer(Some(hwnd), SCRIPT_SCAN, 300, None) };
        }
    } else if let Some(p) = r.page {
        click(hwnd, Hit::Nav(p));
    }
    if r.settings {
        with_app(|a| a.settings = Some(crate::pages::settings::SettingsView::open()));
    }
    invalidate(hwnd);
}
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_NCMOUSELEAVE: u32 = 0x02A2;
const SPIN_TIMER: usize = 1;
/// Polls the scan worker (~30 fps) while a scan runs.
const SCAN_TIMER: usize = 5;
/// Polls the Windows Update driver search while it runs.
const DRIVER_TIMER: usize = 8;
const UPDATE_TIMER: usize = 9;
/// Test hook timers: with `BOSUN_HOVER_SCRIPT=1` the window replays the
/// reference recording's hover (in, then out 1733 ms later, as in the
/// recording) without a mouse, starting 1.5 s after launch,
/// so the animation can be captured and compared frame by frame.
const SCRIPT_IN: usize = 3;
const SCRIPT_OUT: usize = 4;
/// With `BOSUN_SCAN_SCRIPT=1` a scan starts 1.5 s after launch (capture hook).
const SCRIPT_SCAN: usize = 6;
/// With `BOSUN_MODE_SCRIPT=1` Manual mode is clicked 1.5 s after launch.
const SCRIPT_MODE: usize = 7;

/// `BOSUN_SCAN_SCRIPT=fix`: the capture hook also runs Fix, always as a dry run.
fn script_fix() -> bool {
    std::env::var_os("BOSUN_SCAN_SCRIPT").is_some_and(|v| v == "fix")
}

/// Fix walks and journals but deletes nothing (`BOSUN_FIX_DRYRUN=1`, or the
/// capture hook).
fn fix_dry_run() -> bool {
    std::env::var_os("BOSUN_FIX_DRYRUN").is_some() || script_fix()
}

fn scripted() -> bool {
    [
        "BOSUN_HOVER_SCRIPT",
        "BOSUN_SCAN_SCRIPT",
        "BOSUN_MODE_SCRIPT",
    ]
    .iter()
    .any(|v| std::env::var_os(v).is_some())
}
/// Default size in DIPs (the reference window).
pub const DEFAULT_W: f32 = 960.0;
pub const DEFAULT_H: f32 = 660.0;
pub const MIN_W: f32 = 900.0;
pub const MIN_H: f32 = 620.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Recommended,
    Manual,
}

/// Everything the pointer can be over.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    None,
    Nav(Page),
    Bell,
    Menu,
    Min,
    Max,
    Close,
    Scan,
    Mode(Mode),
    ModeHelp,
    ScanSkip,
    ScanStop,
    ScanBack,
    ScanFix,
    AutoFix,
    /// A summary row (click ticks / unticks the module).
    ScanRow(usize),
    /// Manual mode: a module's tick box, or Select All.
    Pick(usize),
    PickAll,
    /// Speed Up: a card's button, a Real-Time TuneUp switch.
    Card(u8),
    Toggle(u8),
    /// Startup Optimizer (indices are into its item list).
    OptTab(u8),
    OptBack,
    OptCheck(usize),
    OptCheckAll,
    OptIgnore(usize),
    OptGo,
    OptStatus(usize),
    OptRefresh,
    OptRescue,
    OptSystem,
    OptRemove(usize),
    /// App Cleaner: sort Programs by name (0), size (1) or date (2).
    OptSort(u8),
    /// Turbo Boost settings and the Speed Up card's split button.
    TurboMode,
    TurboTab(u8),
    TurboGo,
    TurboPower,
    TurboStartup,
    TurboRecommend,
    TurboTick(Option<usize>),
    TurboModeCard,
    TurboConfigure,
    /// More Optimization Tools: a tool (see `pages::tools::COLUMNS`), or the window.
    Tool(u8),
    ToolsOpen,
    /// Protect: a privacy switch, Homepage Advisor, Windows Security.
    Privacy(u8),
    Advisor,
    OpenSecurity,
    /// Software Updater (indices are into winget's list).
    UpdAll,
    UpdOne(usize),
    UpdIgnore(usize),
    UpdRow(usize),
    /// Rescue Center rows (indices into the journal list, newest first).
    RescueRestore(usize),
    RescueBin(usize),
    RescueRow(usize),
    RescueFolder,
    /// The ☰ panel: outside it (closes), an item, a settings switch.
    MenuOutside,
    MenuItem(u8),
    MenuToggle(u8),
    /// A skin preview: 0 Dark, 1 Light.
    MenuSkin(u8),
    /// About dialog.
    AboutClose,
    AboutLink(u8),
    /// Settings: a tree row, a checkbox, the Logs & Backup buttons, the footer.
    SetNav(u8),
    SetCheck(u8),
    SetOpenLogs,
    SetPrune,
    SetQuick,
    SetRecommended,
    SetCancel,
    SetOk,
    /// The bell's panel: a switch, "turn everything on", close.
    BellToggle(u8),
    BellAll,
    BellClose,
}

impl Hit {
    pub fn clickable(self) -> bool {
        self != Hit::None
    }
}

/// Read-only snapshot handed to the drawing code.
pub struct View {
    pub page: Page,
    pub w: f32,
    pub h: f32,
    pub hover: Hit,
    pub pressed: Hit,
    pub mode: Mode,
    /// SCAN hover transition, 0 (rest) → 1 (hovered), animated.
    pub scan_hover: f32,
    /// (SCAN opacity, magnifier opacity), timed on the clock, not the easing.
    pub scan_label: (f32, f32),
    /// The scan screen's state while a scan is shown (replaces the sidebar).
    pub scan: Option<crate::scan::Progress>,
    pub auto_fix: bool,
    /// Manual mode's ticked modules (bit i = `scan::MODULES[i]`).
    pub picks: u16,
    /// Recommended (0) → Manual (1) layout, animated.
    pub manual_t: f32,
    pub speed: crate::pages::speedup::SpeedInfo,
    /// The Startup Optimizer, when open (it replaces the page and sidebar).
    pub opt: Option<crate::pages::optimizer::OptView>,
    /// Turbo Boost settings, when open (also replaces the page and sidebar).
    pub turbo: Option<crate::pages::turbo::TurboView>,
    /// The More Optimization Tools window is open.
    pub tools: bool,
    pub protect: crate::pages::protect::ProtectInfo,
    /// The Software Updater's list, on that page.
    pub updater: Option<crate::pages::updater::UpdView>,
    /// The Rescue Center's journal, on that page.
    pub rescue: Option<crate::pages::rescue::RescueView>,
    /// The ☰ panel, when open, and the About dialog.
    pub menu: Option<crate::pages::menu::MenuInfo>,
    pub about: bool,
    /// The Settings dialog, when open (it covers the whole window).
    pub settings: Option<crate::pages::settings::SettingsView>,
    /// The bell's panel, when open (it covers the whole window).
    pub bell: Option<crate::pages::actions::Actions>,
    pub note: Option<&'static str>,
    pub maximized: bool,
}

struct App {
    hwnd: HWND,
    dpi: u32,
    rt: Option<(ID2D1HwndRenderTarget, ID2D1SolidColorBrush)>,
    layers: Option<Layers>,
    page: Page,
    hover: Hit,
    pressed: Hit,
    mode: Mode,
    scan_hover: f32,
    scan_label: (f32, f32),
    scan: Option<crate::scan::Scan>,
    auto_fix: bool,
    /// Auto fix already chained a Fix onto this scan.
    auto_ran: bool,
    /// Manual mode's ticked modules (bit i = `scan::MODULES[i]`).
    picks: u16,
    manual_t: f32,
    speed: crate::pages::speedup::SpeedInfo,
    opt: Option<crate::pages::optimizer::Optimizer>,
    turbo: Option<crate::pages::turbo::Turbo>,
    tools: bool,
    protect: crate::pages::protect::ProtectInfo,
    updater: crate::pages::updater::Updater,
    rescue: crate::pages::rescue::RescueView,
    menu: Option<crate::pages::menu::MenuInfo>,
    about: bool,
    settings: Option<crate::pages::settings::SettingsView>,
    bell: Option<crate::pages::actions::Actions>,
    /// Running layout change between modes: (start, from, to).
    mode_anim: Option<(Instant, f32, f32)>,
    /// Running hover transition: (start, from, to).
    anim: Option<(Instant, f32, f32)>,
    note: Option<(&'static str, Instant)>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

impl App {
    fn size(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        // SAFETY: our own window.
        let _ = unsafe { GetClientRect(self.hwnd, &mut rc) };
        (gfx::dip(rc.right, self.dpi), gfx::dip(rc.bottom, self.dpi))
    }

    fn view(&self) -> View {
        let (w, h) = self.size();
        View {
            page: self.page,
            w,
            h,
            hover: self.hover,
            pressed: self.pressed,
            mode: self.mode,
            scan_hover: self.scan_hover,
            scan_label: self.scan_label,
            scan: self.scan.as_ref().and_then(|s| s.snapshot()),
            auto_fix: self.auto_fix,
            picks: self.picks,
            manual_t: self.manual_t,
            speed: self.speed,
            opt: self.opt.as_ref().map(|o| o.view()),
            turbo: self.turbo.as_ref().map(|t| t.view()),
            tools: self.tools,
            protect: self.protect.clone(),
            updater: (self.page == Page::Updater).then(|| self.updater.view()),
            rescue: (self.page == Page::Rescue).then(|| self.rescue.clone()),
            menu: self.menu,
            about: self.about,
            settings: self.settings.clone(),
            bell: self.bell,
            note: self
                .note
                .filter(|(_, at)| at.elapsed().as_secs() < 5)
                .map(|(n, _)| n),
            // SAFETY: our own window.
            maximized: unsafe { IsZoomed(self.hwnd) }.as_bool(),
        }
    }
}

/// Chrome first (title buttons, sidebar), then the active page.
pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    if v.about {
        return pages::menu::about_hit(v, x, y);
    }
    if v.menu.is_some() {
        return pages::menu::menu_hit(v, x, y);
    }
    if let Some(s) = &v.settings {
        return pages::settings::hit(v, s, x, y);
    }
    if v.bell.is_some() {
        return pages::actions::hit(v, x, y);
    }
    if v.tools {
        let h = pages::tools::hit(v, x, y);
        if h != Hit::None {
            return h;
        }
    }
    if let Some(t) = &v.turbo {
        let h = pages::turbo::hit(v, t, x, y);
        if h != Hit::None {
            return h;
        }
    }
    if let Some(o) = &v.opt {
        let h = pages::optimizer::hit(v, o, x, y);
        if h != Hit::None {
            return h;
        }
    }
    match chrome::hit(v, x, y) {
        Hit::None => pages::hit(v, x, y),
        h => h,
    }
}

pub fn create(request: Request) -> Result<()> {
    let page = request.page.unwrap_or(Page::Care); // the saved skin; capture hook: BOSUN_LIGHT=1 forces the light skin
    bosun_ui::gfx::set_light(
        std::env::var_os("BOSUN_LIGHT").is_some()
            || crate::tray_ini::get_from(crate::tray_ini::APP, "light_skin", false),
    );
    // SAFETY: class registration and window creation for our own process.
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc),
            hInstance: hinst.into(),
            hIcon: LoadIconW(Some(hinst.into()), PCWSTR(1 as _)).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        });
        let hwnd = CreateWindowExW(
            Default::default(),
            CLASS,
            w!("Bosun SystemCare"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            100,
            100,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;
        let dpi = GetDpiForWindow(hwnd).max(96);
        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                hwnd,
                dpi,
                rt: None,
                layers: None,
                page,
                hover: Hit::None,
                pressed: Hit::None,
                mode: if std::env::var_os("BOSUN_MANUAL").is_some() {
                    Mode::Manual
                } else {
                    Mode::Recommended
                },
                // capture hook: BOSUN_SCAN_HOVER=<0..1> freezes the SCAN hover there
                scan_hover: std::env::var("BOSUN_SCAN_HOVER")
                    .ok()
                    .and_then(|h| h.parse().ok())
                    .unwrap_or(0.0),
                scan_label: std::env::var("BOSUN_SCAN_HOVER")
                    .ok()
                    .and_then(|h| h.parse().ok())
                    .map_or((1.0, 0.0), |h| crate::pages::care::label_fades(h, true)),
                scan: None,
                auto_fix: script_fix()
                    || crate::tray_ini::get_from(crate::tray_ini::APP, "auto_fix", false),
                auto_ran: false,
                picks: crate::pages::settings::saved_picks(),
                // capture hook: BOSUN_MANUAL=1 starts in Manual mode
                manual_t: if std::env::var_os("BOSUN_MANUAL").is_some() {
                    1.0
                } else {
                    0.0
                },
                speed: speed_info(),
                // capture hook: BOSUN_OPT_TAB=<n> opens the Startup Optimizer
                opt: std::env::var("BOSUN_OPT_TAB")
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .map(|t| {
                        if t == 6 || t == 7 || t == 8 {
                            // snapshots show the finished list: wait for the search
                            let mut o = if t == 6 {
                                crate::pages::optimizer::Optimizer::open_drivers()
                            } else {
                                crate::pages::optimizer::Optimizer::open_finder(t)
                            };
                            while o.poll_drivers() {
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            o
                        } else if t == 5 {
                            crate::pages::optimizer::Optimizer::open_apps()
                        } else {
                            crate::pages::optimizer::Optimizer::open(t)
                        }
                    }),
                // capture hook: BOSUN_TURBO_TAB=<n> opens the Turbo Boost settings
                // capture hook: BOSUN_TOOLS=1 opens More Optimization Tools
                tools: std::env::var_os("BOSUN_TOOLS").is_some(),
                protect: if page == Page::Protect {
                    crate::pages::protect::ProtectInfo::load()
                } else {
                    Default::default()
                },
                updater: Default::default(),
                // capture hooks: BOSUN_MENU=1 opens the ☰ panel, BOSUN_ABOUT=1 the About dialog
                menu: std::env::var_os("BOSUN_MENU").map(|_| crate::pages::menu::MenuInfo::load()),
                about: std::env::var_os("BOSUN_ABOUT").is_some(),
                // capture hook: BOSUN_BELL=1 opens the bell's panel
                bell: std::env::var_os("BOSUN_BELL")
                    .map(|_| crate::pages::actions::Actions::load(false)),
                // capture hook: BOSUN_SETTINGS=<tree row> opens Settings on that page
                settings: std::env::var("BOSUN_SETTINGS")
                    .ok()
                    .and_then(|k| k.parse().ok())
                    .map(|k| {
                        let mut s = crate::pages::settings::SettingsView::open();
                        s.sel = k;
                        s
                    }),
                rescue: if page == Page::Rescue {
                    crate::pages::rescue::RescueView::load()
                } else {
                    Default::default()
                },
                turbo: std::env::var("BOSUN_TURBO_TAB")
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .map(|t| {
                        let mut x =
                            crate::pages::turbo::Turbo::open(bosun_core::turbo::State::load().mode);
                        x.tab = t;
                        x
                    }),
                mode_anim: None,
                anim: None,
                note: None,
            })
        });
        frame::init(hwnd, dpi);
        if std::env::var_os("BOSUN_HOVER_SCRIPT").is_some() {
            SetTimer(Some(hwnd), SCRIPT_IN, 1500, None);
            SetTimer(Some(hwnd), SCRIPT_OUT, 3233, None);
        }
        if std::env::var_os("BOSUN_MODE_SCRIPT").is_some() {
            SetTimer(Some(hwnd), SCRIPT_MODE, 1500, None);
        }
        if std::env::var_os("BOSUN_SCAN_SCRIPT").is_some() {
            SetTimer(Some(hwnd), SCRIPT_SCAN, 1500, None);
        }
        if page == Page::Updater {
            with_app(|a| a.updater.load());
            SetTimer(Some(hwnd), UPDATE_TIMER, 250, None);
        }
        if request.settings {
            with_app(|a| a.settings = Some(crate::pages::settings::SettingsView::open()));
        }
        if request.scan {
            SetTimer(Some(hwnd), SCRIPT_SCAN, 300, None);
        }
        // capture hook: BOSUN_SNAPSHOT=<png> draws the first view off-screen at
        // the reference size, saves it and exits without showing the window
        if let Some(out) = std::env::var_os("BOSUN_SNAPSHOT") {
            // snapshots show the finished winget list
            while page == Page::Updater && with_app(|a| a.updater.poll()) == Some(true) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            with_app(|a| {
                let mut v = a.view();
                v.w = DEFAULT_W;
                v.h = DEFAULT_H;
                let bg = bosun_ui::gfx::tint(col(backdrop::BASE));
                let _ = bosun_ui::snapshot::save_png(
                    graphics(),
                    960,
                    660,
                    bg,
                    std::path::Path::new(&out),
                    |p| {
                        chrome::draw(p, &v);
                        pages::draw(p, &v);
                    },
                );
            });
            std::process::exit(0);
        }
        set_icons(hwnd, hinst.into(), dpi);
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    Ok(())
}

/// Hands Windows our icon at the exact big/small sizes for this DPI, so the
/// taskbar uses the crisp, edge-to-edge artwork instead of scaling one size
/// (and never shows a stale cached copy).
fn set_icons(hwnd: HWND, hinst: windows::Win32::Foundation::HINSTANCE, dpi: u32) {
    use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
    use windows::Win32::UI::WindowsAndMessaging::{
        ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON,
        SendMessageW, WM_SETICON,
    };
    // SAFETY: icon resource 1 is embedded by build.rs; handles live for the process.
    unsafe {
        for (which, metric) in [(ICON_BIG, SM_CXICON), (ICON_SMALL, SM_CXSMICON)] {
            let size = GetSystemMetricsForDpi(metric, dpi);
            if let Ok(h) = LoadImageW(
                Some(hinst),
                PCWSTR(1 as _),
                IMAGE_ICON,
                size,
                size,
                LR_DEFAULTCOLOR,
            ) {
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    Some(WPARAM(which as usize)),
                    Some(LPARAM(h.0 as isize)),
                );
            }
        }
    }
}

fn invalidate(hwnd: HWND) {
    // SAFETY: our own window.
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn ensure_target(a: &mut App) -> Option<(ID2D1HwndRenderTarget, ID2D1SolidColorBrush)> {
    if a.rt.is_none() {
        let mut rc = RECT::default();
        // SAFETY: render target bound to our client area.
        unsafe {
            let _ = GetClientRect(a.hwnd, &mut rc);
            // Software rendering keeps the GPU driver (~50 MB) out of the process.
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                dpiX: a.dpi as f32,
                dpiY: a.dpi as f32,
                ..Default::default()
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: a.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: rc.right as u32,
                    height: rc.bottom as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let rt = graphics().d2d.CreateHwndRenderTarget(&props, &hp).ok()?;
            let br = rt.CreateSolidColorBrush(&col(0xFFFFFF), None).ok()?;
            a.rt = Some((rt, br));
        }
    }
    a.rt.clone()
}

/// Everything the cached foreground depends on (all of the view except the glow).
#[derive(Clone, PartialEq)]
struct Key {
    w: f32,
    h: f32,
    page: Page,
    hover: Hit,
    pressed: Hit,
    mode: Mode,
    scan_hover: f32,
    scan_label: (f32, f32),
    scan: Option<crate::scan::Progress>,
    auto_fix: bool,
    picks: u16,
    manual_t: f32,
    speed: crate::pages::speedup::SpeedInfo,
    opt: Option<crate::pages::optimizer::OptView>,
    turbo: Option<crate::pages::turbo::TurboView>,
    tools: bool,
    protect: crate::pages::protect::ProtectInfo,
    updater: Option<crate::pages::updater::UpdView>,
    rescue: Option<crate::pages::rescue::RescueView>,
    menu: Option<crate::pages::menu::MenuInfo>,
    about: bool,
    settings: Option<crate::pages::settings::SettingsView>,
    bell: Option<crate::pages::actions::Actions>,
    note: Option<&'static str>,
    maximized: bool,
}

impl Key {
    fn of(v: &View) -> Self {
        Key {
            w: v.w,
            h: v.h,
            page: v.page,
            hover: v.hover,
            pressed: v.pressed,
            mode: v.mode,
            scan_hover: v.scan_hover,
            scan_label: v.scan_label,
            scan: v.scan.clone(),
            auto_fix: v.auto_fix,
            picks: v.picks,
            manual_t: v.manual_t,
            speed: v.speed,
            opt: v.opt.clone(),
            turbo: v.turbo.clone(),
            tools: v.tools,
            protect: v.protect.clone(),
            updater: v.updater.clone(),
            rescue: v.rescue.clone(),
            menu: v.menu,
            about: v.about,
            settings: v.settings.clone(),
            bell: v.bell,
            note: v.note,
            maximized: v.maximized,
        }
    }
}

/// Cached layers: the full interface, and the static backdrop overlay (grain,
/// grid, circles). The drifting blobs are re-rendered at 1/8 resolution per frame.
struct Layers {
    key: Key,
    fg: ID2D1Bitmap,
    overlay: Option<(f32, f32, ID2D1Bitmap)>,
}

/// Draws into an off-screen bitmap compatible with `rt`.
fn layer(
    rt: &ID2D1HwndRenderTarget,
    pixels: Option<D2D_SIZE_U>,
    size: (f32, f32),
    draw: impl FnOnce(&Painter),
) -> Option<ID2D1Bitmap> {
    // SAFETY: a temporary compatible target; its bitmap outlives it via COM refcount.
    unsafe {
        let ds = D2D_SIZE_F {
            width: size.0,
            height: size.1,
        };
        let opts = D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE;
        let bt = rt
            .CreateCompatibleRenderTarget(
                Some(&ds),
                pixels.as_ref().map(|p| p as *const _),
                None,
                opts,
            )
            .ok()?;
        let br = bt.CreateSolidColorBrush(&col(0xFFFFFF), None).ok()?;
        bt.BeginDraw();
        bt.Clear(Some(&bosun_ui::gfx::cola(0, 0.0)));
        let base: &ID2D1RenderTarget = &bt;
        draw(&Painter {
            rt: base,
            br: &br,
            g: graphics(),
        });
        bt.EndDraw(None, None).ok()?;
        bt.GetBitmap().ok()
    }
}

fn refresh_layers(a: &mut App, rt: &ID2D1HwndRenderTarget, v: &View) -> Option<()> {
    let key = Key::of(v);
    if a.layers.as_ref().is_some_and(|l| l.key == key) {
        return Some(());
    }
    let size = (v.w, v.h);
    let overlay = match a
        .layers
        .take()
        .and_then(|l| l.overlay)
        .filter(|(w, h, _)| (*w, *h) == size)
    {
        Some(cached) => Some(cached),
        None => layer(rt, None, size, |p| backdrop::overlay(p, v.w, v.h)).map(|b| (v.w, v.h, b)),
    };
    let fg = layer(rt, None, size, |p| {
        chrome::draw(p, v);
        pages::draw(p, v);
    })?;
    a.layers = Some(Layers { key, fg, overlay });
    Some(())
}

fn paint(a: &mut App) {
    let Some((rt, br)) = ensure_target(a) else {
        return;
    };
    let v = a.view();
    let _ = br;
    if refresh_layers(a, &rt, &v).is_none() {
        a.rt = None;
        a.layers = None;
        return;
    }
    let Some(l) = a.layers.as_ref() else { return };
    let full = rect(0.0, 0.0, v.w, v.h);
    // SAFETY: Begin/EndDraw pair on our render target; bitmaps come from it.
    unsafe {
        rt.BeginDraw();
        rt.Clear(Some(&bosun_ui::gfx::tint(col(backdrop::BASE))));
        if let Some((_, _, o)) = &l.overlay {
            rt.DrawBitmap(
                o,
                Some(&full),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                None,
            );
        }
        rt.DrawBitmap(
            &l.fg,
            Some(&full),
            1.0,
            D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
            None,
        );
        if rt.EndDraw(None, None).is_err() {
            a.rt = None;
            a.layers = None;
        }
    }
}

fn set_hover(hwnd: HWND, h: Hit) {
    let changed = with_app(|a| {
        if a.hover == h {
            return false;
        }
        a.hover = h;
        // Entering or leaving SCAN starts a short cross-fade (face lightens,
        // label turns into a magnifier), like the reference.
        let target = if h == Hit::Scan { 1.0 } else { 0.0 };
        let going = a.anim.map_or(a.scan_hover, |(_, _, to)| to);
        if (target - going).abs() > f32::EPSILON {
            a.anim = Some((Instant::now(), a.scan_hover, target));
            // SAFETY: ~60 fps timer on our own window, killed when the fade ends.
            unsafe { SetTimer(Some(hwnd), SPIN_TIMER, 16, None) };
        }
        true
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

fn client_dip(hwnd: HWND, lp: LPARAM) -> (f32, f32) {
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let x = (lp.0 & 0xFFFF) as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
    (gfx::dip(x, dpi), gfx::dip(y, dpi))
}

fn hit_at(x: f32, y: f32) -> Hit {
    with_app(|a| hit(&a.view(), x, y)).unwrap_or(Hit::None)
}

fn click(hwnd: HWND, h: Hit) {
    match h {
        // Closing the optimizer returns to Speed Up, as its own window would.
        Hit::Close | Hit::OptBack
            if with_app(|a| {
                std::mem::take(&mut a.tools) || a.turbo.take().is_some() || a.opt.take().is_some()
            }) == Some(true) =>
        {
            with_app(|a| a.speed = speed_info());
            invalidate(hwnd);
        }
        Hit::OptStatus(i) => {
            option_status(hwnd, i);
            invalidate(hwnd);
        }
        Hit::OptRemove(i)
            if with_app(|a| a.opt.as_ref().is_some_and(|o| o.tab == 7 || o.tab == 8))
                == Some(true) =>
        {
            let note = with_app(|a| {
                let o = a.opt.as_mut()?;
                let list = if o.tab == 7 {
                    o.large.as_mut()?
                } else {
                    o.empty.as_mut()?
                };
                let found = list.get(i)?.clone();
                let dir = bosun_core::clean::journal_dir()?;
                let label = if o.tab == 7 {
                    "Large File Finder"
                } else {
                    "Empty Folder Finder"
                };
                Some(match bosun_core::finder::recycle(&found, &dir, label) {
                    Ok(()) => {
                        list.remove(i);
                        "Moved to the Recycle Bin."
                    }
                    Err(_) => "Couldn't move that to the Recycle Bin (it may be in use).",
                })
            })
            .flatten();
            if let Some(n) = note {
                with_app(|a| a.note = Some((n, Instant::now())));
            }
            invalidate(hwnd);
        }
        Hit::Tool(id) => {
            tool(hwnd, id);
            invalidate(hwnd);
        }
        Hit::OptRemove(_)
            if with_app(|a| a.opt.as_ref().is_some_and(|o| o.tab == 6)) == Some(true) =>
        {
            // Windows Update installs drivers itself (Settings → Optional updates)
            // SAFETY: opens a Settings page by its URI.
            unsafe {
                windows::Win32::UI::Shell::ShellExecuteW(
                    None,
                    w!("open"),
                    w!("ms-settings:windowsupdate-optionalupdates"),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
            with_app(|a| {
                a.note = Some((
                    "Opened Windows Update's optional updates: install drivers there.",
                    Instant::now(),
                ))
            });
            invalidate(hwnd);
        }
        Hit::OptRemove(i)
            if with_app(|a| a.opt.as_ref().is_some_and(|o| o.tab == 5)) == Some(true) =>
        {
            let app = with_app(|a| {
                a.opt
                    .as_ref()
                    .and_then(|o| o.programs.as_ref().map(|v| v[i].clone()))
            })
            .flatten();
            if let Some(app) = app {
                let note = if bosun_core::apps::uninstall(&app) {
                    "Started the program's own uninstaller. Press Refresh when it's done."
                } else {
                    "Couldn't start that program's uninstaller."
                };
                with_app(|a| a.note = Some((note, Instant::now())));
                invalidate(hwnd);
            }
        }
        Hit::OptRemove(i) => {
            let ext = with_app(|a| {
                a.opt
                    .as_ref()
                    .and_then(|o| o.extensions.as_ref().map(|v| v[i].clone()))
            })
            .flatten();
            if let Some(e) = ext {
                let exe = e
                    .browser
                    .exe_path()
                    .map_or_else(|| e.browser.exe().into(), |p| p.display().to_string());
                // SAFETY: opens the browser on its own page for this extension.
                let r = unsafe {
                    windows::Win32::UI::Shell::ShellExecuteW(
                        None,
                        w!("open"),
                        &windows::core::HSTRING::from(exe),
                        &windows::core::HSTRING::from(e.browser.page(&e.id)),
                        None,
                        SW_SHOWNORMAL,
                    )
                };
                let note = if r.0 as isize > 32 {
                    "Opened the extension's page in its browser: remove it there."
                } else {
                    "Couldn't open that browser."
                };
                with_app(|a| a.note = Some((note, Instant::now())));
                invalidate(hwnd);
            }
        }
        Hit::TurboMode | Hit::TurboModeCard => {
            use bosun_core::turbo::Mode;
            let cur = with_app(|a| a.turbo.as_ref().map_or(a.speed.turbo_mode, |t| t.mode))
                .unwrap_or_default();
            let labels: Vec<&str> = Mode::ALL.iter().map(|m| m.label()).collect();
            let at = Mode::ALL.iter().position(|m| *m == cur).unwrap_or(0);
            if let Some(k) = choice_menu(hwnd, &labels, at) {
                with_app(|a| {
                    if let Some(t) = a.turbo.as_mut() {
                        t.mode = Mode::ALL[k];
                    }
                    a.speed.turbo_mode = Mode::ALL[k];
                    let mut st = bosun_core::turbo::State::load();
                    st.mode = Mode::ALL[k];
                    let _ = st.save();
                });
            }
            invalidate(hwnd);
        }
        Hit::OptRescue => open_rescue(hwnd),
        // SAFETY: closing our own window ends the process.
        Hit::Close => unsafe {
            let _ = DestroyWindow(hwnd);
        },
        // SAFETY: our own window.
        Hit::Min => unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        },
        Hit::Max => frame::toggle_maximize(hwnd),
        Hit::Menu => {
            with_app(|a| a.menu = Some(crate::pages::menu::MenuInfo::load()));
            invalidate(hwnd);
        }
        Hit::MenuItem(k) => menu_item(hwnd, k),
        Hit::MenuOutside | Hit::AboutClose => {
            with_app(|a| {
                a.menu = None;
                a.about = false;
            });
            invalidate(hwnd);
        }
        _ => {
            with_app(|a| match h {
                Hit::Nav(p) => {
                    if p == Page::SpeedUp {
                        a.speed = speed_info();
                    }
                    if p == Page::Rescue {
                        a.rescue = crate::pages::rescue::RescueView::load();
                    }
                    if p == Page::Protect {
                        a.protect = crate::pages::protect::ProtectInfo::load();
                    }
                    if p == Page::Updater {
                        a.updater.load();
                        // SAFETY: poll timer on our own window, killed when the work ends.
                        unsafe { SetTimer(Some(hwnd), UPDATE_TIMER, 250, None) };
                    }
                    a.page = p;
                }
                Hit::Card(1) => a.opt = Some(crate::pages::optimizer::Optimizer::open(0)),
                Hit::TurboConfigure => {
                    a.turbo = Some(crate::pages::turbo::Turbo::open(a.speed.turbo_mode))
                }
                Hit::Card(0) => {
                    a.note = Some((
                        turbo_toggle(a.speed.turbo_on, a.speed.turbo_mode, None),
                        Instant::now(),
                    ));
                    a.speed = speed_info();
                }
                Hit::TurboTab(t) => a.turbo.iter_mut().for_each(|x| {
                    x.tab = t;
                    x.scroll = 0.0;
                }),
                Hit::TurboTick(i) => a.turbo.iter_mut().for_each(|x| x.toggle(i)),
                Hit::TurboRecommend => a.turbo.iter_mut().for_each(|x| x.recommend()),
                Hit::TurboPower => a.turbo.iter_mut().for_each(|x| x.power = !x.power),
                Hit::TurboStartup => {
                    if let Some(t) = a.turbo.as_mut() {
                        if crate::tray_ini::set("turbo_at_startup", !t.startup).is_ok() {
                            t.startup = !t.startup;
                        } else {
                            a.note = Some(("Couldn't save the setting.", Instant::now()));
                        }
                    }
                }
                Hit::TurboGo => {
                    if let Some(t) = a.turbo.take() {
                        a.note =
                            Some((turbo_toggle(false, t.mode, Some(t.plan())), Instant::now()));
                        a.speed = speed_info();
                    }
                }
                Hit::OptTab(t) => {
                    if let Some(o) = a.opt.as_mut() {
                        o.tab = t;
                        o.scroll = 0.0;
                        o.ensure();
                    }
                }
                Hit::OptSystem => {
                    if let Some(o) = a.opt.as_mut() {
                        o.show_system = !o.show_system;
                        o.scroll = 0.0;
                    }
                }
                Hit::Card(3) => a.opt = Some(crate::pages::optimizer::Optimizer::open_apps()),
                Hit::ToolsOpen => a.tools = true,
                Hit::Privacy(k) => {
                    let on = a.protect.privacy[k as usize];
                    let ok = bosun_core::clean::journal_dir()
                        .is_some_and(|d| bosun_core::privacy::set(k as usize, !on, &d).is_ok());
                    if !ok {
                        a.note = Some(("Couldn't change that setting.", Instant::now()));
                    }
                    a.protect = crate::pages::protect::ProtectInfo::load();
                }
                Hit::Advisor => {
                    if bosun_core::homepage::set_advisor(!a.protect.advisor).is_err() {
                        a.note = Some((
                            "Couldn't save the Homepage Advisor setting.",
                            Instant::now(),
                        ));
                    }
                    a.protect = crate::pages::protect::ProtectInfo::load();
                }
                Hit::OpenSecurity => {
                    shell("windowsdefender:", "");
                }
                Hit::UpdAll | Hit::UpdOne(_) => {
                    a.updater.enqueue(match h {
                        Hit::UpdOne(i) => Some(i),
                        _ => None,
                    });
                    // SAFETY: poll timer on our own window, killed when the work ends.
                    unsafe { SetTimer(Some(hwnd), UPDATE_TIMER, 250, None) };
                }
                Hit::UpdIgnore(i) => a.updater.ignore(i),
                Hit::SetNav(k) => a.settings.iter_mut().for_each(|s| s.sel = k),
                Hit::SetCheck(k) => a.settings.iter_mut().for_each(|s| s.check(k)),
                Hit::SetRecommended => a.settings.iter_mut().for_each(|s| {
                    s.draft = crate::pages::settings::Values::RECOMMENDED;
                }),
                Hit::SetCancel => a.settings = None,
                Hit::SetQuick => {
                    a.settings = None;
                    a.menu = Some(crate::pages::menu::MenuInfo::load());
                }
                Hit::SetOk => {
                    if let Some(s) = a.settings.take() {
                        match s.apply() {
                            Ok(()) => {
                                a.picks = s.draft.picks;
                                a.speed = speed_info();
                            }
                            Err(e) => {
                                // keep the dialog and the draft so nothing is lost
                                a.note = Some((e, Instant::now()));
                                a.settings = Some(s);
                            }
                        }
                    }
                }
                Hit::SetOpenLogs => {
                    if let Some(d) = bosun_core::clean::journal_dir() {
                        let _ = std::fs::create_dir_all(&d);
                        shell(&d.to_string_lossy(), "");
                    }
                }
                Hit::SetPrune => {
                    let r =
                        bosun_core::clean::journal_dir().map(|d| bosun_core::rescue::prune(&d, 30));
                    a.note = Some((
                        match r {
                            Some(Ok(0)) => "No logs are older than 30 days.",
                            Some(Ok(_)) => "Deleted the logs older than 30 days.",
                            _ => "Couldn't delete the old logs.",
                        },
                        Instant::now(),
                    ));
                    a.settings
                        .iter_mut()
                        .for_each(|s| s.logs = crate::pages::settings::logs());
                }
                Hit::MenuToggle(k) => {
                    if let Some(m) = a.menu {
                        if let Err(e) = crate::pages::menu::flip(&m, k) {
                            a.note = Some((e, Instant::now()));
                        }
                        a.menu = Some(crate::pages::menu::MenuInfo::load());
                        a.speed = speed_info();
                    }
                }
                Hit::MenuSkin(l) => {
                    let light = l == 1;
                    bosun_ui::gfx::set_light(light);
                    let _ = crate::tray_ini::set_to(crate::tray_ini::APP, "light_skin", light);
                    // the widget and monitor follow the skin too
                    crate::tray_ini::notify_tray();
                    // every cached layer was drawn in the other skin
                    a.layers = None;
                }
                Hit::AboutLink(k) => {
                    a.about = false;
                    match k {
                        0 => a.page = Page::Rescue,
                        1 => {
                            if let Some(d) = bosun_core::clean::journal_dir() {
                                let _ = std::fs::create_dir_all(&d);
                                shell(&d.to_string_lossy(), "");
                            }
                        }
                        _ => {
                            shell("ms-settings:appsfeatures", "");
                        }
                    }
                    if k == 0 {
                        a.rescue = crate::pages::rescue::RescueView::load();
                    }
                }
                Hit::RescueRestore(k) => {
                    let r = a.rescue.entries.get(k).cloned().map(|e| {
                        bosun_core::clean::journal_dir()
                            .ok_or_else(|| std::io::Error::other("no journal folder"))
                            .and_then(|d| bosun_core::rescue::restore(&e, &d))
                    });
                    a.note = Some((
                        match r {
                            Some(Ok(())) => "Restored.",
                            Some(Err(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                                "Restoring this needs administrator rights."
                            }
                            _ => "Couldn't restore that change.",
                        },
                        Instant::now(),
                    ));
                    // a Turbo Boost restore changes what Speed Up shows
                    a.speed = speed_info();
                    let scroll = a.rescue.scroll;
                    a.rescue = crate::pages::rescue::RescueView::load();
                    a.rescue.scroll = scroll;
                }
                Hit::RescueBin(_) => {
                    shell("shell:RecycleBinFolder", "");
                }
                Hit::RescueFolder => {
                    if let Some(d) = bosun_core::clean::journal_dir() {
                        let _ = std::fs::create_dir_all(&d);
                        shell(&d.to_string_lossy(), "");
                    }
                }
                Hit::Card(2) => {
                    a.opt = Some(crate::pages::optimizer::Optimizer::open_drivers());
                    // SAFETY: poll timer on our own window, killed when the search ends.
                    unsafe { SetTimer(Some(hwnd), DRIVER_TIMER, 250, None) };
                }
                Hit::OptSort(k) => {
                    if let Some(o) = a.opt.as_mut() {
                        o.sort = k;
                        o.scroll = 0.0;
                    }
                }
                Hit::OptCheck(i) => {
                    if let Some(o) = a.opt.as_mut() {
                        o.checked[i] = !o.checked[i];
                    }
                }
                Hit::OptCheckAll => {
                    if let Some(o) = a.opt.as_mut() {
                        let listed = o.listed();
                        let all = listed.iter().all(|&i| o.checked[i]);
                        for i in listed {
                            o.checked[i] = !all;
                        }
                    }
                }
                Hit::OptIgnore(i) => {
                    if let Some(o) = a.opt.as_mut() {
                        o.ignored[i] = true;
                    }
                }
                Hit::OptRefresh => {
                    if let Some(o) = a.opt.as_mut() {
                        if o.finder_mode {
                            o.search_finder();
                            // SAFETY: poll timer on our own window, killed when the search ends.
                            unsafe { SetTimer(Some(hwnd), DRIVER_TIMER, 250, None) };
                        } else if o.drivers_mode {
                            o.search_drivers();
                            // SAFETY: poll timer on our own window, killed when the search ends.
                            unsafe { SetTimer(Some(hwnd), DRIVER_TIMER, 250, None) };
                        } else {
                            o.reload();
                        }
                    }
                }
                Hit::OptGo => {
                    if let Some(o) = a.opt.as_mut() {
                        let picked: Vec<usize> =
                            o.listed().into_iter().filter(|&i| o.checked[i]).collect();
                        if picked.is_empty() {
                            a.note = Some(("Tick the items to turn off first.", Instant::now()));
                        } else {
                            let failed = picked
                                .iter()
                                .filter(|&&i| set_startup(&o.items[i], false).is_err())
                                .count();
                            let done = picked.len() - failed;
                            a.note = Some((
                                if failed == 0 {
                                    "Done: those items no longer start with Windows."
                                } else if done == 0 {
                                    "Those items are for all users and need administrator rights."
                                } else {
                                    "Some items were turned off; all-user ones need administrator rights."
                                },
                                Instant::now(),
                            ));
                            o.reload();
                        }
                    }
                }
                Hit::Card(_) => {
                    a.note = Some((
                        "Turbo Boost, Driver Updates and the App Cleaner arrive later in Phase 4.",
                        Instant::now(),
                    ))
                }
                Hit::Toggle(i) => {
                    let (key, on) = match i {
                        0 => ("auto_ram_clean", !a.speed.auto_ram),
                        2 => ("auto_clean", !a.speed.auto_clean),
                        _ => ("widget_visible", !a.speed.monitor),
                    };
                    match crate::tray_ini::set(key, on) {
                        Ok(()) => {
                            a.speed = speed_info();
                            a.note = Some(("Saved.", Instant::now()));
                        }
                        Err(_) => a.note = Some(("Couldn't save the setting.", Instant::now())),
                    }
                }
                Hit::Mode(m) if a.mode != m => {
                    a.mode = m;
                    let to = if m == Mode::Manual { 1.0 } else { 0.0 };
                    a.mode_anim = Some((Instant::now(), a.manual_t, to));
                    // SAFETY: ~60 fps timer on our own window, killed when idle.
                    unsafe { SetTimer(Some(hwnd), SPIN_TIMER, 16, None) };
                }
                Hit::Pick(i) => a.picks ^= 1 << i,
                Hit::PickAll => {
                    a.picks = if a.picks == crate::scan::ALL {
                        0
                    } else {
                        crate::scan::ALL
                    }
                }
                Hit::Bell => a.bell = Some(crate::pages::actions::Actions::load(a.auto_fix)),
                Hit::BellClose => a.bell = None,
                Hit::BellToggle(_) | Hit::BellAll => {
                    if let Some(b) = a.bell {
                        let ks: Vec<usize> = match h {
                            Hit::BellToggle(k) => vec![k as usize],
                            _ => (0..b.on.len()).filter(|&k| !b.on[k]).collect(),
                        };
                        for k in ks {
                            let on = !b.on[k];
                            match crate::pages::actions::set(k, on) {
                                Ok(()) if k == 2 => a.auto_fix = on,
                                Ok(()) => {}
                                Err(e) => a.note = Some((e, Instant::now())),
                            }
                        }
                        a.bell = Some(crate::pages::actions::Actions::load(a.auto_fix));
                        a.speed = speed_info();
                    }
                }
                Hit::Scan if a.mode == Mode::Manual && a.picks == 0 => {
                    a.note = Some(("Tick at least one module to scan.", Instant::now()));
                }
                Hit::Scan => {
                    a.auto_ran = false;
                    let picks = match a.mode {
                        Mode::Recommended => crate::pages::settings::saved_picks(),
                        Mode::Manual => a.picks,
                    };
                    a.scan = Some(crate::scan::start(picks));
                    a.hover = Hit::None;
                    // SAFETY: poll timer on our own window, killed when the scan ends.
                    unsafe { SetTimer(Some(hwnd), SCAN_TIMER, 33, None) };
                }
                Hit::ScanSkip => a.scan.iter().for_each(|s| s.skip()),
                Hit::ScanStop => a.scan.iter().for_each(|s| s.stop()),
                Hit::ScanBack => a.scan = None,
                Hit::AutoFix => {
                    a.auto_fix = !a.auto_fix;
                    let _ = crate::tray_ini::set_to(crate::tray_ini::APP, "auto_fix", a.auto_fix);
                }
                Hit::ScanRow(i) => a.scan.iter().for_each(|s| s.toggle(i)),
                Hit::ScanFix => {
                    if a.scan.as_ref().is_some_and(|s| s.fix(fix_dry_run())) {
                        // SAFETY: poll timer on our own window, killed when the fix ends.
                        unsafe { SetTimer(Some(hwnd), SCAN_TIMER, 33, None) };
                    } else {
                        a.note = Some(("Tick at least one module to fix.", Instant::now()));
                    }
                }
                Hit::ModeHelp => {
                    a.note = Some((
                        "Recommended runs the safe set of checks. Manual lets you pick them.",
                        Instant::now(),
                    ))
                }
                _ => {}
            });
            invalidate(hwnd);
        }
    }
}

/// Opens a page from outside (e.g. the menu's About entry).
pub fn go(hwnd: HWND, page: Page) {
    with_app(|a| {
        if page == Page::Rescue {
            a.rescue = crate::pages::rescue::RescueView::load();
        }
        a.page = page;
    });
    invalidate(hwnd);
}

/// A ☰ panel item (see `pages::menu::ITEMS`); all but Quick Settings close it.
fn menu_item(hwnd: HWND, k: u8) {
    if k == 0 {
        return;
    }
    with_app(|a| a.menu = None);
    match k {
        1 => {
            with_app(|a| a.settings = Some(crate::pages::settings::SettingsView::open()));
        }
        2 => {
            // the tray opens its monitor when started a second time
            if let Some(t) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("bosun-tray.exe")))
            {
                shell(&t.to_string_lossy(), "");
            }
        }
        3 => open_rescue(hwnd),
        4 => {
            if let Some(d) = bosun_core::clean::journal_dir() {
                let _ = std::fs::create_dir_all(&d);
                shell(&d.to_string_lossy(), "");
            }
        }
        5 => {
            shell("ms-settings:windowsupdate", "");
        }
        6 => {
            with_app(|a| a.about = true);
        }
        // SAFETY: closing our own window ends the process.
        _ => unsafe {
            let _ = DestroyWindow(hwnd);
        },
    }
    invalidate(hwnd);
}

/// Closes any overlay (optimizer, Turbo, tools) and shows the Rescue Center.
fn open_rescue(hwnd: HWND) {
    with_app(|a| {
        a.opt = None;
        a.turbo = None;
        a.tools = false;
    });
    go(hwnd, Page::Rescue);
}

/// Hover transition length, measured from the reference recording (300 ms).
const HOVER_SECS: f32 = 0.30;

/// Easing measured from the reference: half linear, half smoothstep
/// (progress 0.07 / 0.43 / 0.94 at 11 % / 44 % / 78 % of the time).
pub fn hover_ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    0.5 * t + 0.5 * t * t * (3.0 - 2.0 * t)
}

/// Mode switch layout change, measured: ease-out over 250 ms (54 % and 91 %
/// of the way at the first two recorded frames).
const MODE_SECS: f32 = 0.25;

pub fn mode_ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

fn on_timer(hwnd: HWND) {
    let done = with_app(|a| {
        let mode_done = match a.mode_anim {
            Some((at, from, to)) => {
                let t = (at.elapsed().as_secs_f32() / MODE_SECS).min(1.0);
                a.manual_t = from + (to - from) * mode_ease(t);
                if t >= 1.0 {
                    a.mode_anim = None;
                }
                t >= 1.0
            }
            None => true,
        };
        let Some((at, from, to)) = a.anim else {
            return mode_done;
        };
        let t = (at.elapsed().as_secs_f32() / HOVER_SECS).min(1.0);
        let eased = hover_ease(t);
        a.scan_hover = from + (to - from) * eased;
        a.scan_label = crate::pages::care::label_fades(from + (to - from) * t, to > from);
        if t >= 1.0 {
            a.anim = None;
        }
        t >= 1.0 && mode_done
    });
    if done != Some(false) {
        // SAFETY: our own timer.
        let _ = unsafe { KillTimer(Some(hwnd), SPIN_TIMER) };
    }
    invalidate(hwnd);
}

extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCCALCSIZE if wp.0 != 0 => return frame::nc_calc_size(hwnd, lp),
        WM_NCHITTEST => {
            let maximized = with_app(|a| a.view().maximized).unwrap_or(false);
            return LRESULT(frame::hit_test(hwnd, lp, maximized, hit_at) as isize);
        }
        WM_GETMINMAXINFO => {
            frame::min_max(hwnd, lp);
            return LRESULT(0);
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: Begin/EndPaint pair validates the window.
            unsafe { BeginPaint(hwnd, &mut ps) };
            with_app(paint);
            // SAFETY: matches BeginPaint.
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        WM_SIZE => {
            let (w, h) = ((lp.0 & 0xFFFF) as u32, ((lp.0 >> 16) & 0xFFFF) as u32);
            with_app(|a| {
                if let Some((rt, _)) = &a.rt {
                    // SAFETY: resize our render target to the new client size.
                    let _ = unsafe {
                        rt.Resize(&D2D_SIZE_U {
                            width: w,
                            height: h,
                        })
                    };
                }
            });
            invalidate(hwnd);
        }
        WM_DPICHANGED => {
            with_app(|a| {
                a.dpi = (wp.0 & 0xFFFF) as u32;
                a.rt = None;
                a.layers = None;
            });
            frame::apply_suggested_rect(hwnd, lp);
            return LRESULT(0);
        }
        // The hover replay owns the pointer state; ignore the real mouse meanwhile.
        WM_MOUSEMOVE | WM_MOUSELEAVE | WM_NCMOUSELEAVE if scripted() => {
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            let (x, y) = client_dip(hwnd, lp);
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: requests WM_MOUSELEAVE for our window.
            let _ = unsafe { TrackMouseEvent(&mut tme) };
            set_hover(hwnd, hit_at(x, y));
            return LRESULT(0);
        }
        WM_MOUSELEAVE | WM_NCMOUSELEAVE => set_hover(hwnd, Hit::None),
        WM_NCMOUSEMOVE => {
            let h = if frame::is_max_button(wp) {
                Hit::Max
            } else {
                Hit::None
            };
            frame::track_nc_leave(hwnd);
            set_hover(hwnd, h);
        }
        WM_NCLBUTTONDOWN if frame::is_max_button(wp) => {
            with_app(|a| a.pressed = Hit::Max);
            return LRESULT(0);
        }
        WM_NCLBUTTONUP if frame::is_max_button(wp) => {
            with_app(|a| a.pressed = Hit::None);
            frame::toggle_maximize(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            let (x, y) = client_dip(hwnd, lp);
            let h = hit_at(x, y);
            with_app(|a| a.pressed = h);
            invalidate(hwnd);
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let (x, y) = client_dip(hwnd, lp);
            let h = hit_at(x, y);
            let pressed = with_app(|a| std::mem::replace(&mut a.pressed, Hit::None));
            if pressed == Some(h) && h.clickable() {
                click(hwnd, h);
            }
            invalidate(hwnd);
            return LRESULT(0);
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
            let changed = with_app(|a| {
                let h = a.size().1;
                let dy = -delta / 120.0;
                a.turbo.as_mut().map(|t| t.scroll_by(dy * 40.0)).is_some()
                    || a.opt.as_mut().map(|o| o.scroll_by(dy * 61.0, h)).is_some()
                    || (a.page == Page::Rescue && {
                        a.rescue.scroll_by(dy * 78.0, h);
                        true
                    })
                    || (a.page == Page::Updater && {
                        a.updater.scroll_by(dy * 78.0, h);
                        true
                    })
            });
            if changed == Some(true) {
                invalidate(hwnd);
            }
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            if with_app(|a| a.hover.clickable() && a.hover != Hit::Max).unwrap_or(false) {
                // SAFETY: system cursor.
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return LRESULT(1);
            }
        }
        WM_TIMER if wp.0 == SPIN_TIMER => on_timer(hwnd),
        WM_TIMER if wp.0 == DRIVER_TIMER => {
            let busy = with_app(|a| a.opt.as_mut().is_some_and(|o| o.poll_drivers()));
            if busy != Some(true) {
                // SAFETY: our own timer.
                let _ = unsafe { KillTimer(Some(hwnd), DRIVER_TIMER) };
            }
            invalidate(hwnd);
        }
        WM_TIMER if wp.0 == UPDATE_TIMER => {
            if with_app(|a| a.updater.poll()) != Some(true) {
                // SAFETY: our own timer.
                let _ = unsafe { KillTimer(Some(hwnd), UPDATE_TIMER) };
            }
            invalidate(hwnd);
        }
        WM_TIMER if wp.0 == SCAN_TIMER => {
            let done = with_app(|a| {
                let Some(p) = a.scan.as_ref().and_then(|s| s.snapshot()) else {
                    return true;
                };
                // Auto fix: go straight from the summary into Fix, once.
                if p.is_summary() && a.auto_fix && !a.auto_ran {
                    a.auto_ran = true;
                    if a.scan.as_ref().is_some_and(|s| s.fix(fix_dry_run())) {
                        return false;
                    }
                }
                p.finished
            });
            if done != Some(false) {
                // SAFETY: our own timer.
                let _ = unsafe { KillTimer(Some(hwnd), SCAN_TIMER) };
            }
            invalidate(hwnd);
        }
        WM_TIMER if wp.0 == SCRIPT_MODE => {
            // SAFETY: one-shot test-hook timer on our own window.
            let _ = unsafe { KillTimer(Some(hwnd), wp.0) };
            click(hwnd, Hit::Mode(Mode::Manual));
        }
        WM_REQUEST => {
            apply_request(hwnd, Request::decode(wp.0));
            return LRESULT(0);
        }
        WM_TIMER if wp.0 == SCRIPT_SCAN => {
            // SAFETY: one-shot test-hook timer on our own window.
            let _ = unsafe { KillTimer(Some(hwnd), wp.0) };
            click(hwnd, Hit::Scan);
        }
        WM_TIMER if wp.0 == SCRIPT_IN || wp.0 == SCRIPT_OUT => {
            // SAFETY: one-shot test-hook timer on our own window.
            let _ = unsafe { KillTimer(Some(hwnd), wp.0) };
            set_hover(
                hwnd,
                if wp.0 == SCRIPT_IN {
                    Hit::Scan
                } else {
                    Hit::None
                },
            );
        }
        WM_DESTROY => {
            // SAFETY: ends the message loop; the process exits.
            unsafe { PostQuitMessage(0) };
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Screen point → client DIPs (for non-client messages).
pub fn screen_to_dip(hwnd: HWND, pt: POINT) -> (f32, f32) {
    let mut p = pt;
    // SAFETY: our own window.
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
        let dpi = GetDpiForWindow(hwnd).max(96);
        (gfx::dip(p.x, dpi), gfx::dip(p.y, dpi))
    }
}

/// Turns a startup entry on or off, journaled; the message is for the user.
fn set_startup(
    item: &bosun_core::startup::Item,
    on: bool,
) -> std::result::Result<(), &'static str> {
    let dir = bosun_core::clean::journal_dir().ok_or("No place to write the journal.")?;
    bosun_core::startup::set_enabled(item, on, &dir).map_err(|_| {
        if item.source.machine_wide() {
            "That item is for all users and needs administrator rights."
        } else {
            "Couldn't change that item."
        }
    })
}

/// A menu of `labels` under the cursor, `current` ticked; the chosen index
/// when it differs from `current`.
fn choice_menu(hwnd: HWND, labels: &[&str], current: usize) -> Option<usize> {
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_CHECKED, MF_STRING,
        TPM_RETURNCMD, TrackPopupMenu,
    };
    // SAFETY: a popup menu owned by our window, destroyed after use.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (k, l) in labels.iter().enumerate() {
            let flags = if k == current {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let _ = AppendMenuW(menu, flags, k + 1, &windows::core::HSTRING::from(*l));
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let c = TrackPopupMenu(menu, TPM_RETURNCMD, pt.x, pt.y, None, hwnd, None).0;
        let _ = DestroyMenu(menu);
        let picked = usize::try_from(c).ok()?.checked_sub(1)?;
        (picked != current).then_some(picked)
    }
}

/// The status menu for row `i` on the optimizer's current tab, applied.
fn option_status(hwnd: HWND, i: usize) {
    use bosun_core::services::Start;
    let Some(tab) = with_app(|a| a.opt.as_ref().map(|o| o.tab)).flatten() else {
        return;
    };
    let dir = bosun_core::clean::journal_dir();
    let admin = "That needs administrator rights.";
    let msg: Option<&'static str> = match tab {
        2 => {
            const STARTS: [Start; 3] = [Start::Automatic, Start::Manual, Start::Disabled];
            let svc = with_app(|a| {
                a.opt
                    .as_ref()
                    .and_then(|o| o.services.as_ref().map(|v| v[i].clone()))
            })
            .flatten();
            let Some(svc) = svc else { return };
            let cur = STARTS
                .iter()
                .position(|s| *s == svc.start)
                .unwrap_or(usize::MAX);
            let Some(k) = choice_menu(hwnd, &["Automatic", "Manual", "Disabled"], cur) else {
                return;
            };
            match dir.map(|d| bosun_core::services::set_start(&svc, STARTS[k], &d)) {
                Some(Ok(())) => None,
                _ => Some(admin),
            }
        }
        3 => {
            let task = with_app(|a| {
                a.opt
                    .as_ref()
                    .and_then(|o| o.tasks.as_ref().map(|v| v[i].clone()))
            })
            .flatten();
            let Some(task) = task else { return };
            let Some(k) = choice_menu(hwnd, &["Enabled", "Disabled"], usize::from(!task.enabled))
            else {
                return;
            };
            match dir.map(|d| bosun_core::tasks::set_enabled(&task, k == 0, &d)) {
                Some(Ok(())) => None,
                _ => Some("That task belongs to another account and needs administrator rights."),
            }
        }
        _ => {
            let item = with_app(|a| a.opt.as_ref().map(|o| o.items[i].clone())).flatten();
            let Some(item) = item else { return };
            let Some(k) = choice_menu(hwnd, &["Enabled", "Disabled"], usize::from(!item.enabled))
            else {
                return;
            };
            set_startup(&item, k == 0).err()
        }
    };
    with_app(|a| {
        if let Some(m) = msg {
            a.note = Some((m, Instant::now()));
        }
        if let Some(o) = a.opt.as_mut() {
            let scroll = o.scroll;
            o.reload();
            o.tab = tab;
            o.ensure();
            o.scroll = scroll;
        }
    });
}

impl View {
    /// A full-window view (Startup Optimizer, Turbo Boost) replaces the
    /// sidebar and title.
    pub fn overlay(&self) -> bool {
        self.opt.is_some() || self.turbo.is_some() || self.tools
    }
}

/// Opens `target` (a program, URI or folder) with optional arguments.
fn shell(target: &str, args: &str) -> bool {
    // SAFETY: plain ShellExecute with owned strings.
    let r = unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            None,
            w!("open"),
            &windows::core::HSTRING::from(target),
            &windows::core::HSTRING::from(args),
            None,
            SW_SHOWNORMAL,
        )
    };
    r.0 as isize > 32
}

/// Runs a More Optimization Tools entry (ids from `pages::tools::COLUMNS`).
fn tool(hwnd: HWND, id: u8) {
    use crate::pages::optimizer::Optimizer;
    let scan = |picks: u16| {
        with_app(|a| {
            a.tools = false;
            a.opt = None;
            a.turbo = None;
            a.page = Page::Care;
            a.auto_ran = false;
            a.scan = Some(crate::scan::start(picks));
        });
        // SAFETY: poll timer on our own window, killed when the scan ends.
        unsafe { SetTimer(Some(hwnd), SCAN_TIMER, 33, None) };
    };
    let overlay = |o: Optimizer, poll: bool| {
        with_app(|a| {
            a.tools = false;
            a.opt = Some(o);
        });
        if poll {
            // SAFETY: poll timer on our own window, killed when the search ends.
            unsafe { SetTimer(Some(hwnd), DRIVER_TIMER, 250, None) };
        }
    };
    let ok = match id {
        0 => {
            overlay(Optimizer::open(0), false);
            true
        }
        1 => {
            with_app(|a| {
                a.tools = false;
                a.turbo = Some(crate::pages::turbo::Turbo::open(a.speed.turbo_mode));
            });
            true
        }
        2 => {
            overlay(Optimizer::open_drivers(), true);
            true
        }
        3 => {
            overlay(Optimizer::open_apps(), false);
            true
        }
        4 => std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("bosun-tray.exe")))
            .is_some_and(|t| shell(&t.to_string_lossy(), "")),
        5 => shell("msinfo32.exe", ""),
        6 => shell("control.exe", "powercfg.cpl"),
        7 => shell("ms-settings:defaultapps", ""),
        8 => shell("windowsdefender:", ""),
        9 => {
            open_rescue(hwnd);
            true
        }
        // Care scans limited to one module (bits = scan::MODULES order)
        10 => {
            scan(1 << 3);
            true
        }
        11 => shell("ms-settings:storagesense", ""),
        12 => {
            scan(0b11);
            true
        }
        13 => {
            overlay(Optimizer::open_finder(8), true);
            true
        }
        14 => {
            scan(1 << 2);
            true
        }
        _ => {
            overlay(Optimizer::open_finder(7), true);
            true
        }
    };
    if !ok {
        with_app(|a| a.note = Some(("Couldn't open that tool.", Instant::now())));
    }
}

/// Turns Turbo Boost on (with `plan`, or the recommended one) or off; the
/// message is for the user.
fn turbo_toggle(
    on: bool,
    mode: bosun_core::turbo::Mode,
    plan: Option<bosun_core::turbo::Plan>,
) -> &'static str {
    let Some(dir) = bosun_core::clean::journal_dir() else {
        return "No place to write the journal.";
    };
    if on {
        return match bosun_core::turbo::restore(&dir) {
            Ok(r) if r.services_failed > 0 => {
                "Turbo Boost is off; some services need administrator rights to restart."
            }
            Ok(_) => "Turbo Boost is off: your power plan and services are back.",
            Err(_) => "Couldn't turn Turbo Boost off.",
        };
    }
    let plan = plan.unwrap_or_else(|| bosun_core::turbo::recommended(mode));
    match bosun_core::turbo::boost(&plan, &dir) {
        Ok(r) if r.services_failed > 0 && r.services_done == 0 && !plan.services.is_empty() => {
            "Turbo Boost is on (power plan); stopping services needs administrator rights."
        }
        Ok(r) if r.services_failed > 0 => {
            "Turbo Boost is on; some services need administrator rights."
        }
        Ok(r) if r.apps_failed > 0 => "Turbo Boost is on; some apps couldn't be closed.",
        Ok(_) => "Turbo Boost is on.",
        Err(_) => "Couldn't turn Turbo Boost on.",
    }
}

/// Startup count and tray switches for the Speed Up page.
fn speed_info() -> crate::pages::speedup::SpeedInfo {
    crate::pages::speedup::SpeedInfo {
        optimizable: bosun_core::startup::optimizable(&bosun_core::startup::list()),
        auto_ram: crate::tray_ini::get("auto_ram_clean", false),
        auto_clean: crate::tray_ini::get("auto_clean", false),
        monitor: crate::tray_ini::get("widget_visible", true),
        turbo_on: bosun_core::turbo::State::load().on,
        turbo_mode: bosun_core::turbo::State::load().mode,
    }
}

/// A default-size Care view for layout tests.
#[cfg(test)]
pub fn test_view() -> View {
    idle_view()
}

/// The Care page at rest at the default size (tests, skin previews).
pub fn idle_view() -> View {
    View {
        page: Page::Care,
        w: DEFAULT_W,
        h: DEFAULT_H,
        hover: Hit::None,
        pressed: Hit::None,
        mode: Mode::Recommended,
        scan_hover: 0.0,
        scan_label: (1.0, 0.0),
        scan: None,
        auto_fix: false,
        picks: crate::scan::ALL,
        manual_t: 0.0,
        speed: Default::default(),
        opt: None,
        turbo: None,
        tools: false,
        protect: Default::default(),
        updater: None,
        rescue: None,
        menu: None,
        about: false,
        settings: None,
        bell: None,
        note: None,
        maximized: false,
    }
}

#[cfg(test)]
mod tests {
    use super::hover_ease;

    #[test]
    fn launch_requests_survive_the_trip() {
        use super::{Page, Request};
        for r in [
            Request::default(),
            Request {
                page: Some(Page::Rescue),
                ..Default::default()
            },
            Request {
                page: Some(Page::Care),
                scan: true,
                settings: false,
            },
            Request {
                page: None,
                scan: false,
                settings: true,
            },
        ] {
            assert_eq!(Request::decode(r.encode()), r);
        }
    }

    #[test]
    fn mode_easing_tracks_the_reference_samples() {
        // 54 % and 91 % at the first two recorded frames (≈ 0.33 and 0.73 of 250 ms)
        assert!((super::mode_ease(0.33) - 0.54).abs() < 0.06);
        assert!((super::mode_ease(0.73) - 0.91).abs() < 0.05);
        assert_eq!(super::mode_ease(1.0), 1.0);
    }

    #[test]
    fn hover_easing_tracks_the_reference_samples() {
        // (fraction of 300 ms, measured progress) from .reanimate/scan-hover/spec.md
        for (t, want) in [
            (0.11, 0.07),
            (0.22, 0.16),
            (0.33, 0.28),
            (0.44, 0.43),
            (0.78, 0.94),
        ] {
            let got = hover_ease(t);
            // one recorded frame (33 ms) at the steepest slope ≈ 0.15 of progress
            assert!((got - want).abs() < 0.12, "t={t} got {got} want {want}");
        }
        assert_eq!(hover_ease(0.0), 0.0);
        assert_eq!(hover_ease(1.0), 1.0);
    }
}
