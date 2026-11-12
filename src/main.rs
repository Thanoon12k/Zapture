#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod gif;
mod i18n;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod desktop;
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> eframe::Result {
    desktop::run()
}
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("Zapture: this platform does not yet have a capture backend.");
}
