# Platform support — alpha 1

| Platform | Status | Main gaps |
|---|---|---|
| Windows 10 2004+ / 11 x64 | Native implementation and build | Real-device capture/audio and performance validation pending |
| Linux X11 x64 | Experimental FFmpeg adapter and build | External dependencies, no pause/global hotkeys, session-only settings, controls can appear in capture |
| macOS Apple Silicon | Experimental FFmpeg adapter and build | Manual device indices, loopback device for system audio, no notarization, no pause/global hotkeys, runtime permission/audio validation pending |
| Linux Wayland | UI only; capture explicitly rejected | Portal/PipeWire backend not implemented |
| macOS Intel | Source may build; no alpha 1 release artifact | Dedicated build/runtime testing pending |
| Windows ARM / Linux ARM | No release artifact | Target validation pending |
| Android / iOS | Planned only | Native capture adapters, mobile UI/lifecycle and permission flows not implemented |

No promise of support for every OS, version or Linux compositor is made by this alpha. See the roadmap for the native cross-platform direction. Memory and CPU claims require benchmarks; Windows executable size is not total working memory or the Unix FFmpeg footprint.
