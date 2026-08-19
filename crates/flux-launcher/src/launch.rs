#[cfg(windows)]
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
static TRACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Append a launch lifecycle event when the smoke harness requests tracing.
///
/// The trace is intentionally opt-in and has no effect during normal use.
pub fn trace_launch_event(event: &str) {
    #[cfg(windows)]
    {
        let Ok(path) = std::env::var("FLUX_LAUNCH_TRACE_FILE") else {
            return;
        };
        let Ok(_guard) = TRACE_LOCK.get_or_init(|| Mutex::new(())).lock() else {
            return;
        };
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
            return;
        };
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .unwrap_or_default();
        let _ = writeln!(file, "{timestamp_ms:.3}\t{event}");
    }
}

#[cfg(windows)]
pub fn open_path(path: &str) -> bool {
    shell_execute("open", path, None)
}

/// Schedule a shell launch without blocking the launcher UI thread.
///
/// The dispatch is intentionally queued before the hide operation, matching
/// Flow Launcher: the shell can start resolving the target while windui
/// applies the pending hide after the event callback returns.
pub fn open_path_async(path: &str) {
    let path = path.to_owned();
    trace_launch_event("launch-dispatch");
    let _ = std::thread::Builder::new()
        .name(String::from("flux-shell-launch"))
        .spawn(move || {
            trace_launch_event("shell-worker-start");
            let _ = open_path(&path);
        });
}

/// Schedule opening the Recycle Bin without blocking the launcher UI thread.
pub fn open_recycle_bin_async() {
    trace_launch_event("launch-dispatch");
    let _ = std::thread::Builder::new()
        .name(String::from("flux-shell-launch"))
        .spawn(|| {
            trace_launch_event("shell-worker-start");
            let _ = open_recycle_bin();
        });
}

#[cfg(windows)]
pub fn open_url(url: &str) -> bool {
    shell_execute("open", url, None)
}

#[cfg(windows)]
pub fn open_recycle_bin() -> bool {
    shell_execute("open", "shell:RecycleBinFolder", None)
}

#[cfg(windows)]
pub fn empty_recycle_bin() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOPROGRESSUI, SHERB_NOSOUND};

    // Keep the Windows confirmation prompt enabled. Flux asks for confirmation
    // first, then Windows provides its standard final safety prompt.
    unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOPROGRESSUI | SHERB_NOSOUND).is_ok() }
}

#[cfg(windows)]
pub fn run_as_admin(path: &str) -> bool {
    shell_execute("runas", path, None)
}

#[cfg(windows)]
pub fn open_file_location(path: &str) -> bool {
    let argument = format!("/select,\"{path}\"");
    shell_execute("open", "explorer.exe", Some(&argument))
}

#[cfg(windows)]
fn shell_execute(verb: &str, target: &str, arguments: Option<&str>) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let directory = Path::new(target)
        .parent()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>()
        });
    let target = target
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let verb = verb
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let arguments = arguments.map(|value| {
        value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    });
    let arguments_ptr = arguments
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or_else(PCWSTR::null);
    let directory_ptr = directory
        .as_ref()
        .map(|value| PCWSTR(value.as_ptr()))
        .unwrap_or_else(PCWSTR::null);

    trace_launch_event("shell-execute-start");
    let mut execute_info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(target.as_ptr()),
        lpParameters: arguments_ptr,
        lpDirectory: directory_ptr,
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    let success = unsafe { ShellExecuteExW(&mut execute_info).is_ok() };
    if !success {
        trace_launch_event("shell-execute-failed");
        return false;
    }
    if !execute_info.hProcess.is_invalid() {
        trace_launch_event("process-created");
        unsafe {
            let _ = CloseHandle(execute_info.hProcess);
        }
    } else {
        trace_launch_event("shell-return");
    }
    true
}

#[cfg(not(windows))]
pub fn open_path(_path: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_url(_url: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_recycle_bin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn empty_recycle_bin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn run_as_admin(_path: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn open_file_location(_path: &str) -> bool {
    false
}
