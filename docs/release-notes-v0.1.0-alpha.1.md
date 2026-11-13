# Zapture v0.1.0-alpha.1

First public **prerelease**, intended for testing. Simple local recording and screenshots, plus opt-in local MCP tools for AI agents. English UI. MIT-licensed source with upstream attribution. No accounts, advertisements, telemetry or cloud service.

## Downloads

- Windows x64: portable ZIP, native Rust/Win32 capture. Run Zapture.exe.
- Linux x64: tar.gz, experimental X11 adapter. Requires separately installed FFmpeg and PulseAudio/PipeWire-Pulse support.
- macOS ARM64: tar.gz containing Zapture.app, experimental AVFoundation/FFmpeg adapter. Requires FFmpeg on PATH, permissions, device selection and a loopback device for computer audio. Ad-hoc signed, not notarized.
- SHA256SUMS.txt: checksums for the archives.

Each archive includes documentation, license notices and a dependency license inventory. FFmpeg is not bundled.

## Features

Windows: floating bar, tray controls, start/pause/resume/stop, screenshot save + copy, region snips, computer/microphone/mixed sound, separate output folders and optional Windows startup.

MCP: zapture_screenshot, zapture_start_recording, zapture_stop_recording and zapture_status. Launch with --mcp. Audio defaults off. No mouse/keyboard automation or arbitrary shell/file-access tools.

## Known limitations

This is not a finished all-platform release. Wayland capture and mobile are not implemented. Linux/macOS adapters are experimental and need external dependencies; their features differ from Windows. OS permissions, real-device audio synchronization, resource usage, interruptions and full-disk behavior still require device testing. Tests and successful builds do not substitute for those checks.

See docs/README.md, docs/mcp.md and docs/platform-support.md in the archive or repository.
