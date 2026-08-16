const MAX_RESULTS: usize = 16;

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

    pub fn display_text(&self) -> String {
        format!("{}  -  {}", self.title, self.subtitle)
    }

    /// Lower sort key means a result is more useful for the current query.
    /// Applications deliberately outrank indexed files and folders.
    pub fn relevance(&self, query: &str) -> (u8, u8, String) {
        let query = normalize(query);
        let title = normalize(&self.title);
        let subtitle = normalize(&self.subtitle);
        let (provider_tier, title_tier) = match self.source {
            ResultSource::ApplicationCatalog => (0, match_app_title(&title, &query)),
            ResultSource::BuiltIn => (1, match_app_title(&title, &query)),
            ResultSource::Plugin => (2, match_app_title(&title, &query)),
            ResultSource::Everything => match self.kind {
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
        (
            provider_tier,
            title_tier.saturating_add(subtitle_match),
            title,
        )
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn match_app_title(title: &str, query: &str) -> u8 {
    if query.is_empty() || title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else {
        3
    }
}

fn match_file_title(title: &str, query: &str) -> u8 {
    if query.is_empty() {
        3
    } else if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else {
        3
    }
}

fn is_application_path(path: &str) -> bool {
    let lower = normalize(path);
    [".exe", ".lnk", ".com", ".bat", ".cmd", ".url"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub fn rank_results(query: &str, results: &mut [SearchResult]) {
    results.sort_by_key(|result| result.relevance(query));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchModel {
    query: String,
    results: Vec<SearchResult>,
    selected: usize,
}

impl Default for SearchModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchModel {
    pub fn new() -> Self {
        let mut model = Self {
            query: String::new(),
            results: Vec::with_capacity(MAX_RESULTS),
            selected: 0,
        };
        model.set_query("");
        model
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }

    pub fn selected(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.results = built_in_results(&self.query);
        self.selected = 0;
    }

    pub fn replace_results(&mut self, mut results: Vec<SearchResult>) {
        rank_results(&self.query, &mut results);
        results.truncate(MAX_RESULTS);
        self.results = results;
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1) % self.results.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.results.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.results.len().saturating_sub(1));
        }
    }
}

pub fn history_results(history: &[String], query: &str) -> Vec<SearchResult> {
    let normalized = query.trim().to_ascii_lowercase();
    history
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, item)| {
            normalized.is_empty() || item.to_ascii_lowercase().contains(&normalized)
        })
        .take(MAX_RESULTS)
        .map(|(index, item)| SearchResult {
            id: format!("history:{index}"),
            title: item.clone(),
            subtitle: String::from("Previous search"),
            kind: ResultKind::Placeholder,
            source: ResultSource::BuiltIn,
            target: None,
        })
        .collect()
}

fn built_in_results(query: &str) -> Vec<SearchResult> {
    let normalized = query.trim().to_ascii_lowercase();
    let commands = [
        ("settings", "Settings", "Configure Flux Launcher"),
        (
            "toggle-game-mode",
            "Toggle Game Mode",
            "Suppress launcher activation",
        ),
        (
            "everything",
            "Search with Everything",
            "Index files and folders",
        ),
        ("plugins", "Plugin directory", "Manage native Flow plugins"),
        ("about", "About Flux Launcher", "Version and diagnostics"),
    ];

    commands
        .into_iter()
        .filter(|(id, title, subtitle)| {
            normalized.is_empty()
                || id.contains(&normalized)
                || title.to_ascii_lowercase().contains(&normalized)
                || subtitle.to_ascii_lowercase().contains(&normalized)
        })
        .take(MAX_RESULTS)
        .map(|(id, title, subtitle)| SearchResult {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_query_shows_bounded_command_palette() {
        let mut model = SearchModel::new();
        model.set_query("");
        assert!(!model.results().is_empty());
        assert!(model.results().len() <= MAX_RESULTS);
        assert_eq!(model.selected_index(), 0);
    }

    #[test]
    fn query_filters_results_case_insensitively() {
        let mut model = SearchModel::new();
        model.set_query("EVERYTHING");
        assert_eq!(model.results().len(), 1);
        assert_eq!(model.selected().unwrap().id, "everything");
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut model = SearchModel::new();
        let last = model.results().len() - 1;
        model.select_previous();
        assert_eq!(model.selected_index(), last);
        model.select_next();
        assert_eq!(model.selected_index(), 0);
    }

    #[test]
    fn history_results_are_newest_first_and_filterable() {
        let history = vec![
            String::from("steam"),
            String::from("ext:zip"),
            String::from("chrome"),
        ];
        let all = history_results(&history, "");
        assert_eq!(all[0].title, "chrome");
        assert_eq!(all[1].title, "ext:zip");
        assert_eq!(history_results(&history, "zip")[0].title, "ext:zip");
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
    fn external_results_are_truncated_and_selection_stays_valid() {
        let mut model = SearchModel::new();
        model.select_next();
        model.replace_results(
            (0..16)
                .map(|index| SearchResult {
                    id: "fixture".to_owned(),
                    title: format!("Result {index}"),
                    subtitle: String::new(),
                    kind: ResultKind::Placeholder,
                    source: ResultSource::BuiltIn,
                    target: None,
                })
                .collect(),
        );
        assert_eq!(model.results().len(), MAX_RESULTS);
        assert!(model.selected().is_some());
    }
}
