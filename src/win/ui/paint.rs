//! A small immediate-mode canvas over the GDI+ flat API.
//!
//! Each canvas wraps a premultiplied-BGRA DIB section: GDI+ draws
//! antialiased shapes straight into the pixel buffer (PARGB, so the bits are
//! already premultiplied), and `present` hands the same DC to
//! UpdateLayeredWindow. That keeps every window fully alpha-composited
//! without a compositor dependency.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows::Win32::Graphics::GdiPlus::{
    FillModeAlternate, GdipAddPathArc, GdipClosePathFigure, GdipCreateBitmapFromScan0,
    GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreatePath, GdipCreatePen1,
    GdipCreateSolidFill, GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont,
    GdipDeleteFontFamily, GdipDeleteGraphics, GdipDeletePath, GdipDeletePen,
    GdipDeleteStringFormat, GdipDisposeImage, GdipDrawArc, GdipDrawEllipse, GdipDrawLine,
    GdipDrawPath, GdipDrawString, GdipFillEllipse, GdipFillPath, GdipFillPolygon,
    GdipFillRectangle, GdipGetGenericFontFamilySansSerif, GdipGetImageGraphicsContext,
    GdipMeasureString,
    GdipSetSmoothingMode, GdipSetStringFormatAlign, GdipSetStringFormatLineAlign,
    GdipSetTextRenderingHint, GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpBitmap,
    GpBrush, GpFont, GpFontFamily, GpGraphics, GpImage, GpPath, GpPen, GpSolidFill,
    GpStringFormat, PointF, RectF, SmoothingModeAntiAlias, StringAlignmentCenter,
    StringAlignmentFar, StringAlignmentNear, TextRenderingHintAntiAlias, UnitPixel,
    CombineModeReplace, GdipDrawImageRectRect, GdipResetClip, GdipSetClipPath,
    GdipCreateLineBrushFromRect, GpLineGradient, LinearGradientModeVertical, WrapModeTileFlipXY,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::win::util;

const PIXEL_FORMAT_32BPP_PARGB: i32 = 0xE200B;
const PIXEL_FORMAT_32BPP_RGB: i32 = 0x22009;

static TOKEN: AtomicUsize = AtomicUsize::new(0);

pub fn init() -> Result<(), u32> {
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        ..Default::default()
    };
    let mut token = 0usize;
    let mut output = Default::default();
    let status = unsafe { GdiplusStartup(&mut token, &input, &mut output) };
    if status.0 != 0 {
        return Err(status.0 as u32);
    }
    TOKEN.store(token, Ordering::Relaxed);
    Ok(())
}

pub fn shutdown() {
    let token = TOKEN.swap(0, Ordering::Relaxed);
    if token != 0 {
        unsafe { GdiplusShutdown(token) };
    }
}

pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub fn with_alpha(color: u32, a: u8) -> u32 {
    (color & 0x00FF_FFFF) | ((a as u32) << 24)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    gfx: *mut GpGraphics,
    img: *mut GpBitmap,
    bits: *mut u8,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        unsafe {
            let dc = CreateCompatibleDC(HDC::default());
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(HDC::default(), &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(dc, bitmap);
            std::ptr::write_bytes(bits as *mut u8, 0, (w * h * 4) as usize);

            let mut img: *mut GpBitmap = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, Some(bits as *const u8), &mut img).0 != 0 {
                SelectObject(dc, old);
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(dc);
                return None;
            }
            let mut gfx: *mut GpGraphics = std::ptr::null_mut();
            GdipGetImageGraphicsContext(img as *mut GpImage, &mut gfx);
            GdipSetSmoothingMode(gfx, SmoothingModeAntiAlias);
            GdipSetTextRenderingHint(gfx, TextRenderingHintAntiAlias);
            Some(Canvas { w, h, dc, bitmap, old, gfx, img, bits: bits as *mut u8 })
        }
    }

    unsafe fn brush(&self, color: u32) -> *mut GpSolidFill {
        let mut b: *mut GpSolidFill = std::ptr::null_mut();
        GdipCreateSolidFill(color, &mut b);
        b
    }

    unsafe fn rounded(&self, x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
        let mut p: *mut GpPath = std::ptr::null_mut();
        GdipCreatePath(FillModeAlternate, &mut p);
        let d = (r * 2.0).min(w).min(h);
        GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(p);
        p
    }

    pub fn fill_rounded(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        unsafe {
            let p = self.rounded(x, y, w, h, r);
            let b = self.brush(color);
            GdipFillPath(self.gfx, b as *mut GpBrush, p);
            GdipDeleteBrush(b as *mut GpBrush);
            GdipDeletePath(p);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rounded(&self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, color: u32) {
        unsafe {
            let p = self.rounded(x, y, w, h, r);
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawPath(self.gfx, pen, p);
            GdipDeletePen(pen);
            GdipDeletePath(p);
        }
    }

    pub fn fill_rect(&self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        unsafe {
            let b = self.brush(color);
            GdipFillRectangle(self.gfx, b as *mut GpBrush, x, y, w, h);
            GdipDeleteBrush(b as *mut GpBrush);
        }
    }

    /// Vertical linear-gradient fill (top color → bottom color).
    pub fn fill_gradient(&self, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
        unsafe {
            let rect = RectF { X: x, Y: y, Width: w.max(1.0), Height: h.max(1.0) };
            let mut brush: *mut GpLineGradient = std::ptr::null_mut();
            if GdipCreateLineBrushFromRect(&rect, top, bottom, LinearGradientModeVertical, WrapModeTileFlipXY, &mut brush).0 == 0 {
                GdipFillRectangle(self.gfx, brush as *mut GpBrush, x, y, w, h);
                GdipDeleteBrush(brush as *mut GpBrush);
            }
        }
    }

    /// Copy the canvas out as top-down BGRA (`w * h * 4` bytes).
    pub fn read_bgra(&self) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h * 4) as usize).to_vec() }
    }

    /// Force every pixel fully opaque.
    ///
    /// A layered window is composited per-pixel through this alpha channel, so
    /// any pixel GDI+ left at alpha zero shows the live desktop underneath —
    /// which is why a surface meant to cover the screen (the snip's frozen
    /// desktop) has to be flattened before it is presented.
    pub fn opaque(&self) {
        unsafe {
            let px = std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize);
            for p in px.chunks_exact_mut(4) {
                p[3] = 0xFF;
            }
        }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, color: u32) {
        unsafe {
            let b = self.brush(color);
            GdipFillEllipse(self.gfx, b as *mut GpBrush, cx - r, cy - r, r * 2.0, r * 2.0);
            GdipDeleteBrush(b as *mut GpBrush);
        }
    }

    /// Draw top-down BGRA image bytes into the rounded rect (x, y, w, h),
    /// cover-fitting (center-crop) and clipping to the rounded shape. A radius
    /// of `w/2` (with a square target) yields a circle. Alpha is ignored.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(&self, bgra: &[u8], iw: i32, ih: i32, x: f32, y: f32, w: f32, h: f32, radius: f32) {
        if iw <= 0 || ih <= 0 || bgra.len() < (iw * ih * 4) as usize {
            return;
        }
        unsafe {
            let mut img: *mut GpBitmap = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(iw, ih, iw * 4, PIXEL_FORMAT_32BPP_RGB, Some(bgra.as_ptr()), &mut img).0 != 0 {
                return;
            }
            // A zero radius would build a degenerate rounded path — clipping to
            // it discards everything — so square targets simply skip the clip.
            let path = if radius > 0.5 {
                let p = self.rounded(x, y, w, h, radius);
                GdipSetClipPath(self.gfx, p, CombineModeReplace);
                Some(p)
            } else {
                None
            };
            // cover fit: scale so the source fills the target, crop the rest
            let scale = (w / iw as f32).max(h / ih as f32).max(0.0001);
            let (sw, sh) = (w / scale, h / scale);
            let (sx, sy) = ((iw as f32 - sw) / 2.0, (ih as f32 - sh) / 2.0);
            GdipDrawImageRectRect(
                self.gfx,
                img as *mut GpImage,
                x, y, w, h,
                sx, sy, sw, sh,
                UnitPixel,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
            if let Some(p) = path {
                GdipResetClip(self.gfx);
                GdipDeletePath(p);
            }
            GdipDisposeImage(img as *mut GpImage);
        }
    }

    /// Blit an explicit source rectangle of top-down BGRA `bgra` into the
    /// destination rectangle (scaling as needed), clipped to a rounded corner
    /// when `radius > 0`. Used for zoom (crop a sub-rect and scale it up).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_crop(
        &self,
        bgra: &[u8],
        iw: i32,
        ih: i32,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
        radius: f32,
    ) {
        if iw <= 0 || ih <= 0 || bgra.len() < (iw * ih * 4) as usize {
            return;
        }
        unsafe {
            let mut img: *mut GpBitmap = std::ptr::null_mut();
            if GdipCreateBitmapFromScan0(iw, ih, iw * 4, PIXEL_FORMAT_32BPP_RGB, Some(bgra.as_ptr()), &mut img).0 != 0 {
                return;
            }
            let path = if radius > 0.5 {
                let p = self.rounded(dx, dy, dw, dh, radius);
                GdipSetClipPath(self.gfx, p, CombineModeReplace);
                Some(p)
            } else {
                None
            };
            GdipDrawImageRectRect(
                self.gfx,
                img as *mut GpImage,
                dx, dy, dw, dh,
                sx, sy, sw, sh,
                UnitPixel,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            );
            if let Some(p) = path {
                GdipResetClip(self.gfx);
                GdipDeletePath(p);
            }
            GdipDisposeImage(img as *mut GpImage);
        }
    }

    pub fn circle_stroke(&self, cx: f32, cy: f32, r: f32, width: f32, color: u32) {
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawEllipse(self.gfx, pen, cx - r, cy - r, r * 2.0, r * 2.0);
            GdipDeletePen(pen);
        }
    }

    /// Circular arc centered at (cx, cy); angles in degrees, 0 = +x.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&self, cx: f32, cy: f32, r: f32, start: f32, sweep: f32, width: f32, color: u32) {
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawArc(self.gfx, pen, cx - r, cy - r, r * 2.0, r * 2.0, start, sweep);
            GdipDeletePen(pen);
        }
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: u32) {
        unsafe {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawLine(self.gfx, pen, x1, y1, x2, y2);
            GdipDeletePen(pen);
        }
    }

    pub fn polygon(&self, pts: &[(f32, f32)], color: u32) {
        unsafe {
            let points: Vec<PointF> = pts.iter().map(|&(x, y)| PointF { X: x, Y: y }).collect();
            let b = self.brush(color);
            GdipFillPolygon(
                self.gfx,
                b as *mut GpBrush,
                points.as_ptr(),
                points.len() as i32,
                FillModeAlternate,
            );
            GdipDeleteBrush(b as *mut GpBrush);
        }
    }

    unsafe fn font(&self, size: f32, mono: bool, bold: bool) -> (*mut GpFontFamily, *mut GpFont) {
        let mut family: *mut GpFontFamily = std::ptr::null_mut();
        let name = util::wide(if mono { "Consolas" } else { "Segoe UI" });
        if GdipCreateFontFamilyFromName(PCWSTR(name.as_ptr()), std::ptr::null_mut(), &mut family).0 != 0 {
            GdipGetGenericFontFamilySansSerif(&mut family);
        }
        let mut font: *mut GpFont = std::ptr::null_mut();
        GdipCreateFont(family, size, if bold { 1 } else { 0 }, UnitPixel, &mut font);
        (family, font)
    }

    /// Draw `text` inside the given rect, vertically centered.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&self, text: &str, x: f32, y: f32, w: f32, h: f32, size: f32, color: u32, mono: bool, bold: bool, align: Align) {
        unsafe {
            let (family, font) = self.font(size, mono, bold);
            let mut fmt: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            GdipSetStringFormatAlign(
                fmt,
                match align {
                    Align::Left => StringAlignmentNear,
                    Align::Center => StringAlignmentCenter,
                    Align::Right => StringAlignmentFar,
                },
            );
            GdipSetStringFormatLineAlign(fmt, StringAlignmentCenter);
            let b = self.brush(color);
            let wide_text: Vec<u16> = text.encode_utf16().collect();
            let layout = RectF { X: x, Y: y, Width: w, Height: h };
            GdipDrawString(
                self.gfx,
                PCWSTR(wide_text.as_ptr()),
                wide_text.len() as i32,
                font,
                &layout,
                fmt,
                b as *mut GpBrush,
            );
            GdipDeleteBrush(b as *mut GpBrush);
            GdipDeleteStringFormat(fmt);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
        }
    }

    pub fn text_width(&self, text: &str, size: f32, mono: bool, bold: bool) -> f32 {
        unsafe {
            let (family, font) = self.font(size, mono, bold);
            let mut fmt: *mut GpStringFormat = std::ptr::null_mut();
            GdipCreateStringFormat(0, 0, &mut fmt);
            let wide_text: Vec<u16> = text.encode_utf16().collect();
            let layout = RectF { X: 0.0, Y: 0.0, Width: 4096.0, Height: 256.0 };
            let mut bounds = RectF::default();
            GdipMeasureString(
                self.gfx,
                PCWSTR(wide_text.as_ptr()),
                wide_text.len() as i32,
                font,
                &layout,
                fmt,
                &mut bounds,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            GdipDeleteStringFormat(fmt);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
            bounds.Width
        }
    }

    /// Blit the canvas into an ordinary window's DC (a `WM_PAINT` handler).
    ///
    /// Layered windows take `present` instead; this path exists for the few
    /// windows that host real child controls — a layered window cannot, and a
    /// text field has to be a genuine `EDIT` control to get a caret, selection
    /// and the IME for free.
    pub fn blit(&self, hdc: HDC) {
        unsafe {
            let _ = BitBlt(hdc, 0, 0, self.w, self.h, self.dc, 0, 0, SRCCOPY);
        }
    }

    /// Push the canvas to a layered window. Position is left to the window
    /// manager; only the surface (and thus the size) is updated.
    pub fn present(&self, hwnd: HWND) {
        let size = SIZE { cx: self.w, cy: self.h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                HDC::default(),
                None,
                Some(&size),
                self.dc,
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.gfx);
            GdipDisposeImage(self.img as *mut GpImage);
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}
