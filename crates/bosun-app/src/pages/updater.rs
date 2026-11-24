//! Software Updater page, measured from the reference (`.reanimate/phase5`,
//! scaled from 150 %): the Speed Up banner with "Update All" (154 × 34 at
//! (780, 70)); rows 757 × 71, 78 px apart, centred from y 169; a 31 px icon
//! at x 199, the name at x 253, "Current / New Version" at x 508 (± 8 px),
//! an Update button 114 × 31 at x 802, and "Ignore" 24 px below it on hover.
//!
//! Updates come from winget, one package at a time (spec §2.4).

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bosun_core::updater::Update;
use bosun_ui::gfx::{Align, Painter, col, cola, contains, rect};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::UI::WindowsAndMessaging::HICON;

use crate::pages::speedup::{pill, text_at};
use crate::window::{Hit, View};

const ROW_X: f32 = 175.0;
const ROW_H: f32 = 71.0;
const PITCH: f32 = 78.0;
const FIRST: f32 = 169.0;
const LIST_TOP: f32 = 128.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum St {
    Idle,
    Queued,
    Updating,
    Done,
    Failed,
}

#[derive(Clone, PartialEq, Debug)]
pub struct URow {
    pub idx: usize,
    pub name: String,
    pub current: String,
    pub available: String,
    pub st: St,
    pub icon: isize,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct UpdView {
    pub loading: bool,
    pub rows: Vec<URow>,
    pub scroll: f32,
    pub busy: bool,
}

/// The page's state: the winget list, per-row status, and the update queue.
#[derive(Default)]
pub struct Updater {
    list: Option<Vec<Update>>,
    /// winget's list and each row's icon (an HICON as `isize`, 0 for none).
    rx: Option<Receiver<(Vec<Update>, Vec<isize>)>>,
    icons: Vec<HICON>,
    st: Vec<St>,
    ignored: Vec<bool>,
    pub scroll: f32,
    queue: Vec<usize>,
    job: Option<(usize, Receiver<bool>)>,
}

/// The installed app a winget row is about: the same name (any case), else
/// one name continuing the other at a word break ("Audacity" ~ "Audacity
/// 3.7.8"). Never a bare prefix ("Go" is not "Google Chrome") or an empty name.
fn same_app(winget: &str, installed: &str) -> bool {
    let (a, b) = (
        winget.trim().to_lowercase(),
        installed.trim().to_lowercase(),
    );
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    short == long
        || (short.len() >= 3
            && long.starts_with(short.as_str())
            && long[short.len()..].starts_with([' ', '(', '-']))
}

fn icon_for(name: &str, apps: &[bosun_core::apps::App]) -> HICON {
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    use windows::core::HSTRING;
    let exact = apps
        .iter()
        .find(|a| a.name.trim().eq_ignore_ascii_case(name.trim()));
    let Some(path) = exact
        .or_else(|| apps.iter().find(|a| same_app(name, &a.name)))
        .and_then(|a| a.icon.clone())
        .filter(|p| p.exists())
    else {
        return HICON::default();
    };
    let mut info = SHFILEINFOW::default();
    // SAFETY: fills `info`; the icon is destroyed with the page state.
    unsafe {
        SHGetFileInfoW(
            &HSTRING::from(path.as_os_str()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
    }
    info.hIcon
}

impl Updater {
    /// Starts asking winget (slow) unless a list is loaded or loading.
    pub fn load(&mut self) {
        if self.list.is_some() || self.rx.is_some() {
            return;
        }
        let (tx, rx) = channel();
        // winget, the installed-app scan and the icons all stay off the UI thread
        std::thread::spawn(move || {
            let v = bosun_core::updater::list().unwrap_or_default();
            let apps = bosun_core::apps::list();
            let icons = v
                .iter()
                .map(|u| icon_for(&u.name, &apps).0 as isize)
                .collect();
            let _ = tx.send((v, icons));
        });
        self.rx = Some(rx);
    }

    fn free_icons(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        for i in self.icons.drain(..) {
            if !i.is_invalid() {
                // SAFETY: icons we got from SHGetFileInfoW and own.
                unsafe {
                    let _ = DestroyIcon(i);
                }
            }
        }
    }

    /// Picks up the list and finished updates, starts the next queued one;
    /// `true` while anything is still running.
    pub fn poll(&mut self) -> bool {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok((v, icons)) => {
                    self.icons = icons.into_iter().map(|i| HICON(i as _)).collect();
                    self.st = vec![St::Idle; v.len()];
                    let ignored = bosun_core::ignore::keys(bosun_core::ignore::List::Update);
                    self.ignored = v.iter().map(|u| ignored.contains(&u.id)).collect();
                    self.list = Some(v);
                    self.rx = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.list = Some(Vec::new());
                    self.rx = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((i, rx)) = &self.job {
            match rx.try_recv() {
                Ok(ok) => {
                    self.st[*i] = if ok { St::Done } else { St::Failed };
                    self.job = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.st[*i] = St::Failed;
                    self.job = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.job.is_none()
            && let Some(i) = (!self.queue.is_empty()).then(|| self.queue.remove(0))
        {
            let id = self
                .list
                .as_ref()
                .map(|v| v[i].id.clone())
                .unwrap_or_default();
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(bosun_core::updater::update(&id).unwrap_or(false));
            });
            self.st[i] = St::Updating;
            self.job = Some((i, rx));
        }
        self.rx.is_some() || self.job.is_some() || !self.queue.is_empty()
    }

    /// Queues package `i` (or every pending one).
    pub fn enqueue(&mut self, i: Option<usize>) {
        let n = self.st.len();
        let pick: Vec<usize> = match i {
            Some(i) => vec![i],
            None => (0..n).filter(|&k| !self.ignored[k]).collect(),
        };
        for k in pick {
            if matches!(self.st[k], St::Idle | St::Failed) {
                self.st[k] = St::Queued;
                self.queue.push(k);
            }
        }
    }

    /// Hides update `i` now and on later checks (Settings > Ignore List).
    pub fn ignore(&mut self, i: usize) {
        if let Some(x) = self.ignored.get_mut(i) {
            *x = true;
        }
        if let Some(u) = self.list.as_ref().and_then(|v| v.get(i)) {
            let _ = bosun_core::ignore::add(bosun_core::ignore::List::Update, &u.id, &u.name);
        }
    }

    pub fn view(&self) -> UpdView {
        let rows = self
            .list
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.ignored[*i])
            .map(|(i, u)| URow {
                idx: i,
                name: u.name.clone(),
                current: u.current.clone(),
                available: u.available.clone(),
                st: self.st[i],
                icon: self.icons[i].0 as isize,
            })
            .collect();
        UpdView {
            loading: self.rx.is_some(),
            rows,
            scroll: self.scroll,
            busy: self.job.is_some() || !self.queue.is_empty(),
        }
    }

    pub fn scroll_by(&mut self, dy: f32, h: f32) {
        let n = self.view().rows.len() as f32;
        let max = (n * PITCH - (h - LIST_TOP - 10.0)).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.free_icons();
    }
}

fn update_all(v: &View) -> D2D_RECT_F {
    rect(v.w - 26.0 - 154.0, 70.0, 154.0, 34.0)
}

fn row_rect(v: &View, u: &UpdView, k: usize) -> D2D_RECT_F {
    let cy = FIRST + PITCH * k as f32 - u.scroll;
    rect(ROW_X, cy - ROW_H / 2.0, v.w - 28.0 - ROW_X, ROW_H)
}

fn button(r: &D2D_RECT_F) -> D2D_RECT_F {
    let cy = (r.top + r.bottom) / 2.0;
    rect(r.right - 16.0 - 114.0, cy - 15.5, 114.0, 31.0)
}

pub fn hit(v: &View, x: f32, y: f32) -> Hit {
    let Some(u) = &v.updater else {
        return Hit::None;
    };
    if contains(&update_all(v), x, y) && !u.rows.is_empty() {
        return Hit::UpdAll;
    }
    if y < LIST_TOP {
        return Hit::None;
    }
    for (k, r) in u.rows.iter().enumerate() {
        let rr = row_rect(v, u, k);
        if !contains(
            &rect(rr.left, rr.top, rr.right - rr.left, ROW_H + 7.0),
            x,
            y,
        ) {
            continue;
        }
        let b = button(&rr);
        if contains(&b, x, y) {
            return Hit::UpdOne(r.idx);
        }
        if contains(&rect(b.left + 30.0, b.bottom + 1.0, 60.0, 16.0), x, y) {
            return Hit::UpdIgnore(r.idx);
        }
        return Hit::UpdRow(r.idx);
    }
    Hit::None
}

pub fn draw(p: &Painter, v: &View) {
    let f = &p.g.f;
    let Some(u) = &v.updater else { return };
    let n = u.rows.iter().filter(|r| r.st != St::Done).count();
    if u.loading {
        crate::pages::protect::banner(
            p,
            false,
            "Checking for outdated software…",
            None,
            "Asking winget which of your apps have updates.",
        );
    } else if n == 0 {
        crate::pages::protect::banner(
            p,
            true,
            "All your software is up to date.",
            None,
            "winget found nothing newer to install.",
        );
    } else {
        let count = n.to_string();
        crate::pages::protect::banner(
            p,
            false,
            "outdated software",
            Some(&count),
            "Update them one by one, or all at once with \"Update All\".",
        );
        let label = if u.busy { "Updating…" } else { "Update All" };
        pill(p, update_all(v), label, v.hover == Hit::UpdAll);
    }
    p.clip(rect(ROW_X, LIST_TOP, v.w - ROW_X, v.h - LIST_TOP));
    for (k, r) in u.rows.iter().enumerate() {
        let rr = row_rect(v, u, k);
        if rr.bottom < LIST_TOP || rr.top > v.h {
            continue;
        }
        let hot =
            matches!(v.hover, Hit::UpdRow(i) | Hit::UpdOne(i) | Hit::UpdIgnore(i) if i == r.idx);
        p.fill_rr(rr, 6.0, col(if hot { 0x222222 } else { 0x1C1C1C }));
        let cy = (rr.top + rr.bottom) / 2.0;
        let ir = rect(199.0, cy - 15.5, 31.0, 31.0);
        let bmp = (r.icon != 0)
            .then(|| p.g.icon_bitmap(p.rt, HICON(r.icon as _)))
            .flatten();
        match bmp {
            Some(b) => p.bitmap(&b, ir, 1.0),
            None => p.stroke_rr(rect(203.0, cy - 11.5, 23.0, 23.0), 4.0, 1.6, col(0x5A5A5A)),
        }
        text_at(p, &r.name, &f.m14sb, 15.0, 253.0, 240.0, cy + 0.5, 0xF2F2F2);
        text_at(
            p,
            &format!("Current Version: {}", r.current),
            &f.m12,
            12.0,
            508.0,
            270.0,
            cy - 8.5,
            0xB0B0B0,
        );
        text_at(
            p,
            &format!("New Version: {}", r.available),
            &f.m12,
            12.0,
            508.0,
            270.0,
            cy + 7.5,
            0xB0B0B0,
        );
        let b = button(&rr);
        match r.st {
            St::Idle | St::Failed => {
                pill(p, b, "Update", v.hover == Hit::UpdOne(r.idx));
                if r.st == St::Failed {
                    p.text(
                        "Failed",
                        &f.m12,
                        rect(b.left - 80.0, b.top, 70.0, 31.0),
                        col(0xF5A56A),
                        Align::Right,
                    );
                }
                if hot && r.st == St::Idle {
                    let c = if v.hover == Hit::UpdIgnore(r.idx) {
                        0xFFFFFF
                    } else {
                        0xD0D0D0
                    };
                    let w = p.g.measure("Ignore", &f.m12);
                    let x = (b.left + b.right) / 2.0 - w / 2.0;
                    text_at(p, "Ignore", &f.m12, 12.0, x, w + 4.0, b.bottom + 9.0, c);
                    p.line(x, b.bottom + 15.5, x + w, b.bottom + 15.5, 1.0, col(c));
                }
            }
            St::Queued | St::Updating | St::Done => {
                let (t, c) = match r.st {
                    St::Queued => ("Queued", 0xB0B0B0),
                    St::Updating => ("Updating…", 0x61ADF8),
                    _ => ("Updated", 0x61ADF8),
                };
                p.text(t, &f.m13, b, col(c), Align::Center);
            }
        }
    }
    p.unclip();
    if !u.loading && u.rows.is_empty() {
        p.text(
            "Nothing to update.",
            &f.m13,
            rect(ROW_X, 300.0, v.w - ROW_X - 28.0, 20.0),
            col(0x8A8A8A),
            Align::Center,
        );
    }
    let _ = cola;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_installed_apps_without_grabbing_lookalikes() {
        assert!(same_app("VLC media player", "vlc media player"));
        assert!(same_app("Audacity", "Audacity 3.7.8"));
        assert!(same_app("Zoom Workplace (64-bit)", "Zoom Workplace"));
        assert!(!same_app("Go", "Google Chrome"));
        assert!(!same_app("Git", "GitHub Desktop"));
        assert!(!same_app("Blender", ""));
    }

    #[test]
    fn rows_and_buttons_sit_where_the_reference_has_them() {
        let v = crate::window::test_view();
        let u = UpdView {
            rows: Vec::new(),
            ..Default::default()
        };
        let r = row_rect(&v, &u, 0);
        assert_eq!(
            (r.left, r.right, (r.top + r.bottom) / 2.0),
            (175.0, 932.0, 169.0)
        );
        let b = button(&r);
        assert_eq!((b.left, b.right), (802.0, 916.0));
        let a = update_all(&v);
        assert_eq!((a.left, a.top, a.right), (780.0, 70.0, 934.0));
    }
}
