use super::search::SearchResult;

pub fn matches_search_text(candidate: &str, query: &str) -> bool {
    let normalized_candidate = normalize(candidate);
    let normalized_query = normalize(query);
    if normalized_query.is_empty() || compact_search_key(&normalized_query).is_empty() {
        return false;
    }
    normalized_candidate.contains(&normalized_query)
        || compact_contains(&normalized_candidate, &normalized_query)
}

pub(crate) fn match_app_title(title: &str, query: &str) -> u8 {
    if query.is_empty() || title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if matches_search_text(title, query) {
        2
    } else {
        3
    }
}

pub(crate) fn match_file_title(title: &str, query: &str) -> u8 {
    if query.is_empty() {
        3
    } else if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if matches_search_text(title, query) {
        2
    } else {
        3
    }
}

pub(crate) fn is_application_path(path: &str) -> bool {
    let lower = normalize(path);
    [".exe", ".lnk", ".com", ".bat", ".cmd", ".url"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub(crate) fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn compact_search_key(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn compact_contains(title: &str, query: &str) -> bool {
    let compact_query = compact_search_key(query);
    !compact_query.is_empty() && compact_search_key(title).contains(&compact_query)
}

pub fn rank_results(query: &str, results: &mut [SearchResult]) {
    results.sort_by_key(|result| result.relevance(query));
}

pub fn rank_results_with_priorities(
    query: &str,
    results: &mut [SearchResult],
    priorities: &[String],
) {
    results.sort_by_key(|result| result.priority_relevance(query, priorities));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{ResultKind, ResultSource};

    #[test]
    fn calculator_result_outranks_application_matches_for_expression_queries() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:uninstall-3d-sdk"),
                title: String::from("Uninstall 3D SDK 1.10.15"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Program Files\\SDK\\uninstall.exe")),
            },
            SearchResult {
                id: String::from("builtin:calculator"),
                title: String::from("= 2"),
                subtitle: String::from("Calculator • 1+1"),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
        ];

        rank_results_with_priorities("1+1", &mut results, &[]);

        assert_eq!(results[0].id, "builtin:calculator");
        assert_eq!(results[1].id, "application:uninstall-3d-sdk");
    }

    #[test]
    fn calculator_priority_is_visible_for_date_like_queries() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:calendar-entry"),
                title: String::from("Calendar 2026-08"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\\Calendar.exe")),
            },
            SearchResult {
                id: String::from("builtin:calculator"),
                title: String::from("= 2018"),
                subtitle: String::from("Calculator • 2026-08"),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
        ];

        rank_results_with_priorities("2026-08", &mut results, &[]);

        assert_eq!(results[0].id, "builtin:calculator");
        assert_eq!(results[1].id, "application:calendar-entry");
    }

    #[test]
    fn compact_queries_match_spaced_app_titles_and_filenames() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:lm-studio"),
                title: String::from("LM Studio"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from("C:/LM Studio.lnk")),
            },
            SearchResult::file(
                String::from("C:/Downloads/lmstudio.json"),
                String::from("lmstudio.json"),
                String::from("C:/Downloads"),
            ),
        ];
        rank_results("lmstudio", &mut results);
        assert_eq!(results[0].title, "LM Studio");
        assert_eq!(results[1].title, "lmstudio.json");

        assert_eq!(match_app_title("LM Studio", "lmstudio"), 2);
        assert!(matches_search_text("LM Studio", "lmstudio"));
        assert!(matches_search_text("lmstudio.json", "lmstudio"));
        assert!(matches_search_text("LM-Studio", "lmstudio"));
        assert!(!matches_search_text("LM Studio", "!!!"));
        assert_eq!(match_file_title("lmstudio.json", "lmstudio"), 1);
        assert_eq!(match_app_title("Chrome", "..."), 3);
    }

    #[test]
    fn executable_results_are_ranked_before_indexed_files() {
        let mut results = vec![
            SearchResult::file(
                String::from("C:/Windows/Steam/cache.txt"),
                String::from("cache.txt"),
                String::from("C:/Windows/Steam"),
            ),
            SearchResult::file(
                String::from("C:/Program Files/Steam/Steam.exe"),
                String::from("Steam.exe"),
                String::from("C:/Program Files/Steam"),
            ),
        ];
        rank_results("steam", &mut results);
        assert_eq!(results[0].kind, ResultKind::Application);
        assert_eq!(results[0].title, "Steam");
    }

    #[test]
    fn application_catalog_outranks_everything_executable_for_exact_name() {
        let mut results = vec![
            SearchResult::file(
                String::from("C:/Users/Test/workspace/Steam.exe"),
                String::from("Steam.exe"),
                String::from("C:/Users/Test/workspace"),
            ),
            SearchResult {
                id: String::from("application:start-menu:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(
                    r"C:\Users\Test\AppData\Roaming\Microsoft\Windows\Start Menu\Steam.lnk",
                )),
            },
        ];
        rank_results("Steam", &mut results);
        assert_eq!(results[0].source, ResultSource::ApplicationCatalog);
        assert_eq!(results[0].title, "Steam");
    }

    #[test]
    fn explicit_priority_outranks_all_other_application_results() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from("C:/Steam.lnk")),
            },
            SearchResult {
                id: String::from("application:chrome"),
                title: String::from("Chrome"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from("C:/Chrome.lnk")),
            },
        ];
        rank_results_with_priorities(
            "app",
            &mut results,
            &[
                String::from("application:chrome"),
                String::from("application:steam"),
            ],
        );
        assert_eq!(results[0].id, "application:chrome");
        assert_eq!(results[1].id, "application:steam");
    }
}
