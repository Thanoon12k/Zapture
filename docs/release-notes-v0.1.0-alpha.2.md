# Zapture v0.1.0-alpha.2 — Windows settings

Adds a small native Settings window to Windows, keeping the lightweight, local-only design.

- Choose a capture folder with Browse; screenshots and recordings stay in separate subfolders.
- Edit the four global shortcuts. Duplicate/conflicting shortcuts are reported without silently discarding the previous bindings.
- Choose video quality/bitrate and 15/24/30/60 FPS.
- Choose small, normal, large or extra-large floating controls.
- Automatically switch to a draggable red recording dot. Click for controls; paused recording turns amber, and stopping restores the bar.
- Independently open the folder after a saved screenshot or successfully finalized recording, selecting the new file. Both switches default to on and can be disabled. MCP does not auto-open folders.
- Persist settings atomically and preserve old settings files.

The first Windows preview was reported working by a user. This update has automated tests and native/cross-build checks; real-device testing of the new Settings window, shortcut conflicts, dot dragging and Explorer behavior is still needed.

Linux/macOS packages remain experimental FFmpeg-based adapters. These new Settings controls are Windows-only. Wayland capture and mobile are not implemented. Source is MIT-licensed; dependency notices and docs are included in the archives.
