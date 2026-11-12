//! The caption bar editor: pick the edge, type the label, see it previewed.
//!
//! Unlike the rest of Cadre's chrome this is an ordinary window rather than a
//! layered one, because the text field is a real `EDIT` control — that is what
//! buys a caret, selection, clipboard and the IME (Chinese typing) for free,
//! and child controls cannot live inside a layered window. The background is
//! still drawn with the shared `Canvas` and blitted in `WM_PAINT`, so it looks
//! like the rest of the app.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, InvalidateRect, SetBkColor,
    SetTextColor, DEFAULT_CHARSET, FF_DONTCARE, FW_NORMAL, HBRUSH, HFONT, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW,
    LoadCursorW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, ES_AUTOVSCROLL, ES_MULTILINE, ES_WANTRETURN, HTCAPTION,
    HTCLIENT, HWND_TOPMOST, IDC_ARROW, SWP_SHOWWINDOW, SW_SHOW, WM_COMMAND, WM_CTLCOLOREDIT,
    WM_DESTROY, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SETFONT,
    WINDOW_STYLE, WNDCLASSW, WS_CHILD, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    WS_VSCROLL,
};

use crate::config::CaptionEdge;
use crate::i18n::{t, Lang, Msg};
use crate::win::caption;
use crate::win::ui::paint::{argb, Align, Canvas};
use crate::win::{ui, util};

const W: i32 = 520;
const H: i32 = 378;

const BG: u32 = argb(255, 0x16, 0x18, 0x1E);
const CARD: u32 = argb(255, 0x0B, 0x0D, 0x12);
const PILL: u32 = argb(255, 0x25, 0x28, 0x30);
const BLUE: u32 = argb(255, 0x4C, 0x8D, 0xFF);
const ON_BLUE: u32 = argb(255, 0x0A, 0x1A, 0x33);
const FG: u32 = argb(255, 0xE8, 0xEA, 0xED);
const FG_FAINT: u32 = argb(255, 0x8A, 0x90, 0x9C);
const GREEN: u32 = argb(255, 0x5D, 0xCA, 0xA5);

const EDGES: [(CaptionEdge, Msg); 5] = [
    (CaptionEdge::None, Msg::BgNone),
    (CaptionEdge::Top, Msg::CapTop),
    (CaptionEdge::Bottom, Msg::CapBottom),
    (CaptionEdge::Left, Msg::CapLeft),
    (CaptionEdge::Right, Msg::CapRight),
];
const SIZES: [(i32, &str); 4] = [(0, "Auto"), (48, "48"), (72, "72"), (96, "96")];

struct Ui {
    edge: CaptionEdge,
    size: i32,
    lang: Lang,
    k: f32,
    edit: HWND,
    font: HFONT,
    brush: HBRUSH,
    hover: Option<usize>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

static HWND_CELL: AtomicIsize = AtomicIsize::new(0);

fn window() -> Option<HWND> {
    let v = HWND_CELL.load(Ordering::Relaxed);
    (v != 0).then_some(HWND(v as *mut core::ffi::c_void))
}

/// Open the editor (or raise it if it is already up).
pub fn open() {
    if let Some(h) = window() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    let (edge, text, size, lang) = crate::app::with(|a| {
        (a.cfg.caption_edge, a.cfg.caption_text.clone(), a.cfg.caption_size, a.lang)
    })
    .unwrap_or((CaptionEdge::None, String::new(), 0, Lang::En));

    let Ok(h) = create() else { return };
    HWND_CELL.store(h.0 as isize, Ordering::Relaxed);
    let k = ui::scale_of(h);

    // Centre on the primary work area.
    let (sw, sh) = (ui::px(W, k), ui::px(H, k));
    let mut work = RECT::default();
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        };
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let _ = SetWindowPos(
            h,
            HWND_TOPMOST,
            work.left + (work.right - work.left - sw) / 2,
            work.top + (work.bottom - work.top - sh) / 2,
            sw,
            sh,
            SWP_SHOWWINDOW,
        );
    }

    // The real text field.
    let (ex, ey, ew, eh) = edit_rect(k);
    let edit = unsafe {
        CreateWindowExW(
            Default::default(),
            w!("EDIT"),
            PCWSTR::null(),
            // The ES_* edit styles are plain i32 constants, not WINDOW_STYLE.
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
            ex,
            ey,
            ew,
            eh,
            h,
            None,
            HINSTANCE(GetModuleHandleW(None).map(|m| m.0).unwrap_or_default()),
            None,
        )
    }
    .unwrap_or_default();

    let font = unsafe {
        let face = util::wide("Segoe UI");
        CreateFontW(
            ui::px(17, k),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            0,
            0,
            0,
            FF_DONTCARE.0 as u32,
            PCWSTR(face.as_ptr()),
        )
    };
    unsafe {
        SendMessageW(edit, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        let wide = util::wide(&text);
        let _ = SetWindowTextW(edit, PCWSTR(wide.as_ptr()));
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        let _ = SetFocus(edit);
    }

    let brush = unsafe { CreateSolidBrush(COLORREF(0x120D0B)) }; // BGR of CARD
    UI.with(|c| *c.borrow_mut() = Some(Ui { edge, size, lang, k, edit, font, brush, hover: None }));
    repaint(h);
}

fn close(h: HWND) {
    UI.with(|c| {
        if let Some(u) = c.borrow_mut().take() {
            unsafe {
                let _ = DeleteObject(u.font);
                let _ = DeleteObject(u.brush);
            }
        }
    });
    unsafe {
        let _ = DestroyWindow(h);
    }
    HWND_CELL.store(0, Ordering::Relaxed);
}

fn create() -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc_),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("CadreCaptionWin"),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            w!("CadreCaptionWin"),
            w!("Cadre caption"),
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

// ---- layout (logical px, scaled at use) ----

fn edit_rect(k: f32) -> (i32, i32, i32, i32) {
    (ui::px(24, k), ui::px(112, k), ui::px(W - 48, k), ui::px(76, k))
}

/// Buttons: 0..5 edges, 5..9 sizes, 9 = save, 10 = cancel.
fn buttons(k: f32) -> Vec<(f32, f32, f32, f32)> {
    let mut v = Vec::with_capacity(11);
    let (pw, ph) = (ui::px(84, k) as f32, ui::px(30, k) as f32);
    for i in 0..EDGES.len() {
        v.push((ui::px(24, k) as f32 + i as f32 * (pw + 6.0 * k), ui::px(56, k) as f32, pw, ph));
    }
    let sw = ui::px(64, k) as f32;
    for i in 0..SIZES.len() {
        v.push((ui::px(24, k) as f32 + i as f32 * (sw + 6.0 * k), ui::px(218, k) as f32, sw, ph));
    }
    let (bw, bh) = (ui::px(96, k) as f32, ui::px(34, k) as f32);
    v.push((ui::px(W - 24, k) as f32 - bw, ui::px(H - 24, k) as f32 - bh, bw, bh));
    v.push((ui::px(W - 30, k) as f32 - bw * 2.0, ui::px(H - 24, k) as f32 - bh, bw, bh));
    v
}

fn hit(k: f32, x: i32, y: i32) -> Option<usize> {
    let (fx, fy) = (x as f32, y as f32);
    buttons(k)
        .iter()
        .position(|(bx, by, bw, bh)| fx >= *bx && fx < bx + bw && fy >= *by && fy < by + bh)
}

fn edit_text(u: &Ui) -> String {
    unsafe {
        let n = GetWindowTextLengthW(u.edit);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(u.edit, &mut buf);
        String::from_utf16_lossy(&buf[..got as usize])
    }
}

// ---- painting ----

fn repaint(h: HWND) {
    unsafe {
        let _ = InvalidateRect(h, None, false);
    }
}

fn draw(hdc: windows::Win32::Graphics::Gdi::HDC) {
    UI.with(|c| {
        let b = c.borrow();
        let Some(u) = b.as_ref() else { return };
        let k = u.k;
        let (cw, ch) = (ui::px(W, k), ui::px(H, k));
        let Some(canvas) = Canvas::new(cw, ch) else { return };
        let s = |v: i32| ui::px(v, k) as f32;
        canvas.fill_rect(0.0, 0.0, cw as f32, ch as f32, BG);

        canvas.text(
            t(u.lang, Msg::MenuCaption),
            s(24), s(16), s(300), s(28), 15.0, FG, false, true, Align::Left,
        );
        canvas.text(
            t(u.lang, Msg::CapEdge),
            s(24), s(38), s(300), s(18), 11.0, FG_FAINT, false, false, Align::Left,
        );

        let btns = buttons(k);
        for (i, (bx, by, bw, bh)) in btns.iter().enumerate() {
            let (label, active) = if i < EDGES.len() {
                (t(u.lang, EDGES[i].1).to_string(), u.edge == EDGES[i].0)
            } else if i < EDGES.len() + SIZES.len() {
                let (v, name) = SIZES[i - EDGES.len()];
                (name.to_string(), u.size == v)
            } else if i == btns.len() - 2 {
                (t(u.lang, Msg::CapSave).to_string(), false)
            } else {
                (t(u.lang, Msg::CapCancel).to_string(), false)
            };
            let save = i == btns.len() - 2;
            let bgc = if save { GREEN } else if active { BLUE } else { PILL };
            let fgc = if save || active { ON_BLUE } else { FG };
            canvas.fill_rounded(*bx, *by, *bw, *bh, bh / 2.0, bgc);
            if u.hover == Some(i) && !active && !save {
                canvas.stroke_rounded(*bx, *by, *bw, *bh, bh / 2.0, 1.2, argb(90, 255, 255, 255));
            }
            canvas.text(&label, *bx, *by, *bw, *bh, 12.0, fgc, false, active || save, Align::Center);
        }

        // Caption text label + a live preview of the bar.
        canvas.text(
            t(u.lang, Msg::CapText),
            s(24), s(92), s(400), s(18), 11.0, FG_FAINT, false, false, Align::Left,
        );
        canvas.text(
            t(u.lang, Msg::CapThickness),
            s(24), s(198), s(400), s(18), 11.0, FG_FAINT, false, false, Align::Left,
        );

        let (px, py, pw, ph) = (s(24), s(262), s(W - 48), s(40));
        canvas.fill_rounded(px, py, pw, ph, 4.0 * k, CARD);
        let text = edit_text(u);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        canvas.text(first, px + 12.0 * k, py, pw * 0.6, ph, 12.0, FG, false, true, Align::Left);
        // Same proportions the real bar uses, shrunk to fit the preview.
        let mut csize = ph * 0.52;
        let measured = canvas.text_width(caption::CREDIT, csize, true, false);
        if measured > pw * 0.45 {
            csize *= pw * 0.45 / measured.max(1.0);
        }
        canvas.text(
            caption::CREDIT,
            px + pw * 0.48, py, pw * 0.52 - 12.0 * k, ph,
            csize, argb(255, 0x9A, 0xA4, 0xB4), true, false, Align::Right,
        );

        canvas.blit(hdc);
    });
}

// ---- input ----

fn apply(h: HWND, save: bool) {
    if save {
        let vals = UI.with(|c| {
            let b = c.borrow();
            b.as_ref().map(|u| (u.edge, edit_text(u), u.size))
        });
        if let Some((edge, text, size)) = vals {
            crate::app::act_set_caption(edge, text, size);
        }
    }
    close(h);
}

unsafe extern "system" fn proc_(h: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(h, &mut ps);
            draw(hdc);
            let _ = EndPaint(h, &ps);
            LRESULT(0)
        }
        // EN_CHANGE from the text field: refresh the live preview.
        WM_COMMAND => {
            if util::hiword_i(wparam.0 as isize) == 0x0300 {
                repaint(h);
            }
            LRESULT(0)
        }
        // Dark field: the EDIT control asks for its colours here.
        WM_CTLCOLOREDIT => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetTextColor(hdc, COLORREF(0xEDEAE8));
            SetBkColor(hdc, COLORREF(0x120D0B));
            let br = UI.with(|c| c.borrow().as_ref().map(|u| u.brush)).unwrap_or_default();
            LRESULT(br.0 as isize)
        }
        // Drag the dialog by its empty chrome, not by the controls.
        WM_NCHITTEST => {
            let k = UI.with(|c| c.borrow().as_ref().map(|u| u.k)).unwrap_or(1.0);
            let mut r = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h, &mut r);
            let (x, y) = (util::loword_i(lparam.0) - r.left, util::hiword_i(lparam.0) - r.top);
            if y < ui::px(48, k) && hit(k, x, y).is_none() {
                LRESULT(HTCAPTION as isize)
            } else {
                LRESULT(HTCLIENT as isize)
            }
        }
        WM_MOUSEMOVE => {
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let changed = UI.with(|c| {
                let mut b = c.borrow_mut();
                let Some(u) = b.as_mut() else { return false };
                let hv = hit(u.k, x, y);
                let ch = hv != u.hover;
                u.hover = hv;
                ch
            });
            if changed {
                repaint(h);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let idx = UI.with(|c| c.borrow().as_ref().and_then(|u| hit(u.k, x, y)));
            let Some(i) = idx else { return LRESULT(0) };
            let n_edges = EDGES.len();
            let n_sizes = SIZES.len();
            if i < n_edges {
                UI.with(|c| {
                    if let Some(u) = c.borrow_mut().as_mut() {
                        u.edge = EDGES[i].0;
                    }
                });
                repaint(h);
            } else if i < n_edges + n_sizes {
                UI.with(|c| {
                    if let Some(u) = c.borrow_mut().as_mut() {
                        u.size = SIZES[i - n_edges].0;
                    }
                });
                repaint(h);
            } else if i == n_edges + n_sizes {
                apply(h, true);
            } else {
                apply(h, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 as u16 == VK_ESCAPE.0 => {
            apply(h, false);
            LRESULT(0)
        }
        WM_DESTROY => {
            HWND_CELL.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wparam, lparam),
    }
}

