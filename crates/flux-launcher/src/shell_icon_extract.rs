#[cfg(windows)]
pub(crate) fn shortcut_icon_location(target: &str) -> Option<(String, i32)> {
    let extension = std::path::Path::new(target)
        .extension()
        .and_then(|value| value.to_str())?;
    if extension.eq_ignore_ascii_case("lnk") {
        return shell_link_icon_location(target);
    }
    if extension.eq_ignore_ascii_case("url") {
        let contents = std::fs::read_to_string(target).ok()?;
        let (path, index) = parse_internet_shortcut_icon_location(&contents)?;
        return resolve_shortcut_icon_path(target, &path).map(|path| (path, index));
    }
    None
}

fn parse_internet_shortcut_icon_location(contents: &str) -> Option<(String, i32)> {
    let mut icon_file = None;
    let mut icon_index = 0_i32;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key.eq_ignore_ascii_case("IconFile") && !value.is_empty() {
            icon_file = Some(value.to_owned());
        } else if key.eq_ignore_ascii_case("IconIndex") {
            icon_index = value.parse::<i32>().unwrap_or(0);
        }
    }
    icon_file.map(|path| (path, icon_index))
}

fn resolve_shortcut_icon_path(shortcut_path: &str, icon_path: &str) -> Option<String> {
    let expanded = expand_percent_variables_for_icon(icon_path)?;
    let expanded = expanded.trim().trim_matches('"');
    if expanded.is_empty() {
        return None;
    }
    let path = std::path::Path::new(expanded);
    if path.is_absolute() {
        return Some(expanded.to_owned());
    }
    let parent = std::path::Path::new(shortcut_path).parent()?;
    Some(parent.join(path).to_string_lossy().into_owned())
}

fn expand_percent_variables_for_icon(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let variable = &rest[start + 1..];
        let end = variable.find('%')?;
        let name = &variable[..end];
        let replacement = std::env::var_os(name)?.to_string_lossy().into_owned();
        output.push_str(&replacement);
        rest = &variable[end + 1..];
    }
    output.push_str(rest);
    Some(output)
}

#[cfg(windows)]
fn shell_link_icon_location(path: &str) -> Option<(String, i32)> {
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::IShellLinkW;

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let wide_path = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ).ok()?;
        let mut icon_path = [0_u16; 32_768];
        let mut icon_index = 0_i32;
        link.GetIconLocation(&mut icon_path, &mut icon_index).ok()?;
        let end = icon_path
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(icon_path.len());
        let icon_path = String::from_utf16_lossy(&icon_path[..end]);
        resolve_shortcut_icon_path(path, &icon_path).map(|path| (path, icon_index))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
pub(crate) fn extract_shell_thumbnail_rgba(target: &str) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_ICONONLY,
        SIIGBF_SCALEUP,
    };

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let shell_item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()? };
    let image_factory: IShellItemImageFactory = shell_item.cast().ok()?;
    let bitmap = unsafe {
        image_factory
            .GetImage(
                SIZE {
                    cx: ICON_SIZE,
                    cy: ICON_SIZE,
                },
                SIIGBF(SIIGBF_ICONONLY.0 | SIIGBF_SCALEUP.0),
            )
            .ok()?
    };
    if bitmap.0.is_null() {
        return None;
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let copied = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            ICON_SIZE as u32,
            Some(bgra.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    if copied == 0 {
        return None;
    }

    let has_alpha = bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend([
            pixel[2],
            pixel[1],
            pixel[0],
            if has_alpha { pixel[3] } else { 255 },
        ]);
    }
    Some(rgba)
}

#[cfg(windows)]
pub(crate) fn extract_shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    extract_icon_rgba_from_source(target, None)
}

pub(crate) fn is_executable_icon_target(target: &str) -> bool {
    matches!(
        std::path::Path::new(target.trim().trim_matches('"'))
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("exe") | Some("com") | Some("bat") | Some("cmd")
    )
}

#[cfg(windows)]
pub(crate) fn extract_icon_rgba_from_source(
    source: &str,
    icon_index: Option<i32>,
) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
    let icon = if let Some(index) = icon_index {
        let mut icon = HICON::default();
        let extracted = unsafe {
            ExtractIconExW(
                PCWSTR(path.as_ptr()),
                index,
                Some(&mut icon as *mut HICON),
                None,
                1,
            )
        };
        (extracted > 0 && !icon.is_invalid()).then_some(icon)?
    } else {
        let mut file_info = SHFILEINFOW::default();
        let flags = SHGFI_FLAGS(SHGFI_ICON.0 | SHGFI_LARGEICON.0);
        let result = unsafe {
            SHGetFileInfoW(
                PCWSTR(path.as_ptr()),
                Default::default(),
                Some(&mut file_info),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        (result != 0 && !file_info.hIcon.is_invalid()).then_some(file_info.hIcon)?
    };

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe { DestroyIcon(icon).ok()? };
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let bitmap =
        unsafe { CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) };
    let Ok(bitmap) = bitmap else {
        unsafe {
            DestroyIcon(icon).ok();
            let _ = DeleteDC(hdc);
        }
        return None;
    };
    let previous = unsafe { SelectObject(hdc, HGDIOBJ(bitmap.0)) };
    let drawn =
        unsafe { DrawIconEx(hdc, 0, 0, icon, ICON_SIZE, ICON_SIZE, 0, None, DI_NORMAL).is_ok() };
    let rgba = if drawn && !bits.is_null() {
        let bgra = unsafe {
            std::slice::from_raw_parts(bits.cast::<u8>(), (ICON_SIZE * ICON_SIZE * 4) as usize)
        };
        let mut rgba = Vec::with_capacity(bgra.len());
        for pixel in bgra.chunks_exact(4) {
            rgba.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        Some(rgba)
    } else {
        None
    };
    unsafe {
        SelectObject(hdc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(hdc);
        DestroyIcon(icon).ok();
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steam_internet_shortcut_icon_file_and_index() {
        let shortcut = "[InternetShortcut]\nURL=steam://rungameid/730\nIconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\730.ico\nIconIndex=0\n";
        assert_eq!(
            parse_internet_shortcut_icon_location(shortcut),
            Some((
                String::from(r"C:\Program Files (x86)\Steam\steam\games\730.ico"),
                0
            ))
        );
    }

    #[test]
    fn parses_shortcut_icon_keys_case_insensitively_and_defaults_index() {
        let shortcut = "[InternetShortcut]\nurl=steam://rungameid/10\niconfile=game.ico\n";
        assert_eq!(
            parse_internet_shortcut_icon_location(shortcut),
            Some((String::from("game.ico"), 0))
        );
    }

    #[test]
    fn resolves_relative_shortcut_icon_file_against_shortcut_directory() {
        let expected = std::path::Path::new("/tmp/Steam")
            .join("icons/game.ico")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_shortcut_icon_path("/tmp/Steam/Game.url", "icons/game.ico"),
            Some(expected)
        );
    }

    #[test]
    fn executable_icon_target_detection_accepts_shell_executables_only() {
        assert!(is_executable_icon_target(
            r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
        ));
        assert!(is_executable_icon_target(
            r"C:\\Program Files\\PowerShell\\7\\pwsh.exe"
        ));
        assert!(!is_executable_icon_target(
            r"C:\\Users\\m1nus\\PowerShell.lnk"
        ));
        assert!(!is_executable_icon_target("ms-settings:network-wifi"));
    }
}
