//! Offline export: re-encode an existing MP4 to be faster and/or smaller.
//!
//! A Media Foundation Source Reader decodes the input (its built-in video
//! processor handles the optional downscale), each frame is re-stamped at
//! `timestamp / speed`, frames that fall closer together than the output
//! frame interval are dropped, and a Sink Writer re-encodes to H.264. Speed
//! changes are therefore true frame-dropping timelapse, not just playback
//! rate metadata, so the file really does get shorter and smaller. Exports
//! are video-only.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFSample, IMFSinkWriter, IMFSourceReader, MFCreateAttributes,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
    MFCreateSourceReaderFromURL,
    MFStartup, MFMediaType_Video, MFShutdown, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoFormat_RGB32, MF_API_VERSION, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
    MF_SDK_VERSION, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ALL_STREAMS,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MFSTARTUP_NOSOCKET,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::config::{auto_bitrate_kbps, BgStyle};
use crate::gif::GifWriter;
use crate::win::compose;
use crate::win::encoder::HwndSend;
use crate::win::ui::paint::Canvas;
use crate::win::{util, MSG_EXPORT_DONE, MSG_EXPORT_PROGRESS};

const MF_VERSION: u32 = (MF_SDK_VERSION << 16) | MF_API_VERSION;
const MFVIDEO_INTERLACE_PROGRESSIVE: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Mp4,
    Gif,
}

#[derive(Clone, Copy)]
pub struct Settings {
    pub format: Format,
    /// Playback multiplier: 1.0, 2.0, 4.0, 6.0, or 8.0.
    pub speed: f64,
    /// Target height in pixels, or None to keep the source resolution.
    pub target_h: Option<u32>,
    /// MP4 only: lower the bitrate for a smaller file at a small quality cost.
    pub compress: bool,
    /// GIF only: 0 = low, 1 = medium, 2 = high (palette size and frame rate).
    pub quality: u8,
    /// GIF only: cap the output length in seconds; 0 keeps the whole clip.
    pub max_secs: u32,
    /// MP4 only: auto-zoom toward recorded clicks (needs a `.clicks` sidecar).
    pub autozoom: bool,
    /// MP4 only: place the video on a decorative backdrop.
    pub bg: BgStyle,
}

/// GIF palette size and target frame rate for a quality level.
pub fn gif_params(quality: u8) -> (u16, u32) {
    match quality {
        2 => (256, 20),
        1 => (128, 15),
        _ => (64, 10),
    }
}

/// GIF output frame rate: the quality target, never above the source.
pub fn gif_fps(quality: u8, src_fps: u32) -> u32 {
    gif_params(quality).1.min(src_fps.max(1))
}

#[derive(Clone, Copy)]
pub struct ProbeInfo {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub dur_100ns: i64,
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

/// Even, non-upscaled target dimensions and the encode frame rate.
pub fn plan_dimensions(info: &ProbeInfo, target_h: Option<u32>) -> (u32, u32, u32) {
    let src_h = info.h.max(2);
    let src_w = info.w.max(2);
    let mut th = target_h.unwrap_or(src_h).min(src_h).max(64);
    let mut tw = ((src_w as u64 * th as u64 + src_h as u64 / 2) / src_h as u64) as u32;
    tw &= !1;
    th &= !1;
    let fps = info.fps.clamp(1, 120);
    (tw.max(2), th.max(2), fps)
}

pub fn plan_bitrate_kbps(tw: u32, th: u32, fps: u32, compress: bool) -> u32 {
    let base = auto_bitrate_kbps(tw, th, fps);
    if compress {
        (base / 2).max(800)
    } else {
        base
    }
}

/// Estimated output size in bytes, or None without a duration.
pub fn estimate_bytes(info: &ProbeInfo, s: &Settings) -> Option<u64> {
    if info.dur_100ns <= 0 {
        return None;
    }
    let (tw, th, src_fps) = plan_dimensions(info, s.target_h);
    let mut out_secs = (info.dur_100ns as f64 / 10_000_000.0) / s.speed.max(0.1);
    match s.format {
        Format::Mp4 => {
            let (ew, eh) = if s.bg != BgStyle::None {
                let (w, h) = compose::output_size(tw as i32, th as i32);
                (w as u32, h as u32)
            } else {
                (tw, th)
            };
            let kbps = plan_bitrate_kbps(ew, eh, src_fps, s.compress);
            Some((kbps as f64 * 1000.0 / 8.0 * out_secs) as u64)
        }
        Format::Gif => {
            if s.max_secs > 0 {
                out_secs = out_secs.min(s.max_secs as f64);
            }
            let (colors, gfps) = gif_params(s.quality);
            let fps = gfps.min(src_fps.max(1)) as f64;
            // Rough: indexed frames compress to well under a byte per pixel;
            // scale with palette depth.
            let per_px = 0.18 + 0.0016 * colors as f64;
            Some((tw as f64 * th as f64 * per_px * fps * out_secs) as u64)
        }
    }
}

/// Suggested output path next to the source, tagged with the settings.
pub fn output_path(src: &Path, s: &Settings) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "export".into());
    let dir = src.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let mut tag = String::new();
    if s.speed > 1.0 {
        tag.push_str(&format!("_{}x", s.speed as u32));
    }
    if let Some(h) = s.target_h {
        tag.push_str(&format!("_{h}p"));
    }
    let ext = match s.format {
        Format::Mp4 => {
            if s.compress {
                tag.push_str("_c");
            }
            "mp4"
        }
        Format::Gif => {
            tag.push_str(match s.quality {
                2 => "_hi",
                1 => "_med",
                _ => "_lo",
            });
            if s.max_secs > 0 {
                tag.push_str(&format!("_{}s", s.max_secs));
            }
            "gif"
        }
    };
    if tag.is_empty() {
        tag.push_str("_export");
    }
    let mut path = dir.join(format!("{stem}{tag}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}{tag}_{n}.{ext}"));
        n += 1;
    }
    path
}

struct MfGuard;
impl MfGuard {
    fn new() -> windows::core::Result<MfGuard> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)? };
        Ok(MfGuard)
    }
}
impl Drop for MfGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

unsafe fn native_video_type(reader: &IMFSourceReader) -> windows::core::Result<(u32, u32, u32)> {
    let t = reader.GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, 0)?;
    let (w, h) = unpack(t.GetUINT64(&MF_MT_FRAME_SIZE)?);
    let fps = match t.GetUINT64(&MF_MT_FRAME_RATE) {
        Ok(v) => {
            let (num, den) = unpack(v);
            if den == 0 {
                30
            } else {
                ((num as f64 / den as f64).round() as u32).clamp(1, 120)
            }
        }
        Err(_) => 30,
    };
    Ok((w.max(2), h.max(2), fps))
}

/// Probe the source for resolution, frame rate, and duration.
pub fn probe(path: &Path) -> Result<ProbeInfo, String> {
    unsafe {
        // Called from the UI (STA) thread; requesting MTA here fails without
        // changing the apartment, so only balance CoUninitialize when this
        // call actually initialized COM.
        let co = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| -> windows::core::Result<ProbeInfo> {
            let _mf = MfGuard::new()?;
            let url = util::wide(&path.to_string_lossy());
            let reader = MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), None)?;
            let (w, h, fps) = native_video_type(&reader)?;
            let dur = duration_100ns(&reader);
            Ok(ProbeInfo { w, h, fps, dur_100ns: dur })
        })();
        if co.is_ok() {
            CoUninitialize();
        }
        result.map_err(|e| e.to_string())
    }
}

unsafe fn duration_100ns(reader: &IMFSourceReader) -> i64 {
    use windows::Win32::Media::MediaFoundation::{MF_PD_DURATION, MF_SOURCE_READER_MEDIASOURCE};
    use windows::Win32::System::Com::StructuredStorage::PropVariantToInt64;
    match reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION) {
        Ok(pv) => {
            let v = PropVariantToInt64(&pv).unwrap_or(0);
            v.max(0)
        }
        Err(_) => 0,
    }
}

unsafe fn open_reader(path: &Path) -> windows::core::Result<IMFSourceReader> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 2)?;
    let attrs = attrs.unwrap();
    attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
    attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    let url = util::wide(&path.to_string_lossy());
    let reader = MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), Some(&attrs))?;

    // Video only: drop every stream, then re-enable the first video track.
    reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
    reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
    Ok(reader)
}

unsafe fn set_reader_output(reader: &IMFSourceReader, tw: u32, th: u32) -> windows::core::Result<()> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
    t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    t.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &t)
}

unsafe fn set_reader_output_rgb(reader: &IMFSourceReader, tw: u32, th: u32) -> windows::core::Result<()> {
    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
    t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    t.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    // A positive stride requests top-down rows, matching the GIF read loop.
    t.SetUINT32(&MF_MT_DEFAULT_STRIDE, tw * 4)?;
    reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &t)
}

unsafe fn open_writer(
    path: &Path,
    tw: u32,
    th: u32,
    fps: u32,
    bitrate_bps: u32,
) -> windows::core::Result<(IMFSinkWriter, u32)> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 1)?;
    let attrs = attrs.unwrap();
    attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;

    let url = util::wide(&path.to_string_lossy());
    let writer = MFCreateSinkWriterFromURL(PCWSTR(url.as_ptr()), None, Some(&attrs))?;

    let out = MFCreateMediaType()?;
    out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
    out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps)?;
    out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    out.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
    out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
    let stream = writer.AddStream(&out)?;

    let inp = MFCreateMediaType()?;
    inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
    inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    inp.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    inp.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
    writer.SetInputMediaType(stream, &inp, None)?;

    writer.BeginWriting()?;
    Ok((writer, stream))
}

#[allow(clippy::too_many_arguments)]
unsafe fn transcode(
    reader: &IMFSourceReader,
    writer: &IMFSinkWriter,
    stream: u32,
    speed: f64,
    fps: u32,
    dur_100ns: i64,
    cancel: &AtomicBool,
    notify: HwndSend,
) -> Result<(), String> {
    let out_dur = 10_000_000i64 / fps.max(1) as i64;
    let mut next_emit = 0i64;
    let mut wrote_any = false;
    let mut last_permille = -1i32;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("canceled".into());
        }
        let mut flags = 0u32;
        let mut ts = 0i64;
        let mut sample: Option<IMFSample> = None;
        reader
            .ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )
            .map_err(|e| format!("read failed: {e}"))?;

        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else {
            continue; // stream tick without a frame (format change, gap)
        };

        let out_pts = (ts as f64 / speed) as i64;
        if out_pts < next_emit {
            continue; // dropped to hold the output frame rate
        }
        sample.SetSampleTime(out_pts).map_err(|e| e.to_string())?;
        sample.SetSampleDuration(out_dur).map_err(|e| e.to_string())?;
        writer.WriteSample(stream, &sample).map_err(|e| format!("write failed: {e}"))?;
        next_emit = out_pts + out_dur;
        wrote_any = true;

        if dur_100ns > 0 {
            let permille = ((ts.min(dur_100ns) * 1000) / dur_100ns) as i32;
            if permille != last_permille {
                last_permille = permille;
                let _ = PostMessageW(notify.hwnd(), MSG_EXPORT_PROGRESS, WPARAM(permille as usize), LPARAM(0));
            }
        }
    }

    writer.Finalize().map_err(|e| format!("finalize failed: {e}"))?;
    if !wrote_any {
        return Err("no frames were exported".into());
    }
    Ok(())
}

/// Decode frames and write a looping GIF, honoring speed (frame-dropping),
/// the quality's frame rate, and an optional duration cap.
#[allow(clippy::too_many_arguments)]
unsafe fn export_gif(
    reader: &IMFSourceReader,
    tw: u32,
    th: u32,
    fps: u32,
    speed: f64,
    dur_100ns: i64,
    max_secs: u32,
    max_colors: u16,
    dst: &Path,
    cancel: &AtomicBool,
    notify: HwndSend,
) -> Result<(), String> {
    let mut gw = GifWriter::new(tw as u16, th as u16);
    let interval = 10_000_000i64 / fps.max(1) as i64;
    let cap = if max_secs > 0 { max_secs as i64 * 10_000_000 } else { i64::MAX };
    let denom = {
        let full = (dur_100ns as f64 / speed.max(0.1)) as i64;
        (if cap < full { cap } else { full }).max(1)
    };
    let stride = tw as usize * 4;
    let mut rgb = vec![0u8; tw as usize * th as usize * 3];
    let mut next_emit = 0i64;
    let mut delay_acc = 0.0f64;
    let mut wrote = false;
    let mut last_permille = -1i32;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("canceled".into());
        }
        let mut flags = 0u32;
        let mut ts = 0i64;
        let mut sample: Option<IMFSample> = None;
        reader
            .ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )
            .map_err(|e| format!("read failed: {e}"))?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else { continue };

        let out_pts = (ts as f64 / speed.max(0.1)) as i64;
        if out_pts > cap {
            break;
        }
        if out_pts < next_emit {
            continue;
        }

        let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut ptr, None, Some(&mut len)).map_err(|e| e.to_string())?;
        if !ptr.is_null() && len as usize >= stride * th as usize {
            let src = std::slice::from_raw_parts(ptr, len as usize);
            for y in 0..th as usize {
                let row = y * stride;
                let orow = y * tw as usize * 3;
                for x in 0..tw as usize {
                    let i = row + x * 4;
                    let o = orow + x * 3;
                    rgb[o] = src[i + 2]; // R (source is BGRA)
                    rgb[o + 1] = src[i + 1]; // G
                    rgb[o + 2] = src[i]; // B
                }
            }
        }
        let _ = buffer.Unlock();

        delay_acc += 100.0 / fps.max(1) as f64;
        let d = delay_acc.round();
        delay_acc -= d;
        gw.add_frame(&rgb, (d as u16).max(2), max_colors);
        next_emit = out_pts + interval;
        wrote = true;

        let permille = ((out_pts.min(denom) * 1000) / denom) as i32;
        if permille != last_permille {
            last_permille = permille;
            let _ = PostMessageW(notify.hwnd(), MSG_EXPORT_PROGRESS, WPARAM(permille as usize), LPARAM(0));
        }
    }

    if !wrote {
        return Err("no frames were exported".into());
    }
    std::fs::write(dst, gw.finish()).map_err(|e| format!("write failed: {e}"))?;
    Ok(())
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Click-driven zoom timeline: output-time seconds → scale + center (content px).
struct Zoom {
    events: Vec<(f64, f32, f32)>,
}

impl Zoom {
    const ZMAX: f32 = 1.6;
    const IN: f64 = 0.35;
    const OUT: f64 = 0.45;
    const DUR: f64 = 1.8;

    /// Load the `.clicks` sidecar. Click coords are in the recording's native
    /// pixels; scale them to the content size and its times onto the sped-up
    /// output timeline.
    fn load(src: &Path, sw: u32, sh: u32, tw: u32, th: u32, speed: f64, enabled: bool) -> Zoom {
        let mut events = Vec::new();
        if enabled {
            if let Ok(txt) = std::fs::read_to_string(src.with_extension("clicks")) {
                let sp = speed.max(0.1);
                for line in txt.lines() {
                    let mut it = line.split_whitespace();
                    if let (Some(a), Some(b), Some(c)) = (it.next(), it.next(), it.next()) {
                        if let (Ok(ms), Ok(x), Ok(y)) = (a.parse::<i64>(), b.parse::<i32>(), c.parse::<i32>()) {
                            let t = (ms as f64 / 1000.0) / sp;
                            let fx = x as f32 * tw as f32 / sw.max(1) as f32;
                            let fy = y as f32 * th as f32 / sh.max(1) as f32;
                            events.push((t, fx, fy));
                        }
                    }
                }
            }
        }
        Zoom { events }
    }

    /// (scale, center_x, center_y) at output time `t` seconds.
    fn at(&self, t: f64, tw: u32, th: u32) -> (f32, f32, f32) {
        let mut active = None;
        for &(ts, x, y) in &self.events {
            if t >= ts && t < ts + Self::DUR {
                active = Some((ts, x, y)); // latest wins
            }
        }
        match active {
            None => (1.0, tw as f32 / 2.0, th as f32 / 2.0),
            Some((ts, x, y)) => {
                let u = t - ts;
                let env = if u < Self::IN {
                    smoothstep(u / Self::IN)
                } else if u > Self::DUR - Self::OUT {
                    smoothstep((Self::DUR - u) / Self::OUT)
                } else {
                    1.0
                };
                (1.0 + (Self::ZMAX - 1.0) * env as f32, x, y)
            }
        }
    }
}

unsafe fn write_rgb_sample(writer: &IMFSinkWriter, stream: u32, bgra: &[u8], ts: i64, dur: i64) -> windows::core::Result<()> {
    let buffer = MFCreateMemoryBuffer(bgra.len() as u32)?;
    let mut dst = std::ptr::null_mut();
    buffer.Lock(&mut dst, None, None)?;
    std::ptr::copy_nonoverlapping(bgra.as_ptr(), dst, bgra.len());
    buffer.Unlock()?;
    buffer.SetCurrentLength(bgra.len() as u32)?;
    let sample: IMFSample = MFCreateSample()?;
    sample.AddBuffer(&buffer)?;
    sample.SetSampleTime(ts)?;
    sample.SetSampleDuration(dur)?;
    writer.WriteSample(stream, &sample)
}

unsafe fn open_writer_rgb(path: &Path, tw: u32, th: u32, fps: u32, bitrate_bps: u32) -> windows::core::Result<(IMFSinkWriter, u32)> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 1)?;
    let attrs = attrs.unwrap();
    attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
    let url = util::wide(&path.to_string_lossy());
    let writer = MFCreateSinkWriterFromURL(PCWSTR(url.as_ptr()), None, Some(&attrs))?;

    let out = MFCreateMediaType()?;
    out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
    out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps)?;
    out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    out.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
    out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
    let stream = writer.AddStream(&out)?;

    let inp = MFCreateMediaType()?;
    inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
    inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
    inp.SetUINT64(&MF_MT_FRAME_SIZE, pack(tw, th))?;
    inp.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
    inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, tw * 4)?;
    writer.SetInputMediaType(stream, &inp, None)?;

    writer.BeginWriting()?;
    Ok((writer, stream))
}

/// RGB compositing transcode: decode RGB32, apply click auto-zoom and/or a
/// backdrop per frame, re-encode. Content is `tw`×`th`; output is `fw`×`fh`.
#[allow(clippy::too_many_arguments)]
unsafe fn transcode_fx(
    reader: &IMFSourceReader,
    writer: &IMFSinkWriter,
    stream: u32,
    tw: u32,
    th: u32,
    fw: u32,
    fh: u32,
    fps: u32,
    speed: f64,
    dur_100ns: i64,
    bg: BgStyle,
    zoom: &Zoom,
    cancel: &AtomicBool,
    notify: HwndSend,
) -> Result<(), String> {
    let out_dur = 10_000_000i64 / fps.max(1) as i64;
    let stride = tw as usize * 4;
    let (pad, radius) = if bg != BgStyle::None {
        let (p, r) = compose::metrics(tw as i32, th as i32);
        (p as f32, r)
    } else {
        (0.0, 0.0)
    };
    let mut next_emit = 0i64;
    let mut wrote_any = false;
    let mut last_permille = -1i32;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("canceled".into());
        }
        let mut flags = 0u32;
        let mut ts = 0i64;
        let mut sample: Option<IMFSample> = None;
        reader
            .ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )
            .map_err(|e| format!("read failed: {e}"))?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else { continue };

        let out_pts = (ts as f64 / speed.max(0.1)) as i64;
        if out_pts < next_emit {
            continue;
        }

        let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut ptr, None, Some(&mut len)).map_err(|e| e.to_string())?;
        let composed = if !ptr.is_null() && len as usize >= stride * th as usize {
            let src = std::slice::from_raw_parts(ptr, len as usize);
            let t = out_pts as f64 / 10_000_000.0;
            let (z, cx, cy) = zoom.at(t, tw, th);
            let cw = (tw as f32 / z).max(2.0);
            let ch = (th as f32 / z).max(2.0);
            let sx = (cx - cw / 2.0).clamp(0.0, tw as f32 - cw);
            let sy = (cy - ch / 2.0).clamp(0.0, th as f32 - ch);
            Canvas::new(fw as i32, fh as i32).map(|c| {
                if bg != BgStyle::None {
                    let (top, bot) = bg.colors();
                    c.fill_gradient(0.0, 0.0, fw as f32, fh as f32, top, bot);
                    for i in 0..7 {
                        let e = i as f32 * 2.2;
                        let a = (16 - i * 2).max(0) as u8;
                        c.fill_rounded(pad - e, pad - e + 7.0, tw as f32 + e * 2.0, th as f32 + e * 2.0, radius + e, crate::win::ui::paint::argb(a, 0, 0, 0));
                    }
                    c.draw_image_crop(src, tw as i32, th as i32, sx, sy, cw, ch, pad, pad, tw as f32, th as f32, radius);
                } else {
                    c.draw_image_crop(src, tw as i32, th as i32, sx, sy, cw, ch, 0.0, 0.0, fw as f32, fh as f32, 0.0);
                }
                c.read_bgra()
            })
        } else {
            None
        };
        let _ = buffer.Unlock();

        if let Some(px) = composed {
            write_rgb_sample(writer, stream, &px, out_pts, out_dur).map_err(|e| format!("write failed: {e}"))?;
            next_emit = out_pts + out_dur;
            wrote_any = true;
        }

        if dur_100ns > 0 {
            let permille = ((ts.min(dur_100ns) * 1000) / dur_100ns) as i32;
            if permille != last_permille {
                last_permille = permille;
                let _ = PostMessageW(notify.hwnd(), MSG_EXPORT_PROGRESS, WPARAM(permille as usize), LPARAM(0));
            }
        }
    }

    writer.Finalize().map_err(|e| format!("finalize failed: {e}"))?;
    if !wrote_any {
        return Err("no frames were exported".into());
    }
    Ok(())
}

/// Export worker body. Posts MSG_EXPORT_PROGRESS (wparam = permille) during
/// the run and MSG_EXPORT_DONE at the end (lparam = boxed Result<name, err>).
pub fn run(src: PathBuf, dst: PathBuf, settings: Settings, info: ProbeInfo, cancel: Arc<AtomicBool>, notify: HwndSend) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let (tw, th, src_fps) = plan_dimensions(&info, settings.target_h);

    let result: Result<String, String> = unsafe {
        (|| -> Result<String, String> {
            let _mf = MfGuard::new().map_err(|e| e.to_string())?;
            let reader = open_reader(&src).map_err(|e| e.to_string())?;
            match settings.format {
                Format::Mp4 if settings.autozoom || settings.bg != BgStyle::None => {
                    // Pixel-touching path for zoom and/or backdrop.
                    let _ = set_reader_output_rgb(&reader, tw, th);
                    let (fw, fh) = if settings.bg != BgStyle::None {
                        let (w, h) = compose::output_size(tw as i32, th as i32);
                        (w as u32, h as u32)
                    } else {
                        (tw, th)
                    };
                    let kbps = plan_bitrate_kbps(fw, fh, src_fps, settings.compress);
                    let (writer, stream) = open_writer_rgb(&dst, fw, fh, src_fps, kbps.saturating_mul(1000))
                        .map_err(|e| e.to_string())?;
                    let zoom = Zoom::load(&src, info.w, info.h, tw, th, settings.speed, settings.autozoom);
                    transcode_fx(&reader, &writer, stream, tw, th, fw, fh, src_fps, settings.speed, info.dur_100ns, settings.bg, &zoom, &cancel, notify)?;
                }
                Format::Mp4 => {
                    // Ask the reader to scale; if it refuses, keep native size.
                    let _ = set_reader_output(&reader, tw, th);
                    let kbps = plan_bitrate_kbps(tw, th, src_fps, settings.compress);
                    let (writer, stream) = open_writer(&dst, tw, th, src_fps, kbps.saturating_mul(1000))
                        .map_err(|e| e.to_string())?;
                    transcode(&reader, &writer, stream, settings.speed, src_fps, info.dur_100ns, &cancel, notify)?;
                }
                Format::Gif => {
                    let _ = set_reader_output_rgb(&reader, tw, th);
                    let (colors, _) = gif_params(settings.quality);
                    let fps = gif_fps(settings.quality, src_fps);
                    export_gif(&reader, tw, th, fps, settings.speed, info.dur_100ns, settings.max_secs, colors, &dst, &cancel, notify)?;
                }
            }
            Ok(dst.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
        })()
    };

    if result.is_err() {
        let _ = std::fs::remove_file(&dst); // no orphaned partial file
    }
    unsafe {
        CoUninitialize();
    }

    let payload: Box<Result<String, String>> = Box::new(result);
    unsafe {
        let _ = PostMessageW(notify.hwnd(), MSG_EXPORT_DONE, WPARAM(0), LPARAM(Box::into_raw(payload) as isize));
    }}
