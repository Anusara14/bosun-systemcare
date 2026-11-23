//! Footer: Clean RAM, Scan, Turbo Boost, Screenshot (+ menu), End Task.

use super::*;

/// Footer tools at the reference positions (centres x = 24, 54, 86, 118 + caret 134).
pub(super) fn footer_tools() -> [(Hit, D2D_RECT_F); 5] {
    let y = FOOT_CY - 13.0;
    [
        (Hit::Clean, rect(12.0, y, 24.0, 26.0)),
        (Hit::Scan, rect(42.0, y, 24.0, 26.0)),
        (Hit::Turbo, rect(73.0, y, 26.0, 26.0)),
        (Hit::Shot, rect(105.0, y, 24.0, 26.0)),
        (Hit::ShotMenu, rect(129.0, y, 11.0, 26.0)),
    ]
}

pub(super) fn end_rect() -> D2D_RECT_F {
    let (x, y, w, h) = END_TASK;
    rect(x, y, w, h)
}

pub(super) fn can_end(m: &Monitor) -> bool {
    m.tab.is_process() && !m.checked.is_empty()
}

pub(super) fn tool_tip(h: &Hit, has_main: bool, turbo: bool) -> Option<&'static str> {
    Some(match h {
        Hit::Clean => "Clean RAM",
        Hit::Scan if has_main => "Scan with Bosun SystemCare",
        Hit::Scan => "Scan (available with the full app)",
        Hit::Turbo if turbo => "Turbo Boost: ON (click to turn off)",
        Hit::Turbo => "Turbo Boost: OFF (click to turn on)",
        Hit::Shot => "Screenshot",
        Hit::ShotMenu => "Screenshot options",
        _ => return None,
    })
}

pub(super) fn draw_footer(p: &Painter, m: &Monitor) {
    let f = &p.g.f;
    let has_main = shell::main_app_path().is_some();
    let turbo = m.turbo;
    for (hit, r) in footer_tools() {
        let enabled = hit != Hit::Scan || has_main;
        let base = match hit {
            Hit::Turbo if turbo => mc::ACCENT,
            _ => mc::ICON,
        };
        let c = col(match (enabled, enabled && m.hover == hit) {
            (false, _) => 0x4A4A4A,
            (true, true) => mc::TEXT,
            (true, false) => base,
        });
        let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        match hit {
            Hit::Clean => p.rocket(cx, cy + 1.0, c, col(mc::BG), 0.0),
            Hit::Scan => p.glyph(glyph::SEARCH, &f.icon, r, c),
            Hit::Turbo => p.glyph(glyph::SPEED, &f.icon20, r, c),
            Hit::Shot => p.glyph(glyph::CROP, &f.icon, r, c),
            Hit::ShotMenu => p.caret_down(cx, cy + 1.0, 7.0, c),
            _ => {}
        }
    }
    let toast = m
        .toast
        .as_ref()
        .filter(|(_, at)| at.elapsed() < Duration::from_secs(4));
    let tip = match toast {
        Some((t, _)) => Some((t.as_str(), mc::TEXT)),
        None => tool_tip(&m.hover, has_main, turbo).map(|t| (t, mc::TEXT_3)),
    };
    if let Some((t, c)) = tip {
        p.text(
            t,
            &f.caption,
            rect(152.0, FOOT_CY - 10.0, 220.0, 20.0),
            col(c),
            Align::Left,
        );
    }
    let on = can_end(m);
    let bg = match (on, m.hover == Hit::EndTask) {
        (true, true) => cola(mc::ACCENT, 0.9),
        (true, false) => col(mc::ACCENT),
        _ => col(mc::BTN_OFF),
    };
    p.fill_rr(end_rect(), 3.0, bg);
    let tc = col(if on { 0x000000 } else { 0x6E6E6E });
    p.text("End Task", &f.m13, end_rect(), tc, Align::Center);
}

/// Dropdown next to the screenshot tool.
pub(super) fn shot_menu(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        TrackPopupMenu,
    };
    let mut pt = POINT::default();
    // SAFETY: popup menu owned by our window, destroyed after use.
    let choice = unsafe {
        let _ = GetCursorPos(&mut pt);
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Capture a region"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Open Snipping Tool"));
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON;
        let c = TrackPopupMenu(menu, flags, pt.x, pt.y, None, hwnd, None).0;
        let _ = DestroyMenu(menu);
        c
    };
    match choice {
        1 => {
            post(Cmd::HideMonitor);
            tools::screenshot();
        }
        2 => {
            post(Cmd::HideMonitor);
            tools::snipping_tool();
        }
        _ => {}
    }
}
