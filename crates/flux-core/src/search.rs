use crate::search_match::{
    is_application_path, match_app_title, match_file_title, normalize, path_match_tier,
    path_query_text,
};

pub(crate) const MAX_RESULTS: usize = 16;

/// Rank for a row that has nothing to do with the typed path. Every query that is
/// not a path puts all rows here, so their order is exactly what it was before.
const NO_PATH_MATCH: u8 = 9;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Command,
    Application,
    File,
    Placeholder,
}

/// Identifies the provider that produced a result. Flow keeps program search
/// separate from file search; the source tier lets Flux preserve that boundary
/// even when both providers return executable-looking paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultSource {
    BuiltIn,
    ApplicationCatalog,
    Everything,
    Plugin,
    /// A place resolved by asking the filesystem directly, because the indexer
    /// may not cover the volume it lives on.
    FileSystem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: ResultKind,
    pub source: ResultSource,
    pub target: Option<String>,
}

impl SearchResult {
    pub fn file(path: String, title: String, subtitle: String) -> Self {
        let is_application = is_application_path(&path);
        let display_title = if is_application {
            title
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_owned())
                .unwrap_or(title)
        } else {
            title
        };
        Self {
            id: format!("file:{path}"),
            title: display_title,
            subtitle,
            kind: if is_application {
                ResultKind::Application
            } else {
                ResultKind::File
            },
            source: ResultSource::Everything,
            target: Some(path),
        }
    }

    /// A row for a place that exists on disk, for the volumes an indexer may
    /// never have read.
    ///
    /// A folder reads exactly like a folder row from the file provider - the leaf
    /// as the title, the parent as the subtitle - and an executable keeps the
    /// application shape, so the icon, the row and Enter all behave the way the
    /// user already expects from a search hit.
    pub fn from_existing_path(path: &str) -> Self {
        let place = path.trim().trim_matches('"');
        let steps = place.trim_end_matches(['\\', '/']);
        let leaf = steps.rsplit(['\\', '/']).next().unwrap_or(steps);
        let parent = steps
            .get(..steps.len() - leaf.len())
            .unwrap_or_default()
            .trim_end_matches(['\\', '/']);
        let mut result = Self::file(place.to_owned(), leaf.to_owned(), parent.to_owned());
        result.source = ResultSource::FileSystem;
        result
    }

    pub fn display_text(&self) -> String {
        format!("{}  -  {}", self.title, self.subtitle)
    }

    /// Lower sort key means a result is more useful for the current query.
    /// The place a typed path names comes first, and otherwise applications
    /// deliberately outrank indexed files and folders.
    pub fn relevance(&self, query: &str) -> (u8, u8, u8, String) {
        let (path_tier, priority, _, provider_tier, title_tier, title) =
            self.priority_relevance(query, &[]);
        debug_assert_eq!(priority, 1);
        (path_tier, provider_tier, title_tier, title)
    }

    pub(crate) fn priority_relevance(
        &self,
        query: &str,
        priorities: &[String],
    ) -> (u8, u8, usize, u8, u8, String) {
        let query = normalize(query);
        let title = normalize(&self.title);
        let subtitle = normalize(&self.subtitle);
        // Compared against the full path rather than the leaf: `C:\Tools\cmd` names
        // a place, and the folder itself has to win over its own contents no matter
        // which provider listed it. A query that is not a path leaves every row at
        // the same tier, so nothing else on screen reorders.
        let path_tier = path_query_text(&query).map_or(NO_PATH_MATCH, |path_query| {
            path_match_tier(self.target.as_deref().unwrap_or_default(), &path_query)
        });
        let (provider_tier, title_tier) = match self.source {
            ResultSource::ApplicationCatalog => (0, match_app_title(&title, &query)),
            ResultSource::BuiltIn => (1, match_app_title(&title, &query)),
            ResultSource::Plugin => (2, match_app_title(&title, &query)),
            ResultSource::Everything | ResultSource::FileSystem => match self.kind {
                ResultKind::Application => (3, match_app_title(&title, &query)),
                ResultKind::Command | ResultKind::Placeholder | ResultKind::File => {
                    (4, match_file_title(&title, &query))
                }
            },
        };
        let subtitle_match = if !query.is_empty() && subtitle.contains(&query) {
            0
        } else {
            1
        };
        let priority = if matches!(self.kind, ResultKind::Application) {
            priorities
                .iter()
                .position(|id| id == &self.id)
                .map(|index| index.saturating_add(1))
        } else {
            None
        };
        let exact_calculator = self.id == "builtin:calculator";
        let exact_shell_command = (self.id == "system:command-prompt"
            && matches!(query.as_str(), "cmd" | "command prompt" | "command line"))
            || (self.id == "system:powershell"
                && matches!(query.as_str(), "powershell" | "pwsh" | "power shell"));
        (
            path_tier,
            if exact_calculator || exact_shell_command {
                0
            } else {
                priority.map_or(1, |_| 0)
            },
            if exact_calculator || exact_shell_command {
                0
            } else {
                priority.unwrap_or_default()
            },
            provider_tier,
            title_tier.saturating_add(subtitle_match),
            title,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank_results;

    #[test]
    fn shortcut_and_executable_paths_are_applications() {
        assert_eq!(
            SearchResult::file(
                String::from("C:/Users/Test/Google Chrome.lnk"),
                String::from("Google Chrome.lnk"),
                String::new(),
            )
            .kind,
            ResultKind::Application
        );
    }

    #[test]
    fn from_existing_path_shapes_a_folder_row_like_a_provider_row() {
        let row = SearchResult::from_existing_path(r"C:\Tools\cmd");
        assert_eq!(row.title, "cmd");
        assert_eq!(row.subtitle, r"C:\Tools");
        assert_eq!(row.kind, ResultKind::File);
        assert_eq!(row.source, ResultSource::FileSystem);
        assert_eq!(row.target.as_deref(), Some(r"C:\Tools\cmd"));
    }

    #[test]
    fn from_existing_path_for_a_url_is_an_application() {
        let row = SearchResult::from_existing_path(r"C:\Tools\cmd\download office.url");
        assert_eq!(row.kind, ResultKind::Application);
        assert_eq!(row.title, "download office");
        assert_eq!(row.subtitle, r"C:\Tools\cmd");
    }

    #[test]
    fn from_existing_path_keeps_a_bare_drive_readable() {
        let row = SearchResult::from_existing_path(r"C:\");
        assert_eq!(row.title, "C:");
        assert_eq!(row.subtitle, "");
        assert_eq!(
            row.target.as_deref(),
            Some(r"C:\"),
            "a drive root has to stay a root, not the drive-relative `C:` a stat reads as a different place"
        );
    }

    #[test]
    fn the_place_a_path_names_beats_applications_and_its_own_contents() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:chrome"),
                title: String::from("Chrome"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\Program Files\Chrome\chrome.exe")),
            },
            SearchResult::file(
                String::from(r"C:\Tools\cmd\download office.url"),
                String::from("download office.url"),
                String::from(r"C:\Tools\cmd"),
            ),
            SearchResult::from_existing_path(r"C:\Tools\cmd"),
        ];

        rank_results(r"C:\Tools\cmd", &mut results);

        assert_eq!(results[0].target.as_deref(), Some(r"C:\Tools\cmd"));
        assert_eq!(results[1].title, "download office");
        assert_eq!(results[2].id, "application:chrome");
    }

    #[test]
    fn a_word_query_leaves_applications_before_files() {
        // The path tier is the same constant for every row when the query names no
        // place, so the standing invariant - applications outrank indexed files -
        // cannot be moved by this rule.
        let mut results = vec![
            SearchResult::file(
                String::from(r"C:\Library\steam.txt"),
                String::from("steam.txt"),
                String::from(r"C:\Library"),
            ),
            SearchResult {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(r"C:\Steam\steam.exe")),
            },
        ];

        rank_results("steam", &mut results);

        assert_eq!(results[0].source, ResultSource::ApplicationCatalog);
        assert_eq!(results[1].source, ResultSource::Everything);
    }
}
