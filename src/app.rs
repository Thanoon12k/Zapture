//! Application state, the recording session lifecycle, and the actions
//! triggered by buttons, hotkeys, and the tray.
//!
//! Threading model: the UI thread owns `App` (thread-local). A session spawns
//! three workers — screen capture, optional audio capture, and the encoder —
//! wired together with channels. Capture threads stamp samples with a shared
//! pause-aware clock so pausing simply removes wall time from the media
//! timeline.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU8, Ordering};
use std::sync::mpsc::{channel, sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetSystemMetrics, GetWindowRect,
    PostMessageW, PostQuitMessage, SetWindowPos, ShowWindow, HWND_TOPMOST, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE,
};

use crate::config::{auto_bitrate_kbps, AudioSource, BgStyle, Config, LangPref};
use crate::i18n::{t, Lang, Msg};
use crate::win::{audio, encoder, screen, ui, util, MSG_SHOT_DONE};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Recording,
    Paused,
    Saving,
}

static STATE_CACHE: AtomicU8 = AtomicU8::new(0);

/// State readable from window procedures without touching the `App` cell.
pub fn cached_state() -> State {
    match STATE_CACHE.load(Ordering::Relaxed) {
        1 => State::Recording,
        2 => State::Paused,
        3 => State::Saving,
        _ => State::Idle,
    }
}

fn set_state(app: &mut App, s: State) {
    app.state = s;
    STATE_CACHE.store(
        match s {
            State::Idle => 0,
            State::Recording => 1,
            State::Paused => 2,
            State::Saving => 3,
        },
        Ordering::Relaxed,
    );
}

/// Monotonic clock that can subtract paused spans; media timestamps come from
/// here so audio and video stay on one timeline across pause/resume.
pub struct Clock {
    start: AtomicI64,
    pause_at: AtomicI64,
    paused_total: AtomicI64,
}

impl Clock {
    fn new() -> Clock {
        Clock {
            start: AtomicI64::new(0),
            pause_at: AtomicI64::new(0),
            paused_total: AtomicI64::new(0),
        }
    }

    fn begin(&self) {
        self.start.store(util::qpc(), Ordering::Release);
    }

    fn pause(&self) {
        self.pause_at.store(util::qpc(), Ordering::Release);
    }

    fn resume(&self) {
        let at = self.pause_at.swap(0, Ordering::AcqRel);
        if at != 0 {
            self.paused_total.fetch_add(util::qpc() - at, Ordering::AcqRel);
        }
    }

    /// 100 ns units since `begin`, with paused time removed.
    pub fn rel_100ns(&self) -> i64 {
        let delta = util::qpc() - self.start.load(Ordering::Acquire) - self.paused_total.load(Ordering::Acquire);
        (delta as i128 * 10_000_000 / util::qpf() as i128) as i64
    }
}

/// State shared with the capture threads. `x`/`y` follow the frame while it
/// is dragged mid-recording; `w`/`h` are fixed for the session.
pub struct Shared {
    pub x: AtomicI32,
    pub y: AtomicI32,
    pub armed: AtomicBool,
    pub stop: AtomicBool,
    /// Live toggle for the cursor/click highlight, read each captured frame.
    pub highlight: AtomicBool,
    /// Recorded clicks (media-ms, region-relative x, y) for export auto-zoom.
    pub clicks: Mutex<Vec<(i64, i32, i32)>>,
    pub clock: Clock,
}

impl Shared {
    fn new(x: i32, y: i32) -> Shared {
        Shared {
            x: AtomicI32::new(x),
            y: AtomicI32::new(y),
            armed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            highlight: AtomicBool::new(false),
            clicks: Mutex::new(Vec::new()),
            clock: Clock::new(),
        }
    }
}

pub enum EncMsg {
    Video { buf: Vec<u8>, ts: i64 },
    Audio { pcm: Vec<i16>, ts: i64, frames: u32 },
    Finish,
}

struct Session {
    shared: Arc<Shared>,
    tx: SyncSender<EncMsg>,
    workers: Vec<JoinHandle<()>>,
    out_name: String,
}

pub struct App {
    pub cfg: Config,
    pub state: State,
    pub frame: HWND,
    pub bar: HWND,
    pub audio: AudioSource,
    pub lang: Lang,
    pub visible: bool,
    pub wda_ok: bool,
    pub quit_after_save: bool,
    cam: Option<(Arc<crate::win::camera::Camera>, JoinHandle<()>)>,
    pending_out: Option<String>,
    session: Option<Session>,
}

impl App {
    pub fn new(cfg: Config, frame: HWND, bar: HWND, wda_ok: bool, lang: Lang) -> App {
        let audio = cfg.audio;
        App {
            cfg,
            state: State::Idle,
            frame,
            bar,
            audio,
            lang,
            visible: true,
            wda_ok,
            quit_after_save: false,
            cam: None,
            pending_out: None,
            session: None,
        }
    }

    pub fn elapsed_text(&self) -> String {
        match self.state {
            State::Idle => "00:00:00".into(),
            State::Saving => t(self.lang, Msg::Saving).into(),
            _ => {
                let secs = self
                    .session
                    .as_ref()
                    .map(|s| s.shared.clock.rel_100ns() / 10_000_000)
                    .unwrap_or(0)
                    .max(0);
                format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
            }
        }
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn init(app: App) {
    STATE_CACHE.store(0, Ordering::Relaxed);
    APP.with(|cell| *cell.borrow_mut() = Some(app));
}

/// Runs `f` against the app unless the cell is already borrowed (re-entrant
/// window message); callers treat `None` as "try again on the next repaint".
pub fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

fn lang() -> Lang {
    with(|a| a.lang).unwrap_or(Lang::En)
}

// ---------------------------------------------------------------------------
// Actions (buttons, hotkeys, menu, and tray land here, on the UI thread)
// ---------------------------------------------------------------------------

pub fn act_toggle_record() {
    match cached_state() {
        State::Idle => start_record(),
        State::Recording => {
            with(|a| {
                if let Some(s) = &a.session {
                    s.shared.armed.store(false, Ordering::Release);
                    s.shared.clock.pause();
                }
                set_state(a, State::Paused);
            });
            ui::refresh_all();
        }
        State::Paused => {
            with(|a| {
                if let Some(s) = &a.session {
                    s.shared.clock.resume();
                    s.shared.armed.store(true, Ordering::Release);
                }
                set_state(a, State::Recording);
            });
            ui::refresh_all();
        }
        State::Saving => {}
    }
}

fn start_record() {
    let Some((frame_hwnd, bar_hwnd, cfg, source, lang)) =
        with(|a| (a.frame, a.bar, a.cfg.clone(), a.audio, a.lang))
    else {
        return;
    };

    let (x, y, mut w, mut h) = ui::frame::capture_rect(frame_hwnd);
    w &= !1; // H.264 wants even dimensions
    h &= !1;
    if w < 64 || h < 64 {
        util::alert(t(lang, Msg::ErrSmall));
        return;
    }
    ui::frame::set_capture_rect(frame_hwnd, x, y, w, h);

    let dir = util::videos_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        util::alert(&format!("{}\n{}\n{e}", t(lang, Msg::ErrFolder), dir.display()));
        return;
    }
    let out_name = format!("REC_{}.mp4", util::timestamp());
    let path = dir.join(&out_name);

    let shared = Arc::new(Shared::new(x, y));
    shared.highlight.store(cfg.highlight, Ordering::Release);
    let (tx, rx) = sync_channel::<EncMsg>(32);
    let (pool_tx, pool_rx) = channel::<Vec<u8>>();
    let mut workers = Vec::new();

    // Audio first: the encoder needs the master sample rate up front.
    let mut audio_params = None;
    if source != AudioSource::Off {
        let (ready_tx, ready_rx) = channel();
        let sh = shared.clone();
        let atx = tx.clone();
        workers.push(std::thread::spawn(move || audio::run(sh, atx, ready_tx, source)));
        match ready_rx.recv_timeout(std::time::Duration::from_millis(2000)) {
            Ok(Ok(rate)) => audio_params = Some(encoder::AudioParams { rate }),
            Ok(Err(e)) => util::alert(&format!("{}\n({e})", t(lang, Msg::ErrAudioUnavailable))),
            Err(_) => util::alert(t(lang, Msg::ErrAudioTimeout)),
        }
    }

    // Optional caption bar welded to one edge; it enlarges the encoded frame.
    let caption = crate::win::caption::Caption::build(w, h, &cfg).map(Arc::new);
    let (out_w, out_h) = match &caption {
        Some(c) => c.out_size(w, h),
        None => (w, h),
    };

    let kbps = if cfg.bitrate_kbps == 0 {
        auto_bitrate_kbps(out_w as u32, out_h as u32, cfg.fps)
    } else {
        cfg.bitrate_kbps
    };
    let vparams = encoder::VideoParams {
        w: out_w as u32,
        h: out_h as u32,
        fps: cfg.fps,
        bitrate_bps: kbps.saturating_mul(1000),
    };

    // The encoder is deliberately not in `workers`: the stop sequence joins
    // the capture threads first, then sends Finish, and the encoder reports
    // back with MSG_ENC_DONE once the file is finalized.
    let notify = encoder::HwndSend::from(bar_hwnd);
    let enc_pool = pool_tx.clone();
    std::thread::spawn(move || encoder::run(path, vparams, audio_params, rx, enc_pool, notify));

    let sh = shared.clone();
    let vtx = tx.clone();
    let (fps, cursor) = (cfg.fps, cfg.cursor);
    workers.push(std::thread::spawn(move || {
        screen::capture_loop(sh, w, h, fps, cursor, caption, vtx, pool_rx, pool_tx)
    }));

    shared.clock.begin();
    shared.armed.store(true, Ordering::Release);
    ui::frame::set_session(frame_hwnd, Some(shared.clone()));

    with(|a| {
        a.session = Some(Session { shared, tx, workers, out_name });
        set_state(a, State::Recording);
    });
    ui::refresh_all();
}

pub fn act_stop() {
    let taken = with(|a| {
        if !matches!(a.state, State::Recording | State::Paused) {
            return None;
        }
        set_state(a, State::Saving);
        let sess = a.session.take();
        if let Some(s) = &sess {
            a.pending_out = Some(s.out_name.clone());
        }
        sess.map(|s| (s, a.frame))
    })
    .flatten();

    let Some((sess, frame_hwnd)) = taken else { return };
    ui::frame::set_session(frame_hwnd, None);
    sess.shared.armed.store(false, Ordering::Release);
    sess.shared.stop.store(true, Ordering::Release);

    let shared = sess.shared.clone();
    let out_name = sess.out_name.clone();
    let Session { tx, workers, .. } = sess;
    std::thread::spawn(move || {
        for worker in workers {
            let _ = worker.join();
        }
        let _ = tx.send(EncMsg::Finish);
        // Persist recorded clicks next to the MP4 so export can drive the
        // optional auto-zoom. One "ms x y" line per click.
        if let Ok(clicks) = shared.clicks.lock() {
            if !clicks.is_empty() {
                let mut s = String::with_capacity(clicks.len() * 12);
                for (ms, x, y) in clicks.iter() {
                    s.push_str(&format!("{ms} {x} {y}\n"));
                }
                let path = util::videos_dir().join(&out_name).with_extension("clicks");
                let _ = std::fs::write(path, s);
            }
        }
    });
    ui::refresh_all();
}

/// Called from the bar's window procedure when MSG_ENC_DONE arrives.
pub fn on_encoder_done(err: Option<String>) {
    let (quit, name, lang) = with(|a| {
        set_state(a, State::Idle);
        (a.quit_after_save, a.pending_out.take(), a.lang)
    })
    .unwrap_or((false, None, Lang::En));

    match err {
        None => {
            if let Some(n) = name {
                ui::bar::toast(&format!("{}Recordings\\{n}", t(lang, Msg::SavedPrefix)));
            }
        }
        Some(e) => util::alert(&format!("{}\n{e}", t(lang, Msg::ErrRecFailed))),
    }
    ui::refresh_all();
    if quit {
        unsafe { PostQuitMessage(0) };
    }
}

/// Freeform region snip: freeze the desktop, drag a rectangle, copy it to the
/// clipboard.
pub fn act_snip() {
    ui::snip::start();
}

pub fn act_shot() {
    if cached_state() == State::Saving {
        return;
    }
    let Some((frame_hwnd, bar_hwnd, cursor, bg, wda_ok, lang)) =
        with(|a| (a.frame, a.bar, a.cfg.cursor, a.cfg.shot_bg, a.wda_ok, a.lang))
    else {
        return;
    };
    let (x, y, w, h) = ui::frame::capture_rect(frame_hwnd);
    if w < 8 || h < 8 {
        return;
    }
    let dir = util::screenshots_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        util::alert(&format!("{}\n{}\n{e}", t(lang, Msg::ErrFolder), dir.display()));
        return;
    }
    let name = format!("SHOT_{}.png", util::timestamp());
    let path = dir.join(&name);

    match screen::snapshot(x, y, w, h, cursor) {
        Err(e) => util::alert(&format!("{}\n{e}", t(lang, Msg::ErrShotFailed))),
        Ok(pixels) => {
            // Optional decorative backdrop (padding, rounded corners, gradient).
            let (pixels, ow, oh) = match crate::win::compose::adorn(&pixels, w, h, bg) {
                Some((p, ow, oh)) => (p, ow as u32, oh as u32),
                None => (pixels, w as u32, h as u32),
            };
            if !ui::snip::to_clipboard(bar_hwnd, &pixels, ow as i32, oh as i32) {
                ui::bar::toast("Clipboard busy; screenshot will still be saved.");
            }
            // The white blink would land in the recording on systems without
            // capture exclusion, so skip it there while a session is running.
            if wda_ok || cached_state() == State::Idle {
                ui::frame::flash(frame_hwnd);
            }
            let notify = encoder::HwndSend::from(bar_hwnd);
            std::thread::spawn(move || {
                let (ok, text) = match screen::save_png(&path, ow, oh, &pixels) {
                    Ok(()) => (1usize, name),
                    Err(e) => (0usize, e),
                };
                let boxed = Box::into_raw(Box::new(text));
                unsafe {
                    let _ = PostMessageW(notify.hwnd(), MSG_SHOT_DONE, WPARAM(ok), LPARAM(boxed as isize));
                }
            });
        }
    }
}

/// Called from the bar's window procedure when MSG_SHOT_DONE arrives.
pub fn on_shot_done(ok: bool, text: String) {
    let lang = lang();
    if ok {
        ui::bar::toast(&format!("{}Screenshots\\{text}", t(lang, Msg::SavedPrefix)));
    } else {
        util::alert(&format!("{}\n{text}", t(lang, Msg::ErrShotFailed)));
    }
}

fn audio_label(source: AudioSource) -> Msg {
    match source {
        AudioSource::Off => Msg::AudioOff,
        AudioSource::System => Msg::AudioSystem,
        AudioSource::Mic => Msg::AudioMic,
        AudioSource::Both => Msg::AudioBoth,
    }
}

pub fn act_set_audio(source: AudioSource) {
    if cached_state() != State::Idle {
        return; // the audio graph is fixed while a session is running
    }
    let lang = with(|a| {
        a.audio = source;
        a.cfg.audio = source;
        a.lang
    })
    .unwrap_or(Lang::En);
    ui::refresh_all();
    ui::bar::toast(t(lang, audio_label(source)));
}

pub fn act_cycle_audio() {
    if let Some(next) = with(|a| a.audio.next()) {
        act_set_audio(next);
    }
}

/// Set the screenshot / snip backdrop (the "Screenshot background" submenu).
/// Persisted immediately — a hard shutdown must not lose the choice.
pub fn act_set_bg(style: BgStyle) {
    let lang = with(|a| {
        a.cfg.shot_bg = style;
        let _ = a.cfg.save(&util::config_path());
        a.lang
    })
    .unwrap_or(Lang::En);
    ui::refresh_all();
    let name = match style {
        BgStyle::None => Msg::BgNone,
        BgStyle::Graphite => Msg::BgGraphite,
        BgStyle::Ocean => Msg::BgOcean,
        BgStyle::Sunset => Msg::BgSunset,
        BgStyle::Snow => Msg::BgSnow,
        BgStyle::Forest => Msg::BgForest,
        BgStyle::Rose => Msg::BgRose,
        BgStyle::Lavender => Msg::BgLavender,
        BgStyle::Sky => Msg::BgSky,
    };
    ui::bar::toast(&format!("{} · {}", t(lang, Msg::MenuBackground), t(lang, name)));
}

pub fn act_set_lang(pref: LangPref) {
    with(|a| {
        a.cfg.language = pref;
        a.lang = match pref {
            LangPref::En => Lang::En,
            LangPref::Zh => Lang::Zh,
            LangPref::Auto => crate::win::system_lang(),
        };
    });
    ui::refresh_all();
}

fn set_visible(show: bool) {
    let Some((frame, bar)) = with(|a| {
        a.visible = show;
        (a.frame, a.bar)
    }) else {
        return;
    };
    unsafe {
        if show {
            let _ = ShowWindow(frame, SW_SHOWNOACTIVATE);
            let _ = ShowWindow(bar, SW_SHOWNOACTIVATE);
            // Re-assert topmost z-order. The nudge forces the layered frame to
            // recomposite; without it the frame can return blank after a
            // hide/show cycle, while the bar (which gets repositioned) repaints.
            for h in [frame, bar] {
                let _ = SetWindowPos(
                    h,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        } else {
            let _ = ShowWindow(frame, SW_HIDE);
            let _ = ShowWindow(bar, SW_HIDE);
        }
    }
    if show {
        ui::refresh_all();
    } else {
        ui::bar::hide_tip();
    }
}

/// Tray left-click: hide or reveal the frame and the bar (recording carries
/// on either way; hotkeys stay live).
pub fn act_toggle_visibility() {
    let show = with(|a| !a.visible).unwrap_or(true);
    set_visible(show);
}

/// The bar's close button minimizes to the tray rather than quitting; the app
/// only truly exits from the tray's (or the menu's) Quit item.
pub fn act_hide() {
    set_visible(false);
}

pub fn act_open_export() {
    ui::export_win::open(lang());
}

/// Toggle the webcam picture-in-picture overlay. It is an independent layered
/// window (kept in `App` so its capture thread outlives this call), so it
/// stays visible in recordings even when the frame and bar are hidden.
pub fn act_toggle_camera() {
    if ui::cam_win::is_open() {
        with(|a| {
            if let Some(r) = ui::cam_win::rect() {
                a.cfg.cam_frame = Some(r);
            }
            a.cfg.cam_circle = ui::cam_win::is_circle();
            if let Some((cam, worker)) = a.cam.take() {
                cam.request_stop();
                let _ = worker.join();
            }
        });
        ui::cam_win::close();
    } else {
        match crate::win::camera::start() {
            Some((cam, worker)) => {
                let (circle, place) = with(|a| (a.cfg.cam_circle, a.cfg.cam_frame)).unwrap_or((true, None));
                ui::cam_win::open(cam.clone(), circle, place);
                with(|a| a.cam = Some((cam, worker)));
            }
            None => util::alert(t(lang(), Msg::ErrNoCamera)),
        }
    }
    ui::refresh_all();
}

/// Stop the webcam thread and remember its shape and position. Called on quit.
pub fn stop_camera_and_save(a: &mut App) {
    if ui::cam_win::is_open() {
        if let Some(r) = ui::cam_win::rect() {
            a.cfg.cam_frame = Some(r);
        }
        a.cfg.cam_circle = ui::cam_win::is_circle();
    }
    if let Some((cam, worker)) = a.cam.take() {
        cam.request_stop();
        let _ = worker.join();
    }
}

/// Flip the cursor/click highlight. Applies live if a recording is running.
pub fn act_toggle_highlight() {
    let lang = with(|a| {
        a.cfg.highlight = !a.cfg.highlight;
        if let Some(s) = &a.session {
            s.shared.highlight.store(a.cfg.highlight, Ordering::Release);
        }
        a.lang
    })
    .unwrap_or(Lang::En);
    ui::refresh_all();
    ui::bar::toast(t(lang, Msg::MenuHighlight));
}

pub fn act_toggle_autozoom() {
    let lang = with(|a| {
        a.cfg.autozoom = !a.cfg.autozoom;
        a.lang
    })
    .unwrap_or(Lang::En);
    ui::refresh_all();
    ui::bar::toast(t(lang, Msg::MenuAutozoom));
}

/// Open the caption bar editor.
pub fn act_open_caption() {
    ui::caption_win::open();
}

/// Store the caption bar settings (persisted immediately). They take effect on
/// the next recording — the encoder's frame size is fixed for a session.
pub fn act_set_caption(edge: crate::config::CaptionEdge, text: String, size: i32) {
    let lang = with(|a| {
        a.cfg.caption_edge = edge;
        a.cfg.caption_text = text;
        a.cfg.caption_size = size;
        let _ = a.cfg.save(&util::config_path());
        a.lang
    })
    .unwrap_or(Lang::En);
    ui::refresh_all();
    ui::bar::toast(t(lang, Msg::MenuCaption));
}

pub fn act_toggle_autostart() {
    if let Err(e) = crate::win::autostart::toggle() {
        let lang = lang();
        util::alert(&format!("{}\n{e}", t(lang, Msg::ErrAutostart)));
    }
}

/// Snap the capture frame to the foreground application window. Because all of
/// Cadre's own windows are `WS_EX_NOACTIVATE`, clicking the bar button never
/// steals the foreground, so this targets whatever app the user was last using:
/// bring a window to the front, then hit Snap.
pub fn act_snap_to_window() {
    if cached_state() != State::Idle {
        return; // capture dimensions are fixed once a session starts
    }
    let Some((frame, bar, lang)) = with(|a| (a.frame, a.bar, a.lang)) else {
        return;
    };

    let fg = unsafe { GetForegroundWindow() };
    let (desktop, shell) = unsafe { (GetDesktopWindow(), GetShellWindow()) };
    if fg.0.is_null() || fg.0 == frame.0 || fg.0 == bar.0 || fg.0 == desktop.0 || fg.0 == shell.0 {
        util::alert(t(lang, Msg::ErrSnapNone));
        return;
    }

    // DWM's extended frame bounds exclude the invisible resize border Win10/11
    // pads windows with, so the frame hugs the visible window instead of
    // overshooting it. Fall back to the raw window rect if DWM declines.
    let mut r = RECT::default();
    let via_dwm = unsafe {
        DwmGetWindowAttribute(
            fg,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok()
    };
    if !via_dwm {
        unsafe {
            let _ = GetWindowRect(fg, &mut r);
        }
    }

    // Clamp into the virtual screen, then even the dimensions the way a real
    // recording start would (H.264 wants even width/height).
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let left = r.left.max(vx);
    let top = r.top.max(vy);
    let right = r.right.min(vx + vw);
    let bottom = r.bottom.min(vy + vh);
    let w = ((right - left) & !1).max(64);
    let h = ((bottom - top) & !1).max(64);

    ui::frame::set_capture_rect(frame, left, top, w, h);
    ui::frame::repaint(frame);
    ui::bar::place(bar, frame);
}

pub fn act_quit() {
    match cached_state() {
        State::Recording | State::Paused => {
            with(|a| a.quit_after_save = true);
            act_stop();
        }
        State::Saving => {
            with(|a| a.quit_after_save = true);
        }
        State::Idle => unsafe { PostQuitMessage(0) },
    }
}
