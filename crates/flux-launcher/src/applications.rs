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

        let mut entries = by_title.into_values().collect::<Vec<_>>();
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
            .filter(|result| {
                let title = normalize(&result.title);
                title.contains(&normalized)
            })
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
    Some(SearchResult {
        id: format!("application:{}", normalize(&target)),
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
                    id: format!("application:app-paths:{}", normalize(&target)),
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
        let target = value
            .strip_prefix('"')
            .and_then(|rest| rest.find('"').map(|end| &rest[..end]))
            .unwrap_or_else(|| value.split_whitespace().next().unwrap_or_default())
            .to_owned();
        if target.is_empty() {
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
    use super::ApplicationCatalog;
    use flux_core::{ResultKind, ResultSource, SearchResult};

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
}
