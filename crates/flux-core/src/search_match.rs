use super::search::SearchResult;

pub fn matches_search_text(candidate: &str, query: &str) -> bool {
    PreparedQuery::new(query).matches(candidate, &mut String::new())
}

fn is_path_query(normalized_query: &str) -> bool {
    normalized_query.contains('/') || normalized_query.contains('\\')
}

/// The typed text as a path in the form the filesystem takes, when the text *is* a
/// path. One pair of quotes and a trailing separator are not part of the place, and
/// `/` and `\` separate steps the same way, so both are handled here while the
/// caller's own spelling is kept - a folder row has to read like the path typed.
///
/// Accepted: `X:\...`, `X:/...`, a bare drive or drive root, `\\host\share\...`,
/// and a drive-less chain of two or more non-empty steps (`projects\warlocktest`).
pub fn path_query_body(query: &str) -> Option<String> {
    let body = strip_one_quote_pair(query.trim());
    if body.is_empty() || body.contains(':') && !has_drive_prefix(body) {
        // A `:` anywhere else is Everything's own syntax (`ext:zip`, `parent:`,
        // `dm:today`, `size:1mb`) or a URI, not a drive.
        return None;
    }
    let separators_unified = body.replace('/', "\\");
    let unified = strip_trailing_separator(&separators_unified);
    if unified.starts_with("\\\\") || has_drive_prefix(unified) {
        return Some(unified.to_owned());
    }
    // Drive-less: only a chain of real steps counts, so a lone word stays a word.
    let mut steps = unified.split('\\');
    let first = steps.next().unwrap_or_default();
    let second = steps.next().unwrap_or_default();
    if !first.is_empty() && !second.is_empty() {
        return Some(unified.to_owned());
    }
    None
}

/// [`path_query_body`] in the comparison form: which volume a path names is not part
/// of what the user meant, so case is dropped here and nowhere else.
pub fn path_query_text(query: &str) -> Option<String> {
    path_query_body(query).map(|body| body.to_lowercase())
}

/// The comparison form of a path: case and separator style are not identity, and a
/// trailing separator is punctuation the caller did not mean.
pub fn normalized_path_key(path: &str) -> String {
    let unified = normalize_path_text(path);
    strip_trailing_separator(&unified).to_owned()
}

/// How well a full path answers a path query: the place itself, something inside
/// it, something that merely mentions it.
///
/// Steps are compared as whole steps, so the folder `cmd` is not "inside" a query
/// for `cmdline`, and a drive the user did not type does not hide the place.
pub fn path_match_tier(candidate: &str, normalized_query: &str) -> u8 {
    if normalized_query.is_empty() {
        return 9;
    }
    let candidate = normalize_path_text(candidate);
    let candidate = strip_trailing_separator(&candidate);
    if candidate.is_empty() {
        return 9;
    }
    let names_the_place = candidate == normalized_query
        || candidate
            .strip_suffix(normalized_query)
            .is_some_and(|head| head.ends_with('\\'));
    if names_the_place {
        0
    } else if holds_the_place(candidate, normalized_query) {
        1
    } else if candidate.contains(normalized_query) {
        2
    } else {
        9
    }
}

/// True when the query is a run of complete steps somewhere inside the candidate,
/// which is what "a child of that folder" means once a root is prefixed to it.
fn holds_the_place(candidate: &str, query: &str) -> bool {
    format!("\\{candidate}").contains(&format!("\\{query}\\"))
}

fn strip_one_quote_pair(value: &str) -> &str {
    if value.len() > 1 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn has_drive_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    matches!(bytes.get(1), Some(b':')) && bytes[0].is_ascii_alphabetic()
}

fn normalize_path_text(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .to_lowercase()
        .replace('/', "\\")
}

fn strip_trailing_separator(value: &str) -> &str {
    value.trim_end_matches('\\')
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

    /// [`PreparedQuery::matches`] for a candidate whose two forms are already known.
    ///
    /// The three tests and their order are the ones `matches` runs, so a candidate that
    /// was prepared once answers exactly as it would have on every call - the pinned
    /// corpus test is the contract. What it drops is the per-candidate normalisation:
    /// for a catalog re-tested on every letter that was the whole cost of a keystroke.
    pub fn matches_keys(&self, keys: &CandidateKeys) -> bool {
        if self.matches_nothing {
            return false;
        }
        if keys.normalized.contains(&self.normalized) {
            return true;
        }
        if let Some(unified_query) = &self.unified {
            // A separator is a step in a path, not punctuation to forgive: with the
            // compact comparison a query of `d:/` matches every name holding a "d", so
            // the results of the previous keystroke keep answering the new query and
            // stay on screen until the file provider replies.
            return unified_separators(&keys.normalized).contains(unified_query.as_str());
        }
        keys.compact.contains(&self.compact)
    }
}

/// A candidate's two derived forms, computed once instead of per keystroke.
///
/// [`PreparedQuery::matches`] derives exactly these two strings from the candidate and
/// then runs the same three tests on them, so a caller that tests the same candidate on
/// every keystroke - an application catalog, where one letter re-tests every installed
/// program - can pay for them once when it loads and keep only the comparisons after
/// that. The forms are the ones the matcher already builds: the trimmed ASCII-lowercased
/// text, and its alphanumeric-only compact form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CandidateKeys {
    /// `trim()` + `to_ascii_lowercase()`.
    pub normalized: String,
    /// `trim()` + Unicode lowercase, keeping only alphanumerics.
    pub compact: String,
}

/// The two forms of `value` that [`PreparedQuery::matches`] would derive for it.
pub fn candidate_keys(value: &str) -> CandidateKeys {
    let mut keys = CandidateKeys::default();
    normalize_into(value, &mut keys.normalized);
    compact_search_key_into(value, &mut keys.compact);
    keys
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
            "C:\\Tools\\cmd",
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
            "c:\\tools\\cmd",
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

            // The catalog prepares a candidate once and re-uses those forms for every
            // later query, so the prepared forms have to answer exactly as deriving them
            // per query does. This is the negative control for that change: it is what
            // fails if `candidate_keys` ever stops being what `matches` would have built.
            for candidate in candidates {
                let keys = candidate_keys(candidate);
                let mut scratch = String::new();
                assert_eq!(
                    prepared.matches_keys(&keys),
                    prepared.matches(candidate, &mut scratch),
                    "prepared candidate keys disagree with the per-query match on {candidate:?} / {query:?}"
                );
            }
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
    fn path_query_text_accepts_windows_paths_and_refuses_everything_syntax() {
        for path in [
            r"C:\Tools\cmd",
            r"c:/Tools/cmd",
            r"C:\Tools\cmd\",
            "C:",
            r"C:\",
            r"\\nas\media\films",
            r"projectrs\warlocktest",
            "\"C:\\Tools\\cmd\"",
        ] {
            assert!(path_query_text(path).is_some(), "{path} names a place");
        }
        let expected = r"c:\tools\cmd";
        assert_eq!(
            path_query_text(r"C:\Tools\cmd"),
            Some(String::from(expected))
        );
        assert_eq!(
            path_query_text(r"C:\Tools\cmd\"),
            Some(String::from(expected))
        );
        assert_eq!(
            path_query_text("c:/Tools/cmd"),
            Some(String::from(expected))
        );
        assert_eq!(
            path_query_text("\"C:\\Tools\\cmd\""),
            Some(String::from(expected)),
            "quotes are allowed but are not what makes it a path"
        );
        for not_a_path in [
            "cmd",
            "notepad.exe",
            "ext:zip",
            "parent:C:\\Tools",
            "dm:today",
            "size:1mb",
            "regex:^cmd",
            "2026-08",
            "12+34",
            ".mp4",
            "",
            "   ",
        ] {
            assert!(
                path_query_text(not_a_path).is_none(),
                "{not_a_path} is a search, not a place"
            );
        }
    }

    #[test]
    fn the_path_body_keeps_the_spelling_that_has_to_be_displayed() {
        assert_eq!(
            path_query_body(r"C:\Tools\cmd"),
            Some(String::from(r"C:\Tools\cmd"))
        );
        assert_eq!(
            path_query_body("c:/Tools/Cmd/"),
            Some(String::from(r"c:\Tools\Cmd")),
            "separators are unified, spelling is not rewritten"
        );
        assert_eq!(
            path_query_text("c:/Tools/Cmd/"),
            Some(String::from(r"c:\tools\cmd")),
            "only the comparison form loses case"
        );
        assert_eq!(
            normalized_path_key(r"D:\Music\Song.mp4\"),
            r"d:\music\song.mp4"
        );
    }

    #[test]
    fn an_exact_path_beats_its_own_descendants() {
        let query = r"c:\tools\cmd";
        assert_eq!(path_match_tier(r"C:\Tools\cmd", query), 0);
        assert_eq!(path_match_tier(r"c:\tools\cmd\", query), 0);
        assert_eq!(
            path_match_tier(r"c:\tools\cmd\download office.url", query),
            1
        );
        assert_eq!(path_match_tier(r"c:\tools\cmdline.txt", query), 2);
        assert_eq!(path_match_tier(r"d:\media\!LAUNCH", query), 9);
        assert_eq!(path_match_tier("", query), 9);
        assert_eq!(path_match_tier(r"C:\Windows", ""), 9);
    }

    #[test]
    fn a_drive_less_path_finds_the_place_the_user_did_not_name_a_drive_for() {
        let query = r"projectrs\warlocktest";
        assert_eq!(
            path_match_tier(r"C:\Projectrs\warlocktest", query),
            0,
            "the folder itself, whatever drive holds it"
        );
        assert_eq!(path_match_tier(r"D:\Projectrs\warlocktest", query), 0);
        assert_eq!(
            path_match_tier(r"C:\Projectrs\warlocktest\data\db.sqlite", query),
            1
        );
        assert_eq!(
            path_match_tier(r"C:\Projectrs\warlocktest-helper\notes.txt", query),
            2
        );
        assert_eq!(path_match_tier(r"C:\cmd", query), 9);
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
