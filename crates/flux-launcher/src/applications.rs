use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use flux_core::{candidate_keys, rank_results, CandidateKeys, PreparedQuery, SearchResult};
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

/// One catalog row with the two matcher forms of its title and of its executable name.
///
/// The forms are derived from the row and never change, so they are built once when the
/// catalog is loaded. A keystroke re-tests every row, and deriving them per test was the
/// whole cost of a keystroke on the applications path: the compact form lowercases every
/// character through the Unicode mapping and grows a string, for 878 rows and up to two
/// tests each. They live in the same struct as the row so the two cannot drift apart.
#[derive(Clone, Debug)]
struct CatalogEntry {
    result: SearchResult,
    title_keys: CandidateKeys,
    executable_keys: CandidateKeys,
}

impl CatalogEntry {
    fn new(result: SearchResult) -> Self {
        let title_keys = candidate_keys(&result.title);
        let executable_keys = candidate_keys(executable_name(&result));
        Self {
            result,
            title_keys,
            executable_keys,
        }
    }
}

/// The executable name a row is also searched by, or an empty string when the row is not
/// an application target.
fn executable_name(result: &SearchResult) -> &str {
    let Some(identity) = result.id.strip_prefix("application:target:") else {
        return "";
    };
    identity
        .split_once('|')
        .map_or(identity, |(target, _)| target)
        .rsplit('\\')
        .next()
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
struct ApplicationCatalog {
    entries: Vec<CatalogEntry>,
}

pub struct ApplicationWorker {
    latest: Arc<Mutex<Option<ApplicationRequest>>>,
    wake: mpsc::SyncSender<()>,
}

impl ApplicationWorker {
    pub fn spawn(output: Sender<ApplicationResponse>, preferred_ids: Vec<String>) -> Self {
        let latest = Arc::new(Mutex::new(None::<ApplicationRequest>));
        let latest_for_worker = Arc::clone(&latest);
        let (wake, receiver) = mpsc::sync_channel::<()>(1);
        thread::Builder::new()
            .name(String::from("flux-applications"))
            .spawn(move || {
                let catalog = ApplicationCatalog::load();
                // The catalog is known before the first keystroke, so its pictures can
                // be loaded while nothing is on screen. A page of application rows
                // otherwise costs one shell round trip per row, every session.
                super::shell_icon_cache::warm_shell_icons(
                    catalog.icon_targets_in_order(&preferred_ids),
                );
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
        Self::from_results(entries)
    }

    /// Builds the catalog from its rows, deriving each row's matcher forms once.
    fn from_results(entries: Vec<SearchResult>) -> Self {
        Self {
            entries: entries.into_iter().map(CatalogEntry::new).collect(),
        }
    }

    /// Every icon target the catalog holds, without duplicates, for the idle
    /// warm-up. The ids the owner launches most go first: the pass is slow enough
    /// that its order decides which icons are ready when he opens the launcher
    /// right after start, and everything else keeps the catalog order.
    fn icon_targets_in_order(&self, preferred_ids: &[String]) -> Vec<String> {
        let ranks: HashMap<&str, usize> = preferred_ids
            .iter()
            .enumerate()
            .map(|(rank, id)| (id.as_str(), rank))
            .collect();
        let mut ordered: Vec<(usize, String)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let target = entry.result.target.clone()?;
                Some((
                    ranks
                        .get(entry.result.id.as_str())
                        .copied()
                        .unwrap_or(usize::MAX),
                    target,
                ))
            })
            .collect();
        ordered.sort_by_key(|(rank, _target)| *rank);
        let mut seen = HashSet::new();
        let warmed = ordered
            .into_iter()
            .filter_map(|(_rank, target)| seen.insert(target.clone()).then_some(target))
            .collect::<Vec<_>>();
        // Warming more icons than the cache holds would evict the ones it just
        // loaded, so the pass stops at the capacity: the list is ordered by what the
        // owner launches, so what fits is what matters.
        let capacity = super::shell_icon_cache::MAX_SHELL_ICON_CACHE_ENTRIES;
        warmed.into_iter().take(capacity).collect()
    }

    fn search(&self, query: &str) -> Vec<SearchResult> {
        // The query is prepared once and every entry's two matcher forms were built when
        // the catalog was loaded, so a keystroke is 878 substring tests and nothing else:
        // 878 entries used to re-derive the query's forms and each entry's own on every
        // character typed.
        let prepared = PreparedQuery::new(query);
        if normalize(query).is_empty() {
            return Vec::new();
        }
        let mut results = Vec::new();
        for entry in &self.entries {
            if application_matches_query(&entry.title_keys, &entry.executable_keys, &prepared) {
                results.push(entry.result.clone());
            }
        }
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

fn application_matches_query(
    title_keys: &CandidateKeys,
    executable_keys: &CandidateKeys,
    prepared: &PreparedQuery,
) -> bool {
    if prepared.matches_keys(title_keys) {
        return true;
    }
    if executable_keys.normalized.is_empty() && executable_keys.compact.is_empty() {
        return false;
    }
    prepared.matches_keys(executable_keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::{ResultKind, ResultSource, SearchResult};

    #[test]
    fn application_catalog_search_is_title_based_and_application_tiered() {
        let catalog = ApplicationCatalog::from_results(vec![SearchResult {
            id: String::from("application:steam"),
            title: String::from("Steam"),
            subtitle: String::from("Application вЂў Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Program Files (x86)\\Steam\\steam.exe")),
        }]);
        let results = catalog.search("steam");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Steam");
        assert_eq!(results[0].kind, ResultKind::Application);
    }

    #[test]
    fn the_warm_up_loads_the_apps_he_launches_before_the_rest_of_the_catalog() {
        let entry = |id: &str, title: &str, target: &str| SearchResult {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: String::from("Application вЂў Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(target.to_owned()),
        };
        let catalog = ApplicationCatalog::from_results(vec![
            entry("application:calibre", "calibre", r"C:\calibre\calibre.exe"),
            entry("application:spotify", "Spotify", r"C:\Spotify\Spotify.exe"),
            entry("application:steam", "Steam", r"C:\Steam\steam.exe"),
            entry("application:calibre", "calibre", r"C:\calibre\calibre.exe"),
        ]);

        let ordered = catalog.icon_targets_in_order(&[
            String::from("application:steam"),
            String::from("application:spotify"),
        ]);

        assert_eq!(
            ordered,
            vec![
                r"C:\Steam\steam.exe",
                r"C:\Spotify\Spotify.exe",
                // Not launched: still warmed, in the catalog's own order, once.
                r"C:\calibre\calibre.exe",
            ]
        );
    }

    #[test]
    fn compact_query_finds_spaced_lm_studio_application_before_ranking() {
        let spaced_title = SearchResult {
            id: String::from(r"application:target:c:\\program files\\lm studio\\lm studio.exe"),
            title: String::from("LM Studio"),
            subtitle: String::from("Application вЂў Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Users\\m1nus\\LM Studio.lnk")),
        };
        let executable_title = SearchResult {
            id: String::from(r"application:target:c:\\tools\\lm studio.exe"),
            title: String::from("Local Model Runner"),
            subtitle: String::from("Application вЂў App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Tools\\LM Studio.exe")),
        };
        let catalog =
            ApplicationCatalog::from_results(vec![spaced_title.clone(), executable_title]);
        let results = catalog.search("lmstudio");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "LM Studio");
        let spaced = CatalogEntry::new(spaced_title.clone());
        assert!(application_matches_query(
            &spaced.title_keys,
            &spaced.executable_keys,
            &PreparedQuery::new("lmstudio"),
        ));
        assert!(!application_matches_query(
            &spaced.title_keys,
            &spaced.executable_keys,
            &PreparedQuery::new("!!!"),
        ));
    }

    #[test]
    fn compact_application_queries_match_common_spaced_titles_and_preserve_literal_precedence() {
        let catalog = ApplicationCatalog::from_results(vec![
            SearchResult {
                id: String::from(r"application:target:c:\\apps\\visual-studio-code.exe"),
                title: String::from("Visual Studio Code"),
                subtitle: String::from("Application вЂў Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Apps\\Visual Studio Code.lnk")),
            },
            SearchResult {
                id: String::from(r"application:target:c:\\apps\\visualstudiocode.exe"),
                title: String::from("visualstudiocode"),
                subtitle: String::from("Application вЂў App Paths"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Apps\\visualstudiocode.exe")),
            },
        ]);

        let compact_results = catalog.search("visualstudiocode");
        assert_eq!(compact_results.len(), 2);
        assert_eq!(compact_results[0].title, "visualstudiocode");
        assert_eq!(compact_results[1].title, "Visual Studio Code");
        assert_eq!(catalog.search("visual-studio-code").len(), 2);
        assert!(catalog.search("!!!").is_empty());
    }

    #[test]
    fn chrome_web_apps_match_by_proxy_executable_and_keep_distinct_app_ids() {
        let catalog = ApplicationCatalog::from_results(vec![
            SearchResult {
                id: String::from(
                    r"application:target:c:\\program files\\google\\chrome\\application\\chrome_proxy.exe|args:--profile-directory=default --app-id=perplexity",
                ),
                title: String::from("Perplexity"),
                subtitle: String::from("Application вЂў Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Users\\m1nus\\Perplexity.lnk")),
            },
            SearchResult {
                id: String::from(
                    r"application:target:c:\\program files\\google\\chrome\\application\\chrome_proxy.exe|args:--profile-directory=default --app-id=grok",
                ),
                title: String::from("Grok"),
                subtitle: String::from("Application вЂў Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Users\\m1nus\\Grok.lnk")),
            },
        ]);
        let results = catalog.search("chrome");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|result| result.title == "Perplexity"));
        assert!(results.iter().any(|result| result.title == "Grok"));
    }
}
