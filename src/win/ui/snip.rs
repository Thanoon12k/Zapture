//! Freeform region snip with annotation. The hotkey freezes the whole virtual
//! desktop under a dimmed overlay — everything on screen stops moving — and a
//! drag cuts a bright rectangle out of the dimming. Releasing does not finish
//! the snip: a toolbar appears and the region can be marked up (pen, arrow,
//! rectangle, mosaic, tiled watermark, six colours) before it is copied to the
//! clipboard or saved as a PNG. Esc or right-click cancels. The overlay is
//! excluded from capture, so an ongoing recording never sees it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, LoadCursorW, RegisterClassW,
    SetForegroundWindow, SetWindowPos, ShowWindow, CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, IDC_CROSS, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW, WM_DESTROY, WM_KEYDOWN,
    WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::config::BgStyle;
use crate::i18n::{t, Lang, Msg};
use crate::win::ui::paint::{argb, with_alpha, Align, Canvas};
use crate::win::{compose, screen, ui, util};

/// `CF_DIB` from winuser; a literal here saves pulling a whole feature gate
/// for one constant.
const CF_DIB: u32 = 8;

const BLUE: u32 = argb(255, 0x4C, 0x8D, 0xFF);
const INK: u32 = argb(255, 0xE8, 0xEA, 0xED);
const INK_DIM: u32 = argb(255, 0x9A, 0xA2, 0xB0);
const PLATE: u32 = argb(238, 0x14, 0x16, 0x1C);

/// Annotation colours, in toolbar order.
const COLORS: [u32; 6] = [
    argb(255, 0xE5, 0x48, 0x4D), // red
    argb(255, 0xE8, 0xA3, 0x3D), // amber
    argb(255, 0x5D, 0xCA, 0xA5), // green
    argb(255, 0x4C, 0x8D, 0xFF), // blue
    argb(255, 0xFF, 0xFF, 0xFF), // white
    argb(255, 0x14, 0x16, 0x1C), // near-black
];

const MOSAIC_BLOCK: i32 = 12; // logical px per mosaic cell

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    None,
    Pen,
    Arrow,
    Rect,
    Mosaic,
}

/// One annotation, in virtual-desktop (overlay client) coordinates.
#[derive(Clone)]
struct Shape {
    tool: Tool,
    color: u32,
    /// Pen keeps every sampled point; the others keep [start, end].
    pts: Vec<(i32, i32)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    Tool(Tool),
    Color(usize),
    Watermark,
    Undo,
    Save,
    Copy,
    Cancel,
}

/// What the pressed mouse button is currently doing.
enum Drag {
    None,
    /// Rubber-banding the initial selection from this anchor.
    NewSel((i32, i32)),
    /// Sliding the whole selection: cursor-to-origin offset.
    MoveSel((i32, i32)),
    /// Dragging handle `idx` of the selection.
    ResizeSel(usize),
    /// Building `Snip::draft`.
    Draw,
}

struct Snip {
    /// Frozen top-down BGRA of the entire virtual desktop.
    shot: Vec<u8>,
    w: i32,
    h: i32,
    k: f32,
    lang: Lang,
    /// Decorative backdrop applied to the finished image.
    bg: BgStyle,
    /// Committed selection; `None` until the first drag ends.
    sel: Option<(i32, i32, i32, i32)>,
    drag: Drag,
    cur: (i32, i32),
    tool: Tool,
    color: usize,
    watermark: bool,
    shapes: Vec<Shape>,
    draft: Option<Shape>,
    hover: Option<Btn>,
}

thread_local! {
    static UI: RefCell<Option<Snip>> = const { RefCell::new(None) };
}

static HWND_CELL: AtomicIsize = AtomicIsize::new(0);

/// Freeze the screen and open the selection overlay (no-op while one is up).
pub fn start() {
    if HWND_CELL.load(Ordering::Relaxed) != 0 {
        return;
    }
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    if vw < 2 || vh < 2 {
        return;
    }
    // Snapshot first, then cover the desktop — the overlay never shoots itself.
    let Ok(shot) = screen::snapshot(vx, vy, vw, vh, false) else { return };
    let Ok(h) = create() else { return };
    HWND_CELL.store(h.0 as isize, Ordering::Relaxed);
    let _ = ui::wda_exclude(h); // keep the overlay out of ongoing recordings
    let state = Snip {
        shot,
        w: vw,
        h: vh,
        k: ui::scale_of(h),
        lang: crate::app::with(|a| a.lang).unwrap_or(Lang::En),
        bg: crate::app::with(|a| a.cfg.shot_bg).unwrap_or(BgStyle::None),
        sel: None,
        drag: Drag::None,
        cur: (-1, -1),
        tool: Tool::None,
        color: 0,
        watermark: false,
        shapes: Vec::new(),
        draft: None,
        hover: None,
    };
    UI.with(|c| *c.borrow_mut() = Some(state));
    unsafe {
        let _ = SetWindowPos(h, HWND_TOPMOST, vx, vy, vw, vh, SWP_SHOWWINDOW);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h); // so Esc and Ctrl+Z reach the overlay
        // Activating can reshuffle the topmost band (a player or another
        // always-on-top window may sit above); claim the top of it again.
        let _ = SetWindowPos(h, HWND_TOPMOST, vx, vy, vw, vh, SWP_NOACTIVATE);
    }
    repaint(h);
}

fn close(h: HWND) {
    unsafe {
        let _ = DestroyWindow(h);
    }
    HWND_CELL.store(0, Ordering::Relaxed);
    UI.with(|c| *c.borrow_mut() = None);
}

fn create() -> Result<HWND, String> {
    unsafe {
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            // CS_DBLCLKS or WM_LBUTTONDBLCLK is never delivered at all.
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(snip_proc),
            hInstance: HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            lpszClassName: w!("CadreSnip"),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("CadreSnip"),
            w!("Cadre snip"),
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

// ---- geometry ----

/// The selection to draw right now: the committed one, or the rubber band.
fn live_sel(s: &Snip) -> Option<(i32, i32, i32, i32)> {
    if let Drag::NewSel((px, py)) = s.drag {
        let (cx, cy) = s.cur;
        let x = px.min(cx).max(0);
        let y = py.min(cy).max(0);
        let r = px.max(cx).min(s.w);
        let b = py.max(cy).min(s.h);
        return (r - x > 0 && b - y > 0).then_some((x, y, r - x, b - y));
    }
    s.sel
}

/// The eight resize handles, clockwise from the top-left corner.
fn handles(x: i32, y: i32, w: i32, h: i32) -> [(i32, i32); 8] {
    [
        (x, y),
        (x + w / 2, y),
        (x + w, y),
        (x + w, y + h / 2),
        (x + w, y + h),
        (x + w / 2, y + h),
        (x, y + h),
        (x, y + h / 2),
    ]
}

fn handle_at(s: &Snip, px: i32, py: i32) -> Option<usize> {
    let (x, y, w, h) = s.sel?;
    let grab = (9.0 * s.k) as i32;
    handles(x, y, w, h)
        .iter()
        .position(|(hx, hy)| (px - hx).abs() <= grab && (py - hy).abs() <= grab)
}

/// Apply a handle drag to the selection, keeping it non-degenerate.
fn resize_sel(s: &Snip, idx: usize, cx: i32, cy: i32) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = s.sel.unwrap_or((0, 0, 0, 0));
    let (mut l, mut t, mut r, mut b) = (x, y, x + w, y + h);
    let cx = cx.clamp(0, s.w);
    let cy = cy.clamp(0, s.h);
    if idx == 0 || (6..=7).contains(&idx) {
        l = cx;
    }
    if (2..=4).contains(&idx) {
        r = cx;
    }
    if (0..=2).contains(&idx) {
        t = cy;
    }
    if (4..=6).contains(&idx) {
        b = cy;
    }
    let (l, r) = (l.min(r), l.max(r));
    let (t, b) = (t.min(b), t.max(b));
    (l, t, (r - l).max(1), (b - t).max(1))
}

// ---- toolbar ----

/// A rectangle as (x, y, w, h) in overlay coordinates.
type Rect = (f32, f32, f32, f32);

/// Toolbar geometry: the plate rect plus each button's rect and identity.
fn toolbar(s: &Snip) -> Option<(Rect, Vec<(Rect, Btn)>)> {
    let (x, y, w, h) = s.sel?;
    let k = s.k;
    let (bw, bh, pad, gap) = (30.0 * k, 30.0 * k, 7.0 * k, 3.0 * k);
    let sw = 20.0 * k; // colour swatch cell
    let sep = 9.0 * k;

    let order: [Btn; 5] = [
        Btn::Tool(Tool::Pen),
        Btn::Tool(Tool::Arrow),
        Btn::Tool(Tool::Rect),
        Btn::Tool(Tool::Mosaic),
        Btn::Watermark,
    ];
    let tail: [Btn; 4] = [Btn::Undo, Btn::Save, Btn::Copy, Btn::Cancel];

    let plate_w = pad * 2.0
        + order.len() as f32 * (bw + gap)
        + sep
        + COLORS.len() as f32 * (sw + gap)
        + sep
        + tail.len() as f32 * (bw + gap)
        - gap;
    let plate_h = bh + pad * 2.0;

    // Below the selection when there is room, else above it, else inside.
    let below = (y + h) as f32 + 10.0 * k;
    let above = y as f32 - 10.0 * k - plate_h;
    let py = if below + plate_h <= s.h as f32 {
        below
    } else if above >= 0.0 {
        above
    } else {
        (s.h as f32 - plate_h - 10.0 * k).max(0.0)
    };
    let px = ((x + w) as f32 - plate_w).clamp(0.0, (s.w as f32 - plate_w).max(0.0));

    let mut out = Vec::with_capacity(order.len() + COLORS.len() + tail.len());
    let mut cx = px + pad;
    let cy = py + pad;
    for b in order {
        out.push(((cx, cy, bw, bh), b));
        cx += bw + gap;
    }
    cx += sep - gap;
    for i in 0..COLORS.len() {
        out.push(((cx, cy, sw, bh), Btn::Color(i)));
        cx += sw + gap;
    }
    cx += sep - gap;
    for b in tail {
        out.push(((cx, cy, bw, bh), b));
        cx += bw + gap;
    }
    Some(((px, py, plate_w, plate_h), out))
}

fn button_at(s: &Snip, px: i32, py: i32) -> Option<Btn> {
    let (_, btns) = toolbar(s)?;
    let (fx, fy) = (px as f32, py as f32);
    btns.iter()
        .find(|((bx, by, bw, bh), _)| fx >= *bx && fx < bx + bw && fy >= *by && fy < by + bh)
        .map(|(_, b)| *b)
}

fn on_toolbar(s: &Snip, px: i32, py: i32) -> bool {
    let Some(((x, y, w, h), _)) = toolbar(s) else { return false };
    let (fx, fy) = (px as f32, py as f32);
    fx >= x && fx < x + w && fy >= y && fy < y + h
}

// ---- annotation rendering ----

/// Draw the committed shapes (plus the in-progress one) onto `canvas`.
/// `ox`/`oy` translate desktop coordinates into canvas coordinates: zero for
/// the overlay, `-sel.x`/`-sel.y` when rendering the finished image.
fn render_shapes(canvas: &Canvas, s: &Snip, ox: f32, oy: f32) {
    let k = s.k;
    let stroke = 3.0 * k;
    for sh in s.shapes.iter().chain(s.draft.iter()) {
        match sh.tool {
            Tool::Pen => {
                for pair in sh.pts.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    canvas.line(
                        a.0 as f32 + ox,
                        a.1 as f32 + oy,
                        b.0 as f32 + ox,
                        b.1 as f32 + oy,
                        stroke,
                        sh.color,
                    );
                }
                if sh.pts.len() == 1 {
                    let p = sh.pts[0];
                    canvas.fill_circle(p.0 as f32 + ox, p.1 as f32 + oy, stroke / 2.0, sh.color);
                }
            }
            Tool::Arrow => {
                let (Some(a), Some(b)) = (sh.pts.first(), sh.pts.last()) else { continue };
                let (x1, y1) = (a.0 as f32 + ox, a.1 as f32 + oy);
                let (x2, y2) = (b.0 as f32 + ox, b.1 as f32 + oy);
                let (dx, dy) = (x2 - x1, y2 - y1);
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let head = (18.0 * k).min(len * 0.45);
                let (ux, uy) = (dx / len, dy / len);
                // shaft stops where the head begins, so the tip stays sharp
                canvas.line(x1, y1, x2 - ux * head * 0.8, y2 - uy * head * 0.8, stroke, sh.color);
                let (bx, by) = (x2 - ux * head, y2 - uy * head);
                let (px, py) = (-uy * head * 0.42, ux * head * 0.42);
                canvas.polygon(&[(x2, y2), (bx + px, by + py), (bx - px, by - py)], sh.color);
            }
            Tool::Rect => {
                let (Some(a), Some(b)) = (sh.pts.first(), sh.pts.last()) else { continue };
                let (x1, y1) = (a.0.min(b.0) as f32 + ox, a.1.min(b.1) as f32 + oy);
                let (w, h) = ((a.0 - b.0).abs() as f32, (a.1 - b.1).abs() as f32);
                canvas.stroke_rounded(x1, y1, w.max(1.0), h.max(1.0), 3.0 * k, stroke * 0.8, sh.color);
            }
            Tool::Mosaic => {
                let (Some(a), Some(b)) = (sh.pts.first(), sh.pts.last()) else { continue };
                mosaic(canvas, s, a.0.min(b.0), a.1.min(b.1), (a.0 - b.0).abs(), (a.1 - b.1).abs(), ox, oy);
            }
            Tool::None => {}
        }
    }
}

/// Pixelate a rectangle of the frozen desktop by filling cells with the colour
/// sampled at each cell's centre.
#[allow(clippy::too_many_arguments)]
fn mosaic(canvas: &Canvas, s: &Snip, x: i32, y: i32, w: i32, h: i32, ox: f32, oy: f32) {
    let cell = ui::px(MOSAIC_BLOCK, s.k).max(4);
    let stride = (s.w * 4) as usize;
    let mut by = y;
    while by < y + h {
        let mut bx = x;
        while bx < x + w {
            let sx = (bx + cell / 2).clamp(0, s.w - 1);
            let sy = (by + cell / 2).clamp(0, s.h - 1);
            let off = sy as usize * stride + sx as usize * 4;
            if off + 2 >= s.shot.len() {
                break;
            }
            let c = argb(255, s.shot[off + 2], s.shot[off + 1], s.shot[off]);
            let cw = cell.min(x + w - bx) as f32;
            let ch = cell.min(y + h - by) as f32;
            canvas.fill_rect(bx as f32 + ox, by as f32 + oy, cw, ch, c);
            bx += cell;
        }
        by += cell;
    }
}

/// Tile a translucent wordmark across the selection.
fn render_watermark(canvas: &Canvas, s: &Snip, x: f32, y: f32, w: f32, h: f32) {
    if !s.watermark {
        return;
    }
    let text = crate::app::with(|a| a.cfg.watermark.clone()).unwrap_or_default();
    let text = if text.trim().is_empty() { t(s.lang, Msg::SnipWatermark).to_string() } else { text };
    let size = 15.0;
    let tw = canvas.text_width(&text, size, false, true);
    let (step_x, step_y) = (tw + 70.0 * s.k, 62.0 * s.k);
    let mut row = 0;
    let mut ty = y;
    while ty < y + h {
        let offset = if row % 2 == 0 { 0.0 } else { step_x / 2.0 };
        let mut tx = x - step_x + offset;
        while tx < x + w {
            canvas.text(
                &text,
                tx,
                ty,
                tw + 4.0,
                26.0 * s.k,
                size,
                argb(64, 255, 255, 255),
                false,
                true,
                Align::Left,
            );
            tx += step_x;
        }
        ty += step_y;
        row += 1;
    }
}

// ---- painting ----

fn repaint(h: HWND) {
    UI.with(|c| {
        let b = c.borrow();
        let Some(s) = b.as_ref() else { return };
        let Some(canvas) = Canvas::new(s.w, s.h) else { return };
        let (sw, sh) = (s.w as f32, s.h as f32);
        canvas.draw_image(&s.shot, s.w, s.h, 0.0, 0.0, sw, sh, 0.0);
        canvas.fill_rect(0.0, 0.0, sw, sh, argb(120, 0, 0, 0));
        let k = s.k;

        let Some((x, y, w, hh)) = live_sel(s) else {
            let hint = t(s.lang, Msg::SnipHint);
            let tw = canvas.text_width(hint, 12.5, false, false) + 28.0 * k;
            let th = 30.0 * k;
            canvas.fill_rounded((sw - tw) / 2.0, 28.0 * k, tw, th, th / 2.0, PLATE);
            canvas.text(hint, (sw - tw) / 2.0, 28.0 * k, tw, th, 12.5, INK, false, false, Align::Center);
            canvas.opaque();
            canvas.present(h);
            return;
        };

        let (xf, yf, wf, hf) = (x as f32, y as f32, w as f32, hh as f32);
        // the selection shows through undimmed, then the annotations on top
        canvas.draw_image_crop(&s.shot, s.w, s.h, xf, yf, wf, hf, xf, yf, wf, hf, 0.0);
        render_shapes(&canvas, s, 0.0, 0.0);
        render_watermark(&canvas, s, xf, yf, wf, hf);

        // border + handles
        canvas.fill_rect(xf - 1.0, yf - 1.0, wf + 2.0, 1.5, BLUE);
        canvas.fill_rect(xf - 1.0, yf + hf - 0.5, wf + 2.0, 1.5, BLUE);
        canvas.fill_rect(xf - 1.0, yf, 1.5, hf, BLUE);
        canvas.fill_rect(xf + wf - 0.5, yf, 1.5, hf, BLUE);
        let hs = 3.5 * k;
        for (hx, hy) in handles(x, y, w, hh) {
            let (hx, hy) = (hx as f32, hy as f32);
            canvas.fill_rect(hx - hs, hy - hs, hs * 2.0, hs * 2.0, BLUE);
            canvas.fill_rect(hx - hs + 1.0, hy - hs + 1.0, hs * 2.0 - 2.0, hs * 2.0 - 2.0, INK);
        }

        // size readout
        let label = format!("{w} × {hh}");
        let tw = canvas.text_width(&label, 11.5, true, false) + 16.0 * k;
        let th = 22.0 * k;
        let ly = if yf - th - 6.0 * k >= 0.0 { yf - th - 6.0 * k } else { yf + 6.0 * k };
        let lx = xf.min(sw - tw).max(0.0);
        canvas.fill_rounded(lx, ly, tw, th, 5.0 * k, PLATE);
        canvas.text(&label, lx, ly, tw, th, 11.5, INK, true, false, Align::Center);

        if s.sel.is_some() {
            draw_toolbar(&canvas, s);
        }
        canvas.opaque();
        canvas.present(h);
    });
}

fn draw_toolbar(canvas: &Canvas, s: &Snip) {
    let Some(((px, py, pw, ph), btns)) = toolbar(s) else { return };
    let k = s.k;
    canvas.fill_rounded(px, py, pw, ph, 10.0 * k, PLATE);
    canvas.stroke_rounded(px, py, pw, ph, 10.0 * k, 1.0, argb(40, 255, 255, 255));

    for ((bx, by, bw, bh), b) in btns {
        let active = match b {
            Btn::Tool(t) => s.tool == t,
            Btn::Watermark => s.watermark,
            Btn::Color(i) => s.color == i,
            _ => false,
        };
        let hovered = s.hover == Some(b);
        if active {
            canvas.fill_rounded(bx, by, bw, bh, 7.0 * k, argb(56, 0x4C, 0x8D, 0xFF));
        } else if hovered {
            canvas.fill_rounded(bx, by, bw, bh, 7.0 * k, argb(40, 255, 255, 255));
        }
        let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
        let tint = if active { BLUE } else if hovered { INK } else { INK_DIM };
        let r = 7.0 * k;
        let lw = 1.9 * k;
        match b {
            Btn::Tool(Tool::Pen) => {
                canvas.line(cx - r, cy + r, cx + r * 0.55, cy - r * 0.85, lw, tint);
                canvas.polygon(
                    &[(cx - r, cy + r), (cx - r * 0.45, cy + r * 0.55), (cx - r * 0.9, cy + r * 0.2)],
                    tint,
                );
            }
            Btn::Tool(Tool::Arrow) => {
                canvas.line(cx - r, cy + r, cx + r * 0.45, cy - r * 0.45, lw, tint);
                canvas.polygon(
                    &[(cx + r, cy - r), (cx + r * 0.1, cy - r * 0.75), (cx + r * 0.75, cy - r * 0.1)],
                    tint,
                );
            }
            Btn::Tool(Tool::Rect) => {
                canvas.stroke_rounded(cx - r, cy - r * 0.78, r * 2.0, r * 1.56, 2.0 * k, lw, tint);
            }
            Btn::Tool(Tool::Mosaic) => {
                let c = r * 0.66;
                for (i, j) in [(0, 0), (1, 1), (2, 0), (0, 2), (1, 0), (2, 2)] {
                    let a = if (i + j) % 2 == 0 { 255 } else { 120 };
                    canvas.fill_rect(
                        cx - r + i as f32 * c,
                        cy - r + j as f32 * c,
                        c - 1.0,
                        c - 1.0,
                        with_alpha(tint, a),
                    );
                }
            }
            Btn::Watermark => {
                for i in 0..3 {
                    let o = -r + i as f32 * r * 0.85;
                    canvas.line(cx + o, cy + r * 0.7, cx + o + r * 0.7, cy - r * 0.7, lw * 0.9, tint);
                }
            }
            Btn::Color(i) => {
                let col = COLORS[i];
                canvas.fill_circle(cx, cy, r * 0.72, col);
                if active {
                    canvas.circle_stroke(cx, cy, r * 0.98, 1.6 * k, INK);
                } else if col == COLORS[5] {
                    canvas.circle_stroke(cx, cy, r * 0.72, 1.0, argb(90, 255, 255, 255));
                }
            }
            Btn::Undo => {
                canvas.arc(cx, cy + r * 0.15, r * 0.75, 160.0, 210.0, lw, tint);
                canvas.polygon(
                    &[
                        (cx - r * 0.78, cy - r * 0.55),
                        (cx - r * 0.1, cy - r * 0.45),
                        (cx - r * 0.62, cy + r * 0.15),
                    ],
                    tint,
                );
            }
            Btn::Save => {
                canvas.line(cx, cy - r * 0.9, cx, cy + r * 0.15, lw, tint);
                canvas.polygon(
                    &[(cx, cy + r * 0.55), (cx - r * 0.42, cy - r * 0.05), (cx + r * 0.42, cy - r * 0.05)],
                    tint,
                );
                canvas.line(cx - r * 0.8, cy + r * 0.85, cx + r * 0.8, cy + r * 0.85, lw, tint);
            }
            Btn::Copy => {
                let g = argb(255, 0x5D, 0xCA, 0xA5);
                canvas.line(cx - r * 0.75, cy + r * 0.05, cx - r * 0.15, cy + r * 0.65, 2.4 * k, g);
                canvas.line(cx - r * 0.15, cy + r * 0.65, cx + r * 0.8, cy - r * 0.6, 2.4 * k, g);
            }
            Btn::Cancel => {
                let rd = argb(255, 0xE5, 0x6B, 0x6B);
                canvas.line(cx - r * 0.6, cy - r * 0.6, cx + r * 0.6, cy + r * 0.6, 2.2 * k, rd);
                canvas.line(cx + r * 0.6, cy - r * 0.6, cx - r * 0.6, cy + r * 0.6, 2.2 * k, rd);
            }
            Btn::Tool(Tool::None) => {}
        }
    }
}

// ---- output ----

/// Render the selection plus its annotations into a tight top-down BGRA buffer,
/// optionally on the decorative backdrop. Returns (pixels, w, h).
fn compose_region(s: &Snip, x: i32, y: i32, w: i32, h: i32) -> Option<(Vec<u8>, i32, i32)> {
    let canvas = Canvas::new(w, h)?;
    canvas.draw_image_crop(
        &s.shot, s.w, s.h,
        x as f32, y as f32, w as f32, h as f32,
        0.0, 0.0, w as f32, h as f32,
        0.0,
    );
    render_shapes(&canvas, s, -x as f32, -y as f32);
    render_watermark(&canvas, s, 0.0, 0.0, w as f32, h as f32);
    let region = canvas.read_bgra();

    let (mut buf, bw, bh) = match compose::adorn(&region, w, h, s.bg) {
        Some((p, ow, oh)) => (p, ow, oh),
        None => (region, w, h),
    };
    // A screen grab carries no alpha (BitBlt leaves the channel at zero), and a
    // receiver that honours alpha would paste the whole thing as a black
    // rectangle. The snip is always fully opaque.
    for px in buf.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
    Some((buf, bw, bh))
}

/// Put a finished top-down BGRA image on the clipboard as a bottom-up CF_DIB.
pub(crate) fn to_clipboard(hwnd: HWND, buf: &[u8], bw: i32, bh: i32) -> bool {
    let stride = (bw * 4) as usize;
    let hdr = std::mem::size_of::<BITMAPINFOHEADER>();
    unsafe {
        if OpenClipboard(hwnd).is_err() {
            return false;
        }
        let mut ok = false;
        let _ = EmptyClipboard();
        if let Ok(hg) = GlobalAlloc(GMEM_MOVEABLE, hdr + stride * bh as usize) {
            let base = GlobalLock(hg) as *mut u8;
            if !base.is_null() {
                let mut info: BITMAPINFOHEADER = std::mem::zeroed();
                info.biSize = hdr as u32;
                info.biWidth = bw;
                info.biHeight = bh; // positive → bottom-up rows
                info.biPlanes = 1;
                info.biBitCount = 32;
                info.biSizeImage = (stride * bh as usize) as u32;
                std::ptr::write(base as *mut BITMAPINFOHEADER, info);
                let pixels = base.add(hdr);
                for row in 0..bh as usize {
                    let sy = bh as usize - 1 - row; // flip top-down source to bottom-up
                    std::ptr::copy_nonoverlapping(
                        buf.as_ptr().add(sy * stride),
                        pixels.add(row * stride),
                        stride,
                    );
                }
                let _ = GlobalUnlock(hg);
                ok = SetClipboardData(CF_DIB, HANDLE(hg.0)).is_ok();
            }
        }
        let _ = CloseClipboard();
        ok
    }
}

/// Finish the snip: copy to the clipboard, and optionally also write a PNG.
fn finish(h: HWND, save: bool) {
    let done = UI.with(|c| {
        let b = c.borrow();
        let s = b.as_ref()?;
        let (x, y, w, hh) = s.sel?;
        let (buf, bw, bh) = compose_region(s, x, y, w, hh)?;
        let copied = to_clipboard(h, &buf, bw, bh);
        let mut msg = if copied { Msg::SnipCopied } else { Msg::ErrShotFailed };
        if save {
            let dir = util::screenshots_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("SHOT_{}.png", util::timestamp()));
            msg = match screen::save_png(&path, bw as u32, bh as u32, &buf) {
                Ok(()) => { util::reveal_capture(&path, true); Msg::SnipSaved },
                Err(_) => Msg::ErrShotFailed,
            };
        }
        Some((s.lang, msg))
    });
    if let Some((lang, msg)) = done {
        close(h);
        crate::win::ui::bar::toast(t(lang, msg));
    }
}

// ---- input ----

fn ctrl_down() -> bool {
    unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 }
}

/// Route a press: toolbar button, handle, tool stroke, or selection move.
fn on_press(s: &mut Snip, px: i32, py: i32) -> Option<Btn> {
    if s.sel.is_some() && on_toolbar(s, px, py) {
        return button_at(s, px, py);
    }
    if s.sel.is_none() {
        s.drag = Drag::NewSel((px, py));
        return None;
    }
    if let Some(idx) = handle_at(s, px, py) {
        s.drag = Drag::ResizeSel(idx);
        return None;
    }
    let (x, y, w, h) = s.sel.unwrap_or((0, 0, 0, 0));
    let inside = px >= x && px < x + w && py >= y && py < y + h;
    if inside && s.tool != Tool::None {
        let p = (px.clamp(x, x + w), py.clamp(y, y + h));
        s.draft = Some(Shape { tool: s.tool, color: COLORS[s.color], pts: vec![p] });
        s.drag = Drag::Draw;
    } else if inside {
        s.drag = Drag::MoveSel((px - x, py - y));
    }
    None
}

fn on_button(h: HWND, b: Btn) {
    let mut act: Option<bool> = None; // Some(save?) → finish
    UI.with(|c| {
        let mut bo = c.borrow_mut();
        let Some(s) = bo.as_mut() else { return };
        match b {
            Btn::Tool(t) => s.tool = if s.tool == t { Tool::None } else { t },
            Btn::Color(i) => s.color = i,
            Btn::Watermark => s.watermark = !s.watermark,
            Btn::Undo => {
                s.shapes.pop();
            }
            Btn::Save => act = Some(true),
            Btn::Copy => act = Some(false),
            Btn::Cancel => act = None,
        }
    });
    match b {
        Btn::Cancel => close(h),
        Btn::Save | Btn::Copy => finish(h, act == Some(true)),
        _ => repaint(h),
    }
}

unsafe extern "system" fn snip_proc(h: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONDOWN => {
            let (px, py) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let btn = UI.with(|c| {
                let mut b = c.borrow_mut();
                let s = b.as_mut()?;
                s.cur = (px, py);
                on_press(s, px, py)
            });
            match btn {
                Some(b) => on_button(h, b),
                None => repaint(h),
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (px, py) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let dirty = UI.with(|c| {
                let mut b = c.borrow_mut();
                let Some(s) = b.as_mut() else { return false };
                s.cur = (px, py);
                let hover = if s.sel.is_some() { button_at(s, px, py) } else { None };
                let hover_changed = hover != s.hover;
                s.hover = hover;
                match s.drag {
                    Drag::None => hover_changed,
                    Drag::NewSel(_) => true,
                    Drag::ResizeSel(idx) => {
                        s.sel = Some(resize_sel(s, idx, px, py));
                        true
                    }
                    Drag::MoveSel((gx, gy)) => {
                        if let Some((_, _, w, hh)) = s.sel {
                            let nx = (px - gx).clamp(0, (s.w - w).max(0));
                            let ny = (py - gy).clamp(0, (s.h - hh).max(0));
                            s.sel = Some((nx, ny, w, hh));
                        }
                        true
                    }
                    Drag::Draw => {
                        let (x, y, w, hh) = s.sel.unwrap_or((0, 0, 0, 0));
                        let p = (px.clamp(x, x + w), py.clamp(y, y + hh));
                        if let Some(d) = s.draft.as_mut() {
                            if d.tool == Tool::Pen || d.pts.len() < 2 {
                                d.pts.push(p);
                            } else {
                                d.pts[1] = p;
                            }
                        }
                        true
                    }
                }
            });
            if dirty {
                repaint(h);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (px, py) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            UI.with(|c| {
                let mut b = c.borrow_mut();
                let Some(s) = b.as_mut() else { return };
                s.cur = (px, py);
                match std::mem::replace(&mut s.drag, Drag::None) {
                    Drag::NewSel(anchor) => {
                        s.drag = Drag::NewSel(anchor);
                        s.sel = live_sel(s).filter(|(_, _, w, hh)| *w >= 2 && *hh >= 2);
                        s.drag = Drag::None;
                    }
                    Drag::Draw => {
                        if let Some(d) = s.draft.take() {
                            let keep = match d.tool {
                                Tool::Pen => d.pts.len() > 1,
                                _ => d.pts.len() > 1 && d.pts[0] != d.pts[1],
                            };
                            if keep {
                                s.shapes.push(d);
                            }
                        }
                    }
                    _ => {}
                }
            });
            repaint(h);
            LRESULT(0)
        }
        // Double-clicking inside the selection finishes the snip, the same as
        // Enter or the check button.
        WM_LBUTTONDBLCLK => {
            let (px, py) = (util::loword_i(lparam.0), util::hiword_i(lparam.0));
            let inside = UI.with(|c| {
                let mut b = c.borrow_mut();
                let Some(s) = b.as_mut() else { return false };
                s.drag = Drag::None;
                s.draft = None; // the opening click of the pair started nothing
                let Some((x, y, w, hh)) = s.sel else { return false };
                !on_toolbar(s, px, py) && px >= x && px < x + w && py >= y && py < y + hh
            });
            if inside {
                finish(h, false);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            close(h);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            if vk == VK_ESCAPE.0 {
                close(h);
            } else if vk == VK_RETURN.0 {
                finish(h, false);
            } else if ctrl_down() && (vk == b'Z' as u16) {
                UI.with(|c| {
                    if let Some(s) = c.borrow_mut().as_mut() {
                        s.shapes.pop();
                    }
                });
                repaint(h);
            } else if ctrl_down() && (vk == b'S' as u16) {
                finish(h, true);
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
