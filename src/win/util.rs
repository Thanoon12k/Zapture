//! Small Win32 helpers shared by the rest of the backend.

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

use windows::core::PCWSTR;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_TOPMOST,
};

use crate::config::APP_NAME;

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn qpf() -> i64 {
    static CACHE: AtomicI64 = AtomicI64::new(0);
    let cached = CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    let mut f = 0i64;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut f);
    }
    let f = f.max(1);
    CACHE.store(f, Ordering::Relaxed);
    f
}

pub fn qpc() -> i64 {
    let mut c = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
    }
    c
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn captures_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
        .unwrap_or_else(exe_dir).join("Videos").join("Zapture")
}
pub fn videos_dir() -> PathBuf { captures_dir().join("Recordings") }
pub fn screenshots_dir() -> PathBuf { captures_dir().join("Screenshots") }
pub fn config_path() -> PathBuf {
    let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        .unwrap_or_else(exe_dir).join("Zapture");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.toml")
}

/// Local time as `2026-07-07_21-30-05`, suitable for file names.
pub fn timestamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}-{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

fn message_box(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let text = wide(text);
    let title = wide(APP_NAME);
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags);
    }
}

pub fn alert(text: &str) {
    if crate::mcp::enabled() { crate::mcp::report_error(text); return; }
    message_box(text, MB_OK | MB_ICONWARNING | MB_TOPMOST);
}

pub fn info(text: &str) {
    if crate::mcp::enabled() { eprintln!("Zapture: {text}"); return; }
    message_box(text, MB_OK | MB_ICONINFORMATION | MB_TOPMOST);
}

pub fn loword_i(l: isize) -> i32 {
    (l & 0xFFFF) as u16 as i16 as i32
}

pub fn hiword_i(l: isize) -> i32 {
    ((l >> 16) & 0xFFFF) as u16 as i16 as i32
}
