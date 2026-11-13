# Quick start

Download the package for your OS and CPU from the GitHub Releases page. Extract it first.

## Windows

Run `Zapture.exe`. Move/resize the frame or choose Capture full monitor from the tray/bar menu. Choose the sound source, then Record. F9 starts/pauses/resumes; F10 stops and saves. F8 saves a PNG and copies it. Alt+A selects a screenshot area; Enter copies, Ctrl+S saves and copies, Escape cancels. Closing the bar hides it; Exit in the tray menu quits after saving.

Recordings and screenshots: `%USERPROFILE%\Videos\Zapture\Recordings` and `Screenshots`.
Settings: `%LOCALAPPDATA%\Zapture\settings.toml`.

## Settings on Windows

Open **Settings...** from the tray or bar menu while idle. Set a destination, quality/FPS, shortcuts, size and whether the control shrinks to a red dot during recording. A click on the dot opens controls; drag it to reposition. The two **Open folder...** switches reveal saved screenshots/recordings in Explorer, with the new file selected. Details: [Settings](settings.md).

## Linux X11

Install FFmpeg with x11grab, pulse, libx264 and AAC, and `pactl` for automatic computer-audio source selection. Run the extracted `zapture` executable in an X11 desktop session. Use Record, Stop, Screenshot and the Sound selector. Wayland capture is not implemented; the app reports this explicitly.

Output: `~/Videos/Zapture/Recordings` and `Screenshots`.

## macOS

The alpha includes an ad-hoc-signed (not Apple-notarized) `Zapture.app`. FFmpeg must be available on the launched app's PATH. Launch from an environment where FFmpeg is accessible. Allow screen/microphone access in macOS privacy settings. In Settings, enter the screen and audio indices listed by `ffmpeg -f avfoundation -list_devices true -i empty`.

System audio currently needs a separately configured loopback audio device. This is an experimental fallback; a native ScreenCaptureKit adapter is planned. Output: `~/Movies/Zapture/Recordings` and `Screenshots`.

## AI agent use

Run the executable with `--mcp` from a compatible local MCP client. Keep the application in a graphical user session. See [MCP setup](mcp.md). MCP is opt-in and does not open a network port.
