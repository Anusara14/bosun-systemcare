//! Custom window frame: our own title bar drawn in the client area, while
//! Windows keeps resize borders, the drop shadow, rounded corners, Aero Snap
//! and (via `HTMAXBUTTON`) the Windows 11 snap-layouts flyout.

use bosun_ui::gfx;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTMAXBUTTON, HTRIGHT,
    HTTOP, HTTOPLEFT, HTTOPRIGHT, IsZoomed, MINMAXINFO, NCCALCSIZE_PARAMS, SM_CXFRAME,
    SM_CXPADDEDBORDER, SM_CYFRAME, SW_MAXIMIZE, SW_RESTORE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOZORDER, SetWindowPos, ShowWindow,
};

use crate::chrome::TITLE_H;
use crate::window::{DEFAULT_H, DEFAULT_W, Hit, MIN_H, MIN_W, screen_to_dip};

/// Resize grip inside our client area, in DIPs.
const GRIP: f32 = 6.0;

/// Sizes and centres the window, turns on dark/rounded DWM styling and
/// recalculates the frame so our client area covers the old title bar.
pub fn init(hwnd: HWND, dpi: u32) {
    let (w, h) = (gfx::px(DEFAULT_W, dpi), gfx::px(DEFAULT_H, dpi));
    let wa = bosun_work_area();
    let (x, y) = (
        wa.left + (wa.right - wa.left - w) / 2,
        wa.top + (wa.bottom - wa.top - h) / 2,
    );
    // SAFETY: DWM attributes and positioning on our own window.
    unsafe {
        let dark: i32 = 1;
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const _ as _,
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as _,
            4,
        );
        // A 1 px sliver of frame keeps the DWM shadow on Windows 10.
        let m = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1,
            cyBottomHeight: 0,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_FRAMECHANGED);
    }
}

fn bosun_work_area() -> RECT {
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW};
    let mut r = RECT::default();
    // SAFETY: SPI_GETWORKAREA fills a RECT.
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut r as *mut _ as _),
            Default::default(),
        )
    };
    r
}

fn frame_px(hwnd: HWND) -> (i32, i32) {
    // SAFETY: plain metric queries for our window's DPI.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let pad = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
        (
            GetSystemMetricsForDpi(SM_CXFRAME, dpi) + pad,
            GetSystemMetricsForDpi(SM_CYFRAME, dpi) + pad,
        )
    }
}

/// Client area = whole window. When maximized, Windows hangs the frame off
/// screen, so we pull the client back in by the frame thickness.
pub fn nc_calc_size(hwnd: HWND, lp: LPARAM) -> LRESULT {
    // SAFETY: for WM_NCCALCSIZE with wParam = TRUE, lParam is NCCALCSIZE_PARAMS.
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            let p = &mut *(lp.0 as *mut NCCALCSIZE_PARAMS);
            let (fx, fy) = frame_px(hwnd);
            let r = &mut p.rgrc[0];
            r.left += fx;
            r.right -= fx;
            r.top += fy;
            r.bottom -= fy;
        }
    }
    LRESULT(0)
}

/// Pure hit-test rule, separated for testing.
pub fn classify(x: f32, y: f32, w: f32, h: f32, maximized: bool, inner: Hit) -> u32 {
    if !maximized {
        let (l, r, t, b) = (x < GRIP, x >= w - GRIP, y < GRIP, y >= h - GRIP);
        match (l, r, t, b) {
            (true, _, true, _) => return HTTOPLEFT,
            (_, true, true, _) => return HTTOPRIGHT,
            (true, _, _, true) => return HTBOTTOMLEFT,
            (_, true, _, true) => return HTBOTTOMRIGHT,
            (true, ..) => return HTLEFT,
            (_, true, ..) => return HTRIGHT,
            (_, _, true, _) => return HTTOP,
            (.., true) => return HTBOTTOM,
            _ => {}
        }
    }
    match inner {
        Hit::Max => HTMAXBUTTON,
        Hit::None if y < TITLE_H => HTCAPTION,
        _ => HTCLIENT,
    }
}

pub fn hit_test(hwnd: HWND, lp: LPARAM, maximized: bool, inner: impl Fn(f32, f32) -> Hit) -> u32 {
    let pt = POINT {
        x: (lp.0 & 0xFFFF) as i16 as i32,
        y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32,
    };
    let (x, y) = screen_to_dip(hwnd, pt);
    let mut rc = RECT::default();
    // SAFETY: our own window.
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc) };
    // SAFETY: our own window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let (w, h) = (gfx::dip(rc.right, dpi), gfx::dip(rc.bottom, dpi));
    classify(x, y, w, h, maximized, inner(x, y))
}

pub fn is_max_button(wp: WPARAM) -> bool {
    wp.0 as u32 == HTMAXBUTTON
}

pub fn track_nc_leave(hwnd: HWND) {
    let mut tme = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE | TME_NONCLIENT,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: requests WM_NCMOUSELEAVE for our window.
    let _ = unsafe { TrackMouseEvent(&mut tme) };
}

pub fn toggle_maximize(hwnd: HWND) {
    // SAFETY: our own window.
    unsafe {
        let cmd = if IsZoomed(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_MAXIMIZE
        };
        let _ = ShowWindow(hwnd, cmd);
    }
}

pub fn min_max(hwnd: HWND, lp: LPARAM) {
    // SAFETY: our own window; lParam is MINMAXINFO for WM_GETMINMAXINFO.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let mmi = &mut *(lp.0 as *mut MINMAXINFO);
        mmi.ptMinTrackSize = POINT {
            x: gfx::px(MIN_W, dpi),
            y: gfx::px(MIN_H, dpi),
        };
    }
}

pub fn apply_suggested_rect(hwnd: HWND, lp: LPARAM) {
    // SAFETY: for WM_DPICHANGED, lParam points at the suggested RECT.
    unsafe {
        let r = &*(lp.0 as *const RECT);
        let flags = SWP_NOZORDER | SWP_NOACTIVATE;
        let _ = SetWindowPos(
            hwnd,
            None,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            flags,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_resize_only_when_restored() {
        assert_eq!(classify(2.0, 300.0, 960.0, 660.0, false, Hit::None), HTLEFT);
        assert_eq!(
            classify(958.0, 2.0, 960.0, 660.0, false, Hit::None),
            HTTOPRIGHT
        );
        assert_eq!(
            classify(2.0, 300.0, 960.0, 660.0, true, Hit::None),
            HTCLIENT
        );
    }

    #[test]
    fn title_bar_drags_and_max_button_snaps() {
        assert_eq!(
            classify(500.0, 20.0, 960.0, 660.0, false, Hit::None),
            HTCAPTION
        );
        assert_eq!(
            classify(903.0, 20.0, 960.0, 660.0, false, Hit::Max),
            HTMAXBUTTON
        );
        assert_eq!(
            classify(940.0, 20.0, 960.0, 660.0, false, Hit::Close),
            HTCLIENT
        );
        assert_eq!(
            classify(500.0, 300.0, 960.0, 660.0, false, Hit::None),
            HTCLIENT
        );
    }
}
