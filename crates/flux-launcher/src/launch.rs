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

#[cfg(not(windows))]
pub fn open_path(_path: &str) -> bool {
    false
}
