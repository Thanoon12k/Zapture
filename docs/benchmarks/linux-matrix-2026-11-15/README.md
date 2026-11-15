# Linux comparison matrix — 15 November 2026

54 distinct scenarios; 162 total runs; 162 passed; 0 failed. Three repetitions per scenario. 54 comparison plots plus one Windows package-size chart.

## Scope

Screenshots: Zapture 0.1.0-alpha.2, FFmpeg, ImageMagick and scrot. Recording: Zapture and a matched FFmpeg command-line baseline. Three resolutions (720p, 1080p, 1440p) × three synthetic workloads (document, animation, busy). See versions.json for exact comparator builds. OBS was excluded because its earlier capture was black; this report makes no OBS runtime-performance claim.

## Measurement protocol

- A fixed random seed shuffles app/trial order inside each resolution/workload group. Resolution and workload groups are ordered, not randomized.
- A synthetic Tk canvas covers the virtual display for all tools. It contains document lines, 8 moving rectangles, or 160 moving rectangles. It raises above app controls. Visual distributions are comparable; frames are not synchronized pixel-for-pixel.
- The Zapture UI remains running throughout each resolution group, including baseline runs, to keep the scene environment consistent. Baseline resource accounting excludes that idle UI; Zapture measurements include its UI plus FFmpeg children. Workload generator and Xvfb are excluded for both.
- Recording: 30 fps, x264 ultrafast CRF23, yuv420p, MP4, audio off. Two-second warmup then eight-second measurement. CPU 100% means one logical core. RSS is summed across the process tree and can double-count shared pages; PSS is retained in raw data.
- Screenshot latency: request/launch through saved output response. Zapture uses a running MCP-enabled UI, inline image transfer disabled. Other tools are launched per screenshot. PNG compression and capture overhead differ; this is end-to-end default behavior, not an isolated encoder comparison.
- Screenshot memory is the maximum RSS sampled every 10 ms; short-lived peaks or children can be missed. Zapture includes its resident UI; command-line tools have no resident GUI. No screenshot CPU comparison is inferred.
- Output validation: each PNG is decoded, checked for dimensions and nontrivial variation. Each video is frame-counted and one frame at two seconds is decoded and checked for variation. This does not establish perfect motion fidelity, unique-frame rate or absence of all frame drops.
- Encoded FPS = decoded frame count / file duration. This can include duplicated frames and must not be interpreted as measured capture latency or motion quality.
- The cloud has no physical GPU. This is not a Windows, hardware-acceleration, audio, battery, thermals, gaming or multi-hour benchmark. Three short repeats do not establish population-level statistical significance.

## Size comparison

All selected size comparisons use Windows portable ZIP release assets, not installers. Sizes are GitHub metadata, checked on the report date, in decimal MB. Feature sets and bundled/system dependencies differ. Zapture is a smaller capture-focused app; this is not feature parity and does not establish RAM/CPU savings. The Linux Zapture build uses external FFmpeg, unlike its native Windows build.

| Package | Version | ZIP size | Zapture ZIP smaller by |
|---|---|---:|---:|
| [Zapture](https://github.com/Thanoon12k/Zapture/releases/tag/v0.1.0-alpha.2) | v0.1.0-alpha.2 | 0.76 MB | 0.00% |
| [ShareX](https://github.com/ShareX/ShareX/releases/tag/v22.4.0) | v22.4.0 | 209.74 MB | 99.64% |
| [obs-studio](https://github.com/obsproject/obs-studio/releases/tag/33.0.1) | 33.0.1 | 223.79 MB | 99.66% |
| [greenshot](https://github.com/greenshot/greenshot/releases/tag/v1.3.328) | v1.3.328 | 4.03 MB | 81.19% |

## Reproduce

Prepare the [existing Linux setup](../../../scripts/benchmarks/README.md), additionally extract scrot 1.12.1-1 and libimlib2t64 1.12.4-2 (Debian trixie); provide ImageMagick 7.1.1-43 and Pillow. Then:

```sh
python3 scripts/benchmarks/linux_matrix.py --root "$BENCH" --seconds 8
python3 scripts/benchmarks/report_matrix.py --root "$BENCH" --repo .
```

The report generator also expects download-sizes.json in the benchmark root with the saved GitHub releases response projection (repository, version, URL and asset name/size/URL). Published selected asset metadata and the original release-assets.json projection are in this report folder; copy release-assets.json to $BENCH/download-sizes.json to regenerate the report.

[Raw data](results.json) · [Summary](summary.json) · [Versions](versions.json) · [Size sources](download-sizes.json)

The website exposes all plots and raw samples. No production app code was modified.
