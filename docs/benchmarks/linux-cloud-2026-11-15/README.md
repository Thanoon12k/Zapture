# Linux cloud measurements — 15 November 2026

**Zapture completed three validated recordings. OBS produced black video in all three trials, so this run cannot establish a recording-performance winner.** Do not use the failed OBS CPU/memory readings for marketing comparisons.

## Zapture results

Published Zapture 0.1.0-alpha.2 Linux x64 release, 1280×720 at 30 fps, x264 ultrafast CRF 23, no audio. Three trials; each recording measured for 15 seconds after a 2-second warmup. Values below are medians across trial summaries; the range is across the three trials, not confidence intervals.

| Metric | Median | Trial range |
|---|---:|---:|
| Idle RAM (RSS) | 73.72 MiB | 73.57–80.19 |
| Recording RAM (RSS) | 195.61 MiB | 195.44–201.90 |
| Idle RAM (PSS) | 63.44 MiB | 63.29–69.87 |
| Recording RAM (PSS) | 182.03 MiB | 181.86–188.31 |
| Idle CPU | 1.26 % of one core | 1.26–1.36 |
| Recording CPU | 20.93 % of one core | 20.80–21.80 |

All three outputs decoded successfully, contained changing visual content, and reported 30 fps. Recordings lasted approximately 17 seconds each. Median API stop-to-completion latency: 0.193 seconds (range 0.188–0.726). This does not measure durable disk flushing. Median output size: 154.1 KiB. The synthetic scene compresses unusually well; file size is not a prediction for real screen content.

A decoded frame was visually inspected: the colored scene, moving rectangle, and Zapture controls were visible. Automated validation also counted frames and decoded frames at 2, 7 and 12 seconds.

## OBS comparison attempt

OBS Studio 30.2.3 (Debian package 30.2.3+dfsg-3), with preview disabled and the same target resolution, fps, CRF and x264 preset. OBS used a silent AAC track and MKV; Zapture used no audio and MP4. Source pixels were not guaranteed identical because application controls can appear.

OBS launched and produced decodable files, but all three failed the changing-content validation. A decoded output was visually confirmed completely black. The exact capture failure was not isolated; this container required TCP X11 instead of a normal Unix X11 socket. No conclusions about OBS on ordinary Linux desktops follow from this failure.

OBS's observed idle RSS median was 176.82 MiB and idle CPU was 43.94% of one core. These describe this unsuccessful capture configuration only and are not a fair product ranking. Its recording resource samples remain in the raw data solely for diagnosis and are excluded from valid recording comparisons.

## Environment and limits

Debian 13 x86_64 cloud container; 9 visible logical CPUs; approximately 10.45 GiB visible memory; Xvfb virtual X11; Mesa llvmpipe software rendering; no GPU device. Visible CPUs/memory do not establish reserved cloud capacity. CPU 100% represents one logical core. RSS sums all recorder processes and may double-count shared pages; PSS apportions shared pages. Zapture's external FFmpeg process is included. The input generator and X server are excluded for both apps.

Packages were extracted privately without root. Debian OBS's hard-coded mux-helper path was relocated to a temporary wrapper, which restores its library search path. No encoder implementation was changed. Initial setup failures were resolved before these six trials; the final black-frame failures remain recorded.

This is a short exploratory test on one cloud environment, not a statistical claim about general performance. It does not validate Windows/macOS, hardware acceleration, Wayland, audio capture, screenshots, long sessions, battery use or quality equivalence. ShareX and other Windows tools were not tested. A valid OBS comparison still requires a normal Linux display environment (for example, an isolated Linux VM or GitHub Actions runner with working X11 capture).

## Reproduction and evidence

- [Procedure and setup](../../../scripts/benchmarks/README.md)
- [Benchmark harness](../../../scripts/benchmarks/linux_cloud.py)
- [Raw samples, including failed trials](raw-results.json)
- [Environment](environment.json)
- [Package filenames and SHA-256 checksums](packages.json)

Only benchmark scripts and documentation were added. No production recording code or public website performance claims were changed.
