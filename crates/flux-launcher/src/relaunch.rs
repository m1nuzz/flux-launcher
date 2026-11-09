use std::io;

/// Start a replacement only after this process releases its single-instance claim.
pub(crate) fn spawn() -> io::Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .arg("--relaunch")
        .arg(std::process::id().to_string())
        .env("FLUX_OPEN_SETTINGS", "1")
        .spawn()?;
    Ok(())
}

pub(crate) fn wait_for_parent() -> bool {
    let mut args = std::env::args();
    if args.nth(1).as_deref() != Some("--relaunch") {
        return true;
    }
    let Some(pid) = args.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
        };
        // SAFETY: OpenProcess supplies an owned handle; it is closed after the bounded wait.
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                let result = WaitForSingleObject(handle, 15_000);
                let _ = CloseHandle(handle);
                return result == WAIT_OBJECT_0;
            }
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
    true
}
