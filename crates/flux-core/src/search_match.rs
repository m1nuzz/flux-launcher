use super::search::SearchResult;

pub fn matches_search_text(candidate: &str, query: &str) -> bool {
    PreparedQuery::new(query).matches(candidate, &mut String::new())
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

/// A query normalised once, for a caller that tests many candidates against it.
///
/// The application catalog asks "does this row match" for every one of its 878 entries on
/// every keystroke, and [`matches_search_text`] re-derived the query's normalised form, its
/// compact form and its path-ness for each of them: five allocations per candidate, of
/// which three depend only on the query. Measured on this machine, 20 searches of that
/// 878-entry catalog cost 5.4-8.2 ms of CPU; the query side is now paid once per keystroke
/// and the candidate side into a buffer the caller reuses, which is what makes a keystroke
/// cheap enough that the thread asking for it is not the reason a keystroke feels late.
///
/// The answers are identical to [`matches_search_text`] by construction - it is the same
/// three tests in the same order - and `prepared_queries_answer_like_the_one_shot_matcher`
/// pins that over a corpus rather than asserting it.
pub struct PreparedQuery {
    normalized: String,
    /// The query's compact form, built once: the candidate's own compact form is written
    /// into the caller's buffer and matched against this with a real substring test.
    compact: String,
    /// Only a path query pays for this: the separator form is needed once, not per
    /// candidate.
    unified: Option<String>,
    /// A query with no compact form matches nothing, and says so once instead of per
    /// candidate.
    matches_nothing: bool,
}

impl PreparedQuery {
    pub fn new(query: &str) -> Self {
        let normalized = normalize(query);
        let compact = compact_search_key(&normalized);
        let unified = is_path_query(&normalized).then(|| unified_separators(&normalized));
        Self {
            matches_nothing: normalized.is_empty() || compact.is_empty(),
            normalized,
            compact,
            unified,
        }
    }

    /// Whether `candidate` answers this query. `scratch` is reused across candidates, so
    /// a loop over a catalog allocates for it once rather than once per entry.
    pub fn matches(&self, candidate: &str, scratch: &mut String) -> bool {
        if self.matches_nothing {
            return false;
        }
        normalize_into(candidate, scratch);
        if scratch.contains(&self.normalized) {
            return true;
        }
        if let Some(unified_query) = &self.unified {
            // A separator is a step in a path, not punctuation to forgive: with the
            // compact comparison a query of `d:/` matches every name holding a "d", so
            // the results of the previous keystroke keep answering the new query and
            // stay on screen until the file provider replies.
            return unified_separators(scratch).contains(unified_query.as_str());
        }
        // The compact comparison forgives the punctuation a launcher query leaves out
        // (`lmstudio` finds "LM Studio"), so it is still the second test. The candidate's
        // compact form lands in the buffer the caller is already reusing.
        compact_search_key_into(candidate, scratch);
        scratch.contains(&self.compact)
    }
}

fn normalize_into(value: &str, scratch: &mut String) {
    scratch.clear();
    scratch.extend(
        value
            .trim()
            .chars()
            .map(|character| character.to_ascii_lowercase()),
    );
}

fn compact_search_key_into(value: &str, scratch: &mut String) {
    scratch.clear();
    scratch.extend(
        value
            .trim()
            .chars()
            .flat_map(char::to_lowercase)
            .filter(|character| character.is_alphanumeric()),
    );
}

fn unified_separators(value: &str) -> String {
    value.replace('/', "\\")
}

/// The rank key depends on the row and the query, never on the rest of the list, so it
/// is computed once per row instead of once per comparison.
///
/// `sort_by_key` calls the key function on every comparison, and the key is not cheap:
/// it normalises the query, decides whether the text is a path, and compares the whole
/// path, allocating for each step. A word that matches four hundred catalog entries meant
/// roughly forty-five hundred key evaluations, and the keystroke paid 54 ms for a sort
/// that takes 1 ms when the keys are cached. Measured on a real catalog of 878 entries,
/// `c`: 54.5 ms before, 1.4 ms after, against 12.2 ms on the build that had the simple
/// two-way matcher.
pub fn rank_results(query: &str, results: &mut [SearchResult]) {
    results.sort_by_cached_key(|result| result.relevance(query));
}

pub fn rank_results_with_priorities(
    query: &str,
    results: &mut [SearchResult],
    priorities: &[String],
) {
    results.sort_by_cached_key(|result| result.priority_relevance(query, priorities));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{ResultKind, ResultSource};

    #[test]
    fn prepared_queries_answer_like_the_one_shot_matcher() {
        // The reference below is the one-shot matcher's own three tests, written out
        // again. If the prepared form ever answers differently - a tighter or looser
        // compact comparison, a path query answered as a word - this fails.
        fn reference(candidate: &str, query: &str) -> bool {
            let normalized_candidate = normalize(candidate);
            let normalized_query = normalize(query);
            if normalized_query.is_empty() || compact_search_key(&normalized_query).is_empty() {
                return false;
            }
            if normalized_query.contains('/') || normalized_query.contains('\\') {
                return normalized_candidate
                    .replace('/', "\\")
                    .contains(&normalized_query.replace('/', "\\"));
            }
            normalized_candidate.contains(&normalized_query)
                || compact_search_key(&normalized_candidate)
                    .contains(&compact_search_key(&normalized_query))
        }

        let candidates = [
            "LM Studio",
            "Visual Studio Code",
            "spaced_title",
            "lmstudio.json",
            "chatgpt.md",
            "D:\\Music\\Song.mp4",
            "F:\\Maxim\\cmd",
            "serialisable.pyi",
            "elisam@nvidia.com",
            "日本語のアプリ",
            "",
            "   ",
            "!!!",
            "aab",
            "aaab",
        ];
        let queries = [
            "l",
            "lm",
            "lmstudio",
            "LM Studio",
            "visual-studio-code",
            "studio",
            "song",
            "d:/",
            "d:\\",
            "f:\\maxim\\cmd",
            "ext:zip",
            "dm:today",
            "2026-08",
            "aab",
            "aa",
            "!!!",
            "",
            "  ",
            ".mp4",
            "日本語",
        ];
        for query in queries {
            let prepared = PreparedQuery::new(query);
            let mut scratch = String::new();
            for candidate in candidates {
                assert_eq!(
                    prepared.matches(candidate, &mut scratch),
                    reference(candidate, query),
                    "prepared disagrees with the one-shot matcher on {candidate:?} / {query:?}"
                );
            }
            // The buffer is reused, so the answer must not depend on what was in it.
            scratch.push_str("leftover from the previous candidate");
            assert_eq!(
                prepared.matches("LM Studio", &mut scratch),
                reference("LM Studio", query),
                "a dirty buffer changed the answer for {query:?}"
            );
        }
    }

    #[test]
    fn a_reused_buffer_never_answers_for_the_previous_candidate() {
        // The buffer is written over, not appended to, so a short candidate after a long
        // one cannot be answered from the tail the long one left behind.
        let prepared = PreparedQuery::new("abc");
        let mut scratch = String::new();
        assert!(prepared.matches("xxabcxx", &mut scratch));
        assert!(
            !prepared.matches("zz", &mut scratch),
            "stale tail answered for a short candidate"
        );
        assert!(!prepared.matches("", &mut scratch));
        assert!(
            prepared.matches("abc", &mut scratch),
            "the candidate itself still answers"
        );
        // A query whose compact form is a prefix of the leftover must not match either.
        let prefix = PreparedQuery::new("verylong");
        assert!(!prefix.matches("zz", &mut scratch));
    }

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
