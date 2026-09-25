use super::search::SearchResult;

pub fn matches_search_text(candidate: &str, query: &str) -> bool {
    let normalized_candidate = normalize(candidate);
    let normalized_query = normalize(query);
    if normalized_query.is_empty() || compact_search_key(&normalized_query).is_empty() {
        return false;
    }
    if is_path_query(&normalized_query) {
        // A separator is a step in a path, not punctuation to forgive: with the
        // compact comparison a query of `d:/` matches every name holding a "d", so
        // the results of the previous keystroke keep answering the new query and
        // stay on screen until the file provider replies.
        return normalized_candidate
            .replace('/', "\\")
            .contains(&normalized_query.replace('/', "\\"));
    }
    normalized_candidate.contains(&normalized_query)
        || compact_contains(&normalized_candidate, &normalized_query)
}

fn is_path_query(normalized_query: &str) -> bool {
    normalized_query.contains('/') || normalized_query.contains('\\')
}

/// How well a title matches, best first: the whole title, its first characters,
/// the start of any word inside it, and only then a substring anywhere. Release
/// folders are titled like `[Hi-Res] LiSA／紅蓮華 [FLAC]`, so the word rule is what
/// puts the album the user typed above files that merely happen to contain it.
fn title_tier(title: &str, query: &str) -> u8 {
    if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if starts_a_word(title, query) {
        2
    } else if matches_search_text(title, query) {
        3
    } else {
        4
    }
}

fn starts_a_word(title: &str, query: &str) -> bool {
    !query.is_empty()
        && title
            .split(|character: char| !character.is_alphanumeric())
            .any(|word| word.starts_with(query))
}

pub(crate) fn match_app_title(title: &str, query: &str) -> u8 {
    if query.is_empty() {
        0
    } else {
        title_tier(title, query)
    }
}

pub(crate) fn match_file_title(title: &str, query: &str) -> u8 {
    if query.is_empty() {
        4
    } else {
        title_tier(title, query)
    }
}

pub fn is_application_path(path: &str) -> bool {
    let lower = normalize(path);
    if lower.ends_with(".exe")
        || lower.ends_with(".lnk")
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".url")
    {
        return true;
    }
    // `.com` is an executable extension and also the tail of every email address,
    // and mailto files sit in ordinary folders: `elisam@nvidia.com` is an address,
    // not a program, and treating it as one puts junk above what the user typed.
    lower.ends_with(".com") && !lower.contains('@')
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

        assert_eq!(match_app_title("LM Studio", "lmstudio"), 3);
        assert!(matches_search_text("LM Studio", "lmstudio"));
        assert!(matches_search_text("lmstudio.json", "lmstudio"));
        assert!(matches_search_text("LM-Studio", "lmstudio"));
        assert!(!matches_search_text("LM Studio", "!!!"));
        assert_eq!(match_file_title("lmstudio.json", "lmstudio"), 1);
        assert_eq!(match_app_title("Chrome", "..."), 4);
    }

    #[test]
    fn a_path_separator_is_not_forgiven_as_punctuation() {
        assert!(matches_search_text(r"D:\Music\song", "d:/"));
        assert!(matches_search_text("D:/Music/song", "d:/"));
        assert!(!matches_search_text("drama.json", "d:/"));
        assert!(!matches_search_text("LM Studio", "studio/"));
        // Without a separator the loose match stays: `lmstudio` still finds
        // "LM Studio", which is what makes a launcher usable.
        assert!(matches_search_text("LM Studio", "lmstudio"));
    }

    #[test]
    fn a_match_at_the_start_of_a_word_beats_one_buried_inside_it() {
        let folder = "[hi-res] lisa／紅蓮華 [flac] [24 bit／48khz]";
        assert_eq!(match_file_title(folder, "lisa"), 2);
        assert_eq!(match_file_title("elisa.json", "lisa"), 3);
        assert_eq!(match_file_title("serialisable.pyi", "lisa"), 3);

        let mut results = vec![
            SearchResult::file(
                String::from("D:/x/elisa.json"),
                String::from("elisa.json"),
                String::from("D:/x"),
            ),
            SearchResult::file(
                String::from("D:/y/folder"),
                folder.to_owned(),
                String::from("D:/y"),
            ),
        ];
        rank_results("lisa", &mut results);
        assert_eq!(results[0].title, folder, "the album the user typed wins");
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
