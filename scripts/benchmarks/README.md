# Linux cloud benchmark

This is a short, exploratory software-rendering experiment, not a universal product ranking. See `docs/benchmarks/linux-cloud-2026-11-15/README.md` for results and limitations.

## Preparation

Use an isolated Debian 13 x86_64 environment. The benchmark opens an unauthenticated Xvfb TCP display and OBS WebSocket port **inside a private network namespace**. Do not expose either port to a network or run this on a desktop containing private screen content. Run one instance at a time; display 98, port 4457 and `/tmp/zapture-obs-mux` are reserved for the experiment.

Requirements already provided by our base image: Python 3 with Tkinter, psutil, FFmpeg/ffprobe 7.1.5, Qt 6 runtime, Mesa software rendering, and Debian shared-library dependencies. Install websocket-client 1.9.2 into a venv or a private Python directory. A different base image may require additional dependencies; inspect `ldd` for missing libraries.

Choose an empty absolute benchmark directory, called `$BENCH` below. Download the Linux x64 archive from the official Zapture `v0.1.0-alpha.2` GitHub release and extract under `$BENCH/apps/`. The harness discovers the `zapture` executable below that directory.

Download the Debian packages listed with exact versions and hashes in `packages.json` alongside the report, using the Debian trixie and trixie-security repositories. Extract each with `dpkg-deb -x package.deb "$BENCH/root"`. Required packages: obs-studio, libobs0t64, xvfb, libxfont2, libsrt1.5-openssl, libpci3, libqrcodegencpp1, libluajit-5.1-2 and libspeexdsp1. This experiment used private package extraction because root installation was unavailable.

Debian's OBS plugin embeds an absolute mux-helper path. Relocate only that string:

```sh
python3 scripts/benchmarks/relocate_obs.py "$BENCH"
python3 scripts/benchmarks/linux_cloud.py --root "$BENCH" --runs 3 --seconds 15
```

The harness writes a temporary helper wrapper that restores the private library path (OBS strips `LD_LIBRARY_PATH` when spawning its muxer). This changes dependency location, not encoder implementation. It writes OBS configuration under `$BENCH/config`, synthetic input and results under `$BENCH`, and Zapture recordings under the app's default `~/Videos/Zapture/Recordings` directory. It overwrites prior benchmark configuration/results. Preserve existing results before another run.

## Measurement

Three trials per app, alternating app order. Five-second settling period, ten-second idle window, then two seconds of recording warmup and a fifteen-second recording measurement window. Sample every 0.5 seconds. Resource measurements cover the recorder process and descendants, including Zapture's FFmpeg process; they exclude Xvfb and the animated input generator. CPU 100% means one logical core. Summed RSS double-counts shared pages; PSS is also retained. Reported summary values are medians of per-trial means (CPU) or medians (memory).

The 1280×720 X11 workload is an animated colored rectangle and stripes. Software H.264 uses x264 ultrafast CRF 23, 30 fps. OBS preview is disabled, baseline profile, MKV plus a silent AAC track. Zapture uses its released defaults, MP4 without audio. App controls may appear, so source pixels and encoded files are not identical. This is not an equal-quality/compression-efficiency experiment.

Every output must pass ffprobe frame counting and decode at 2, 7 and 12 seconds with changing frame content. Failed trials must remain in raw data and must not be included as successful recording measurements. API readiness is diagnostic only: OBS and MCP have different readiness definitions. Finalization is measured at each app's API completion, not filesystem durability.

These short trials do not test long recordings, microphone/system audio, screenshots, GPU acceleration, Wayland, macOS, Windows, battery consumption or thermal behavior.
