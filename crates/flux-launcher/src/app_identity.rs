use std::collections::HashMap;
use std::path::Path;

use flux_core::{ResultKind, ResultSource, SearchResult};

use super::app_scan::{expand_percent_variables, is_executable_target, resolve_shell_link_target};
use super::applications::normalize;

/// Returns the canonical identity for an application result.
///
/// Flow Launcher groups Win32 entries by the resolved executable target and
/// shortcut arguments rather than by the `.lnk` source path. Flux keeps the
/// source path in `target` for launch behavior, but uses this resolved identity
/// to merge App Paths, Start Menu, Desktop, and Everything application hits.
pub(crate) fn canonical_application_key(result: &SearchResult) -> Option<String> {
    if !is_mergeable_application_result(result) {
        return None;
    }
    if let Some(target) = result.target.as_deref() {
        if let Some(identity) = canonical_target_key(target) {
            return Some(identity);
        }
    }
    result
        .id
        .strip_prefix("application:target:")
        .filter(|identity| !identity.is_empty())
        .map(str::to_owned)
}

fn is_mergeable_application_result(result: &SearchResult) -> bool {
    if result.kind == ResultKind::Application {
        return true;
    }
    result.source == ResultSource::BuiltIn
        && result.id.starts_with("system:")
        && result
            .target
            .as_deref()
            .is_some_and(|target| is_bare_executable_target(target) || is_executable_target(target))
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
            let resolved = expand_percent_variables(&resolved).unwrap_or(resolved);
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
    if let Some(resolved) = resolve_bare_executable_path(target) {
        return Some(normalize_windows_path(&resolved));
    }
    Some(normalize_windows_path(target))
}

pub(crate) fn resolve_bare_executable_path(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"');
    if !is_bare_executable_target(value) {
        return None;
    }

    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::SearchPathW;

        let filename = value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut buffer = vec![0_u16; 32_768];
        let length = unsafe {
            SearchPathW(
                PCWSTR::null(),
                PCWSTR(filename.as_ptr()),
                PCWSTR::null(),
                Some(&mut buffer),
                None,
            )
        };
        if length == 0 || length as usize >= buffer.len() {
            return None;
        }
        String::from_utf16(&buffer[..length as usize]).ok()
    }

    #[cfg(not(windows))]
    {
        None
    }
}

fn is_bare_executable_target(value: &str) -> bool {
    let value = value.trim().trim_matches('"');
    !value.is_empty()
        && !value.contains(['\\', '/'])
        && matches!(
            Path::new(value)
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
                .as_deref(),
            Some("exe") | Some("com") | Some("bat") | Some("cmd")
        )
}

pub(crate) fn merge_catalog_candidates(results: Vec<SearchResult>) -> Vec<SearchResult> {
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
    let normalized = normalized.trim_end_matches('\\').to_owned();
    #[cfg(windows)]
    let normalized = std::fs::canonicalize(&normalized)
        .map(|path| path.to_string_lossy().replace('/', "\\"))
        .unwrap_or(normalized);
    normalize(&normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn canonical_target_wins_over_stale_application_id() {
        let result = SearchResult {
            id: String::from(r"application:target:c:\\old\\powershell.exe"),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\new\\powershell.exe")),
        };

        assert_eq!(
            canonical_application_key(&result).as_deref(),
            Some(r"c:\\new\\powershell.exe")
        );
    }

    #[cfg(windows)]
    #[test]
    fn identical_real_power_shell_target_merges_catalog_entries() {
        let powershell_path = resolve_bare_executable_path("powershell.exe")
            .expect("Windows PowerShell should resolve through SearchPathW");
        let start_menu = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path.clone()),
        };
        let app_path = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path),
        };

        assert_eq!(
            canonical_application_key(&start_menu),
            canonical_application_key(&app_path)
        );
        let merged = merge_catalog_candidates(vec![app_path, start_menu]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].subtitle, "Application • Start Menu");
    }

    #[test]
    fn identical_power_shell_app_paths_with_different_display_names_merge() {
        let first = SearchResult {
            id: String::from(r"application:target:c:\program files\powershell\7\pwsh.exe"),
            title: String::from("pwsh"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\Program Files\PowerShell\7\pwsh.exe")),
        };
        let second = SearchResult {
            id: String::from(r"application:target:C:/PROGRAM FILES/POWERSHELL/7/pwsh.exe"),
            title: String::from("PowerShell 7"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"c:/Program Files/PowerShell/7/pwsh.exe")),
        };

        let merged = merge_catalog_candidates(vec![first, second]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "pwsh");
    }

    #[test]
    fn different_power_shell_executable_paths_remain_distinct() {
        let windows_power_shell = SearchResult {
            id: String::from(
                r"application:target:c:\\windows\\system32\\windowspowershell\\v1.0\\powershell.exe",
            ),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(
                r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            )),
        };
        let powershell_7 = SearchResult {
            id: String::from(r"application:target:c:\\program files\\powershell\\7\\pwsh.exe"),
            title: String::from("PowerShell 7"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
        };

        let merged = merge_catalog_candidates(vec![windows_power_shell, powershell_7]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn same_executable_with_different_arguments_remains_distinct() {
        let first = SearchResult {
            id: String::from(
                r"application:target:c:\\program files\\powershell\\7\\pwsh.exe|args:-nologo",
            ),
            title: String::from("PowerShell 7"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\PowerShell.lnk")),
        };
        let second = SearchResult {
            id: String::from(
                r"application:target:c:\\program files\\powershell\\7\\pwsh.exe|args:-nologo -noprofile",
            ),
            title: String::from("PowerShell 7 No Profile"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\PowerShell No Profile.lnk")),
        };

        let merged = merge_catalog_candidates(vec![first, second]);
        assert_eq!(merged.len(), 2);
    }
}
