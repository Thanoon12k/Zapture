//! Windows backend. Everything OS-specific lives below this module; the
//! capture/encode/audio seams are documented in the README for future ports.

pub mod audio;
pub mod autostart;
pub mod camera;
pub mod caption;
pub mod compose;
pub mod encoder;
pub mod export;
pub mod screen;
pub mod tray;
pub mod ui;
pub mod util;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    VK_F1, VK_SNAPSHOT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_APP,
};

use crate::app::{self, App};
use crate::config::{Config, Hotkey, Key, LangPref};
use crate::i18n::{t, Lang, Msg};

pub const MSG_ENC_DONE: u32 = WM_APP + 1;
pub const MSG_SHOT_DONE: u32 = WM_APP + 2;
pub const MSG_TRAY: u32 = WM_APP + 3;
pub const MSG_EXPORT_PROGRESS: u32 = WM_APP + 4;
pub const MSG_EXPORT_DONE: u32 = WM_APP + 5;

pub const HK_RECORD: i32 = 1;
pub const HK_STOP: i32 = 2;
pub const HK_SHOT: i32 = 3;
pub const HK_SNIP: i32 = 4;

/// System UI language, mapped onto the languages Cadre ships.
pub fn system_lang() -> Lang {
    let id = unsafe { GetUserDefaultUILanguage() };
    if id & 0x3FF == 0x04 {
        Lang::Zh
    } else {
        Lang::En
    }
}

pub fn hotkey_to_vk(hk: &Hotkey) -> (HOT_KEY_MODIFIERS, u32) {
    let mut mods = MOD_NOREPEAT;
    if hk.ctrl {
        mods |= MOD_CONTROL;
    }
    if hk.shift {
        mods |= MOD_SHIFT;
    }
    if hk.alt {
        mods |= MOD_ALT;
    }
    if hk.win {
        mods |= MOD_WIN;
    }
    let vk = match hk.key {
        Key::F(n) => VK_F1.0 as u32 + (n as u32 - 1),
        Key::Char(c) => c as u32, // ASCII uppercase letters/digits equal their VK codes
        Key::PrintScreen => VK_SNAPSHOT.0 as u32,
    };
    (mods, vk)
}

fn register_hotkeys(bar: windows::Win32::Foundation::HWND, cfg: &Config) -> Vec<String> {
    let mut failed = Vec::new();
    for (id, hk) in [
        (HK_RECORD, cfg.hk_record),
        (HK_STOP, cfg.hk_stop),
        (HK_SHOT, cfg.hk_shot),
        (HK_SNIP, cfg.hk_snip),
    ] {
        let (mods, vk) = hotkey_to_vk(&hk);
        if unsafe { RegisterHotKey(bar, id, mods, vk) }.is_err() {
            failed.push(hk.to_string());
        }
    }
    failed
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let cfg = Config::load(&util::config_path());
    let lang = match cfg.language {
        LangPref::En => Lang::En,
        LangPref::Zh => Lang::Zh,
        LangPref::Auto => system_lang(),
    };

    let _instance_lock = unsafe { CreateMutexW(None, true, w!("Local\\zapture-single-instance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        util::info(t(lang, Msg::AlreadyRunning));
        return;
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if let Err(e) = ui::paint::init() {
        util::alert(&format!("GDI+ failed to initialize (status {e})."));
        return;
    }

    let frame = match ui::frame::create(&cfg) {
        Ok(h) => h,
        Err(e) => {
            util::alert(&format!("Could not create the capture frame window:\n{e}"));
            return;
        }
    };
    let bar = match ui::bar::create() {
        Ok(h) => h,
        Err(e) => {
            util::alert(&format!("Could not create the control bar window:\n{e}"));
            return;
        }
    };

    // Keep Cadre's own chrome out of recordings and screenshots where the OS
    // supports it (Windows 10 2004+). The frame border sits outside the
    // capture rectangle anyway; this covers the bar and the shot flash.
    let wda_ok = ui::wda_exclude(frame) & ui::wda_exclude(bar);

    let failed = register_hotkeys(bar, &cfg);
    app::init(App::new(cfg, frame, bar, wda_ok, lang));
    tray::add(bar, app::cached_state(), lang);

    if !failed.is_empty() {
        util::alert(&format!(
            "{}\n{}\n\n{}",
            t(lang, Msg::ErrHotkeysTaken),
            failed.join(", "),
            t(lang, Msg::HkRemap)
        ));
    }

    ui::bar::place(bar, frame);
    ui::refresh_all();

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    tray::remove(bar);
    app::with(|a| {
        let (x, y, w, h) = ui::frame::capture_rect(a.frame);
        a.cfg.frame = Some((x, y, w, h));
        a.cfg.audio = a.audio;
        app::stop_camera_and_save(a);
        let _ = a.cfg.save(&util::config_path());
    });
    ui::cam_win::close();
    ui::paint::shutdown();
}
