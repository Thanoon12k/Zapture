# Zapture

Simple, local screen capture. **Record. Stop. Screenshot.** English UI, no account, no subscription, no advertising, no uploads.

**0.1.0-alpha.1 preview — not yet a finished all-platform release.** Windows has a native Rust/Win32 implementation. Linux/X11 and macOS have experimental Rust desktop adapters requiring FFmpeg. Wayland, Android and iOS capture are not implemented. No CPU/RAM performance measurements or real-device Windows/macOS capture tests have been performed for this fork yet.

## Current support

| Platform | Implementation | Requirements / limits |
|---|---|---|
| Windows 10 2004+ / 11 x64 | Native floating bar, tray menu, MP4 recording, pause/resume, PNG save + copy, region snips, computer/mic/both audio | Uses Windows Media Foundation and WASAPI. No external FFmpeg. N editions need the Media Feature Pack. |
| Linux X11 | Experimental floating panel, whole-display MP4, PNG save + copy, PulseAudio/PipeWire-Pulse computer/mic/both audio | FFmpeg with x11grab, pulse, libx264 and AAC; pactl for automatic output-device selection. Panel is visible in recordings. No global hotkeys/pause yet. |
| Linux Wayland | UI can launch; capture explicitly blocked | Portal + PipeWire backend is planned. Xwayland is not treated as whole-desktop capture. |
| macOS | Experimental floating panel, AVFoundation MP4, native interactive screenshot, PNG save + copy | FFmpeg, screen/microphone permissions and device indices. Computer audio currently needs a configured loopback device. No global hotkeys/pause yet. Not notarized. |
| Android / iOS | Architecture roadmap only | MediaProjection / ReplayKit adapters and mobile UI are not implemented. |

## Downloads and docs

- [Releases](https://github.com/Thanoon12k/Zapture/releases): versioned Windows, Linux and macOS packages, published only after all matrix builds and tests succeed.
- [Documentation](docs/README.md): quick start, MCP, build guide, platform support and roadmap.

## Windows quick start

1. Extract the ZIP to a normal folder and run `Zapture.exe`. No installer or administrator rights required.
2. Move/resize the capture frame; or open the bar/tray menu and select **Capture full monitor**.
3. Choose **Audio: none / computer sound / microphone / computer + microphone** before recording.
4. Click the red Record button. **F9** starts/pauses/resumes; **F10** stops and saves.
5. **F8** saves a PNG and copies it. **Alt+A** opens a region screenshot selector; Enter copies and Ctrl+S saves + copies. Escape cancels.
6. Closing the floating bar hides it. Left/right-click the tray icon to open controls. Choose **Exit** to quit (active recording is finalized first).

Output: `%USERPROFILE%\Videos\Zapture\Recordings` and `Screenshots`.
Settings: `%LOCALAPPDATA%\Zapture\settings.toml`. Default: 30 FPS, computer audio, no decorative background. Start with Windows is optional and off by default.

## Linux / macOS previews

Build using `cargo build --release --locked`, then run `target/release/zapture`. FFmpeg must be installed separately; it is **not bundled**. On Linux use an X11 desktop session with PulseAudio or PipeWire's PulseAudio compatibility service.

On macOS list device indices using:

```sh
ffmpeg -f avfoundation -list_devices true -i empty
```

Enter the screen, microphone and optional loopback audio indices in Settings. Grant screen/microphone permission when prompted. The current adapter is a functional prototype, not the desired final zero-configuration experience. Settings in this preview are session-only. Audio failures are reported rather than silently promising system-audio support.

Output: `~/Videos/Zapture` on Linux; `~/Movies/Zapture` on macOS. Errors from recordings are stored beside the MP4 as `.log` files. Click Stop and wait for saving to finish before closing.

## Build and test

Install stable Rust. Windows builds require Visual Studio C++ Build Tools (Desktop development with C++) and Windows SDK. Linux builds require X11/Wayland/OpenGL development libraries. macOS requires Xcode Command Line Tools.

```sh
cargo test --locked
cargo build --release --locked
```

GitHub Actions builds Windows x64, Linux x64 and macOS ARM64 preview artifacts. These are compile/test checks, not proof of successful real-device recording. All-platform native backends remain the project direction; see `docs/architecture.md` and `docs/roadmap.md`.

## AI agents / MCP

Run `Zapture.exe --mcp` (or `zapture --mcp` on Unix) to expose screenshot, start recording, stop recording and status tools to a local compatible AI client. See [MCP guide](docs/mcp.md) for configuration and limitations. Normal double-click operation stays unchanged.

## Free distribution

The Zapture source changes are MIT-licensed and the inherited Cadre MIT notice is preserved in `LICENSE`. You may use, modify, sell or distribute this source under those terms. Include the required copyright and license notices. Dependencies keep their own licenses; see `THIRD_PARTY.md`. Codec patents, bundled third-party binaries, app-store policies and signing requirements depend on the distribution method and jurisdiction; MIT is not a universal codec/distribution clearance.

Based on [Cadre](https://github.com/Im-Midi/cadre), commit `1f895deed2a011e8fb9fc011196069c6c6f2ffee`. Zapture is an independent derivative, not an upstream Cadre release.
