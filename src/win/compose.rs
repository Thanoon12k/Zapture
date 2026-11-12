//! Places a captured region on a decorative backdrop — padding, rounded
//! corners, a soft drop shadow, and a gradient. Shared by screenshots and, when
//! enabled, video export. Pure GDI+ via the `paint` canvas; no new deps.

use crate::config::BgStyle;
use crate::win::ui::paint::{argb, Canvas};

/// Padding and corner radius for a `w`×`h` content area.
pub fn metrics(w: i32, h: i32) -> (i32, f32) {
    let pad = ((w.max(h) as f32) * 0.055).round().max(28.0) as i32;
    let radius = ((w.min(h) as f32) * 0.02).clamp(6.0, 28.0);
    (pad, radius)
}

/// Output dimensions after adorning a `w`×`h` region (even, for encoders).
pub fn output_size(w: i32, h: i32) -> (i32, i32) {
    let (pad, _) = metrics(w, h);
    ((w + pad * 2) & !1, (h + pad * 2) & !1)
}

/// Place top-down BGRA `src` (`w`×`h`) on the `style` backdrop. Returns the
/// composed top-down BGRA and its dimensions, or `None` for `BgStyle::None`.
pub fn adorn(src: &[u8], w: i32, h: i32, style: BgStyle) -> Option<(Vec<u8>, i32, i32)> {
    if style == BgStyle::None || w <= 0 || h <= 0 || src.len() < (w * h * 4) as usize {
        return None;
    }
    let (pad, radius) = metrics(w, h);
    let (ow, oh) = output_size(w, h);
    let c = Canvas::new(ow, oh)?;

    let (top, bot) = style.colors();
    c.fill_gradient(0.0, 0.0, ow as f32, oh as f32, top, bot);

    // Soft drop shadow: a few stacked, growing, translucent rounded rects.
    let (px, py) = (pad as f32, pad as f32);
    for i in 0..7 {
        let e = i as f32 * 2.2;
        let a = (16 - i * 2).max(0) as u8;
        c.fill_rounded(px - e, py - e + 7.0, w as f32 + e * 2.0, h as f32 + e * 2.0, radius + e, argb(a, 0, 0, 0));
    }

    // The content, rounded. A 1:1 aspect means draw_image scales without
    // cropping, so the whole region is preserved.
    c.draw_image(src, w, h, px, py, w as f32, h as f32, radius);

    // Flatten to fully opaque. GDI+ leaves the alpha channel at zero where it
    // only fills colour on this premultiplied surface, and consumers that read
    // alpha then render the backdrop as a transparent (PNG) or black (clipboard
    // bitmap) frame around the content.
    let mut out = c.read_bgra();
    for px in out.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
    Some((out, ow, oh))
}
