#[cfg(windows)]
pub fn open_path(path: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target = path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let verb = ['o' as u16, 'p' as u16, 'e' as u16, 'n' as u16, 0];
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
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
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

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
    unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            arguments_ptr,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
        .0 as isize
            > 32
    }
}

#[cfg(not(windows))]
pub fn open_path(_path: &str) -> bool {
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
