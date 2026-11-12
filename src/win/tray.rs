//! Notification-area icon: recognizable brand mark, show/hide toggle, and a
//! quick menu. The icon switches to the red variant while recording.

use std::sync::atomic::{AtomicU8, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{LoadIconW, HICON, IDI_APPLICATION};

use crate::app::State;
use crate::i18n::{t, Lang, Msg};
use crate::win::MSG_TRAY;

const ICON_APP: u16 = 1;
const ICON_REC: u16 = 2;

// Last pushed look: 0 = none, else (icon id) | (lang bit << 4).
static PUSHED: AtomicU8 = AtomicU8::new(0);

fn icon(id: u16) -> HICON {
    unsafe {
        GetModuleHandleW(None)
            .ok()
            .and_then(|m| LoadIconW(HINSTANCE(m.0), PCWSTR(id as usize as *const u16)).ok())
            .or_else(|| LoadIconW(HINSTANCE::default(), IDI_APPLICATION).ok())
            .unwrap_or_default()
    }
}

fn data(bar: HWND, icon_id: u16, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: bar,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: MSG_TRAY,
        hIcon: icon(icon_id),
        ..Default::default()
    };
    let wide: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
    nid.szTip[..wide.len()].copy_from_slice(&wide);
    nid
}

fn look(state: State, lang: Lang) -> (u16, Msg, u8) {
    let (id, msg) = match state {
        State::Recording | State::Paused => (ICON_REC, Msg::TrayRec),
        _ => (ICON_APP, Msg::TrayIdle),
    };
    let key = id as u8 | (((lang == Lang::Zh) as u8) << 4) | 0x80;
    (id, msg, key)
}

pub fn add(bar: HWND, state: State, lang: Lang) {
    let (id, msg, key) = look(state, lang);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data(bar, id, t(lang, msg)));
    }
    PUSHED.store(key, Ordering::Relaxed);
}

/// Keep the icon and tooltip in sync with the app state; cheap to call from
/// every refresh.
pub fn sync(bar: HWND, state: State, lang: Lang) {
    let (id, msg, key) = look(state, lang);
    let pushed = PUSHED.load(Ordering::Relaxed);
    if pushed == key || pushed == 0 {
        return; // unchanged, or the icon was never added
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data(bar, id, t(lang, msg)));
    }
    PUSHED.store(key, Ordering::Relaxed);
}

pub fn remove(bar: HWND) {
    if PUSHED.swap(0, Ordering::Relaxed) != 0 {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data(bar, ICON_APP, ""));
        }
    }
}
