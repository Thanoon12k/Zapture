# Windows settings

Open **Settings...** from the floating bar menu or the tray menu. Stop recording and wait for saving before applying changes. All settings persist across restarts; shortcuts apply immediately.

| Setting | Behavior |
|---|---|
| Save captures to | Browse for a folder or enter an absolute path. Zapture verifies that it can write there. Existing captures are not moved. New files go into Recordings and Screenshots subfolders. |
| Video quality | Automatic, Small (2.5 Mbps), Balanced (6 Mbps), High (12 Mbps), Very high (20 Mbps), or preserve a previously configured custom bitrate. Higher bitrates usually improve detail and increase file size. |
| Frames per second | 15, 24, 30 or 60, or keep a custom value. More FPS can increase CPU and storage use. |
| Floating control size | 75%, 100%, 125% or 150%, applied to the bar and recording dot. |
| Floating red dot | On by default. During recording the bar becomes a draggable red dot and the capture frame is hidden. Click either mouse button to open recording controls. Paused recording shows amber. The full bar returns after stopping. |
| Open folder after screenshot | On by default. After a PNG is saved, Explorer opens its folder with that file selected. Copy-only snips do not open a folder. |
| Open folder after recording | On by default. Explorer reveals the MP4 only after the encoder successfully finishes saving. |
| Shortcuts | Four editable combinations: record/pause/resume, stop, screenshot save+copy, and region screenshot. Examples: F9, Ctrl+Shift+R, Alt+PrintScreen. Duplicate combinations are rejected; OS conflicts restore the previous registrations. |

**Save** validates the path and shortcuts, then saves and applies the complete configuration. **Cancel** discards edits. Existing frame resizing continues to determine the captured area; control size changes only the UI, not video resolution. PNG screenshots remain lossless regardless of video quality.

Opening folders is suppressed for MCP sessions so an agent's captures do not launch Explorer automatically. The user-selected folder is still used by MCP captures.

Settings file: `%LOCALAPPDATA%\Zapture\settings.toml`. Existing settings files keep their previous FPS, bitrate, shortcuts and frame; the new dot and completion-folder options default to on. These controls are Windows-only in alpha 2; Linux/macOS remain experimental adapters.
