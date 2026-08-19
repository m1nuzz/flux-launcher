const STARTUP_VALUE_NAME: &str = "Flux Launcher";

#[cfg(windows)]
const STARTUP_REGISTRY_KEY: windows::core::PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

#[cfg(windows)]
fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Synchronize the per-user Windows startup entry with the launcher setting.
///
/// The entry is stored under `HKCU`, so enabling startup does not require
/// administrator privileges and applies only to the current Windows user.
#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    unsafe {
        let mut key = HKEY::default();
        let result = if enabled {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                STARTUP_REGISTRY_KEY,
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        } else {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                STARTUP_REGISTRY_KEY,
                None,
                KEY_SET_VALUE,
                &mut key,
            )
        };

        if result != ERROR_SUCCESS {
            if !enabled && result == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            return Err(format!(
                "Could not open Windows startup registry key: {result:?}"
            ));
        }

        let result = if enabled {
            let executable = std::env::current_exe()
                .map_err(|error| format!("Could not resolve launcher path: {error}"))?;
            let command = format!("\"{}\" --startup", executable.display());
            let value = wide_string(&command);
            RegSetValueExW(
                key,
                windows::core::PCWSTR::from_raw(wide_string(STARTUP_VALUE_NAME).as_ptr()),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    value.as_ptr().cast::<u8>(),
                    value.len() * std::mem::size_of::<u16>(),
                )),
            )
        } else {
            RegDeleteValueW(
                key,
                windows::core::PCWSTR::from_raw(wide_string(STARTUP_VALUE_NAME).as_ptr()),
            )
        };

        let _ = RegCloseKey(key);
        if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
            Ok(())
        } else {
            Err(format!(
                "Could not update Windows startup entry: {result:?}"
            ))
        }
    }
}

/// Keep non-Windows builds and cross-target tests independent of Windows startup APIs.
#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_toggle_is_available_on_all_targets() {
        assert!(super::set_enabled(false).is_ok());
    }
}
