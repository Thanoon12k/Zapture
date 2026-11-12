//! "Start with Windows" via the per-user Run key. This needs no elevation and
//! affects only the current user. The stored command is the quoted path of the
//! running executable, so `is_enabled` reports true only when the registered
//! entry still points at this exact build — moving the exe reverts the toggle
//! rather than leaving a stale, broken entry checked.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ,
    REG_VALUE_TYPE,
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: PCWSTR = w!("Zapture");

fn exe_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("\"{exe}\"")
}

unsafe fn open(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut hkey = HKEY::default();
    let rc = RegCreateKeyExW(
        HKEY_CURRENT_USER,
        RUN_KEY,
        0,
        PCWSTR::null(),
        REG_OPTION_NON_VOLATILE,
        access,
        None,
        &mut hkey,
        None,
    );
    (rc == ERROR_SUCCESS).then_some(hkey)
}

fn read_value() -> Option<String> {
    unsafe {
        let hkey = open(KEY_READ)?;
        let mut buf = vec![0u8; 2048];
        let mut size = buf.len() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let rc = RegQueryValueExW(
            hkey,
            VALUE,
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if rc != ERROR_SUCCESS || size < 2 {
            return None;
        }
        // REG_SZ data is UTF-16 including a trailing NUL.
        let n = (size as usize / 2).saturating_sub(1);
        let units = std::slice::from_raw_parts(buf.as_ptr() as *const u16, n);
        Some(String::from_utf16_lossy(units))
    }
}

/// True when the Run entry exists and points at the current executable.
pub fn is_enabled() -> bool {
    match read_value() {
        Some(stored) => stored.eq_ignore_ascii_case(&exe_command()),
        None => false,
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    unsafe {
        let Some(hkey) = open(KEY_READ | KEY_WRITE) else {
            return Err("cannot open the Run registry key".into());
        };
        let rc = if enabled {
            let cmd = exe_command();
            let mut data: Vec<u16> = cmd.encode_utf16().collect();
            data.push(0);
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            RegSetValueExW(hkey, VALUE, 0, REG_SZ, Some(bytes))
        } else {
            let rc = RegDeleteValueW(hkey, VALUE);
            // Removing an absent value is success from the caller's view.
            if rc == ERROR_FILE_NOT_FOUND {
                ERROR_SUCCESS
            } else {
                rc
            }
        };
        let _ = RegCloseKey(hkey);
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("registry write failed (code {})", rc.0))
        }
    }
}

/// Flip the setting; returns the new state on success.
pub fn toggle() -> Result<bool, String> {
    let next = !is_enabled();
    set(next).map(|()| next)
}
