#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    REG_VALUE_TYPE,
};

#[cfg(windows)]
const ACCENT_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent");

/// Read the user's Windows accent color as an opaque RGB tuple.
#[cfg(windows)]
pub fn system_accent_rgb() -> Option<(u8, u8, u8)> {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, ACCENT_KEY, None, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }

        let mut value_type = REG_VALUE_TYPE::default();
        let mut bytes = [0_u8; 4];
        let mut byte_len = bytes.len() as u32;
        let result = RegQueryValueExW(
            key,
            w!("AccentColorMenu"),
            None,
            Some(&mut value_type),
            Some(bytes.as_mut_ptr()),
            Some(&mut byte_len),
        );
        let _ = RegCloseKey(key);
        if result != ERROR_SUCCESS || value_type != REG_DWORD || byte_len < 4 {
            return None;
        }

        let value = u32::from_le_bytes(bytes);
        // Windows stores the registry accent as ABGR; the high byte is alpha.
        Some((
            (value & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            ((value >> 16) & 0xff) as u8,
        ))
    }
}

#[cfg(not(windows))]
pub fn system_accent_rgb() -> Option<(u8, u8, u8)> {
    None
}
