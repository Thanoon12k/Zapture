# Roadmap and acceptance gates

## Preview implemented
- [x] Windows floating bar and tray commands; recording, pause, stop.
- [x] Windows computer/microphone/mixed audio via native APIs.
- [x] PNG save + clipboard and separate Recordings/Screenshots folders.
- [x] English default; optional startup; local-only operation.
- [x] Experimental X11 and macOS capture adapters and simple floating UI.
- [x] Desktop CI build definitions.
- [x] Local stdio MCP tools for screenshot, start, stop and status.
- [ ] Validate end-to-end capture using real MCP clients on each supported desktop.

## Required before claiming desktop completion
- [ ] Test Windows 10/11 real screen, mic, loopback, both, pause/resume and display scaling.
- [ ] Verify silent sections and 30-minute A/V synchronization, device disconnect, denied permission and full disk handling.
- [ ] Implement native macOS ScreenCaptureKit; remove manual indices/loopback-device requirement.
- [ ] Implement Wayland portal/PipeWire capture and explicit user selection.
- [ ] Persist Unix settings, add native global shortcuts, ensure screenshot clipboard ownership across desktops.
- [ ] Hide/exclude capture controls where each platform permits it.
- [ ] Build and test macOS Intel, macOS ARM64 and Linux packaging on supported OS releases.
- [ ] Benchmark 720p/1080p on ordinary hardware; document measured CPU/RAM, file sizes and limitations.
- [ ] Complete dependency license inventory for every shipped binary and chosen packaging strategy.

## Mobile later
- [ ] Extract shared core types and backend interface.
- [ ] Android MediaProjection prototype + permission/foreground-service UI.
- [ ] iOS ReplayKit prototype + broadcast extension.
- [ ] Device tests, interruption/recovery and store submission requirements.

No dates or mobile support are promised by this preview.
