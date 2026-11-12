# Third-party notices

Zapture includes modified Cadre code from https://github.com/Im-Midi/cadre at commit 1f895deed2a011e8fb9fc011196069c6c6f2ffee. Copyright (c) 2026 The Cadre Authors, MIT. Full notice in LICENSE.

Direct Rust dependencies include `serde_json` (MIT OR Apache-2.0) and `base64` (MIT OR Apache-2.0), plus `windows` (MIT OR Apache-2.0), `embed-resource` (MIT), `eframe` (MIT OR Apache-2.0), `arboard` (MIT OR Apache-2.0), `image` (MIT OR Apache-2.0). The lockfile pins the dependency graph. Transitive libraries retain their own notices; distribution packages should include the complete dependency license inventory produced for that build.

Windows uses installed Windows APIs/codecs. Experimental Linux/macOS adapters invoke a separately installed FFmpeg and do not bundle its executable or codecs. FFmpeg licensing depends on its build configuration (LGPL/GPL, and potentially non-redistributable builds). Any future bundling must preserve that build's license, notices and applicable source obligations. Do not assume Zapture's MIT license relicenses FFmpeg or grants codec patent rights.

No third-party binaries are downloaded at application runtime. No telemetry or upload service is included.
