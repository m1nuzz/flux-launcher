use std::path::{Path, PathBuf};

use flux_core::{ResultKind, ResultSource, SearchResult};

use super::app_identity::canonical_application_id;
use super::applications::{normalize, MAX_CATALOG_ENTRIES, MAX_SCAN_DEPTH};

#[cfg(windows)]
pub(crate) fn resolve_shell_link_target(path: &str) -> Option<(String, String)> {
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, SLGP_RAWPATH};

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x0002_1401_0000_0000_c000_0000_0000_0046);
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
        link.Resolve(HWND::default(), 0x0001).ok()?;
        let mut target = [0_u16; 32_768];
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .ok()?;
        let mut arguments = [0_u16; 32_768];
        link.GetArguments(&mut arguments).ok()?;
        let target_end = target
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(target.len());
        let arguments_end = arguments
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(arguments.len());
        let resolved = String::from_utf16_lossy(&target[..target_end]);
        let mut arguments = String::from_utf16_lossy(&arguments[..arguments_end]);
        if arguments.trim().is_empty() {
            arguments = property_store_arguments(&link).unwrap_or_default();
        }
        (!resolved.trim().is_empty()).then_some((resolved, arguments))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
unsafe fn property_store_arguments(
    link: &windows::Win32::UI::Shell::IShellLinkW,
) -> Option<String> {
    use windows::core::Interface;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Link_Arguments;
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

    let store: IPropertyStore = link.cast().ok()?;
    let mut value: PROPVARIANT = store.GetValue(&PKEY_Link_Arguments).ok()?;
    let arguments = (|| {
        let header = unsafe { &value.Anonymous.Anonymous };
        if header.vt != VT_LPWSTR {
            return None;
        }
        let pointer = unsafe { header.Anonymous.pwszVal };
        if pointer.is_null() {
            return None;
        }
        unsafe { pointer.to_string().ok() }
    })();
    // GetValue hands back an owned variant, so the string it points at leaks
    // unless the variant is cleared here.
    let _ = unsafe { PropVariantClear(&mut value) };
    arguments
}

#[cfg(not(windows))]
pub(crate) fn resolve_shell_link_target(_path: &str) -> Option<(String, String)> {
    None
}

pub(crate) fn extract_executable_target(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix('"') {
        return rest
            .find('"')
            .map(|end| rest[..end].trim().to_owned())
            .filter(|target| !target.is_empty());
    }
    let lower = value.to_ascii_lowercase();
    [".exe", ".com", ".bat", ".cmd"]
        .iter()
        .filter_map(|extension| lower.find(extension).map(|end| end + extension.len()))
        .min()
        .map(|end| value[..end].trim().to_owned())
        .filter(|target| !target.is_empty())
}

pub(crate) fn expand_percent_variables(value: &str) -> Option<String> {
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

pub(crate) fn is_executable_target(target: &str) -> bool {
    let path = Path::new(target);
    path.is_file()
        && matches!(
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
                .as_deref(),
            Some("exe") | Some("com") | Some("bat") | Some("cmd")
        )
}

#[cfg(windows)]
pub(crate) fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(2);
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu"),
        );
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        roots.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu"),
        );
    }
    roots
}

#[cfg(windows)]
pub(crate) fn collect_files(root: &Path, depth: usize, candidates: &mut Vec<SearchResult>) {
    if depth > MAX_SCAN_DEPTH || candidates.len() >= MAX_CATALOG_ENTRIES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if candidates.len() >= MAX_CATALOG_ENTRIES {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_files(&path, depth + 1, candidates);
            continue;
        }
        if !file_type.is_file() || !is_application_file(&path) {
            continue;
        }
        let Some(result) = application_result(path, "Start Menu") else {
            continue;
        };
        candidates.push(result);
    }
}

#[cfg(windows)]
fn is_application_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("lnk") | Some("url") | Some("exe") | Some("com") | Some("bat") | Some("cmd")
    )
}

#[cfg(windows)]
fn application_result(path: PathBuf, source: &str) -> Option<SearchResult> {
    let title = path.file_stem()?.to_string_lossy().trim().to_owned();
    if title.is_empty() {
        return None;
    }
    let target = path.to_string_lossy().into_owned();
    let id = canonical_application_id(&target)
        .unwrap_or_else(|| format!("application:source:{}", normalize(&target)));
    Some(SearchResult {
        id,
        title,
        subtitle: format!("Application • {source}"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(target),
    })
}

#[cfg(windows)]
pub(crate) fn collect_app_paths(candidates: &mut Vec<SearchResult>) {
    use windows::core::{w, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };

    unsafe fn collect_root(root: HKEY, subkey: PCWSTR, candidates: &mut Vec<SearchResult>) {
        let mut app_paths = HKEY::default();
        if RegOpenKeyExW(root, subkey, None, KEY_READ, &mut app_paths) != ERROR_SUCCESS {
            return;
        }

        let mut index = 0_u32;
        loop {
            let mut name = [0_u16; 260];
            let mut name_len = (name.len() - 1) as u32;
            let result = RegEnumKeyExW(
                app_paths,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            );
            if result == ERROR_NO_MORE_ITEMS {
                break;
            }
            if result != ERROR_SUCCESS {
                index = index.saturating_add(1);
                continue;
            }
            let key_name = String::from_utf16_lossy(&name[..name_len as usize]);
            if candidates.len() >= MAX_CATALOG_ENTRIES {
                break;
            }
            if let Some((title, target)) = read_app_path(app_paths, &key_name) {
                candidates.push(SearchResult {
                    id: canonical_application_id(&target)
                        .unwrap_or_else(|| format!("application:source:{}", normalize(&target))),
                    title,
                    subtitle: String::from("Application • App Paths"),
                    kind: ResultKind::Application,
                    source: ResultSource::ApplicationCatalog,
                    target: Some(target),
                });
            }
            index = index.saturating_add(1);
        }
        let _ = RegCloseKey(app_paths);
    }

    unsafe fn read_app_path(app_paths: HKEY, key_name: &str) -> Option<(String, String)> {
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW};
        let key_name_w: Vec<u16> = key_name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            app_paths,
            PCWSTR(key_name_w.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        ) != ERROR_SUCCESS
        {
            return None;
        }

        let mut kind = REG_SZ;
        let mut bytes = vec![0_u8; 32 * 1024];
        let mut byte_len = bytes.len() as u32;
        let result = RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut byte_len),
        );
        let _ = RegCloseKey(key);
        if result != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) || byte_len < 2 {
            return None;
        }
        let words = std::slice::from_raw_parts(bytes.as_ptr() as *const u16, byte_len as usize / 2);
        let value = String::from_utf16_lossy(words)
            .trim_end_matches('\0')
            .trim()
            .to_owned();
        let target = extract_executable_target(&value)?;
        let target = expand_percent_variables(&target)?;
        if !is_executable_target(&target) {
            return None;
        }
        let title = Path::new(key_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(key_name)
            .to_owned();
        Some((title, target))
    }

    unsafe {
        collect_root(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
            candidates,
        );
        collect_root(
            HKEY_LOCAL_MACHINE,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
            candidates,
        );
    }
}

#[cfg(not(windows))]
pub(crate) fn start_menu_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
pub(crate) fn collect_files(_root: &Path, _depth: usize, _candidates: &mut Vec<SearchResult>) {}

#[cfg(not(windows))]
pub(crate) fn collect_app_paths(_candidates: &mut Vec<SearchResult>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_path_parser_extracts_executable_before_arguments() {
        assert_eq!(
            extract_executable_target(r#""C:\Program Files\Calibre\calibre.exe" --detach"#),
            Some(String::from(r#"C:\Program Files\Calibre\calibre.exe"#))
        );
        assert_eq!(
            extract_executable_target(r#"C:\Tools\tool.cmd /arg"#),
            Some(String::from(r#"C:\Tools\tool.cmd"#))
        );
        assert_eq!(extract_executable_target("not an executable"), None);
    }

    #[test]
    fn app_path_filter_requires_existing_supported_executable() {
        let path = std::env::temp_dir().join(format!(
            "flux-app-path-filter-{}-test.exe",
            std::process::id()
        ));
        std::fs::write(&path, b"fixture").unwrap();
        assert!(is_executable_target(path.to_str().unwrap()));
        assert!(!is_executable_target(
            path.with_extension("txt").to_str().unwrap()
        ));
        std::fs::remove_file(path).unwrap();
        assert!(!is_executable_target("C:/missing/calibre-complete.exe"));
    }

    #[test]
    fn app_path_environment_expansion_rejects_unknown_variables() {
        assert_eq!(
            expand_percent_variables(r#"C:\Tools\tool.exe"#),
            Some(String::from(r#"C:\Tools\tool.exe"#))
        );
        assert_eq!(
            expand_percent_variables("%FLUX_VARIABLE_THAT_DOES_NOT_EXIST%\\tool.exe"),
            None
        );
    }
}
