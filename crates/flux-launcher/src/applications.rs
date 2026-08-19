use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use flux_core::{rank_results, ResultKind, ResultSource, SearchResult};
use windui::prelude::Sender;

const MAX_APPLICATION_RESULTS: usize = 16;
const MAX_CATALOG_ENTRIES: usize = 4096;
const MAX_SCAN_DEPTH: usize = 8;

#[derive(Clone, Debug)]
pub struct ApplicationResponse {
    pub sequence: u64,
    pub query: String,
    pub results: Vec<SearchResult>,
    pub status: String,
}

#[derive(Clone, Debug)]
struct ApplicationRequest {
    sequence: u64,
    query: String,
}

#[derive(Clone, Debug, Default)]
struct ApplicationCatalog {
    entries: Vec<SearchResult>,
}

pub struct ApplicationWorker {
    latest: Arc<Mutex<Option<ApplicationRequest>>>,
    wake: mpsc::SyncSender<()>,
}

impl ApplicationWorker {
    pub fn spawn(output: Sender<ApplicationResponse>) -> Self {
        let latest = Arc::new(Mutex::new(None::<ApplicationRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);
        thread::Builder::new()
            .name(String::from("flux-applications"))
            .spawn(move || {
                let catalog = ApplicationCatalog::load();
                while receiver.recv().is_ok() {
                    let Some(request) = latest_for_worker
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                    else {
                        continue;
                    };
                    let results = catalog.search(&request.query);
                    let status = format!("{} application result(s)", results.len());
                    let _ = output.send(ApplicationResponse {
                        sequence: request.sequence,
                        query: request.query,
                        results,
                        status,
                    });
                }
            })
            .expect("failed to create application catalog worker thread");
        Self { latest, wake }
    }

    pub fn request(&self, sequence: u64, query: String) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(ApplicationRequest { sequence, query });
            let _ = self.wake.try_send(());
        }
    }
}

impl ApplicationCatalog {
    fn load() -> Self {
        let mut by_title = HashMap::<String, SearchResult>::new();

        #[cfg(windows)]
        {
            for root in start_menu_roots() {
                collect_files(&root, 0, &mut by_title);
            }
            collect_app_paths(&mut by_title);
        }

        let mut entries = merge_catalog_candidates(by_title.into_values().collect());
        entries.sort_by_key(|result| result.title.to_ascii_lowercase());
        entries.truncate(MAX_CATALOG_ENTRIES);
        Self { entries }
    }

    fn search(&self, query: &str) -> Vec<SearchResult> {
        let normalized = normalize(query);
        if normalized.is_empty() {
            return Vec::new();
        }
        let mut results = self
            .entries
            .iter()
            .filter(|result| application_matches_query(result, &normalized))
            .cloned()
            .collect::<Vec<_>>();
        rank_results(query, &mut results);
        results.truncate(MAX_APPLICATION_RESULTS);
        results
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn application_matches_query(result: &SearchResult, normalized_query: &str) -> bool {
    let title = normalize(&result.title);
    if title.contains(normalized_query) {
        return true;
    }
    let Some(identity) = result.id.strip_prefix("application:target:") else {
        return false;
    };
    let executable = identity
        .split_once('|')
        .map_or(identity, |(target, _)| target)
        .rsplit('\\')
        .next()
        .unwrap_or_default();
    normalize(executable).contains(normalized_query)
}

/// Returns the canonical identity for an application result.
///
/// Flow Launcher groups Win32 entries by the resolved executable target and
/// shortcut arguments rather than by the `.lnk` source path. Flux keeps the
/// source path in `target` for launch behavior, but uses this resolved identity
/// to merge App Paths, Start Menu, Desktop, and Everything application hits.
pub(crate) fn canonical_application_key(result: &SearchResult) -> Option<String> {
    if result.kind != ResultKind::Application {
        return None;
    }
    if let Some(identity) = result.id.strip_prefix("application:target:") {
        if !identity.is_empty() {
            return Some(identity.to_owned());
        }
    }
    canonical_target_key(result.target.as_deref()?)
}

pub(crate) fn canonical_application_id(target: &str) -> Option<String> {
    canonical_target_key(target).map(|identity| format!("application:target:{identity}"))
}

pub(crate) fn canonical_target_key(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    if is_shortcut_path(target) {
        if let Some((resolved, arguments)) = resolve_shell_link_target(target) {
            let key = normalize_windows_path(&resolved);
            let arguments = normalize(&arguments);
            return Some(if arguments.is_empty() {
                if is_chrome_proxy_target(&resolved) {
                    format!("{key}|shortcut:{}", normalize_windows_path(target))
                } else {
                    key
                }
            } else {
                format!("{key}|args:{arguments}")
            });
        }
    }
    Some(normalize_windows_path(target))
}

fn merge_catalog_candidates(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut positions = HashMap::<String, usize>::new();
    let mut merged = Vec::with_capacity(results.len());
    for result in results {
        let Some(identity) = canonical_application_key(&result) else {
            merged.push(result);
            continue;
        };
        let Some(existing_index) = positions.get(&identity).copied() else {
            positions.insert(identity, merged.len());
            merged.push(result);
            continue;
        };
        if source_rank(&result) < source_rank(&merged[existing_index]) {
            merged[existing_index] = result;
        }
    }
    merged
}

fn source_rank(result: &SearchResult) -> u8 {
    if result.subtitle.to_ascii_lowercase().contains("start menu") {
        0
    } else {
        1
    }
}

fn is_chrome_proxy_target(value: &str) -> bool {
    normalize_windows_path(value)
        .rsplit('\\')
        .next()
        .is_some_and(|name| name == "chrome_proxy.exe")
}

fn is_shortcut_path(value: &str) -> bool {
    Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
}

fn normalize_windows_path(value: &str) -> String {
    let trimmed = value.trim().trim_matches('"');
    let normalized = trimmed.replace('/', "\\");
    let normalized = normalized.trim_end_matches('\\');
    normalize(normalized)
}

#[cfg(windows)]
fn resolve_shell_link_target(path: &str) -> Option<(String, String)> {
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Storage::EnhancedStorage::PKEY_Link_Arguments;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
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
unsafe fn property_store_arguments(link: &IShellLinkW) -> Option<String> {
    let store: IPropertyStore = link.cast().ok()?;
    let mut value: PROPVARIANT = store.GetValue(&PKEY_Link_Arguments).ok()?;
    let result = (|| {
        let header = unsafe { value.Anonymous.Anonymous };
        if header.vt != VT_LPWSTR {
            return None;
        }
        let pointer = unsafe { header.Anonymous.pwszVal };
        if pointer.is_null() {
            return None;
        }
        unsafe { pointer.to_string().ok() }
    })();
    unsafe {
        windows::Win32::System::Variant::PropVariantClear(&mut value).ok();
    }
    result
}

#[cfg(not(windows))]
fn resolve_shell_link_target(_path: &str) -> Option<(String, String)> {
    None
}

fn extract_executable_target(value: &str) -> Option<String> {
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

fn expand_percent_variables(value: &str) -> Option<String> {
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

fn is_executable_target(target: &str) -> bool {
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
fn start_menu_roots() -> Vec<PathBuf> {
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
fn collect_files(root: &Path, depth: usize, by_title: &mut HashMap<String, SearchResult>) {
    if depth > MAX_SCAN_DEPTH || by_title.len() >= MAX_CATALOG_ENTRIES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if by_title.len() >= MAX_CATALOG_ENTRIES {
            return;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_files(&path, depth + 1, by_title);
            continue;
        }
        if !file_type.is_file() || !is_application_file(&path) {
            continue;
        }
        let Some(result) = application_result(path, "Start Menu") else {
            continue;
        };
        let key = normalize(&result.title);
        by_title.entry(key).or_insert(result);
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
fn collect_app_paths(by_title: &mut HashMap<String, SearchResult>) {
    use windows::core::{w, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER,
        HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ,
    };

    unsafe fn collect_root(
        root: HKEY,
        subkey: PCWSTR,
        by_title: &mut HashMap<String, SearchResult>,
    ) {
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
            if let Some((title, target)) = read_app_path(app_paths, &key_name) {
                let key = normalize(&title);
                by_title.entry(key).or_insert_with(|| SearchResult {
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
            by_title,
        );
        collect_root(
            HKEY_LOCAL_MACHINE,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths"),
            by_title,
        );
    }
}

#[cfg(not(windows))]
fn start_menu_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
fn collect_files(_root: &Path, _depth: usize, _by_title: &mut HashMap<String, SearchResult>) {}

#[cfg(not(windows))]
fn collect_app_paths(_by_title: &mut HashMap<String, SearchResult>) {}

#[allow(dead_code)]
fn _keep_os_string_type_available(_: OsString) {}

#[cfg(test)]
mod tests {
    use super::{
        canonical_application_id, canonical_target_key, expand_percent_variables,
        extract_executable_target, is_executable_target, ApplicationCatalog,
    };
    use flux_core::{ResultKind, ResultSource, SearchResult};

    #[test]
    fn canonical_application_identity_normalizes_windows_target_paths() {
        let first = canonical_application_id(r"C:\Program Files\Google\Chrome\chrome.exe");
        let second = canonical_target_key(r"c:/Program Files/Google/Chrome/chrome.exe");
        assert_eq!(
            first.as_deref(),
            Some("application:target:c:\\program files\\google\\chrome\\chrome.exe")
        );
        assert_eq!(
            second.as_deref(),
            Some("c:\\program files\\google\\chrome\\chrome.exe")
        );
    }

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

    #[test]
    fn application_catalog_search_is_title_based_and_application_tiered() {
        let catalog = ApplicationCatalog {
            entries: vec![SearchResult {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Program Files (x86)\\Steam\\steam.exe")),
            }],
        };
        let results = catalog.search("steam");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Steam");
        assert_eq!(results[0].kind, ResultKind::Application);
    }

    #[test]
    fn chrome_web_apps_match_by_proxy_executable_and_keep_distinct_app_ids() {
        let catalog = ApplicationCatalog {
            entries: vec![
                SearchResult {
                    id: String::from(
                        r"application:target:c:\\program files\\google\\chrome\\application\\chrome_proxy.exe|args:--profile-directory=default --app-id=perplexity",
                    ),
                    title: String::from("Perplexity"),
                    subtitle: String::from("Application • Start Menu"),
                    kind: ResultKind::Application,
                    source: ResultSource::ApplicationCatalog,
                    target: Some(String::from(r"C:\\Users\\m1nus\\Perplexity.lnk")),
                },
                SearchResult {
                    id: String::from(
                        r"application:target:c:\\program files\\google\\chrome\\application\\chrome_proxy.exe|args:--profile-directory=default --app-id=grok",
                    ),
                    title: String::from("Grok"),
                    subtitle: String::from("Application • Start Menu"),
                    kind: ResultKind::Application,
                    source: ResultSource::ApplicationCatalog,
                    target: Some(String::from(r"C:\\Users\\m1nus\\Grok.lnk")),
                },
            ],
        };
        let results = catalog.search("chrome");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|result| result.title == "Perplexity"));
        assert!(results.iter().any(|result| result.title == "Grok"));
    }
}
