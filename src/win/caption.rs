//! Caption bar: a solid strip welded to one edge of the recording, carrying
//! whatever label the user configured — a model name, a handle, a build tag —
//! plus a small Cadre credit at the far end of the strip.
//!
//! The strip's pixels never change during a session, so it is rasterised once
//! when recording starts and then memcpy'd into each frame alongside the
//! captured region. That keeps the capture loop's per-frame cost to a copy.

use crate::config::{CaptionEdge, Config};
use crate::win::ui::paint::{argb, Align, Canvas};

/// Stamped at the far end of every caption bar.
pub const CREDIT: &str = "From Cadre · https://github.com/Im-Midi/cadre";

const PLATE: u32 = argb(255, 0x0B, 0x0D, 0x12);
const INK: u32 = argb(255, 0xE8, 0xEA, 0xED);
const DIM: u32 = argb(255, 0x9A, 0xA4, 0xB4);

/// Default strip thickness in capture pixels, before extra text lines.
const BAR_H: i32 = 56;
const BAR_W: i32 = 240; // for the vertical edges
const LINE: i32 = 26;

pub struct Caption {
    edge: CaptionEdge,
    /// Strip pixels, top-down BGRA, `w` × `h`.
    bgra: Vec<u8>,
    w: i32,
    h: i32,
}

impl Caption {
    /// Rasterise the strip for a `rw` × `rh` capture region, or `None` when no
    /// edge is configured.
    pub fn build(rw: i32, rh: i32, cfg: &Config) -> Option<Caption> {
        let edge = cfg.caption_edge;
        if edge == CaptionEdge::None || rw < 8 || rh < 8 {
            return None;
        }
        let lines: Vec<&str> =
            cfg.caption_text.lines().filter(|l| !l.trim().is_empty()).collect();
        let horizontal = matches!(edge, CaptionEdge::Top | CaptionEdge::Bottom);

        let thickness = if cfg.caption_size > 0 {
            cfg.caption_size
        } else if horizontal {
            BAR_H + LINE * (lines.len().max(1) as i32 - 1)
        } else {
            BAR_W
        };
        // Even, and never more than half the region it is attached to.
        let limit = if horizontal { rh } else { rw };
        let thickness = (thickness.clamp(24, (limit / 2).max(24))) & !1;
        let (w, h) = if horizontal { (rw, thickness) } else { (thickness, rh) };

        let canvas = Canvas::new(w, h)?;
        canvas.fill_rect(0.0, 0.0, w as f32, h as f32, PLATE);
        if horizontal {
            paint_horizontal(&canvas, w, h, &lines);
        } else {
            paint_vertical(&canvas, w, h, &lines);
        }
        canvas.opaque();
        Some(Caption { edge, bgra: canvas.read_bgra(), w, h })
    }

    /// Frame size once the strip is attached.
    pub fn out_size(&self, rw: i32, rh: i32) -> (i32, i32) {
        match self.edge {
            CaptionEdge::Top | CaptionEdge::Bottom => (rw, rh + self.h),
            CaptionEdge::Left | CaptionEdge::Right => (rw + self.w, rh),
            CaptionEdge::None => (rw, rh),
        }
    }

    /// Where the captured region sits inside the frame.
    pub fn origin(&self) -> (i32, i32) {
        match self.edge {
            CaptionEdge::Top => (0, self.h),
            CaptionEdge::Left => (self.w, 0),
            _ => (0, 0),
        }
    }

    /// Write the strip and the captured region into an output frame.
    pub fn place(&self, region: &[u8], rw: i32, rh: i32, dst: &mut [u8], out_w: i32) {
        let (ox, oy) = self.origin();
        let dst_stride = (out_w * 4) as usize;
        let reg_stride = (rw * 4) as usize;
        let strip_stride = (self.w * 4) as usize;

        // Horizontal bars span the full width, so both halves are contiguous.
        if matches!(self.edge, CaptionEdge::Top | CaptionEdge::Bottom) {
            let strip_off = if self.edge == CaptionEdge::Top { 0 } else { rh as usize * dst_stride };
            let reg_off = oy as usize * dst_stride;
            let strip_len = strip_stride * self.h as usize;
            let reg_len = reg_stride * rh as usize;
            if dst.len() >= strip_off + strip_len && dst.len() >= reg_off + reg_len {
                dst[strip_off..strip_off + strip_len].copy_from_slice(&self.bgra[..strip_len]);
                dst[reg_off..reg_off + reg_len].copy_from_slice(&region[..reg_len]);
            }
            return;
        }

        // Vertical bars: every row is strip + region (or region + strip).
        let strip_x = if self.edge == CaptionEdge::Left { 0usize } else { rw as usize * 4 };
        let reg_x = ox as usize * 4;
        for row in 0..rh as usize {
            let d = row * dst_stride;
            if d + dst_stride > dst.len()
                || (row + 1) * strip_stride > self.bgra.len()
                || (row + 1) * reg_stride > region.len()
            {
                break;
            }
            let s = row * strip_stride;
            let r = row * reg_stride;
            dst[d + strip_x..d + strip_x + strip_stride]
                .copy_from_slice(&self.bgra[s..s + strip_stride]);
            dst[d + reg_x..d + reg_x + reg_stride].copy_from_slice(&region[r..r + reg_stride]);
        }
    }
}

/// Largest credit size at or below `want` that still fits `max_w`. The credit
/// is a long string, so on a narrow recording it has to give way rather than
/// run off the end of the bar.
fn fit(canvas: &Canvas, want: f32, max_w: f32) -> f32 {
    let measured = canvas.text_width(CREDIT, want, true, false);
    if measured > max_w && measured > 0.0 {
        (want * max_w / measured).max(9.0)
    } else {
        want
    }
}

/// Label on the left, credit hugging the far (right) end.
fn paint_horizontal(canvas: &Canvas, w: i32, h: i32, lines: &[&str]) {
    let (wf, hf) = (w as f32, h as f32);
    let pad = (hf * 0.30).clamp(12.0, 28.0);
    let size = (hf * 0.30).clamp(11.0, 24.0);
    let credit = fit(canvas, (hf * 0.52).clamp(26.0, 42.0), wf * 0.45);
    let cw = canvas.text_width(CREDIT, credit, true, false) + pad;

    let lh = size * 1.35;
    let block = lh * lines.len() as f32;
    let mut ty = (hf - block) / 2.0;
    let tw = (wf - cw - pad * 2.0).max(1.0);
    for line in lines {
        canvas.text(line, pad, ty, tw, lh, size, INK, false, true, Align::Left);
        ty += lh;
    }
    canvas.text(CREDIT, wf - cw, 0.0, cw - pad * 0.5, hf, credit, DIM, true, false, Align::Right);
}

/// Label centred, credit along the bottom of the strip.
fn paint_vertical(canvas: &Canvas, w: i32, h: i32, lines: &[&str]) {
    let (wf, hf) = (w as f32, h as f32);
    let pad = 16.0;
    let size = (wf * 0.085).clamp(11.0, 22.0);
    let credit = fit(canvas, (wf * 0.144).clamp(24.0, 36.0), wf - pad * 2.0);
    let ch = credit * 3.2;

    let lh = size * 1.35;
    let block = lh * lines.len() as f32;
    let mut ty = (hf - ch - block) / 2.0;
    for line in lines {
        canvas.text(line, pad, ty, wf - pad * 2.0, lh, size, INK, false, true, Align::Center);
        ty += lh;
    }
    canvas.text(CREDIT, pad * 0.5, hf - ch, wf - pad, ch, credit, DIM, true, false, Align::Center);
}
