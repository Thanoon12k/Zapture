//! Audio capture through WASAPI shared mode: system loopback, microphone,
//! or both mixed into one stereo track.
//!
//! Loopback only delivers packets while the render engine is active, so a
//! silent render stream on the same device keeps it running; without it,
//! silence would produce gaps and desynchronize the mix. When mixing, the
//! system stream is the master clock and microphone frames are pulled
//! through a small linear resampler to match its rate. Timestamps are
//! contiguous from the master sample counter, anchored to the shared clock
//! at the first packet.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};

use crate::app::{EncMsg, Shared};
use crate::config::AudioSource;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const AUDCLNT_BUFFERFLAGS_SILENT: u32 = 0x2;
const BUFFER_100NS: i64 = 2_000_000; // 200 ms

#[derive(Clone, Copy, PartialEq)]
enum SampleKind {
    F32,
    I16,
}

struct Mix {
    kind: SampleKind,
    channels: usize,
    rate: u32,
}

unsafe fn classify(wf: *const WAVEFORMATEX) -> Result<Mix, String> {
    let tag = (*wf).wFormatTag;
    let bits = (*wf).wBitsPerSample;
    let kind = match tag {
        WAVE_FORMAT_IEEE_FLOAT => SampleKind::F32,
        WAVE_FORMAT_PCM if bits == 16 => SampleKind::I16,
        WAVE_FORMAT_EXTENSIBLE => {
            let ext = &*(wf as *const WAVEFORMATEXTENSIBLE);
            // Data1 of the subformat GUID mirrors the classic wave tag.
            match ext.SubFormat.data1 as u16 {
                WAVE_FORMAT_IEEE_FLOAT => SampleKind::F32,
                WAVE_FORMAT_PCM if bits == 16 => SampleKind::I16,
                other => return Err(format!("unsupported mix subformat {other:#x}")),
            }
        }
        other => return Err(format!("unsupported mix format tag {other:#x}")),
    };
    Ok(Mix {
        kind,
        channels: (*wf).nChannels.max(1) as usize,
        rate: (*wf).nSamplesPerSec,
    })
}

/// Convert an interleaved device packet to stereo f32 pairs. Multichannel
/// layouts keep their front left/right pair.
unsafe fn packet_to_stereo(
    kind: SampleKind,
    data: *const u8,
    frames: usize,
    channels: usize,
    silent: bool,
    out: &mut Vec<(f32, f32)>,
) {
    out.clear();
    out.reserve(frames);
    if silent || data.is_null() {
        out.resize(frames, (0.0, 0.0));
        return;
    }
    match kind {
        SampleKind::F32 => {
            let s = std::slice::from_raw_parts(data as *const f32, frames * channels);
            for f in 0..frames {
                let l = s[f * channels];
                let r = s[f * channels + 1.min(channels - 1)];
                out.push((l, r));
            }
        }
        SampleKind::I16 => {
            let s = std::slice::from_raw_parts(data as *const i16, frames * channels);
            for f in 0..frames {
                let l = s[f * channels] as f32 / 32768.0;
                let r = s[f * channels + 1.min(channels - 1)] as f32 / 32768.0;
                out.push((l, r));
            }
        }
    }
}

struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    mix: Mix,
}

struct Keepalive {
    client: IAudioClient,
    render: IAudioRenderClient,
    frames: u32,
}

unsafe fn open_capture(device: &IMMDevice, loopback: bool) -> Result<Stream, String> {
    let fail = |ctx: &'static str| move |e: windows::core::Error| format!("{ctx}: {e}");
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(fail("activate"))?;
    let wf = client.GetMixFormat().map_err(fail("mix format"))?;
    let result = classify(wf).and_then(|mix| {
        let flags = if loopback { AUDCLNT_STREAMFLAGS_LOOPBACK } else { 0 };
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_100NS, 0, wf, None)
            .map_err(fail("capture init"))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(fail("capture service"))?;
        Ok(Stream { client, capture, mix })
    });
    CoTaskMemFree(Some(wf as *const core::ffi::c_void));
    result
}

unsafe fn open_keepalive(device: &IMMDevice) -> Result<Keepalive, String> {
    let fail = |ctx: &'static str| move |e: windows::core::Error| format!("{ctx}: {e}");
    let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(fail("activate render"))?;
    let wf = client.GetMixFormat().map_err(fail("render mix format"))?;
    let result = (|| {
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER_100NS, 0, wf, None)
            .map_err(fail("render init"))?;
        let render: IAudioRenderClient = client.GetService().map_err(fail("render service"))?;
        let frames = client.GetBufferSize().map_err(fail("render buffer"))?;
        Ok(Keepalive { client, render, frames })
    })();
    CoTaskMemFree(Some(wf as *const core::ffi::c_void));
    result
}

unsafe fn top_up_silence(ka: &Keepalive) {
    if let Ok(padding) = ka.client.GetCurrentPadding() {
        let free = ka.frames.saturating_sub(padding);
        if free > 0 && ka.render.GetBuffer(free).is_ok() {
            let _ = ka.render.ReleaseBuffer(free, AUDCLNT_BUFFERFLAGS_SILENT);
        }
    }
}

/// Pulls microphone frames at the master rate via linear interpolation.
struct MicFeed {
    fifo: VecDeque<(f32, f32)>,
    pos: f64,
    step: f64,
    cap: usize,
}

impl MicFeed {
    fn new(mic_rate: u32, master_rate: u32) -> MicFeed {
        MicFeed {
            fifo: VecDeque::new(),
            pos: 0.0,
            step: mic_rate as f64 / master_rate.max(1) as f64,
            cap: mic_rate as usize * 4, // keep at most ~4 s buffered
        }
    }

    fn push(&mut self, frames: &[(f32, f32)]) {
        self.fifo.extend(frames.iter().copied());
        if self.fifo.len() > self.cap {
            let drop = self.fifo.len() - self.cap;
            self.fifo.drain(..drop);
            self.pos = 0.0;
        }
    }

    fn clear(&mut self) {
        self.fifo.clear();
        self.pos = 0.0;
    }

    /// Mix `out.len()` master-rate frames of microphone signal into `out`.
    fn mix_into(&mut self, out: &mut [(f32, f32)]) {
        for slot in out.iter_mut() {
            let i = self.pos as usize;
            if i + 1 >= self.fifo.len() {
                break; // starved; the rest stays as-is (silence from the mic)
            }
            let frac = (self.pos - i as f64) as f32;
            let a = self.fifo[i];
            let b = self.fifo[i + 1];
            slot.0 += a.0 + (b.0 - a.0) * frac;
            slot.1 += a.1 + (b.1 - a.1) * frac;
            self.pos += self.step;
        }
        // Drop consumed history, keeping one frame for interpolation.
        let consumed = (self.pos as usize).saturating_sub(1);
        if consumed > 0 {
            self.fifo.drain(..consumed.min(self.fifo.len()));
            self.pos -= consumed as f64;
        }
    }
}

fn to_i16(pairs: &[(f32, f32)], out: &mut Vec<i16>) {
    out.clear();
    out.reserve(pairs.len() * 2);
    for &(l, r) in pairs {
        out.push((l.clamp(-1.0, 1.0) * 32767.0) as i16);
        out.push((r.clamp(-1.0, 1.0) * 32767.0) as i16);
    }
}

/// Drain every pending packet from a capture stream into `sink`.
/// Returns false when the device turned invalid.
unsafe fn drain(stream: &Stream, mut sink: impl FnMut(&mut Vec<(f32, f32)>, u32)) -> bool {
    let mut scratch = Vec::new();
    loop {
        let pending = match stream.capture.GetNextPacketSize() {
            Ok(n) => n,
            Err(_) => return false,
        };
        if pending == 0 {
            return true;
        }
        let mut data = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        if stream
            .capture
            .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
            .is_err()
        {
            return false;
        }
        if frames > 0 {
            let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT != 0;
            packet_to_stereo(
                stream.mix.kind,
                data,
                frames as usize,
                stream.mix.channels,
                silent,
                &mut scratch,
            );
            sink(&mut scratch, frames);
        }
        let _ = stream.capture.ReleaseBuffer(frames);
    }
}

struct Graph {
    master: Stream,
    keepalive: Option<Keepalive>,
    mic: Option<(Stream, MicFeed)>,
}

unsafe fn open_graph(source: AudioSource) -> Result<Graph, String> {
    let fail = |ctx: &'static str| move |e: windows::core::Error| format!("{ctx}: {e}");
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(fail("device enumerator"))?;

    let render_dev = || enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(fail("no output device"));
    let capture_dev = || enumerator.GetDefaultAudioEndpoint(eCapture, eConsole).map_err(fail("no microphone"));

    match source {
        AudioSource::System => {
            let dev = render_dev()?;
            Ok(Graph {
                master: open_capture(&dev, true)?,
                keepalive: Some(open_keepalive(&dev)?),
                mic: None,
            })
        }
        AudioSource::Mic => Ok(Graph {
            master: open_capture(&capture_dev()?, false)?,
            keepalive: None,
            mic: None,
        }),
        AudioSource::Both => {
            let dev = render_dev()?;
            let master = open_capture(&dev, true)?;
            let keepalive = Some(open_keepalive(&dev)?);
            let mic_stream = open_capture(&capture_dev()?, false)?;
            let feed = MicFeed::new(mic_stream.mix.rate, master.mix.rate);
            Ok(Graph { master, keepalive, mic: Some((mic_stream, feed)) })
        }
        AudioSource::Off => Err("audio disabled".into()),
    }
}

/// Audio thread body. Reports the master sample rate (or an error) on
/// `ready`, then streams stereo i16 packets until `sh.stop`.
pub fn run(sh: Arc<Shared>, tx: SyncSender<EncMsg>, ready: Sender<Result<u32, String>>, source: AudioSource) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let mut graph = match unsafe { open_graph(source) } {
        Ok(g) => g,
        Err(e) => {
            let _ = ready.send(Err(e));
            unsafe { CoUninitialize() };
            return;
        }
    };

    let started = unsafe {
        graph
            .master
            .client
            .Start()
            .and_then(|_| match &graph.keepalive {
                Some(ka) => ka.client.Start(),
                None => Ok(()),
            })
            .and_then(|_| match &graph.mic {
                Some((m, _)) => m.client.Start(),
                None => Ok(()),
            })
    };
    if let Err(e) = started {
        let _ = ready.send(Err(format!("audio start: {e}")));
        unsafe { CoUninitialize() };
        return;
    }
    let _ = ready.send(Ok(graph.master.mix.rate));

    let master_rate = graph.master.mix.rate.max(1);
    let mut pcm = Vec::new();
    let mut base_ts: Option<i64> = None;
    let mut samples_sent: u64 = 0;

    'outer: while !sh.stop.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(5));
        let armed = sh.armed.load(Ordering::Acquire);
        unsafe {
            if let Some(ka) = &graph.keepalive {
                top_up_silence(ka);
            }

            // Microphone side first: buffer (or discard while paused).
            let mut mic_dead = false;
            if let Some((mic_stream, feed)) = &mut graph.mic {
                let ok = drain(mic_stream, |frames, _| {
                    if armed {
                        feed.push(frames);
                    }
                });
                if !ok {
                    mic_dead = true;
                }
                if !armed {
                    feed.clear();
                }
            }
            if mic_dead {
                graph.mic = None; // mic vanished; keep recording system audio
            }

            // Master side drives timestamps and delivery.
            let mic = &mut graph.mic;
            let clock = &sh.clock;
            let tx = &tx;
            let ok = drain(&graph.master, |frames, count| {
                if !armed {
                    return;
                }
                if let Some((_, feed)) = mic {
                    feed.mix_into(frames);
                }
                to_i16(frames, &mut pcm);
                let base = *base_ts.get_or_insert_with(|| clock.rel_100ns());
                let ts = base + (samples_sent as i64) * 10_000_000 / master_rate as i64;
                samples_sent += count as u64;
                let _ = tx.try_send(EncMsg::Audio { pcm: pcm.clone(), ts, frames: count });
            });
            if !ok {
                // Master device invalidated (unplugged, format change): the
                // recording continues video-only; idle until the session ends.
                while !sh.stop.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                break 'outer;
            }
        }
    }

    unsafe {
        let _ = graph.master.client.Stop();
        if let Some(ka) = &graph.keepalive {
            let _ = ka.client.Stop();
        }
        if let Some((m, _)) = &graph.mic {
            let _ = m.client.Stop();
        }
        drop(graph);
        CoUninitialize();
    }
}
