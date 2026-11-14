# Size, hardware and performance

Measured from the published **v0.1.0-alpha.2 Windows x64 ZIP**, downloaded November 14, 2026:

| Measurement | Bytes | Decimal MB (rounded) |
|---|---:|---:|
| Compressed download | 758,272 | 0.76 |
| Zapture.exe | 1,198,080 | 1.20 |
| All extracted files, including docs/licenses | 1,677,378 | 1.68 |

Method: ZIP file byte length, executable entry's uncompressed size, and the sum of all ZIP entries' uncompressed sizes. Filesystem allocation and recorded media are not included. These figures describe disk/download size, **not RAM consumption**.

## Implementation facts

Windows uses a native Rust/Win32 UI, WASAPI audio, GDI screen capture, and Media Foundation H.264 video encoding. It has no embedded browser/Electron runtime and does not require an external FFmpeg installation. Hardware transforms are enabled in `src/win/encoder.rs`; hardware encoders are preferred when available, with software fallback. Availability and actual acceleration depend on Windows, codecs, drivers and hardware; this is not a guarantee of GPU encoding on every PC.

Settings offer 15/24/30/60 FPS and bitrate choices. FPS is a target, not a sustained-performance guarantee. Lower frame rates request fewer frames; smaller capture areas reduce pixel workload. Higher bitrate does not directly imply a fixed CPU/RAM change. PNG screenshots remain lossless.

Website file-size estimator: video bitrate in Mbps × seconds ÷ 8 gives approximate decimal MB. Audio/container overhead and encoder rate-control behavior are excluded. It is not a measured recording output size.

## Not measured yet

There are no published RAM, CPU, startup-latency, energy-use, or sustained-FPS benchmarks. Minimum RAM/GPU requirements have not been established. Cross-platform CI proves compilation and tests, not speed or successful recording on every device. Do not claim a percentage advantage over another recorder or fixed CPU/RAM usage without reproducible measurements.

For future benchmarks record CPU/GPU model, RAM, OS build, resolution/capture area, FPS, bitrate, audio mode, encoder selection, idle/recording duration and output size. Report repeated measurements and compare identical workloads/settings.

Linux/macOS are experimental and require external FFmpeg. These Windows package measurements do not describe those platforms.
