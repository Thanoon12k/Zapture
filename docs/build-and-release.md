# Build and release

Install stable Rust and the platform compiler/SDK. Run `cargo test --locked` and `cargo build --release --locked`.

Windows: Visual Studio C++ tools and Windows SDK. Linux: X11/Wayland/OpenGL development libraries (the CI lists Debian/Ubuntu package names). macOS: Xcode Command Line Tools.

Run `python scripts/package.py` after a successful native build to produce a versioned archive in dist/. The script reads Cargo.toml, collects dependency license files from Cargo's filtered metadata, includes the docs/examples, and packages the appropriate executable. macOS is ad-hoc signed before archiving. FFmpeg is not bundled.

## Publishing

The Desktop builds workflow tests and builds the three OS targets. Normal pushes produce test artifacts only. A main-branch commit containing `[release]` in its message, or workflow_dispatch with `publish_release: true`, enables the release job after **all** matrix jobs succeed.

The release job downloads the built packages, generates SHA256SUMS.txt, and creates a GitHub prerelease tagged `v` plus the Cargo.toml version. It refuses to overwrite an existing release. Update the package version, Cargo.lock and versioned release notes before preparing another release. The GitHub token only has write permission in the publish job.

Do not label an alpha as stable until the roadmap's runtime and platform acceptance gates are complete. App-store publication, notarization and developer certificate signing are separate tasks.
