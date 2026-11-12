//! The export dialog: choose a recording, pick a speed and a target
//! resolution, optionally compress, and re-encode. Custom-painted to match
//! the rest of Cadre; a worker thread does the transcode and reports back.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_FILEMUSTEXIST, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowRect, LoadCursorW, LoadIconW, RegisterClassW,
    SetForegroundWindow, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, HTCAPTION,
    HTCLIENT, IDC_ARROW, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_DESTROY,
    WM_LBUTTONUP, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::i18n::{t, Lang, Msg};
use crate::win::export::{self, Format, ProbeInfo, Settings};
use crate::win::encoder::HwndSend;
use crate::win::ui::{self, paint::{argb, Align, Canvas}};
use crate::win::{util, MSG_EXPORT_DONE, MSG_EXPORT_PROGRESS};

const W: i32 = 384;
const H: i32 = 452;
const P: i32 = 18;

const SPEEDS: [f64; 5] = [1.0, 2.0, 4.0, 6.0, 8.0];
const RES_H: [Option<u32>; 4] = [None, Some(1080), Some(720), Some(480)];
const DURATIONS: [u32; 4] = [0, 5, 10, 15]; // GIF length cap in seconds; 0 = full

const BG: u32 = argb(250, 0x16, 0x18, 0x1E);
const CARD: u32 = argb(255, 0x1E, 0x21, 0x28);
const PILL: u32 = argb(255, 0x25, 0x28, 0x30);
const BLUE: u32 = argb(255, 0x4C, 0x8D, 0xFF);
const ON_BLUE: u32 = argb(255, 0x0A, 0x1A, 0x33);
const FG: u32 = argb(255, 0xE8, 0xEA, 0xED);
const FG_DIM: u32 = argb(150, 0xE8, 0xEA, 0xED);
const FG_FAINT: u32 = argb(255, 0x8A, 0x90, 0x9C);
const GREEN: u32 = argb(255, 0x5D, 0xCA, 0xA5);
const RED: u32 = argb(255, 0xE5, 0x6B, 0x6B);
const WHITE: u32 = argb(255, 255, 255, 255);

#[derive(Clone, Copy, PartialEq)]
enum Act {
    Close,
    Format(bool), // true = GIF
    Choose,
    Speed(usize),
    Res(usize),
    Compress,
    Quality(usize),
    Duration(usize),
    Export,
}

struct Ui {
    src: Option<PathBuf>,
    info: Option<ProbeInfo>,
    gif: bool,
    speed_idx: usize,
    res_idx: usize,
    compress: bool,
    quality_idx: usize,
    dur_idx: usize,
    running: bool,
    progress: i32,
    message: Option<(String, bool)>,
    cancel: Arc<AtomicBool>,
    lang: Lang,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            src: None,
            info: None,
            gif: false,
            speed_idx: 1,
            res_idx: 0,
            compress: false,
            quality_idx: 2,
            dur_idx: 0,
            running: false,
            progress: 0,
            message: None,
            cancel: Arc::new(AtomicBool::new(false)),
            lang: Lang::En,
        }
    }
}

impl Ui {
    fn settings(&self) -> Settings {
        let (autozoom, bg) = crate::app::with(|a| (a.cfg.autozoom, a.cfg.bg))
            .unwrap_or((false, crate::config::BgStyle::None));
        Settings {
            format: if self.gif { Format::Gif } else { Format::Mp4 },
            speed: SPEEDS[self.speed_idx],
            target_h: RES_H[self.res_idx],
            compress: self.compress,
            quality: self.quality_idx as u8,
            max_secs: DURATIONS[self.dur_idx],
            autozoom,
            bg,
        }
    }
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

static HWND_CELL: AtomicIsize = AtomicIsize::new(0);

fn window() -> Option<HWND> {
    let v = HWND_CELL.load(Ordering::Relaxed);
    (v != 0).then_some(HWND(v as *mut core::ffi::c_void))
}

/// Open (or focus) the export dialog.
pub fn open(lang: Lang) {
    UI.with(|u| u.borrow_mut().lang = lang);
    if let Some(hwnd) = window() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        repaint(Some(hwnd));
        return;
    }
    match create() {
        Ok(hwnd) => {
            HWND_CELL.store(hwnd.0 as isize, Ordering::Relaxed);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
            repaint(Some(hwnd));
        }
        Err(e) => util::alert(&format!("Could not open the export window:\n{e}")),
    }
}

fn create() -> Result<HWND, String> {
    unsafe {
        let hinst = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: LoadIconW(HINSTANCE(hinst.0), PCWSTR(1 as *const u16)).unwrap_or_default(),
            lpszClassName: w!("CadreExport"),
            ..Default::default()
        };
        RegisterClassW(&class);

        // WS_EX_LAYERED is mandatory: the whole dialog is drawn with
        // UpdateLayeredWindow (via Canvas::present), which silently does
        // nothing on a non-layered window. No WS_EX_NOACTIVATE here — unlike
        // the frame and bar, this dialog must take focus for the file picker.
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("CadreExport"),
            w!("Export"),
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

        let k = ui::scale_of(hwnd);
        let (pw, ph) = (ui::px(W, k), ui::px(H, k));
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let x = wa.left + (wa.right - wa.left - pw) / 2;
        let y = wa.top + (wa.bottom - wa.top - ph) / 2;
        let _ = SetWindowPos(hwnd, HWND::default(), x, y, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE);
        Ok(hwnd)
    }
}

struct Slots {
    items: Vec<(Act, RECT)>,
    title_h: i32,
    format: Vec<RECT>,
    source: RECT,
    speed: Vec<RECT>,
    res: Vec<RECT>,
    compress_box: RECT,
    quality: Vec<RECT>,
    duration: Vec<RECT>,
    export: RECT,
    close: RECT,
    k: f32,
}

fn seg(x0: i32, y: i32, total: i32, h: i32, n: usize, gap: i32) -> Vec<RECT> {
    let pw = (total - (n as i32 - 1) * gap) / n as i32;
    (0..n)
        .map(|i| {
            let x = x0 + i as i32 * (pw + gap);
            RECT { left: x, top: y, right: x + pw, bottom: y + h }
        })
        .collect()
}

fn layout(hwnd: HWND) -> Slots {
    let k = ui::scale_of(hwnd);
    let s = |v: i32| ui::px(v, k);
    let cw = s(W) - 2 * s(P);
    let x0 = s(P);
    let mut items = Vec::new();

    let gif = UI.with(|u| u.borrow().gif);

    let close = RECT { left: s(W) - s(P) - s(22), top: s(14), right: s(W) - s(P) + s(2), bottom: s(14) + s(24) };
    let format = seg(x0, s(56), s(156), s(30), 2, s(6));
    let source = RECT { left: x0, top: s(98), right: x0 + cw, bottom: s(98) + s(40) };
    let speed = seg(x0, s(170), cw, s(30), 5, s(6));
    let res = seg(x0, s(230), cw, s(30), 4, s(6));
    let compress_box = RECT { left: x0, top: s(292), right: x0 + s(20), bottom: s(292) + s(20) };
    let quality = seg(x0, s(290), s(240), s(30), 3, s(6));
    let duration = seg(x0, s(348), cw, s(30), 4, s(6));
    let export = RECT { left: x0, top: s(400), right: x0 + cw, bottom: s(400) + s(40) };

    items.push((Act::Close, close));
    items.push((Act::Format(false), format[0]));
    items.push((Act::Format(true), format[1]));
    items.push((Act::Choose, source));
    for (i, r) in speed.iter().enumerate() {
        items.push((Act::Speed(i), *r));
    }
    for (i, r) in res.iter().enumerate() {
        items.push((Act::Res(i), *r));
    }
    if gif {
        for (i, r) in quality.iter().enumerate() {
            items.push((Act::Quality(i), *r));
        }
        for (i, r) in duration.iter().enumerate() {
            items.push((Act::Duration(i), *r));
        }
    } else {
        let compress_hit = RECT { left: x0, top: s(290), right: x0 + cw, bottom: s(290) + s(24) };
        items.push((Act::Compress, compress_hit));
    }
    items.push((Act::Export, export));

    Slots { items, title_h: s(48), format, source, speed, res, compress_box, quality, duration, export, close, k }
}

fn hit(hwnd: HWND, x: i32, y: i32) -> Option<Act> {
    let l = layout(hwnd);
    l.items
        .iter()
        .find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
        .map(|(a, _)| *a)
}

fn fmt_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{} MB", (bytes / 1_000_000).max(1))
    }
}

fn repaint(hwnd: Option<HWND>) {
    let Some(hwnd) = hwnd else { return };
    let l = layout(hwnd);
    let k = l.k;
    let Some(c) = Canvas::new(ui::px(W, k), ui::px(H, k)) else { return };
    let s = |v: i32| ui::px(v, k) as f32;
    let sf = |v: f32| v * k;
    let fw = ui::px(W, k) as f32;
    let fh = ui::px(H, k) as f32;

    c.fill_rounded(0.0, 0.0, fw, fh, s(14), BG);
    c.stroke_rounded(0.5, 0.5, fw - 1.0, fh - 1.0, s(14), 1.0, argb(30, 255, 255, 255));

    UI.with(|u| {
        let u = u.borrow();
        let lang = u.lang;

        c.text(t(lang, Msg::ExportTitle), s(P), s(12), s(240), s(26), s(16), FG, false, true, Align::Left);
        let cx = (l.close.left + l.close.right) as f32 / 2.0;
        let cy = (l.close.top + l.close.bottom) as f32 / 2.0;
        let d = s(6);
        c.line(cx - d, cy - d, cx + d, cy + d, sf(1.6), FG_DIM);
        c.line(cx - d, cy + d, cx + d, cy - d, sf(1.6), FG_DIM);
        c.text(t(lang, Msg::ExportSilent), s(P), s(36), s(320), s(16), s(11), FG_FAINT, false, false, Align::Left);

        // format toggle
        pill(&c, &l.format[0], "MP4", !u.gif, true, k);
        pill(&c, &l.format[1], "GIF", u.gif, true, k);

        // source card
        let (sl, stp, sr, sb) = (l.source.left as f32, l.source.top as f32, l.source.right as f32, l.source.bottom as f32);
        c.fill_rounded(sl, stp, sr - sl, sb - stp, s(8), CARD);
        let icy = (stp + sb) / 2.0;
        let ix = sl + s(16);
        c.fill_rounded(ix, icy - s(3), s(15), s(11), sf(2.0), BLUE);
        c.fill_rounded(ix, icy - s(7), s(7), s(4), sf(1.5), BLUE);
        let (label, col) = match &u.src {
            Some(p) => (p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), FG),
            None => (t(lang, Msg::ExportChoose).to_string(), FG_DIM),
        };
        c.text(&label, ix + s(26), stp, sr - sl - s(96), sb - stp, s(13), col, false, false, Align::Left);
        c.text("Choose", sr - s(70), stp, s(58), sb - stp, s(12), BLUE, false, false, Align::Right);

        // speed
        c.text(t(lang, Msg::ExportSpeed), s(P), s(148), s(200), s(18), s(12), FG_FAINT, false, false, Align::Left);
        for (i, r) in l.speed.iter().enumerate() {
            pill(&c, r, &format!("{}×", SPEEDS[i] as u32), i == u.speed_idx, true, k);
        }

        // resolution
        c.text(t(lang, Msg::ExportResolution), s(P), s(208), s(200), s(18), s(12), FG_FAINT, false, false, Align::Left);
        for (i, r) in l.res.iter().enumerate() {
            let native_h = u.info.map(|inf| inf.h).unwrap_or(0);
            let (label, enabled) = match RES_H[i] {
                None => (t(lang, Msg::ExportOriginal).to_string(), true),
                Some(h) => (format!("{h}p"), native_h == 0 || h < native_h),
            };
            pill(&c, r, &label, i == u.res_idx, enabled, k);
        }

        if u.gif {
            // quality
            c.text(t(lang, Msg::ExportQuality), s(P), s(268), s(200), s(18), s(12), FG_FAINT, false, false, Align::Left);
            let ql = [Msg::ExportQLow, Msg::ExportQMed, Msg::ExportQHigh];
            for (i, r) in l.quality.iter().enumerate() {
                pill(&c, r, t(lang, ql[i]), i == u.quality_idx, true, k);
            }
            // duration
            c.text(t(lang, Msg::ExportLength), s(P), s(326), s(200), s(18), s(12), FG_FAINT, false, false, Align::Left);
            for (i, r) in l.duration.iter().enumerate() {
                let label = if DURATIONS[i] == 0 {
                    t(lang, Msg::ExportFull).to_string()
                } else {
                    format!("{}s", DURATIONS[i])
                };
                pill(&c, r, &label, i == u.dur_idx, true, k);
            }
        } else {
            // compress checkbox
            let (bl, bt, br) = (l.compress_box.left as f32, l.compress_box.top as f32, l.compress_box.right as f32);
            if u.compress {
                c.fill_rounded(bl, bt, s(20), s(20), s(5), BLUE);
                c.line(bl + s(5), bt + s(10), bl + s(9), bt + s(14), sf(2.0), WHITE);
                c.line(bl + s(9), bt + s(14), bl + s(15), bt + s(6), sf(2.0), WHITE);
            } else {
                c.stroke_rounded(bl + 0.5, bt + 0.5, s(19), s(19), s(5), sf(1.4), FG_DIM);
            }
            c.text(t(lang, Msg::ExportCompress), br + s(10), bt - s(2), s(320), s(24), s(12), FG, false, false, Align::Left);
        }

        // estimate line
        if let Some(info) = &u.info {
            let set = u.settings();
            let (_, th, src_fps) = export::plan_dimensions(info, set.target_h);
            let fps = match set.format {
                Format::Gif => export::gif_fps(set.quality, src_fps),
                Format::Mp4 => src_fps,
            };
            let head = t(lang, Msg::ExportEstimate);
            let line = match export::estimate_bytes(info, &set) {
                Some(bytes) => format!("{head} · {}× · {}p · {}fps · ≈ {}", set.speed as u32, th, fps, fmt_size(bytes)),
                None => format!("{head} · {}× · {}p · {}fps", set.speed as u32, th, fps),
            };
            c.text(&line, s(P), s(384), fw - 2.0 * s(P), s(18), s(11), FG_FAINT, false, false, Align::Left);
        }

        // export button / progress / result
        let (el, et, er, eb) = (l.export.left as f32, l.export.top as f32, l.export.right as f32, l.export.bottom as f32);
        let (ew, eh) = (er - el, eb - et);
        let ready = u.src.is_some();
        if u.running {
            c.fill_rounded(el, et, ew, eh, s(9), PILL);
            let frac = (u.progress as f32 / 1000.0).clamp(0.02, 1.0);
            c.fill_rounded(el, et, ew * frac, eh, s(9), argb(255, 0x2C, 0x5A, 0xA8));
            c.text(&format!("{}  {}%", t(lang, Msg::ExportRunning), u.progress / 10), el, et, ew, eh, s(14), FG, false, true, Align::Center);
        } else {
            let fill = if ready { BLUE } else { argb(255, 0x2A, 0x2D, 0x34) };
            let tc = if ready { ON_BLUE } else { FG_DIM };
            c.fill_rounded(el, et, ew, eh, s(9), fill);
            c.text(t(lang, Msg::ExportRun), el, et, ew, eh, s(14), tc, false, true, Align::Center);
        }

        if let Some((msg, err)) = &u.message {
            if !msg.is_empty() {
                let col = if *err { RED } else { GREEN };
                c.text(msg, s(P), eb + s(6), fw - 2.0 * s(P), s(16), s(11), col, false, false, Align::Center);
            }
        }
    });

    c.present(hwnd);
}

fn pill(c: &Canvas, r: &RECT, label: &str, selected: bool, enabled: bool, k: f32) {
    let s = |v: i32| ui::px(v, k) as f32;
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    let fill = if selected { BLUE } else { PILL };
    c.fill_rounded(x, y, w, h, s(7), fill);
    let col = if selected {
        ON_BLUE
    } else if enabled {
        FG
    } else {
        FG_DIM
    };
    c.text(label, x, y, w, h, s(12), col, false, selected, Align::Center);
}

fn choose_file(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let types = [
            COMDLG_FILTERSPEC { pszName: w!("MP4 video"), pszSpec: w!("*.mp4") },
            COMDLG_FILTERSPEC { pszName: w!("All files"), pszSpec: w!("*.*") },
        ];
        let _ = dialog.SetFileTypes(&types);
        let _ = dialog.SetOptions(FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST);
        dialog.Show(owner).ok()?;
        let item = dialog.GetResult().ok()?;
        let pw = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = pw.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(pw.0 as *const core::ffi::c_void));
        path
    }
}

fn on_choose(hwnd: HWND) {
    if let Some(path) = choose_file(hwnd) {
        let info = export::probe(&path).ok();
        UI.with(|u| {
            let mut u = u.borrow_mut();
            u.src = Some(path);
            u.info = info;
            u.message = None;
            if let (Some(inf), Some(h)) = (u.info, RES_H[u.res_idx]) {
                if h >= inf.h {
                    u.res_idx = 0;
                }
            }
        });
        repaint(Some(hwnd));
    }
}

fn start_export(hwnd: HWND) {
    let (src, info, settings, lang, running) = UI.with(|u| {
        let u = u.borrow();
        (u.src.clone(), u.info, u.settings(), u.lang, u.running)
    });
    if running {
        return;
    }
    let Some(src) = src else {
        UI.with(|u| u.borrow_mut().message = Some((t(lang, Msg::ExportPickFirst).to_string(), true)));
        repaint(Some(hwnd));
        return;
    };
    let info = info.unwrap_or(ProbeInfo { w: 1920, h: 1080, fps: 30, dur_100ns: 0 });
    let dst = export::output_path(&src, &settings);

    let cancel = Arc::new(AtomicBool::new(false));
    UI.with(|u| {
        let mut u = u.borrow_mut();
        u.running = true;
        u.progress = 0;
        u.message = None;
        u.cancel = cancel.clone();
    });
    repaint(Some(hwnd));

    let notify = HwndSend::from(hwnd);
    std::thread::spawn(move || export::run(src, dst, settings, info, cancel, notify));
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let y = util::hiword_i(lparam.0) - r.top;
            let x = util::loword_i(lparam.0) - r.left;
            let l = layout(hwnd);
            if y < l.title_h && !(x >= l.close.left && x < l.close.right && y >= l.close.top && y < l.close.bottom) {
                return LRESULT(HTCAPTION as isize);
            }
            LRESULT(HTCLIENT as isize)
        }
        WM_LBUTTONUP => {
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            if let Some(act) = hit(hwnd, x, y) {
                let running = UI.with(|u| u.borrow().running);
                match act {
                    Act::Close => {
                        UI.with(|u| u.borrow().cancel.store(true, Ordering::Relaxed));
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                    _ if running => {}
                    Act::Format(gif) => {
                        UI.with(|u| u.borrow_mut().gif = gif);
                        repaint(Some(hwnd));
                    }
                    Act::Choose => on_choose(hwnd),
                    Act::Speed(i) => {
                        UI.with(|u| u.borrow_mut().speed_idx = i);
                        repaint(Some(hwnd));
                    }
                    Act::Res(i) => {
                        let ok = UI.with(|u| {
                            let mut u = u.borrow_mut();
                            let allowed = match (u.info, RES_H[i]) {
                                (Some(inf), Some(h)) => h < inf.h,
                                _ => true,
                            };
                            if allowed {
                                u.res_idx = i;
                            }
                            allowed
                        });
                        if ok {
                            repaint(Some(hwnd));
                        }
                    }
                    Act::Compress => {
                        UI.with(|u| {
                            let mut u = u.borrow_mut();
                            u.compress = !u.compress;
                        });
                        repaint(Some(hwnd));
                    }
                    Act::Quality(i) => {
                        UI.with(|u| u.borrow_mut().quality_idx = i);
                        repaint(Some(hwnd));
                    }
                    Act::Duration(i) => {
                        UI.with(|u| u.borrow_mut().dur_idx = i);
                        repaint(Some(hwnd));
                    }
                    Act::Export => start_export(hwnd),
                }
            }
            LRESULT(0)
        }
        MSG_EXPORT_PROGRESS => {
            UI.with(|u| u.borrow_mut().progress = wparam.0 as i32);
            repaint(Some(hwnd));
            LRESULT(0)
        }
        MSG_EXPORT_DONE => {
            let result = *Box::from_raw(lparam.0 as *mut Result<String, String>);
            UI.with(|u| {
                let mut u = u.borrow_mut();
                u.running = false;
                u.progress = 0;
                let lang = u.lang;
                u.message = Some(match result {
                    Ok(name) => (format!("{}{name}", t(lang, Msg::SavedPrefix)), false),
                    Err(e) if e == "canceled" => (String::new(), false),
                    Err(e) => (format!("{} {e}", t(lang, Msg::ExportFailed)), true),
                });
            });
            repaint(Some(hwnd));
            LRESULT(0)
        }
        WM_DESTROY => {
            UI.with(|u| u.borrow().cancel.store(true, Ordering::Relaxed));
            HWND_CELL.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
