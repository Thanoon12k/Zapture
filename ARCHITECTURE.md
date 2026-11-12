# Architecture

Keep the product small: record, stop, screenshot, audio selection, open output folder. No accounts, analytics, advertising, cloud or server dependency.

## Current code boundaries

- `src/config.rs`: dependency-free settings, audio modes and hotkey values; unit-tested on desktop targets.
- `src/app.rs` and `src/win/`: inherited native Windows event-driven UI, GDI capture, Media Foundation H.264/AAC, WASAPI microphone/loopback, PNG/clipboard. Platform bindings are gated with `cfg(windows)`.
- `src/desktop.rs`: experimental macOS/Linux eframe UI and FFmpeg process adapter. Process arguments use `Command` directly; no interpolated shell. Stop requests graceful encoder finalization. The X11 clipboard owner is retained. Errors are surfaced, with FFmpeg log files.
- `src/mcp.rs`: bounded newline JSON-RPC stdio server with a fixed capture-tool allowlist. Requests run on the native UI thread through a queue, keeping human controls and agent commands in one state machine.
- `src/main.rs`: compile-time platform dispatch. Unsupported platforms do not silently fall back to fake capture.

This is separation of platform code, not yet a common capture-engine interface. The next refactor should extract a small shared crate with SessionState, CaptureOptions, CaptureEvent, StoragePolicy and a CaptureBackend trait. Keep platform handles and permissions outside that crate. Do not force Windows' frame UI onto mobile.

## Native backend targets

| Platform | Video / screenshot | Computer audio | Microphone |
|---|---|---|---|
| Windows | Windows Graphics Capture / DXGI (future upgrade from GDI) | WASAPI loopback | WASAPI |
| macOS | ScreenCaptureKit / system screenshots | ScreenCaptureKit | native audio capture |
| Linux Wayland | xdg-desktop-portal + PipeWire | PipeWire / PulseAudio monitor | PipeWire / PulseAudio |
| Linux X11 | X11 capture | PulseAudio monitor | PulseAudio |
| Android (future) | MediaProjection, user-approved session | AudioPlaybackCapture when allowed by source app | AudioRecord with permission |
| iOS (future) | ReplayKit / broadcast extension | ReplayKit app/system restrictions | ReplayKit microphone |

Mobile needs its own lifecycle, foreground/background behavior, storage integration, native permission flow, and UI. iOS cannot promise a freely floating global button over other applications. Playback audio capture is subject to OS/source-app restrictions. These are not implemented features.

## Resource goals

No continuous idle polling on Windows; repaint recording timer twice per second. No web browser runtime. Bounded reusable capture-frame pool. Use platform hardware encoders where supported. The Linux/macOS adapter is transitional and may use more disk space/RAM than Windows; its FFmpeg footprint is not included in the executable size. Measure CPU, memory, dropped frames and audio drift before making performance claims.
