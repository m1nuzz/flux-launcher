use std::collections::HashMap;
use std::io::Write;

use flux_core::{ResultKind, ResultSource, SearchResult};

use super::applications::{canonical_application_key, resolve_bare_executable_path};

pub(crate) fn trace_query_probe(query: &str, results: &[SearchResult]) {
    let normalized = query.trim().to_ascii_lowercase();
    if !matches!(
        normalized.as_str(),
        "1+1"
            | "2026-08"
            | "powershell"
            | "pwsh"
            | "q"
            | "中"
            | "文"
            | "中文"
            | "q中"
            | "q文"
            | "q中文"
    ) {
        return;
    }
    let Some(path) = std::env::var_os("FLUX_QUERY_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let snapshot = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    let sanitize = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let _ = writeln!(
        file,
        "snapshot={snapshot}\tquery={}\tcount={}",
        sanitize(&normalized),
        results.len()
    );
    for (index, result) in results.iter().enumerate() {
        let target = result.target.as_deref().map(sanitize).unwrap_or_default();
        let identity = canonical_application_key(result)
            .map(|value| sanitize(&value))
            .unwrap_or_default();
        let _ = writeln!(
            file,
            "snapshot={snapshot}\tquery={}\tindex={index}\tid={}\ttitle={}\tsource={:?}\tkind={:?}\ttarget={}\tidentity={}",
            sanitize(&normalized),
            sanitize(&result.id),
            sanitize(&result.title),
            result.source,
            result.kind,
            target,
            identity
        );
    }
}

pub(crate) fn merge_application_duplicates(results: Vec<SearchResult>) -> Vec<SearchResult> {
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

        let existing_is_exact_console = is_exact_console_result(&merged[existing_index]);
        let result_is_exact_console = is_exact_console_result(&result);
        if application_source_rank(&result) < application_source_rank(&merged[existing_index]) {
            let preserved_id = result_is_exact_console
                .then(|| result.id.clone())
                .or_else(|| existing_is_exact_console.then(|| merged[existing_index].id.clone()));
            merged[existing_index] = result;
            if let Some(id) = preserved_id {
                merged[existing_index].id = id;
            }
        } else if result_is_exact_console && !existing_is_exact_console {
            merged[existing_index].id = result.id;
        }
    }
    merged
}

fn is_exact_console_result(result: &SearchResult) -> bool {
    matches!(
        result.id.as_str(),
        "system:command-prompt" | "system:powershell"
    )
}

fn application_source_rank(result: &SearchResult) -> u8 {
    let subtitle = result.subtitle.to_ascii_lowercase();
    match result.source {
        ResultSource::ApplicationCatalog if subtitle.contains("start menu") => 0,
        ResultSource::ApplicationCatalog => 1,
        ResultSource::Everything => 2,
        ResultSource::Plugin => 3,
        ResultSource::BuiltIn => 4,
    }
}

/// Keep Everything's native modified-date order for non-application files.
///
/// The global ranker still decides which provider tier occupies each result
/// slot, so application results remain first. Only the Everything file slots
/// are replaced in the order returned by the date-sorted IPC query.
pub(crate) fn preserve_everything_file_order(
    merged: &mut [SearchResult],
    provider_order: &[SearchResult],
) {
    let mut available = merged
        .iter()
        .filter(|result| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        })
        .map(|result| (result.id.clone(), result.clone()))
        .collect::<HashMap<_, _>>();
    let slots = merged
        .iter()
        .enumerate()
        .filter(|(_, result)| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    for (slot, provider_result) in slots
        .into_iter()
        .zip(provider_order.iter().filter(|result| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        }))
    {
        let Some(result) = available.remove(&provider_result.id) else {
            continue;
        };
        merged[slot] = result;
    }
}

pub(crate) fn normalize_built_in_executable_targets(results: &mut [SearchResult]) {
    for result in results {
        if result.source != ResultSource::BuiltIn || !result.id.starts_with("system:") {
            continue;
        }
        let Some(target) = result.target.as_deref() else {
            continue;
        };
        if let Some(resolved) = resolve_bare_executable_path(target) {
            result.target = Some(resolved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::applications::canonical_application_id;
    use flux_core::rank_results_with_priorities;

    #[test]
    fn everything_file_order_is_preserved_after_app_first_ranking() {
        let application = SearchResult {
            id: String::from("application:report-viewer"),
            title: String::from("Report Viewer"),
            subtitle: String::from("Application"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\ReportViewer.lnk")),
        };
        let newest = SearchResult::file(
            String::from(r"C:\\workspace\\report-z.txt"),
            String::from("report-z.txt"),
            String::from(r"C:\\workspace"),
        );
        let older = SearchResult::file(
            String::from(r"C:\\workspace\\report-a.txt"),
            String::from("report-a.txt"),
            String::from(r"C:\\workspace"),
        );
        let newest_id = newest.id.clone();
        let older_id = older.id.clone();
        let mut merged = vec![application.clone(), older, newest];
        let provider_order = vec![merged[2].clone(), merged[1].clone()];

        preserve_everything_file_order(&mut merged, &provider_order);

        assert_eq!(merged[0].id, application.id);
        assert_eq!(merged[1].id, newest_id);
        assert_eq!(merged[2].id, older_id);
    }

    #[test]
    fn application_duplicates_merge_by_canonical_target_and_prefer_start_menu() {
        let app_paths = SearchResult {
            id: String::from("application:app-paths:chrome"),
            title: String::from("chrome"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            )),
        };
        let start_menu = SearchResult {
            id: String::from("application:start-menu:google-chrome"),
            title: String::from("Google Chrome"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(
                r"C:/Program Files/Google/Chrome/Application/chrome.exe",
            )),
        };
        let everything = SearchResult::file(
            String::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            String::from("chrome.exe"),
            String::from(r"C:\Program Files\Google\Chrome\Application"),
        );
        let merged = merge_application_duplicates(vec![app_paths, everything, start_menu]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "Google Chrome");
        assert!(merged[0].subtitle.contains("Start Menu"));
    }

    #[cfg(windows)]
    #[test]
    fn system_power_shell_merges_only_with_the_same_real_executable_path() {
        let powershell_path =
            resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
        let system = SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("powershell.exe")),
        };
        let app_path = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path),
        };
        let powershell_7 = SearchResult {
            id: String::from(r"application:target:c:\\program files\\powershell\\7\\pwsh.exe"),
            title: String::from("PowerShell 7"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
        };

        let merged = merge_application_duplicates(vec![system, app_path, powershell_7]);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|result| result.title == "PowerShell"));
        assert!(merged.iter().any(|result| result.title == "PowerShell 7"));
    }

    #[cfg(windows)]
    #[test]
    fn post_merge_exact_console_identity_survives_catalog_collision_and_ranks_first() {
        let powershell_path =
            resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
        let system = SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("powershell.exe")),
        };
        let catalog = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("Windows PowerShell"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path),
        };

        let mut merged = merge_application_duplicates(vec![system, catalog]);
        rank_results_with_priorities("powershell", &mut merged, &[]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "system:powershell");
    }
}
