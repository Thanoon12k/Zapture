//! The floating control bar and its tooltip companion. Layered windows,
//! custom-painted; no common controls involved. The brand mark doubles as
//! the menu button; the speaker button cycles the audio source.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, GetCursorPos,
    GetWindowRect, KillTimer, LoadCursorW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TrackPopupMenu,
    CS_HREDRAW, CS_VREDRAW, HMENU, IDC_ARROW, MENU_ITEM_FLAGS, MF_CHECKED, MF_POPUP,
    MF_SEPARATOR, MF_STRING, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
    SW_SHOWNORMAL, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CLOSE, WM_DESTROY,
    WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NULL, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::app::{self, State};
use crate::config::{APP_NAME, AudioSource, BgStyle, CaptionEdge, LangPref, VERSION};
use crate::i18n::{t, Lang, Msg};
use crate::win::ui::{self, paint::{argb, with_alpha, Align, Canvas}, Snap};
use crate::win::{
    autostart, util, HK_RECORD, HK_SHOT, HK_SNIP, HK_STOP, MSG_ENC_DONE, MSG_SHOT_DONE, MSG_TRAY,
};

const WM_MOUSELEAVE: u32 = 0x02A3;

const TIMER_TICK: usize = 1;
const TIMER_TIP: usize = 2;

const BASE_H: i32 = 46;
const PAD: i32 = 10;
const BTN: i32 = 30;
const LOGO: i32 = 22;
const GAP: i32 = 4;
const SEP: i32 = 12;
const TIMER_W: i32 = 88;
const DIMS_W: i32 = 80;

const MENU_RECORD: usize = 60;
const MENU_STOP: usize = 61;
const MENU_SHOT: usize = 62;
const MENU_SNIP: usize = 63;
const MENU_FULLSCREEN: usize = 64;
const MENU_FOLDER: usize = 1;
const MENU_CONFIG: usize = 2;
const MENU_HOTKEYS: usize = 3;
const MENU_ABOUT: usize = 4;
const MENU_QUIT: usize = 5;
const MENU_SHOWHIDE: usize = 6;
const MENU_AUDIO_OFF: usize = 10;
const MENU_AUDIO_SYSTEM: usize = 11;
const MENU_AUDIO_MIC: usize = 12;
const MENU_AUDIO_BOTH: usize = 13;
const MENU_LANG_EN: usize = 20;
const MENU_LANG_ZH: usize = 21;
const MENU_EXPORT: usize = 30;
const MENU_AUTOSTART: usize = 31;
const MENU_HIGHLIGHT: usize = 32;
const MENU_AUTOZOOM: usize = 33;
const MENU_BG_NONE: usize = 40;
const MENU_BG_GRAPHITE: usize = 41;
const MENU_BG_OCEAN: usize = 42;
const MENU_BG_SUNSET: usize = 43;
const MENU_BG_SNOW: usize = 44;
const MENU_CAPTION: usize = 50;
const MENU_BG_FOREST: usize = 45;
const MENU_BG_ROSE: usize = 46;
const MENU_BG_LAVENDER: usize = 47;
const MENU_BG_SKY: usize = 48;

const FG: u32 = argb(255, 0xE8, 0xEA, 0xED);
const FG_DIM: u32 = argb(110, 0xE8, 0xEA, 0xED);
const FG_FAINT: u32 = argb(255, 0x7E, 0x86, 0x94);
const BLUE: u32 = argb(255, 0x4C, 0x8D, 0xFF);
const RED: u32 = argb(255, 0xE5, 0x48, 0x4D);
const AMBER: u32 = argb(255, 0xE8, 0xA3, 0x3D);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Logo,
    Rec,
    Stop,
    Shot,
    Snap,
    Camera,
    Audio,
    Close,
}

struct Layout {
    w: i32,
    h: i32,
    btns: [(Btn, RECT); 7],
    timer: RECT,
    dims: RECT,
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT { left: x, top: y, right: x + w, bottom: y + h }
}

fn layout(k: f32) -> Layout {
    let s = |v: i32| ui::px(v, k);
    let h = s(BASE_H);
    let btn = s(BTN);
    let logo = s(LOGO);
    let y = (h - btn) / 2;
    let mut x = s(PAD);
    let logo_r = rect(x, (h - logo) / 2, logo, logo);
    x += logo + s(6);
    let rec = rect(x, y, btn, btn);
    x += btn + s(GAP);
    let stop = rect(x, y, btn, btn);
    x += btn + s(GAP);
    let shot = rect(x, y, btn, btn);
    x += btn + s(GAP);
    let snap = rect(x, y, btn, btn);
    x += btn + s(GAP);
    x += s(SEP);
    let timer = rect(x, 0, s(TIMER_W), h);
    x += s(TIMER_W) + s(6);
    let dims = rect(x, 0, s(DIMS_W), h);
    x += s(DIMS_W) + s(SEP);
    let audio = rect(x, y, btn, btn);
    x += btn + s(GAP);
    let close = rect(x, y, btn, btn);
    x += btn + s(PAD);
    Layout {
        w: x,
        h,
        btns: [
            (Btn::Logo, logo_r),
            (Btn::Rec, rec),
            (Btn::Stop, stop),
            (Btn::Shot, shot),
            (Btn::Snap, snap),
            (Btn::Audio, audio),
            (Btn::Close, close),
        ],
        timer,
        dims,
    }
}

fn in_rect(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

#[derive(Default)]
struct BarState {
    hover: Option<Btn>,
    pressed: Option<Btn>,
    tracking: bool,
}

thread_local! {
    static BAR: RefCell<BarState> = RefCell::new(BarState::default());
}

fn enabled(btn: Btn, state: State) -> bool {
    match btn {
        Btn::Rec => state != State::Saving,
        Btn::Stop => matches!(state, State::Recording | State::Paused),
        Btn::Shot => state != State::Saving,
        Btn::Snap => state == State::Idle,
        Btn::Audio => state == State::Idle,
        Btn::Camera | Btn::Logo | Btn::Close => true,
    }
}

pub fn create() -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(bar_proc),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(HINSTANCE(hinst.0), PCWSTR(1 as *const u16)).unwrap_or_default(),
            lpszClassName: w!("CadreBar"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("RegisterClassW(CadreBar) failed".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("CadreBar"),
            w!("Cadre controls"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            HINSTANCE(hinst.0),
            None,
        )
        .map_err(|e| e.to_string())?;
        ui::cache_bar(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Ok(hwnd)
    }
}

/// Dock the bar to the frame: centered below the region, else above it, else
/// tucked inside its bottom-right corner (where capture exclusion hides it).
pub fn place(bar: HWND, frame: HWND) {
    let (cl, ct, cw, ch) = super::frame::capture_rect(frame);
    let k = ui::scale_of(bar);
    let l = layout(k);
    let gap = ui::px(12, k);

    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mon = unsafe { MonitorFromWindow(frame, MONITOR_DEFAULTTONEAREST) };
    unsafe {
        let _ = GetMonitorInfoW(mon, &mut mi);
    }
    let work = mi.rcWork;

    let mut x = cl + (cw - l.w) / 2;
    x = x.clamp(work.left + 8, (work.right - l.w - 8).max(work.left + 8));
    let mut y = ct + ch + gap;
    if y + l.h > work.bottom - 8 {
        y = ct - gap - l.h - ui::px(28, k); // above, clearing the size chip
        if y < work.top + 8 {
            x = (cl + cw - l.w - gap).max(work.left + 8);
            y = ct + ch - l.h - gap;
        }
    }
    unsafe {
        let _ = SetWindowPos(bar, HWND::default(), x, y, l.w, l.h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn draw_speaker(canvas: &Canvas, cx: f32, cy: f32, k: f32, color: u32, waves: bool) {
    let s = |v: f32| v * k;
    canvas.polygon(
        &[
            (cx - s(8.0), cy - s(3.5)),
            (cx - s(3.5), cy - s(3.5)),
            (cx + s(1.5), cy - s(8.0)),
            (cx + s(1.5), cy + s(8.0)),
            (cx - s(3.5), cy + s(3.5)),
            (cx - s(8.0), cy + s(3.5)),
        ],
        color,
    );
    if waves {
        canvas.arc(cx + s(1.5), cy, s(5.0), -55.0, 110.0, s(1.7), color);
        canvas.arc(cx + s(1.5), cy, s(8.0), -55.0, 110.0, s(1.7), with_alpha(color, 170));
    }
}

fn draw_mic(canvas: &Canvas, cx: f32, cy: f32, k: f32, color: u32) {
    let s = |v: f32| v * k;
    canvas.fill_rounded(cx - s(2.8), cy - s(8.5), s(5.6), s(10.5), s(2.8), color);
    canvas.arc(cx, cy + s(0.3), s(5.8), 20.0, 140.0, s(1.7), color);
    canvas.line(cx, cy + s(6.1), cx, cy + s(8.3), s(1.7), color);
    canvas.line(cx - s(3.4), cy + s(8.3), cx + s(3.4), cy + s(8.3), s(1.7), color);
}

fn icon(canvas: &Canvas, btn: Btn, r: &RECT, snap: &Snap, k: f32) {
    let s = |v: f32| v * k;
    let cx = (r.left + r.right) as f32 / 2.0;
    let cy = (r.top + r.bottom) as f32 / 2.0;
    let on = enabled(btn, snap.state);
    let fg = if on { FG } else { FG_DIM };

    match btn {
        Btn::Logo => {
            // The viewfinder mark from the app icon.
            let b = s(8.2);
            let arm = s(5.2);
            let th = s(1.8);
            let rad = s(0.9);
            let bars = [
                (cx - b, cy - b, arm, th), (cx - b, cy - b, th, arm),
                (cx + b - arm, cy - b, arm, th), (cx + b - th, cy - b, th, arm),
                (cx - b, cy + b - th, arm, th), (cx - b, cy + b - arm, th, arm),
                (cx + b - arm, cy + b - th, arm, th), (cx + b - th, cy + b - arm, th, arm),
            ];
            for (x, y, w, h) in bars {
                canvas.fill_rounded(x, y, w, h, rad, BLUE);
            }
            canvas.fill_circle(cx, cy, s(2.7), RED);
        }
        Btn::Rec => match snap.state {
            State::Idle | State::Saving => {
                canvas.fill_circle(cx, cy, s(7.0), if on { RED } else { FG_DIM });
            }
            State::Recording => {
                // pause glyph
                let (bw, bh, g) = (s(3.6), s(13.0), s(2.6));
                canvas.fill_rounded(cx - g - bw, cy - bh / 2.0, bw, bh, s(1.4), fg);
                canvas.fill_rounded(cx + g, cy - bh / 2.0, bw, bh, s(1.4), fg);
            }
            State::Paused => {
                // resume glyph
                canvas.polygon(
                    &[
                        (cx - s(4.5), cy - s(6.5)),
                        (cx - s(4.5), cy + s(6.5)),
                        (cx + s(6.5), cy),
                    ],
                    fg,
                );
            }
        },
        Btn::Stop => {
            let d = s(12.0);
            canvas.fill_rounded(cx - d / 2.0, cy - d / 2.0, d, d, s(2.5), fg);
        }
        Btn::Shot => {
            let (bw, bh) = (s(17.0), s(12.5));
            canvas.stroke_rounded(cx - bw / 2.0, cy - bh / 2.0 + s(1.0), bw, bh, s(3.0), s(1.7), fg);
            canvas.fill_rounded(cx - s(3.5), cy - bh / 2.0 - s(1.6), s(7.0), s(3.0), s(1.2), fg);
            canvas.circle_stroke(cx, cy + s(1.2), s(3.1), s(1.7), fg);
        }
        Btn::Snap => {
            // a small window hugged by the capture frame's inward corner brackets
            let (ww, hh) = (s(9.5), s(8.0));
            canvas.fill_rounded(cx - ww / 2.0, cy - hh / 2.0, ww, hh, s(1.6), with_alpha(fg, 90));
            let (o, a, th) = (s(7.6), s(3.4), s(1.7));
            let col = if on { BLUE } else { FG_DIM };
            for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let (bx, by) = (cx + dx * o, cy + dy * o);
                let hx = if dx < 0.0 { bx } else { bx - a };
                let vy = if dy < 0.0 { by } else { by - a };
                canvas.fill_rounded(hx, by - th / 2.0, a, th, s(0.6), col);
                canvas.fill_rounded(bx - th / 2.0, vy, th, a, s(0.6), col);
            }
        }
        Btn::Camera => {
            // presenter silhouette: head + shoulders, accented when live
            let col = if crate::win::ui::cam_win::is_open() { BLUE } else { fg };
            canvas.fill_circle(cx, cy - s(3.6), s(3.4), col);
            canvas.fill_rounded(cx - s(6.6), cy + s(1.6), s(13.2), s(8.0), s(4.0), col);
        }
        Btn::Audio => {
            let fg = if on { FG } else { FG_DIM };
            match snap.audio {
                AudioSource::System => draw_speaker(canvas, cx, cy, k, fg, true),
                AudioSource::Mic => draw_mic(canvas, cx, cy, k, fg),
                AudioSource::Both => {
                    draw_speaker(canvas, cx - s(1.5), cy, k, fg, false);
                    let (px, py) = (cx + s(6.0), cy - s(5.5));
                    canvas.line(px - s(3.0), py, px + s(3.0), py, s(1.7), fg);
                    canvas.line(px, py - s(3.0), px, py + s(3.0), s(1.7), fg);
                }
                AudioSource::Off => {
                    draw_speaker(canvas, cx, cy, k, FG_DIM, false);
                    canvas.line(cx - s(8.5), cy + s(8.5), cx + s(8.5), cy - s(8.5), s(1.8), RED);
                }
            }
        }
        Btn::Close => {
            let d = s(5.4);
            canvas.line(cx - d, cy - d, cx + d, cy + d, s(1.7), fg);
            canvas.line(cx - d, cy + d, cx + d, cy - d, s(1.7), fg);
        }
    }
}

pub fn repaint_from(snap: &Snap) {
    let bar = snap.bar;
    let k = ui::scale_of(bar);
    let l = layout(k);
    let Some(canvas) = Canvas::new(l.w, l.h) else { return };
    let s = |v: i32| ui::px(v, k) as f32;

    canvas.fill_rounded(0.0, 0.0, l.w as f32, l.h as f32, s(11), argb(244, 0x15, 0x17, 0x1C));
    canvas.stroke_rounded(0.5, 0.5, l.w as f32 - 1.0, l.h as f32 - 1.0, s(11), 1.0, argb(28, 255, 255, 255));

    let (hover, pressed) = BAR.with(|b| {
        let b = b.borrow();
        (b.hover, b.pressed)
    });

    for (btn, r) in &l.btns {
        if enabled(*btn, snap.state) && hover == Some(*btn) {
            let a = if pressed == Some(*btn) { 44 } else { 26 };
            let cx = (r.left + r.right) as f32 / 2.0;
            let cy = (r.top + r.bottom) as f32 / 2.0;
            canvas.fill_circle(cx, cy, (r.right - r.left).max(r.bottom - r.top) as f32 / 2.0 + s(2), argb(a, 255, 255, 255));
        }
        icon(&canvas, *btn, r, snap, k);
    }

    // Timer, with a breathing dot while recording.
    let (dot, text_color) = match snap.state {
        State::Recording => {
            let tsec = util::qpc() as f64 / util::qpf() as f64;
            let a = (0.55 + 0.45 * (tsec * std::f64::consts::TAU / 1.2).sin()) * 255.0;
            (Some(with_alpha(RED, a as u8)), FG)
        }
        State::Paused => (Some(AMBER), FG),
        _ => (None, FG_FAINT),
    };
    let tx = l.timer.left as f32;
    if let Some(c) = dot {
        canvas.fill_circle(tx + s(5), l.h as f32 / 2.0, s(4), c);
    }
    canvas.text(
        &snap.elapsed,
        tx + s(13),
        l.timer.top as f32,
        (l.timer.right - l.timer.left) as f32 - s(13),
        (l.timer.bottom - l.timer.top) as f32,
        s(13),
        text_color,
        true,
        false,
        Align::Left,
    );

    canvas.text(
        &format!("{} × {}", snap.dims.0, snap.dims.1),
        l.dims.left as f32,
        l.dims.top as f32,
        (l.dims.right - l.dims.left) as f32,
        (l.dims.bottom - l.dims.top) as f32,
        s(11),
        FG_FAINT,
        false,
        false,
        Align::Right,
    );

    canvas.present(bar);
}

pub fn sync_timer(snap: &Snap) {
    unsafe {
        if snap.state == State::Recording {
            SetTimer(snap.bar, TIMER_TICK, 500, None);
        } else {
            let _ = KillTimer(snap.bar, TIMER_TICK);
        }
    }
}

fn audio_msg(source: AudioSource) -> Msg {
    match source {
        AudioSource::Off => Msg::AudioOff,
        AudioSource::System => Msg::AudioSystem,
        AudioSource::Mic => Msg::AudioMic,
        AudioSource::Both => Msg::AudioBoth,
    }
}

fn tip_text(btn: Btn, snap: &Snap) -> String {
    let lang = snap.lang;
    match btn {
        Btn::Logo => format!("{APP_NAME} {VERSION}"),
        Btn::Rec => match snap.state {
            State::Recording => format!("{}  ·  {}", t(lang, Msg::TipPause), snap.hk_record),
            State::Paused => format!("{}  ·  {}", t(lang, Msg::TipResume), snap.hk_record),
            _ => format!("{}  ·  {}", t(lang, Msg::TipRecord), snap.hk_record),
        },
        Btn::Stop => format!("{}  ·  {}", t(lang, Msg::TipStop), snap.hk_stop),
        Btn::Shot => format!("{}  ·  {}", t(lang, Msg::TipShot), snap.hk_shot),
        Btn::Snap => "Select screenshot area (Alt+A)".to_string(),
        Btn::Camera => t(lang, Msg::TipCamera).to_string(),
        Btn::Audio => {
            if snap.state != State::Idle {
                t(lang, Msg::TipAudioLocked).to_string()
            } else {
                t(lang, audio_msg(snap.audio)).to_string()
            }
        }
        Btn::Close => t(lang, Msg::TipHide).to_string(),
    }
}

fn hit(bar: HWND, x: i32, y: i32) -> Option<Btn> {
    let l = layout(ui::scale_of(bar));
    l.btns.iter().find(|(_, r)| in_rect(r, x, y)).map(|(b, _)| *b)
}

unsafe fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
    let wide = util::wide(text);
    let _ = AppendMenuW(menu, flags, id, PCWSTR(wide.as_ptr()));
}

fn check(flag: bool) -> MENU_ITEM_FLAGS {
    if flag {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    }
}

unsafe fn language_submenu(snap: &Snap) -> Option<HMENU> {
    let sub = CreatePopupMenu().ok()?;
    append(sub, check(snap.lang == crate::i18n::Lang::En), MENU_LANG_EN, "English");
    append(sub, check(snap.lang == crate::i18n::Lang::Zh), MENU_LANG_ZH, "中文");
    Some(sub)
}

unsafe fn background_submenu(cur: BgStyle, lang: Lang) -> Option<HMENU> {
    let sub = CreatePopupMenu().ok()?;
    append(sub, check(cur == BgStyle::None), MENU_BG_NONE, t(lang, Msg::BgNone));
    append(sub, check(cur == BgStyle::Graphite), MENU_BG_GRAPHITE, t(lang, Msg::BgGraphite));
    append(sub, check(cur == BgStyle::Ocean), MENU_BG_OCEAN, t(lang, Msg::BgOcean));
    append(sub, check(cur == BgStyle::Sunset), MENU_BG_SUNSET, t(lang, Msg::BgSunset));
    append(sub, check(cur == BgStyle::Snow), MENU_BG_SNOW, t(lang, Msg::BgSnow));
    append(sub, check(cur == BgStyle::Forest), MENU_BG_FOREST, t(lang, Msg::BgForest));
    append(sub, check(cur == BgStyle::Rose), MENU_BG_ROSE, t(lang, Msg::BgRose));
    append(sub, check(cur == BgStyle::Lavender), MENU_BG_LAVENDER, t(lang, Msg::BgLavender));
    append(sub, check(cur == BgStyle::Sky), MENU_BG_SKY, t(lang, Msg::BgSky));
    Some(sub)
}


unsafe fn show_menu(bar: HWND, menu: HMENU) -> usize {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(bar);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        bar,
        None,
    );
    let _ = PostMessageW(bar, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    cmd.0 as usize
}

fn dispatch(cmd: usize) {
    match cmd {
        MENU_RECORD => app::act_toggle_record(),
        MENU_STOP => app::act_stop(),
        MENU_SHOT => app::act_shot(),
        MENU_SNIP => app::act_snip(),
        MENU_FULLSCREEN => {
            if app::cached_state() == State::Idle {
                if let Some((frame, bar)) = app::with(|a| (a.frame, a.bar)) { unsafe {
                    let monitor = MonitorFromWindow(frame, MONITOR_DEFAULTTONEAREST);
                    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if GetMonitorInfoW(monitor, &mut mi).as_bool() {
                        let r = mi.rcMonitor;
                        ui::frame::set_capture_rect(frame, r.left, r.top, r.right-r.left, r.bottom-r.top);
                        place(bar, frame);
                    }
                }}
                ui::refresh_all();
            }
        }
        MENU_AUDIO_OFF => app::act_set_audio(AudioSource::Off),
        MENU_AUDIO_SYSTEM => app::act_set_audio(AudioSource::System),
        MENU_AUDIO_MIC => app::act_set_audio(AudioSource::Mic),
        MENU_AUDIO_BOTH => app::act_set_audio(AudioSource::Both),
        MENU_LANG_EN => app::act_set_lang(LangPref::En),
        MENU_LANG_ZH => app::act_set_lang(LangPref::Zh),
        MENU_EXPORT => app::act_open_export(),
        MENU_AUTOSTART => app::act_toggle_autostart(),
        MENU_HIGHLIGHT => app::act_toggle_highlight(),
        MENU_AUTOZOOM => app::act_toggle_autozoom(),
        MENU_BG_NONE => app::act_set_bg(BgStyle::None),
        MENU_BG_GRAPHITE => app::act_set_bg(BgStyle::Graphite),
        MENU_BG_OCEAN => app::act_set_bg(BgStyle::Ocean),
        MENU_BG_SUNSET => app::act_set_bg(BgStyle::Sunset),
        MENU_BG_SNOW => app::act_set_bg(BgStyle::Snow),
        MENU_BG_FOREST => app::act_set_bg(BgStyle::Forest),
        MENU_BG_ROSE => app::act_set_bg(BgStyle::Rose),
        MENU_BG_LAVENDER => app::act_set_bg(BgStyle::Lavender),
        MENU_BG_SKY => app::act_set_bg(BgStyle::Sky),
        MENU_CAPTION => app::act_open_caption(),
        MENU_SHOWHIDE => app::act_toggle_visibility(),
        MENU_FOLDER => {
            let dir = util::captures_dir();
            let _ = std::fs::create_dir_all(&dir);
            let dir_w = util::wide(&dir.to_string_lossy());
            unsafe {
                ShellExecuteW(None, w!("open"), PCWSTR(dir_w.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        MENU_CONFIG => {
            let path = util::config_path();
            if !path.exists() {
                app::with(|a| a.cfg.save(&path).ok());
            }
            let path_w = util::wide(&path.to_string_lossy());
            unsafe {
                ShellExecuteW(None, w!("open"), w!("notepad.exe"), PCWSTR(path_w.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL);
            }
        }
        MENU_HOTKEYS => {
            if let Some(s) = ui::snap() {
                let lang = s.lang;
                util::info(&format!(
                    "{}:  {}\n{}:  {}\n{}:  {}\n{}:  {}\n\n{}",
                    t(lang, Msg::HkRecord),
                    s.hk_record,
                    t(lang, Msg::HkStop),
                    s.hk_stop,
                    t(lang, Msg::HkShot),
                    s.hk_shot,
                    t(lang, Msg::HkSnip),
                    s.hk_snip,
                    t(lang, Msg::HkRemap)
                ));
            }
        }
        MENU_ABOUT => {
            if let Some(s) = ui::snap() {
                util::info(&format!("{APP_NAME} {VERSION}\n{}", "Customized from Cadre (MIT). Local screen and audio capture. No uploads."));
            }
        }
        MENU_QUIT => app::act_quit(),
        _ => {}
    }
}

fn open_menu(bar: HWND) { tray_menu(bar); }

fn tray_menu(bar: HWND) {
    let Some(snap) = ui::snap() else { return };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let active = matches!(snap.state, State::Recording | State::Paused);
        let enabled = |yes: bool| if yes { MF_STRING } else { MF_STRING | windows::Win32::UI::WindowsAndMessaging::MF_GRAYED };
        let rec_label = match snap.state {
            State::Recording => "Pause recording (F9)",
            State::Paused => "Resume recording (F9)",
            _ => "Start recording (F9)",
        };
        append(menu, enabled(snap.state != State::Saving), MENU_RECORD, rec_label);
        append(menu, enabled(active), MENU_STOP, "Stop and save (F10)");
        append(menu, enabled(snap.state != State::Saving), MENU_SHOT, "Screenshot: save + copy (F8)");
        append(menu, MF_STRING, MENU_SNIP, "Select screenshot area (Alt+A)");
        append(menu, enabled(snap.state == State::Idle), MENU_FULLSCREEN, "Capture full monitor");
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        for (source, id, label) in [
            (AudioSource::Off, MENU_AUDIO_OFF, "Audio: none"),
            (AudioSource::System, MENU_AUDIO_SYSTEM, "Audio: computer sound"),
            (AudioSource::Mic, MENU_AUDIO_MIC, "Audio: microphone"),
            (AudioSource::Both, MENU_AUDIO_BOTH, "Audio: computer + microphone"),
        ] {
            append(menu, enabled(snap.state == State::Idle) | check(snap.audio == source), id, label);
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append(menu, MF_STRING, MENU_SHOWHIDE, "Show / hide floating bar");
        append(menu, MF_STRING, MENU_FOLDER, "Open captures folder");
        append(menu, MF_STRING, MENU_CONFIG, "Settings (FPS / hotkeys)");
        append(menu, check(autostart::is_enabled()), MENU_AUTOSTART, "Start with Windows");
        append(menu, MF_STRING, MENU_ABOUT, "About Zapture");
        append(menu, MF_STRING, MENU_QUIT, "Exit");
        dispatch(show_menu(bar, menu));
    }
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let new = hit(hwnd, x, y);
            let changed = BAR.with(|b| {
                let mut b = b.borrow_mut();
                let changed = b.hover != new;
                b.hover = new;
                if !b.tracking {
                    b.tracking = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                changed
            });
            if changed {
                tip::hide();
                if new.is_some() {
                    SetTimer(hwnd, TIMER_TIP, 400, None);
                } else {
                    let _ = KillTimer(hwnd, TIMER_TIP);
                }
                if let Some(s) = ui::snap() {
                    repaint_from(&s);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            BAR.with(|b| {
                let mut b = b.borrow_mut();
                b.hover = None;
                b.pressed = None;
                b.tracking = false;
            });
            tip::hide();
            let _ = KillTimer(hwnd, TIMER_TIP);
            if let Some(s) = ui::snap() {
                repaint_from(&s);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let target = hit(hwnd, x, y);
            BAR.with(|b| b.borrow_mut().pressed = target);
            if target.is_some() {
                SetCapture(hwnd);
                if let Some(s) = ui::snap() {
                    repaint_from(&s);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let target = hit(hwnd, x, y);
            let pressed = BAR.with(|b| b.borrow_mut().pressed.take());
            if let (Some(p), Some(target)) = (pressed, target) {
                if p == target {
                    tip::hide();
                    let state = app::cached_state();
                    if enabled(p, state) {
                        match p {
                            Btn::Logo => open_menu(hwnd),
                            Btn::Rec => app::act_toggle_record(),
                            Btn::Stop => app::act_stop(),
                            Btn::Shot => app::act_shot(),
                            Btn::Snap => app::act_snip(),
                            Btn::Camera => app::act_toggle_camera(),
                            Btn::Audio => app::act_cycle_audio(),
                            Btn::Close => app::act_hide(),
                        }
                    }
                }
            }
            if let Some(s) = ui::snap() {
                repaint_from(&s);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            open_menu(hwnd);
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                9 => crate::mcp::drain(app::agent_call),
                TIMER_TICK => {
                    if let Some(s) = ui::snap() {
                        repaint_from(&s);
                    }
                }
                TIMER_TIP => {
                    let _ = KillTimer(hwnd, TIMER_TIP);
                    let hover = BAR.with(|b| b.borrow().hover);
                    if let (Some(btn), Some(s)) = (hover, ui::snap()) {
                        let l = layout(ui::scale_of(hwnd));
                        if let Some((_, r)) = l.btns.iter().find(|(b, _)| *b == btn) {
                            let mut wr = RECT::default();
                            let _ = GetWindowRect(hwnd, &mut wr);
                            tip::show(
                                &tip_text(btn, &s),
                                wr.left + (r.left + r.right) / 2,
                                wr.top,
                                None,
                            );
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HK_RECORD => app::act_toggle_record(),
                HK_STOP => app::act_stop(),
                HK_SHOT => app::act_shot(),
                HK_SNIP => app::act_snip(),
                _ => {}
            }
            LRESULT(0)
        }
        MSG_TRAY => {
            match lparam.0 as u32 {
                0x0202 => tray_menu(hwnd), // WM_LBUTTONUP
                0x0205 => tray_menu(hwnd),              // WM_RBUTTONUP
                _ => {}
            }
            LRESULT(0)
        }
        MSG_ENC_DONE => {
            let err = if lparam.0 == 0 {
                None
            } else {
                Some(*Box::from_raw(lparam.0 as *mut String))
            };
            app::on_encoder_done(err);
            LRESULT(0)
        }
        MSG_SHOT_DONE => {
            let text = *Box::from_raw(lparam.0 as *mut String);
            app::on_shot_done(wparam.0 == 1, text);
            LRESULT(0)
        }
        WM_CLOSE => {
            // Alt+F4 on the bar minimizes to the tray, like the close button;
            // quitting for real goes through the tray or menu Quit item.
            app::act_hide();
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Toast anchored above the bar; also used for hover tooltips.
pub fn toast(text: &str) {
    if let Some(s) = ui::snap() {
        let mut wr = RECT::default();
        unsafe {
            let _ = GetWindowRect(s.bar, &mut wr);
        }
        tip::show(text, (wr.left + wr.right) / 2, wr.top, Some(2600));
    }
}

pub fn hide_tip() {
    tip::hide();
}

/// A shared, click-through tooltip window.
mod tip {
    use super::*;

    static TIP: AtomicIsize = AtomicIsize::new(0);
    const TIMER_HIDE: usize = 1;

    fn window() -> Option<HWND> {
        let cached = TIP.load(Ordering::Relaxed);
        if cached != 0 {
            return Some(HWND(cached as *mut core::ffi::c_void));
        }
        unsafe {
            let hinst = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(tip_proc),
                hInstance: HINSTANCE(hinst.0),
                lpszClassName: w!("CadreTip"),
                ..Default::default()
            };
            RegisterClassW(&class); // idempotent enough: second call fails, window class persists
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                w!("CadreTip"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                HINSTANCE(hinst.0),
                None,
            )
            .ok()?;
            TIP.store(hwnd.0 as isize, Ordering::Relaxed);
            Some(hwnd)
        }
    }

    /// Show `text` centered at `cx`, just above `top_y`.
    pub fn show(text: &str, cx: i32, top_y: i32, auto_hide_ms: Option<u32>) {
        let Some(hwnd) = window() else { return };
        let k = ui::scale_of(hwnd);
        let s = |v: i32| ui::px(v, k);

        let Some(probe) = Canvas::new(4, 4) else { return };
        let tw = probe.text_width(text, s(12) as f32, false, false);
        drop(probe);

        let (w, h) = (tw as i32 + s(20), s(26));
        let Some(canvas) = Canvas::new(w, h) else { return };
        canvas.fill_rounded(0.0, 0.0, w as f32, h as f32, s(6) as f32, argb(245, 0x1A, 0x1D, 0x23));
        canvas.stroke_rounded(0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, s(6) as f32, 1.0, argb(30, 255, 255, 255));
        canvas.text(
            text,
            0.0,
            0.0,
            w as f32,
            h as f32,
            s(12) as f32,
            argb(255, 0xE6, 0xEA, 0xF0),
            false,
            false,
            Align::Center,
        );

        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                cx - w / 2,
                top_y - h - s(8),
                w,
                h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            canvas.present(hwnd);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = KillTimer(hwnd, TIMER_HIDE);
            if let Some(ms) = auto_hide_ms {
                SetTimer(hwnd, TIMER_HIDE, ms, None);
            }
        }
    }

    pub fn hide() {
        let cached = TIP.load(Ordering::Relaxed);
        if cached != 0 {
            unsafe {
                let _ = ShowWindow(HWND(cached as *mut core::ffi::c_void), SW_HIDE);
            }
        }
    }

    unsafe extern "system" fn tip_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_TIMER && wparam.0 == TIMER_HIDE {
            let _ = KillTimer(hwnd, TIMER_HIDE);
            let _ = ShowWindow(hwnd, SW_HIDE);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
