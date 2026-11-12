//! MP4 muxing and encoding through the Media Foundation sink writer.
//! H.264 for video (hardware encoders are preferred when present, with an
//! automatic software fallback) and AAC for audio. The writer accepts raw
//! BGRA frames and PCM and inserts the required converters itself.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSample, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM,
    MFCreateAttributes, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFShutdown, MFStartup,
    MFVideoFormat_H264, MFVideoFormat_RGB32, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SDK_VERSION, MF_API_VERSION,
    MFSTARTUP_NOSOCKET,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::app::EncMsg;
use crate::win::{util, MSG_ENC_DONE};

const MF_VERSION: u32 = (MF_SDK_VERSION << 16) | MF_API_VERSION;
const MFVIDEO_INTERLACE_PROGRESSIVE: u32 = 2;

#[derive(Clone, Copy)]
pub struct VideoParams {
    pub w: u32,
    pub h: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
}

#[derive(Clone, Copy)]
pub struct AudioParams {
    /// Device mix rate; output AAC uses the same rate (44.1/48 kHz).
    pub rate: u32,
}

/// HWND smuggled across threads for PostMessageW notifications.
#[derive(Clone, Copy)]
pub struct HwndSend(isize);

unsafe impl Send for HwndSend {}

impl From<HWND> for HwndSend {
    fn from(h: HWND) -> Self {
        HwndSend(h.0 as isize)
    }
}

impl HwndSend {
    pub fn hwnd(&self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }
}

fn packed(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

struct Writer {
    writer: IMFSinkWriter,
    video_stream: u32,
    audio_stream: Option<u32>,
    video_duration: i64,
}

fn open_writer(path: &std::path::Path, v: VideoParams, a: Option<AudioParams>) -> windows::core::Result<Writer> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;

        let path_w = util::wide(&path.to_string_lossy());
        let writer: IMFSinkWriter =
            MFCreateSinkWriterFromURL(PCWSTR(path_w.as_ptr()), None, Some(&attrs))?;

        // Video output: H.264.
        let out = MFCreateMediaType()?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, v.bitrate_bps)?;
        out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
        out.SetUINT64(&MF_MT_FRAME_SIZE, packed(v.w, v.h))?;
        out.SetUINT64(&MF_MT_FRAME_RATE, packed(v.fps, 1))?;
        out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, packed(1, 1))?;
        let video_stream = writer.AddStream(&out)?;

        // Video input: top-down BGRA straight from the grabber. The positive
        // stride marks the buffer as top-down.
        let inp = MFCreateMediaType()?;
        inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVIDEO_INTERLACE_PROGRESSIVE)?;
        inp.SetUINT64(&MF_MT_FRAME_SIZE, packed(v.w, v.h))?;
        inp.SetUINT64(&MF_MT_FRAME_RATE, packed(v.fps, 1))?;
        inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, v.w * 4)?;
        writer.SetInputMediaType(video_stream, &inp, None)?;

        // Audio: stereo 16-bit PCM in, AAC out.
        let audio_stream = match a {
            None => None,
            Some(a) => {
                let out = MFCreateMediaType()?;
                out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
                out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.rate)?;
                out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 16_000)?; // 128 kbit/s
                let idx = writer.AddStream(&out)?;

                let inp = MFCreateMediaType()?;
                inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                inp.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
                inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.rate)?;
                inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2)?;
                inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 4)?;
                inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, a.rate * 4)?;
                writer.SetInputMediaType(idx, &inp, None)?;
                Some(idx)
            }
        };

        writer.BeginWriting()?;
        Ok(Writer {
            writer,
            video_stream,
            audio_stream,
            video_duration: 10_000_000 / v.fps.max(1) as i64,
        })
    }
}

unsafe fn write_sample(
    writer: &IMFSinkWriter,
    stream: u32,
    bytes: &[u8],
    ts: i64,
    duration: i64,
) -> windows::core::Result<()> {
    let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
    let mut dst = std::ptr::null_mut();
    buffer.Lock(&mut dst, None, None)?;
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    buffer.Unlock()?;
    buffer.SetCurrentLength(bytes.len() as u32)?;

    let sample: IMFSample = MFCreateSample()?;
    sample.AddBuffer(&buffer)?;
    sample.SetSampleTime(ts)?;
    sample.SetSampleDuration(duration)?;
    writer.WriteSample(stream, &sample)
}

fn pcm_bytes(pcm: &[i16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(pcm.as_ptr() as *const u8, pcm.len() * 2) }
}

/// Encoder thread body. Consumes frames/audio until Finish, finalizes the
/// file, and posts MSG_ENC_DONE (lparam: boxed error string or null).
pub fn run(
    path: PathBuf,
    v: VideoParams,
    a: Option<AudioParams>,
    rx: Receiver<EncMsg>,
    pool_tx: Sender<Vec<u8>>,
    notify: HwndSend,
) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut error: Option<String> = None;

    unsafe {
        match MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
            Ok(()) => {
                match open_writer(&path, v, a) {
                    Err(e) => {
                        error = Some(format!("could not open the MP4 writer: {e}"));
                        // Drain so capture threads never block on a dead sink.
                        while !matches!(rx.recv(), Ok(EncMsg::Finish) | Err(_)) {}
                    }
                    Ok(w) => {
                        let mut wrote_any = false;
                        loop {
                            match rx.recv() {
                                Ok(EncMsg::Video { buf, ts }) => {
                                    if error.is_none() {
                                        if let Err(e) = write_sample(
                                            &w.writer,
                                            w.video_stream,
                                            &buf,
                                            ts,
                                            w.video_duration,
                                        ) {
                                            error = Some(format!("video write failed: {e}"));
                                        } else {
                                            wrote_any = true;
                                        }
                                    }
                                    let _ = pool_tx.send(buf);
                                }
                                Ok(EncMsg::Audio { pcm, ts, frames }) => {
                                    if let (None, Some(stream)) = (&error, w.audio_stream) {
                                        let dur = frames as i64 * 10_000_000
                                            / a.map(|a| a.rate).unwrap_or(48_000).max(1) as i64;
                                        if let Err(e) =
                                            write_sample(&w.writer, stream, pcm_bytes(&pcm), ts, dur)
                                        {
                                            error = Some(format!("audio write failed: {e}"));
                                        }
                                    }
                                }
                                Ok(EncMsg::Finish) | Err(_) => break,
                            }
                        }
                        if let Err(e) = w.writer.Finalize() {
                            if error.is_none() {
                                error = Some(format!("finalize failed: {e}"));
                            }
                        }
                        if !wrote_any && error.is_none() {
                            error = Some("no frames were captured".into());
                            let _ = std::fs::remove_file(&path);
                        }
                    }
                }
                let _ = MFShutdown();
            }
            Err(e) => error = Some(format!("Media Foundation startup failed: {e}")),
        }
        CoUninitialize();
    }

    let lparam = match error {
        None => 0isize,
        Some(e) => Box::into_raw(Box::new(e)) as isize,
    };
    unsafe {
        let _ = PostMessageW(notify.hwnd(), MSG_ENC_DONE, WPARAM(0), LPARAM(lparam));
    }
}
