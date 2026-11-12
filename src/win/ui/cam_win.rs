//! The webcam picture-in-picture window: a layered, always-on-top, draggable
//! overlay that paints the live camera feed as a circle or rounded square.
//! Unlike the frame and bar it is NOT excluded from capture, so it shows up in
//! recordings and screenshots when placed inside the capture region.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, KillTimer, LoadCursorW,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    HTBOTTOMRIGHT, HTCAPTION, HTTRANSPARENT, IDC_ARROW, MINMAXINFO, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOWPOS, WM_DESTROY, WM_GETMINMAXINFO, WM_MOUSEWHEEL,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_TIMER, WM_WINDOWPOSCHANGED, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::win::camera::Camera;
use crate::win::ui::{self, paint::{argb, Canvas}};

const TIMER_TICK: usize = 1;
const MIN: i32 = 120;
const MAX: i32 = 520;

static HWND_CELL: AtomicIsize = AtomicIsize::new(0);
static CIRCLE: AtomicBool = AtomicBool::new(true);

thread_local! {
    static CAM: RefCell<Option<Arc<Camera>>> = const { RefCell::new(None) };
}

fn hwnd() -> Option<HWND> {
    let v = HWND_CELL.load(Ordering::Relaxed);
    (v != 0).then_some(HWND(v as *mut core::ffi::c_void))
}

pub fn is_open() -> bool {
    hwnd().is_some()
}

pub fn is_circle() -> bool {
    CIRCLE.load(Ordering::Relaxed)
}

/// Current window rectangle (x, y, w, h) for persisting to config.
pub fn rect() -> Option<(i32, i32, i32, i32)> {
    let h = hwnd()?;
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(h, &mut r);
    }
    Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
}

/// Show the PIP window bound to `cam`, at `place` (or a default bottom-left).
pub fn open(cam: Arc<Camera>, circle: bool, place: Option<(i32, i32, i32, i32)>) {
    CIRCLE.store(circle, Ordering::Relaxed);
    CAM.with(|c| *c.borrow_mut() = Some(cam));

    if let Some(h) = hwnd() {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
        return;
    }
    let Ok(h) = create() else { return };
    HWND_CELL.store(h.0 as isize, Ordering::Relaxed);

    let (x, y, w, hh) = place.unwrap_or_else(default_place);
    let d = w.max(hh).clamp(MIN, MAX);
    unsafe {
        let _ = SetWindowPos(h, HWND::default(), x, y, d, d, SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        SetTimer(h, TIMER_TICK, 33, None);
    }
    repaint(h);
}

/// Hide and destroy the window. The caller stops the camera thread.
pub fn close() {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = KillTimer(h, TIMER_TICK);
            let _ = DestroyWindow(h);
        }
    }
    HWND_CELL.store(0, Ordering::Relaxed);
    CAM.with(|c| *c.borrow_mut() = None);
}

fn default_place() -> (i32, i32, i32, i32) {
    // bottom-left of the primary work area, with a margin
    let d = 220;
    let margin = 40;
    let mut work = RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut core::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    (work.left + margin, work.bottom - d - margin, d, d)
}

fn create() -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(cam_proc),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("CadreCam"),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("CadreCam"),
            w!("Cadre camera"),
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
        .map_err(|e| e.to_string())
    }
}

fn repaint(h: HWND) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(h, &mut r);
    }
    let (w, ht) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || ht <= 0 {
        return;
    }
    let Some(c) = Canvas::new(w, ht) else { return };
    let circle = CIRCLE.load(Ordering::Relaxed);
    let radius = if circle { (w.min(ht) as f32) / 2.0 } else { (w.min(ht) as f32 * 0.12).max(8.0) };
    let inset = 2.0;
    let (fw, fh) = (w as f32 - inset * 2.0, ht as f32 - inset * 2.0);

    let drew = CAM.with(|cell| {
        let cell = cell.borrow();
        let Some(cam) = cell.as_ref() else { return false };
        cam.with_frame(|f| {
            c.draw_image(&f.bgra, f.w as i32, f.h as i32, inset, inset, fw, fh, radius - inset);
        })
        .is_some()
    });

    if !drew {
        // placeholder while the first frame is still arriving
        c.fill_rounded(inset, inset, fw, fh, radius - inset, argb(235, 0x1A, 0x1D, 0x23));
    }
    // subtle ring
    c.stroke_rounded(inset, inset, fw, fh, radius - inset, 2.0, argb(230, 0xF2, 0xF4, 0xF8));
    c.stroke_rounded(inset - 1.0, inset - 1.0, fw + 2.0, fh + 2.0, radius, 1.0, argb(90, 0, 0, 0));

    c.present(h);
}

unsafe extern "system" fn cam_proc(h: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            let (sx, sy) = (crate::win::util::loword_i(lparam.0), crate::win::util::hiword_i(lparam.0));
            let (px, py) = (sx - r.left, sy - r.top);
            let (w, ht) = (r.right - r.left, r.bottom - r.top);
            let k = ui::scale_of(h);
            let grip = ui::px(20, k);
            // bottom-right grip resizes; the rest of the disc drags
            if px >= w - grip && py >= ht - grip {
                return LRESULT(HTBOTTOMRIGHT as isize);
            }
            let circle = CIRCLE.load(Ordering::Relaxed);
            if circle {
                // only the disc is draggable; corners fall through
                let (cx, cy, rr) = (w as f32 / 2.0, ht as f32 / 2.0, (w.min(ht) as f32) / 2.0);
                let dist = ((px as f32 - cx).powi(2) + (py as f32 - cy).powi(2)).sqrt();
                if dist > rr {
                    return LRESULT(HTTRANSPARENT as isize);
                }
            }
            LRESULT(HTCAPTION as isize)
        }
        WM_GETMINMAXINFO => {
            let info = lparam.0 as *mut MINMAXINFO;
            let k = ui::scale_of(h);
            (*info).ptMinTrackSize.x = ui::px(MIN, k);
            (*info).ptMinTrackSize.y = ui::px(MIN, k);
            (*info).ptMaxTrackSize.x = ui::px(MAX, k);
            (*info).ptMaxTrackSize.y = ui::px(MAX, k);
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED => {
            // keep it square: match height to width on resize
            let wp = &*(lparam.0 as *const WINDOWPOS);
            if wp.cx != wp.cy && wp.cx > 0 {
                let d = wp.cx;
                let _ = SetWindowPos(h, HWND::default(), 0, 0, d, d, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            repaint(h);
            LRESULT(0)
        }
        WM_NCLBUTTONDBLCLK => {
            // The disc/body hit-tests as HTCAPTION so it stays draggable, which
            // means a double-click arrives here as a *non-client* message, not
            // WM_LBUTTONDBLCLK. Toggle circle/square and swallow it so the
            // default caption action (maximize) never runs.
            let c = !CIRCLE.load(Ordering::Relaxed);
            CIRCLE.store(c, Ordering::Relaxed);
            repaint(h);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            // Scroll to zoom: grow / shrink around the centre, kept square and
            // clamped to the same min/max as a corner drag-resize. This is the
            // discoverable way to resize a circle, whose corner grip sits in the
            // invisible transparent area outside the disc.
            let delta = crate::win::util::hiword_i(wparam.0 as isize);
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            let k = ui::scale_of(h);
            let d = r.right - r.left;
            let nd = (d + ui::px(20, k) * delta / 120).clamp(ui::px(MIN, k), ui::px(MAX, k));
            if nd != d {
                let cx = (r.left + r.right) / 2;
                let cy = (r.top + r.bottom) / 2;
                let _ = SetWindowPos(
                    h,
                    HWND::default(),
                    cx - nd / 2,
                    cy - nd / 2,
                    nd,
                    nd,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                repaint(h);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_TICK {
                repaint(h);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            HWND_CELL.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wparam, lparam),
    }
}
