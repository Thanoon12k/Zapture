//! Default webcam capture through a Media Foundation capture Source Reader.
//! A background thread pulls RGB32 frames into a shared latest-frame slot; the
//! picture-in-picture window paints from it. Frames are normalized to
//! top-down BGRA at the device's native size; the window crops and scales.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFMediaSource, IMFSample, IMFSourceReader, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources, MFMediaType_Video,
    MFShutdown, MFStartup, MFVideoFormat_RGB32, MF_API_VERSION,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SDK_VERSION, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MFSTARTUP_NOSOCKET,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

const MF_VERSION: u32 = (MF_SDK_VERSION << 16) | MF_API_VERSION;

pub struct CamFrame {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>, // top-down, tightly packed (w*4 stride)
}

/// Shared state between the capture thread and the UI.
pub struct Camera {
    frame: Mutex<Option<CamFrame>>,
    stop: AtomicBool,
}

impl Camera {
    fn new() -> Camera {
        Camera { frame: Mutex::new(None), stop: AtomicBool::new(false) }
    }

    /// Run `f` against the newest frame, if any has arrived yet.
    pub fn with_frame<R>(&self, f: impl FnOnce(&CamFrame) -> R) -> Option<R> {
        let g = self.frame.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().map(f)
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

/// Open the default video capture device and return a reader configured for
/// RGB32, along with the negotiated width, height, and (signed) stride.
unsafe fn open_reader() -> windows::core::Result<(IMFSourceReader, u32, u32, i32)> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 1)?;
    let attrs = attrs.unwrap();
    attrs.SetGUID(
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    )?;

    let mut devices: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    MFEnumDeviceSources(&attrs, &mut devices, &mut count)?;
    if count == 0 || devices.is_null() {
        return Err(windows::core::Error::from_win32());
    }

    // Own the first device, activate its media source, then release every
    // entry and free the array.
    let first = std::ptr::read(devices);
    let source_res: windows::core::Result<IMFMediaSource> = match first.as_ref() {
        Some(a) => a.ActivateObject::<IMFMediaSource>(),
        None => Err(windows::core::Error::from_win32()),
    };
    drop(first);
    for i in 1..count as usize {
        drop(std::ptr::read(devices.add(i)));
    }
    CoTaskMemFree(Some(devices as *const core::ffi::c_void));
    let source = source_res?;

    let mut rattrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut rattrs, 1)?;
    let rattrs = rattrs.unwrap();
    rattrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
    let reader = MFCreateSourceReaderFromMediaSource(&source, Some(&rattrs))?;

    let t = MFCreateMediaType()?;
    t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
    reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &t)?;

    let cur = reader.GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)?;
    let (w, h) = unpack(cur.GetUINT64(&MF_MT_FRAME_SIZE)?);
    let stride = cur
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|v| v as i32)
        .unwrap_or((w * 4) as i32);
    Ok((reader, w.max(1), h.max(1), stride))
}

fn run(cam: Arc<Camera>, ready: std::sync::mpsc::Sender<bool>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_err() {
            let _ = ready.send(false);
            CoUninitialize();
            return;
        }
    }

    let opened = unsafe { open_reader() };
    let (reader, w, h, stride) = match opened {
        Ok(v) => {
            let _ = ready.send(true);
            v
        }
        Err(_) => {
            let _ = ready.send(false);
            unsafe {
                let _ = MFShutdown();
                CoUninitialize();
            }
            return;
        }
    };

    let row = w as usize * 4;
    let stride_abs = stride.unsigned_abs() as usize;
    let top_down = stride >= 0;

    while !cam.stop.load(Ordering::Acquire) {
        let mut flags = 0u32;
        let mut ts = 0i64;
        let mut sample: Option<IMFSample> = None;
        let r = unsafe {
            reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )
        };
        if r.is_err() || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };

        unsafe {
            if let Ok(buffer) = sample.ConvertToContiguousBuffer() {
                let mut ptr: *mut u8 = std::ptr::null_mut();
                let mut len = 0u32;
                if buffer.Lock(&mut ptr, None, Some(&mut len)).is_ok() {
                    if !ptr.is_null() && stride_abs >= row && len as usize >= stride_abs * h as usize {
                        let src = std::slice::from_raw_parts(ptr, len as usize);
                        let mut out = vec![0u8; row * h as usize];
                        for y in 0..h as usize {
                            let sy = if top_down { y } else { h as usize - 1 - y };
                            out[y * row..y * row + row]
                                .copy_from_slice(&src[sy * stride_abs..sy * stride_abs + row]);
                        }
                        *cam.frame.lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(CamFrame { w, h, bgra: out });
                    }
                    let _ = buffer.Unlock();
                }
            }
        }
    }

    unsafe {
        let _ = MFShutdown();
        CoUninitialize();
    }
}

/// Start the default webcam. Returns the shared state and the worker handle,
/// or `None` if no camera is available or it failed to start within 2s.
pub fn start() -> Option<(Arc<Camera>, JoinHandle<()>)> {
    let cam = Arc::new(Camera::new());
    let (tx, rx) = channel();
    let worker = {
        let cam = cam.clone();
        std::thread::spawn(move || run(cam, tx))
    };
    match rx.recv_timeout(Duration::from_millis(2000)) {
        Ok(true) => Some((cam, worker)),
        _ => {
            cam.request_stop();
            let _ = worker.join();
            None
        }
    }
}
