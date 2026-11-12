//! Region capture. BitBlt from the composited desktop keeps this WYSIWYG:
//! whatever stack of windows is visible inside the frame is what gets
//! recorded, across any number of monitors.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GENERIC_WRITE, POINT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ, ROP_CODE, SRCCOPY,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::Graphics::GdiPlus::{
    GdipCreateFromHDC, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics,
    GdipDeletePen, GdipDrawEllipse, GdipFillEllipse, GdipSetSmoothingMode, GpBrush, GpGraphics,
    GpPen, GpSolidFill, SmoothingModeAntiAlias, UnitPixel,
};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetCursorPos, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL,
    HICON, ICONINFO,
};

use crate::app::{EncMsg, Shared};
use crate::win::util;

pub struct Grabber {
    w: i32,
    h: i32,
    screen_dc: HDC,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    // Active click ripples: screen (x, y) and the QPC tick they started at.
    ripples: Vec<(i32, i32, i64)>,
    lbtn: bool,
}

// The grabber owns its DCs and is only ever used from the thread that drives
// it; Send lets us construct it inside the capture thread closure.
unsafe impl Send for Grabber {}

impl Grabber {
    pub fn new(w: i32, h: i32) -> Result<Grabber, String> {
        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return Err("GetDC failed".into());
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down, matches the encoder input layout
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(screen_dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(mem_dc);
                    ReleaseDC(None, screen_dc);
                    return Err(format!("CreateDIBSection failed: {e}"));
                }
            };
            let old = SelectObject(mem_dc, bitmap);
            Ok(Grabber {
                w, h, screen_dc, mem_dc, bitmap, old,
                bits: bits as *mut u8,
                ripples: Vec::new(),
                lbtn: false,
            })
        }
    }

    /// Copies the screen region at (x, y) into the internal top-down BGRA
    /// buffer, optionally compositing the mouse cursor on top.
    pub fn grab(&mut self, x: i32, y: i32, cursor: bool, highlight: bool) {
        unsafe {
            // CAPTUREBLT includes layered windows, i.e. exactly what the user
            // sees stacked inside the region.
            let rop = ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
            let _ = BitBlt(self.mem_dc, 0, 0, self.w, self.h, self.screen_dc, x, y, rop);
            if highlight {
                self.draw_highlight(x, y);
            }
            if cursor {
                self.draw_cursor(x, y);
            }
            let _ = GdiFlush();
        }
    }

    /// Paint a soft glow under the cursor and an expanding ring on each click,
    /// straight into the captured DIB (GDI+ blends against what BitBlt copied).
    /// Presentation sugar for demos; kept out of screenshots.
    unsafe fn draw_highlight(&mut self, ox: i32, oy: i32) {
        let mut cp = POINT::default();
        if GetCursorPos(&mut cp).is_err() {
            return;
        }
        // Down-edge of the physical left button spawns a ripple at the cursor.
        let down = (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0;
        if down && !self.lbtn {
            self.ripples.push((cp.x, cp.y, util::qpc()));
        }
        self.lbtn = down;

        let now = util::qpc();
        let qpf = util::qpf() as f32;
        const LIFE: f32 = 0.5; // seconds
        self.ripples.retain(|&(_, _, st)| (now - st) as f32 / qpf < LIFE);

        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(self.mem_dc, &mut g).0 != 0 {
            return;
        }
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);

        // Warm glow disc under the cursor hotspot.
        let (lx, ly) = ((cp.x - ox) as f32, (cp.y - oy) as f32);
        let mut brush: *mut GpSolidFill = std::ptr::null_mut();
        GdipCreateSolidFill(0x46FF_D128, &mut brush); // ARGB 70,255,209,40
        let gr = 15.0f32;
        GdipFillEllipse(g, brush as *mut GpBrush, lx - gr, ly - gr, gr * 2.0, gr * 2.0);
        GdipDeleteBrush(brush as *mut GpBrush);

        // Expanding, fading rings for clicks.
        for &(sx, sy, st) in &self.ripples {
            let age = ((now - st) as f32 / qpf / LIFE).clamp(0.0, 1.0);
            let rr = 9.0 + 34.0 * age;
            let alpha = (150.0 * (1.0 - age)) as u32;
            let color = (alpha << 24) | 0x00FF_D128;
            let (rx, ry) = ((sx - ox) as f32, (sy - oy) as f32);
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(color, 3.0, UnitPixel, &mut pen);
            GdipDrawEllipse(g, pen, rx - rr, ry - rr, rr * 2.0, rr * 2.0);
            GdipDeletePen(pen);
        }
        GdipDeleteGraphics(g);
    }

    unsafe fn draw_cursor(&self, ox: i32, oy: i32) {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut ci).is_err() || ci.flags != CURSOR_SHOWING {
            return;
        }
        let icon = HICON(ci.hCursor.0);
        let mut ii = ICONINFO::default();
        let (mut hx, mut hy) = (0, 0);
        if GetIconInfo(icon, &mut ii).is_ok() {
            hx = ii.xHotspot as i32;
            hy = ii.yHotspot as i32;
            // GetIconInfo hands out copies we must release.
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(ii.hbmMask);
            }
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(ii.hbmColor);
            }
        }
        let _ = DrawIconEx(
            self.mem_dc,
            ci.ptScreenPos.x - hx - ox,
            ci.ptScreenPos.y - hy - oy,
            icon,
            0,
            0,
            0,
            None,
            DI_NORMAL,
        );
    }

    pub fn bits(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h * 4) as usize) }
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.mem_dc);
            ReleaseDC(None, self.screen_dc);
        }
    }
}

/// Paced capture loop. Frames are stamped with the shared pause-aware clock,
/// so a slow encoder shows up as dropped frames rather than time drift.
#[allow(clippy::too_many_arguments)]
pub fn capture_loop(
    sh: Arc<Shared>,
    w: i32,
    h: i32,
    fps: u32,
    cursor: bool,
    caption: Option<Arc<crate::win::caption::Caption>>,
    tx: SyncSender<EncMsg>,
    pool_rx: Receiver<Vec<u8>>,
    pool_tx: Sender<Vec<u8>>,
) {
    let mut grabber = match Grabber::new(w, h) {
        Ok(g) => g,
        Err(_) => return,
    };
    unsafe {
        timeBeginPeriod(1);
    }
    // The caption bar grows the encoded frame; the captured region keeps its
    // own size and is placed inside it.
    let (out_w, out_h) = match &caption {
        Some(c) => c.out_size(w, h),
        None => (w, h),
    };
    let (ox, oy) = match &caption {
        Some(c) => c.origin(),
        None => (0, 0),
    };
    let frame_len = (out_w * out_h * 4) as usize;
    let interval = 10_000_000i64 / fps.max(1) as i64;
    let mut allocated = 0usize;
    let mut next: Option<i64> = None;
    let mut click_prev = false;

    loop {
        if sh.stop.load(Ordering::Acquire) {
            break;
        }
        if !sh.armed.load(Ordering::Acquire) {
            next = None;
            std::thread::sleep(Duration::from_millis(15));
            continue;
        }

        let now = sh.clock.rel_100ns();
        let target = next.unwrap_or(now);
        if now < target {
            std::thread::sleep(Duration::from_nanos(((target - now) * 100) as u64));
        }
        let ts = sh.clock.rel_100ns();
        // If capture fell far behind, restart pacing from now instead of
        // bursting to catch up.
        next = Some(if ts > target + 2 * interval { ts + interval } else { target + interval });

        let x = sh.x.load(Ordering::Relaxed);
        let y = sh.y.load(Ordering::Relaxed);
        // Record click positions (region-relative, on the media clock) so an
        // export can auto-zoom toward them.
        let click_now = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
        if click_now && !click_prev {
            let mut cp = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut cp);
            }
            if let Ok(mut v) = sh.clicks.lock() {
                if v.len() < 8192 {
                    // Frame-relative, so an export's auto-zoom still lines up
                    // once the caption bar has shifted the region.
                    v.push((
                        ts / 10_000,
                        (cp.x - x).clamp(0, w - 1) + ox,
                        (cp.y - y).clamp(0, h - 1) + oy,
                    ));
                }
            }
        }
        click_prev = click_now;
        let highlight = sh.highlight.load(Ordering::Relaxed);
        grabber.grab(x, y, cursor, highlight);

        let mut buf = match pool_rx.try_recv() {
            Ok(b) => b,
            Err(_) => {
                if allocated >= 6 {
                    continue; // encoder is saturated; drop this frame
                }
                allocated += 1;
                vec![0u8; frame_len]
            }
        };
        match &caption {
            Some(c) => c.place(grabber.bits(), w, h, &mut buf, out_w),
            None => buf.copy_from_slice(grabber.bits()),
        }
        match tx.try_send(EncMsg::Video { buf, ts }) {
            Ok(()) => {}
            Err(TrySendError::Full(EncMsg::Video { buf, .. })) => {
                let _ = pool_tx.send(buf); // recycle the dropped frame
            }
            Err(_) => break,
        }
    }
    unsafe {
        timeEndPeriod(1);
    }
}

/// One-off grab for screenshots; returns top-down BGRA pixels.
pub fn snapshot(x: i32, y: i32, w: i32, h: i32, cursor: bool) -> Result<Vec<u8>, String> {
    let mut g = Grabber::new(w, h)?;
    g.grab(x, y, cursor, false);
    Ok(g.bits().to_vec())
}

pub fn save_png(path: &std::path::Path, w: u32, h: u32, bgra: &[u8]) -> Result<(), String> {
    let path_w = util::wide(&path.to_string_lossy());
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| -> windows::core::Result<()> {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromFilename(PCWSTR(path_w.as_ptr()), GENERIC_WRITE.0)?;
            let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;

            let mut frame = None;
            let mut props = None;
            encoder.CreateNewFrame(&mut frame, &mut props)?;
            let frame = frame.unwrap();
            frame.Initialize(props.as_ref())?;
            frame.SetSize(w, h)?;
            let mut format = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut format)?;
            frame.WritePixels(h, w * 4, bgra)?;
            frame.Commit()?;
            encoder.Commit()?;
            Ok(())
        })();
        CoUninitialize();
        result.map_err(|e| e.to_string())
    }
}

