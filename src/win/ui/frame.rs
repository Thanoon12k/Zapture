//! The capture frame: a hollow, layered, always-on-top window. Only the
//! border, resize handles, and the size chip have opaque pixels, so clicks
//! inside the region fall straight through to whatever is being recorded.
//!
//! Hit-testing rides on WM_NCHITTEST, but the drag itself is ours: the native
//! SC_MOVE / SC_SIZE loops refuse to take a window's top edge above the top of
//! the screen, which would leave the topmost strip of the desktop permanently
//! unrecordable (the capture rectangle sits `M_TOP` below the window top).
//! Tracking the mouse ourselves lets the window hang above the screen edge so
//! the capture rectangle can reach the very first row of pixels.

use std::sync::atomic::{AtomicI32, AtomicIsize, AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, KillTimer, LoadCursorW, RegisterClassW, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, CS_HREDRAW, CS_VREDRAW,
    GWLP_USERDATA, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTLEFT, HTRIGHT, HTTOP,
    HTTOPLEFT, HTTOPRIGHT, HTTRANSPARENT, IDC_ARROW, MINMAXINFO, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SPI_GETWORKAREA, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_CAPTURECHANGED,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app::{self, Shared, State};
use crate::config::Config;
use crate::win::ui::{self, paint::{argb, Align, Canvas}};
use crate::win::util;

const TIMER_FLASH: usize = 1;
const TIMER_SETTLE: usize = 2;

// Margins around the capture rectangle (base 96 dpi): room for the border,
// the handles, and the size chip above the top-left corner.
const M_SIDE: i32 = 14;
const M_TOP: i32 = 44;

static FLASH: AtomicU8 = AtomicU8::new(0);
static LAST_W: AtomicI32 = AtomicI32::new(0);
static LAST_H: AtomicI32 = AtomicI32::new(0);
static LAST_PAINT_QPC: AtomicIsize = AtomicIsize::new(0);

// Live drag state: which edge (or the body) was grabbed, the cursor position
// at grab time, and the window rectangle it started from.
static DRAG_MODE: AtomicU32 = AtomicU32::new(0);
static GRAB: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static START: [AtomicI32; 4] =
    [AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0), AtomicI32::new(0)];

fn margins(hwnd: HWND) -> (i32, i32, i32, i32) {
    let k = ui::scale_of(hwnd);
    (ui::px(M_SIDE, k), ui::px(M_TOP, k), ui::px(M_SIDE, k), ui::px(M_SIDE, k))
}

pub fn create(cfg: &Config) -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(frame_proc),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("CadreFrame"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("RegisterClassW(CadreFrame) failed".into());
        }

        // Last saved rectangle, or a 1280x720 region centered in the primary
        // work area (shrunk to fit small screens).
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let (ww, wh) = (work.right - work.left, work.bottom - work.top);
        let (x, y, w, h) = cfg.frame.unwrap_or_else(|| {
            let w = 1280.min(ww - 120).max(320);
            let h = 720.min(wh - 160).max(200);
            (work.left + (ww - w) / 2, work.top + (wh - h) / 2, w, h)
        });

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("CadreFrame"),
            w!("Cadre"),
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

        set_capture_rect(hwnd, x, y, w.max(64), h.max(64));
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Ok(hwnd)
    }
}

pub fn capture_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    let (l, t, rr, b) = margins(hwnd);
    (
        r.left + l,
        r.top + t,
        (r.right - r.left - l - rr).max(0),
        (r.bottom - r.top - t - b).max(0),
    )
}

pub fn set_capture_rect(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    let (l, t, r, b) = margins(hwnd);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            x - l,
            y - t,
            w + l + r,
            h + t + b,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Attach or detach the live session; the frame writes its position into the
/// shared atomics whenever it moves so a recording follows the drag.
pub fn set_session(hwnd: HWND, session: Option<Arc<Shared>>) {
    unsafe {
        let new = session.map(|s| Arc::into_raw(s) as isize).unwrap_or(0);
        let old = SetWindowLongPtrW(hwnd, GWLP_USERDATA, new);
        if old != 0 {
            drop(Arc::from_raw(old as *const Shared));
        }
    }
}

pub fn flash(hwnd: HWND) {
    FLASH.store(150, Ordering::Relaxed);
    repaint(hwnd);
    unsafe {
        SetTimer(hwnd, TIMER_FLASH, 30, None);
    }
}

fn border_color(state: State) -> u32 {
    match state {
        State::Idle => argb(255, 0x4C, 0x8D, 0xFF),
        State::Recording => argb(255, 0xE5, 0x48, 0x4D),
        State::Paused => argb(255, 0xE8, 0xA3, 0x3D),
        State::Saving => argb(255, 0x8A, 0x8F, 0x98),
    }
}

/// Eight handle centers on the border line of `rect` (local coords).
fn handle_points(l: i32, t: i32, w: i32, h: i32) -> [(i32, i32); 8] {
    let (cx, cy) = (l + w / 2, t + h / 2);
    let (r, b) = (l + w, t + h);
    [
        (l, t), (cx, t), (r, t),
        (l, cy), (r, cy),
        (l, b), (cx, b), (r, b),
    ]
}

const HANDLE_HITS: [u32; 8] = [
    HTTOPLEFT, HTTOP, HTTOPRIGHT,
    HTLEFT, HTRIGHT,
    HTBOTTOMLEFT, HTBOTTOM, HTBOTTOMRIGHT,
];

pub fn repaint(hwnd: HWND) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let Some(canvas) = Canvas::new(w, h) else { return };
    let k = ui::scale_of(hwnd);
    let s = |v: i32| ui::px(v, k) as f32;
    let (ml, mt, mr, mb) = margins(hwnd);
    let (rw, rh) = ((w - ml - mr) as f32, (h - mt - mb) as f32);
    let (rx, ry) = (ml as f32, mt as f32);

    let state = app::cached_state();
    let color = border_color(state);

    // Near-invisible grab ring filling the margin around the capture region.
    // Layered windows hit-test by pixel alpha (fully transparent pixels are
    // click-through), so these faint pixels are what let the whole border area
    // be grabbed to move the frame — not just the 2px line. They sit outside
    // the capture rectangle, so they never appear in recordings.
    let grab_fill = argb(6, 0x8A, 0x93, 0xA6);
    canvas.fill_rect(0.0, 0.0, rx, h as f32, grab_fill);
    canvas.fill_rect(rx + rw, 0.0, w as f32 - (rx + rw), h as f32, grab_fill);
    canvas.fill_rect(rx, 0.0, rw, ry, grab_fill);
    canvas.fill_rect(rx, ry + rh, rw, h as f32 - (ry + rh), grab_fill);

    // Border drawn just outside the capture rectangle so it never appears in
    // the output.
    let bw = s(2).max(2.0);
    canvas.stroke_rounded(
        rx - bw, ry - bw, rw + bw * 2.0, rh + bw * 2.0,
        s(7), bw, color,
    );

    // A whisper of contrast against light backgrounds.
    canvas.stroke_rounded(
        rx - bw * 2.0, ry - bw * 2.0, rw + bw * 4.0, rh + bw * 4.0,
        s(8), 1.0, argb(70, 0, 0, 0),
    );

    if state == State::Idle {
        let hs = s(4);
        for (hx, hy) in handle_points(ml, mt, w - ml - mr, h - mt - mb) {
            canvas.fill_rounded(
                hx as f32 - hs - 1.0, hy as f32 - hs - 1.0,
                (hs + 1.0) * 2.0, (hs + 1.0) * 2.0, s(2) + 1.0,
                argb(140, 0, 0, 0),
            );
            canvas.fill_rounded(
                hx as f32 - hs, hy as f32 - hs,
                hs * 2.0, hs * 2.0, s(2),
                argb(255, 0xF2, 0xF4, 0xF8),
            );
        }
    }

    // Size chip above the top-left corner — flipped just inside the frame when
    // the region is against the top of the desktop and there is no room above.
    let (_, cy, cw, ch) = capture_rect(hwnd);
    let label = format!("{cw} × {ch}");
    let text_w = canvas.text_width(&label, s(12), false, false);
    let chip_w = text_w + s(16);
    let chip_h = s(24);
    let above = ry - bw - s(6) - chip_h;
    let win_top = (cy - mt) as f32; // window origin in screen pixels
    let chip_y = if win_top + above < virtual_screen().top as f32 { ry + s(6) } else { above };
    canvas.fill_rounded(rx - bw, chip_y, chip_w, chip_h, s(6), argb(235, 0x15, 0x17, 0x1C));
    canvas.text(
        &label,
        rx - bw + s(8), chip_y, text_w + s(4), chip_h,
        s(12), argb(255, 0xD9, 0xDE, 0xE5), false, false, Align::Left,
    );

    // Screenshot blink.
    let f = FLASH.load(Ordering::Relaxed);
    if f > 0 {
        canvas.fill_rounded(rx, ry, rw, rh, s(4), argb(f, 255, 255, 255));
    }

    canvas.present(hwnd);
}

fn hit_test(hwnd: HWND, sx: i32, sy: i32) -> u32 {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    let (px, py) = (sx - r.left, sy - r.top);
    let (ml, mt, mr, mb) = margins(hwnd);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let (rl, rt, rw, rh) = (ml, mt, w - ml - mr, h - mt - mb);
    let k = ui::scale_of(hwnd);

    // Resize handles take priority, but only while idle.
    if app::cached_state() == State::Idle {
        let grab = ui::px(9, k);
        for (i, (hx, hy)) in handle_points(rl, rt, rw, rh).iter().enumerate() {
            if (px - hx).abs() <= grab && (py - hy).abs() <= grab {
                return HANDLE_HITS[i];
            }
        }
    }

    // The capture region itself is click-through so the app underneath stays
    // usable; the whole margin ring around it (border, handles, size chip, and
    // the faint grab band — all outside the recording) moves the frame.
    let interior = px >= rl && px < rl + rw && py >= rt && py < rt + rh;
    if interior {
        HTTRANSPARENT as u32
    } else {
        HTCAPTION
    }
}

/// The virtual desktop, in screen pixels.
fn virtual_screen() -> RECT {
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        RECT {
            left: x,
            top: y,
            right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
            bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Begin tracking a move or resize started on the frame's border.
fn begin_drag(hwnd: HWND, mode: u32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
        SetCapture(hwnd);
    }
    let (cx, cy) = cursor_pos();
    GRAB[0].store(cx, Ordering::Relaxed);
    GRAB[1].store(cy, Ordering::Relaxed);
    for (slot, v) in START.iter().zip([r.left, r.top, r.right, r.bottom]) {
        slot.store(v, Ordering::Relaxed);
    }
    DRAG_MODE.store(mode, Ordering::Relaxed);
}

fn end_drag(release: bool) {
    if DRAG_MODE.swap(0, Ordering::Relaxed) != 0 && release {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
}

/// Apply the cursor delta to the grabbed edge (or the whole window) and move
/// the frame there. Unlike the native tracking loops this allows the window to
/// hang above the top of the screen, which is what lets the capture rectangle
/// reach the topmost row of pixels.
fn drag_to(hwnd: HWND, mode: u32) {
    let (cx, cy) = cursor_pos();
    let dx = cx - GRAB[0].load(Ordering::Relaxed);
    let dy = cy - GRAB[1].load(Ordering::Relaxed);
    let (sl, st, sr, sb) = (
        START[0].load(Ordering::Relaxed),
        START[1].load(Ordering::Relaxed),
        START[2].load(Ordering::Relaxed),
        START[3].load(Ordering::Relaxed),
    );
    let (ml, mt, mr, mb) = margins(hwnd);
    let (min_w, min_h) = (160 + ml + mr, 90 + mt + mb);
    let (mut l, mut t, mut r, mut b) = (sl, st, sr, sb);

    match mode {
        HTCAPTION => {
            l += dx;
            t += dy;
            r += dx;
            b += dy;
        }
        _ => {
            if matches!(mode, HTLEFT | HTTOPLEFT | HTBOTTOMLEFT) {
                l = (sl + dx).min(sr - min_w);
            }
            if matches!(mode, HTRIGHT | HTTOPRIGHT | HTBOTTOMRIGHT) {
                r = (sr + dx).max(sl + min_w);
            }
            if matches!(mode, HTTOP | HTTOPLEFT | HTTOPRIGHT) {
                t = (st + dy).min(sb - min_h);
            }
            if matches!(mode, HTBOTTOM | HTBOTTOMLEFT | HTBOTTOMRIGHT) {
                b = (sb + dy).max(st + min_h);
            }
        }
    }

    // Keep the capture rectangle inside the virtual desktop: its edges may
    // touch the very first and last pixel rows, no further, so the frame can
    // never be dragged out of reach.
    let vs = virtual_screen();
    if mode == HTCAPTION {
        let (w, h) = (r - l, b - t);
        l = l.clamp(vs.left - ml, vs.right - w + mr);
        t = t.clamp(vs.top - mt, vs.bottom - h + mb);
        r = l + w;
        b = t + h;
    } else {
        l = l.max(vs.left - ml);
        t = t.max(vs.top - mt);
        r = r.min(vs.right + mr);
        b = b.min(vs.bottom + mb);
    }

    unsafe {
        let _ = SetWindowPos(hwnd, HWND::default(), l, t, r - l, b - t, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let (sx, sy) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            LRESULT(hit_test(hwnd, sx, sy) as isize)
        }
        // Take the drag over from DefWindowProc's SC_MOVE / SC_SIZE loops,
        // which clamp the window to the screen and so make the top strip of
        // the desktop unreachable.
        WM_NCLBUTTONDOWN => {
            let mode = wparam.0 as u32;
            if mode == HTCAPTION
                || matches!(
                    mode,
                    HTTOP | HTBOTTOM | HTLEFT | HTRIGHT | HTTOPLEFT | HTTOPRIGHT | HTBOTTOMLEFT
                        | HTBOTTOMRIGHT
                )
            {
                begin_drag(hwnd, mode);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let mode = DRAG_MODE.load(Ordering::Relaxed);
            if mode != 0 {
                drag_to(hwnd, mode);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            end_drag(true);
            repaint(hwnd);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            end_drag(false); // capture already lost; just drop the state
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED => {
            // Follow the drag mid-recording.
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if ptr != 0 {
                let shared = &*(ptr as *const Shared);
                let (x, y, _, _) = capture_rect(hwnd);
                shared.x.store(x, Ordering::Relaxed);
                shared.y.store(y, Ordering::Relaxed);
            }
            if let Some(bar) = ui::cached_bar() {
                super::bar::place(bar, hwnd);
            }
            // Repaint on size changes, throttled during interactive resize.
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if LAST_W.swap(w, Ordering::Relaxed) != w || LAST_H.swap(h, Ordering::Relaxed) != h {
                let now = util::qpc() as isize;
                let min_gap = (util::qpf() / 30) as isize;
                if now - LAST_PAINT_QPC.load(Ordering::Relaxed) > min_gap {
                    LAST_PAINT_QPC.store(now, Ordering::Relaxed);
                    repaint(hwnd);
                }
                SetTimer(hwnd, TIMER_SETTLE, 40, None);
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let (l, t, r, b) = margins(hwnd);
            let info = lparam.0 as *mut MINMAXINFO;
            (*info).ptMinTrackSize.x = 160 + l + r;
            (*info).ptMinTrackSize.y = 90 + t + b;
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_FLASH => {
                    let f = FLASH.load(Ordering::Relaxed).saturating_sub(45);
                    FLASH.store(f, Ordering::Relaxed);
                    if f == 0 {
                        let _ = KillTimer(hwnd, TIMER_FLASH);
                    }
                    repaint(hwnd);
                }
                TIMER_SETTLE => {
                    let _ = KillTimer(hwnd, TIMER_SETTLE);
                    repaint(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            repaint(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            app::act_hide();
            LRESULT(0)
        }
        WM_DESTROY => {
            set_session(hwnd, None);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
