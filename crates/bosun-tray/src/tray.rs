//! Notification-area icon and its menu.

use windows::Win32::Foundation::{LPARAM, POINT, WPARAM};
use windows::Win32::UI::Controls::{LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, HICON, MENU_ITEM_FLAGS, MF_CHECKED,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, SetMenuDefaultItem,
    TPM_BOTTOMALIGN, TPM_RIGHTALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, WM_CONTEXTMENU, WM_NULL,
};
use windows::core::{PCWSTR, w};

use crate::{Cmd, WM_TRAY, hinstance, host, post, shell, with_state};

const ICON_ID: u32 = 1;
const NIN_SELECT: u32 = 0x400;
const NIN_KEYSELECT: u32 = 0x401;

const ID_OPEN: usize = 100;
const ID_MONITOR: usize = 101;
const ID_WIDGET: usize = 102;
const ID_EXIT: usize = 103;

fn base() -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: host(),
        uID: ICON_ID,
        ..Default::default()
    }
}

fn copy_tip(dst: &mut [u16; 128], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

pub fn add() {
    let mut nid = base();
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
    nid.uCallbackMessage = WM_TRAY;
    // SAFETY: icon resource 2 is embedded by build.rs; LoadIconMetric picks the DPI size.
    nid.hIcon = unsafe { LoadIconMetric(Some(hinstance()), PCWSTR(2 as _), LIM_SMALL) }
        .unwrap_or(HICON::default());
    copy_tip(&mut nid.szTip, "Bosun SystemCare");
    // SAFETY: nid is fully initialised for our host window.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
    }
}

pub fn remove() {
    // SAFETY: removing our own icon.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &base());
    }
}

pub fn update_tip() {
    let Some(v) = with_state(|s| s.data.vitals) else {
        return;
    };
    let mut nid = base();
    nid.uFlags = NIF_TIP | NIF_SHOWTIP;
    copy_tip(
        &mut nid.szTip,
        &format!(
            "Bosun SystemCare\nRAM {}% · CPU {:.0}%",
            v.mem.load_pct, v.cpu_pct
        ),
    );
    // SAFETY: modifying our own icon.
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn on_message(_wp: WPARAM, lp: LPARAM) {
    match (lp.0 as u32) & 0xFFFF {
        NIN_SELECT | NIN_KEYSELECT => post(Cmd::ToggleMonitor),
        WM_CONTEXTMENU => post(Cmd::Menu),
        _ => {}
    }
}

pub fn menu_command(id: usize) -> Option<Cmd> {
    match id {
        ID_OPEN => Some(Cmd::OpenMain),
        ID_MONITOR => Some(Cmd::ShowMonitor),
        ID_WIDGET => Some(Cmd::ToggleWidget),
        ID_EXIT => Some(Cmd::Exit),
        _ => None,
    }
}

/// Shared by the tray icon and the widget's right-click.
pub fn show_menu() {
    let widget_on = with_state(|s| s.settings.widget_visible).unwrap_or(true);
    let main_flags = if shell::main_app_path().is_some() {
        MF_STRING
    } else {
        MF_STRING | MF_GRAYED
    };
    // SAFETY: standard popup-menu sequence; menu destroyed after tracking.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let item = |flags: MENU_ITEM_FLAGS, id: usize, text: PCWSTR| {
            let _ = AppendMenuW(menu, flags, id, text);
        };
        item(main_flags, ID_OPEN, w!("Open Bosun SystemCare"));
        item(MF_STRING, ID_MONITOR, w!("Performance Monitor"));
        item(
            if widget_on {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            },
            ID_WIDGET,
            w!("Floating widget"),
        );
        item(MF_SEPARATOR, 0, PCWSTR::null());
        item(MF_STRING, ID_EXIT, w!("Exit"));
        let _ = SetMenuDefaultItem(menu, ID_MONITOR as u32, 0);

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(host());
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RIGHTALIGN | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            host(),
            None,
        );
        let _ = PostMessageW(Some(host()), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}
