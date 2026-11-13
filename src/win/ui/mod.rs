//! The two-window UI: a hollow capture frame and a floating control bar,
//! both layered, topmost, and excluded from capture where the OS allows.

pub mod bar;
pub mod settings;
pub mod cam_win;
pub mod caption_win;
pub mod export_win;
pub mod frame;
pub mod paint;
pub mod snip;

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

use crate::app::{self, State};
use crate::config::AudioSource;
use crate::i18n::Lang;
use crate::win::tray;

/// Everything the painters need, snapshotted in one borrow of the app cell.
pub struct Snap {
    pub state: State,
    pub visible: bool,
    pub audio: AudioSource,
    pub lang: Lang,
    pub elapsed: String,
    pub dims: (i32, i32),
    pub frame: HWND,
    pub bar: HWND,
    pub hk_record: String,
    pub hk_stop: String,
    pub hk_shot: String,
    pub hk_snip: String,
}

pub fn snap() -> Option<Snap> {
    app::with(|a| {
        let (_, _, w, h) = frame::capture_rect(a.frame);
        Snap {
            state: a.state,
            visible: a.visible,
            audio: a.audio,
            lang: a.lang,
            elapsed: a.elapsed_text(),
            dims: (w, h),
            frame: a.frame,
            bar: a.bar,
            hk_record: a.cfg.hk_record.to_string(),
            hk_stop: a.cfg.hk_stop.to_string(),
            hk_shot: a.cfg.hk_shot.to_string(),
            hk_snip: a.cfg.hk_snip.to_string(),
        }
    })
}

pub fn refresh_all() {
    if let Some(s) = snap() {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow,SW_HIDE,SW_SHOWNOACTIVATE};
            let _=ShowWindow(s.frame,if s.visible && !bar::dot_mode(){SW_SHOWNOACTIVATE}else{SW_HIDE});
        }
        frame::repaint(s.frame);
        bar::place(s.bar, s.frame);
        bar::repaint_from(&s);
        bar::sync_timer(&s);
        tray::sync(s.bar, s.state, s.lang);
    }
}

pub fn wda_exclude(hwnd: HWND) -> bool {
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok() }
}

/// DPI scale of a window, 1.0 at 96 dpi.
pub fn scale_of(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

pub fn px(v: i32, k: f32) -> i32 {
    (v as f32 * k).round() as i32
}

// The frame's window procedure repositions the bar while dragging; it reads
// the handle from here instead of borrowing the app cell mid-message.
static BAR_CACHE: AtomicIsize = AtomicIsize::new(0);

pub fn cache_bar(hwnd: HWND) {
    BAR_CACHE.store(hwnd.0 as isize, Ordering::Relaxed);
}

pub fn cached_bar() -> Option<HWND> {
    let v = BAR_CACHE.load(Ordering::Relaxed);
    if v == 0 {
        None
    } else {
        Some(HWND(v as *mut core::ffi::c_void))
    }
}
