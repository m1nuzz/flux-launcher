use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use flux_core::{matches_search_text, rank_results, SearchResult};
use windui::prelude::Sender;

pub(crate) use super::app_identity::{
    canonical_application_id, canonical_application_key, resolve_bare_executable_path,
};

pub(crate) const MAX_APPLICATION_RESULTS: usize = 16;
pub(crate) const MAX_CATALOG_ENTRIES: usize = 4096;
pub(crate) const MAX_SCAN_DEPTH: usize = 8;

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
        let mut candidates = Vec::<SearchResult>::new();

        #[cfg(windows)]
        {
            for root in super::app_scan::start_menu_roots() {
                super::app_scan::collect_files(&root, 0, &mut candidates);
            }
            super::app_scan::collect_app_paths(&mut candidates);
        }

        let mut entries = super::app_identity::merge_catalog_candidates(candidates);
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
        trace_application_probe(query, &results);
        results
    }
}

pub(crate) fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn trace_application_probe(query: &str, results: &[SearchResult]) {
    let Some(path) = std::env::var_os("FLUX_COMPACT_APP_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    for result in results {
        let id = result.id.replace(['\t', '\r', '\n'], " ");
        let title = result.title.replace(['\t', '\r', '\n'], " ");
        let target = result
            .target
            .as_deref()
            .unwrap_or_default()
            .replace(['\t', '\r', '\n'], " ");
        let _ = writeln!(
            file,
            "query={query}\ttitle={title}\tid={id}\ttarget={target}"
        );
    }
}

fn application_matches_query(result: &SearchResult, normalized_query: &str) -> bool {
    if matches_search_text(&result.title, normalized_query) {
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
    matches_search_text(executable, normalized_query)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn compact_query_finds_spaced_lm_studio_application_before_ranking() {
        let spaced_title = SearchResult {
            id: String::from(r"application:target:c:\\program files\\lm studio\\lm studio.exe"),
            title: String::from("LM Studio"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Users\\m1nus\\LM Studio.lnk")),
        };
        let executable_title = SearchResult {
            id: String::from(r"application:target:c:\\tools\\lm studio.exe"),
            title: String::from("Local Model Runner"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Tools\\LM Studio.exe")),
        };
        let catalog = ApplicationCatalog {
            entries: vec![spaced_title.clone(), executable_title],
        };
        let results = catalog.search("lmstudio");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "LM Studio");
        assert!(application_matches_query(&spaced_title, "lmstudio"));
        assert!(!application_matches_query(&spaced_title, "!!!"));
    }

    #[test]
    fn compact_application_queries_match_common_spaced_titles_and_preserve_literal_precedence() {
        let catalog = ApplicationCatalog {
            entries: vec![
                SearchResult {
                    id: String::from(r"application:target:c:\\apps\\visual-studio-code.exe"),
                    title: String::from("Visual Studio Code"),
                    subtitle: String::from("Application • Start Menu"),
                    kind: ResultKind::Application,
                    source: ResultSource::ApplicationCatalog,
                    target: Some(String::from(r"C:\\Apps\\Visual Studio Code.lnk")),
                },
                SearchResult {
                    id: String::from(r"application:target:c:\\apps\\visualstudiocode.exe"),
                    title: String::from("visualstudiocode"),
                    subtitle: String::from("Application • App Paths"),
                    kind: ResultKind::Application,
                    source: ResultSource::ApplicationCatalog,
                    target: Some(String::from(r"C:\\Apps\\visualstudiocode.exe")),
                },
            ],
        };

        let compact_results = catalog.search("visualstudiocode");
        assert_eq!(compact_results.len(), 2);
        assert_eq!(compact_results[0].title, "visualstudiocode");
        assert_eq!(compact_results[1].title, "Visual Studio Code");
        assert_eq!(catalog.search("visual-studio-code").len(), 2);
        assert!(catalog.search("!!!").is_empty());
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
