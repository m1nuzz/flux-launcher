const MAX_RESULTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Command,
    Placeholder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub id: &'static str,
    pub title: String,
    pub subtitle: String,
    pub kind: ResultKind,
}

impl SearchResult {
    pub fn display_text(&self) -> String {
        format!("{}  -  {}", self.title, self.subtitle)
    }
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
            id,
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            kind: ResultKind::Command,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_query_shows_bounded_command_palette() {
        let model = SearchModel::new();
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
    fn external_results_are_truncated_and_selection_stays_valid() {
        let mut model = SearchModel::new();
        model.select_next();
        model.replace_results(
            (0..16)
                .map(|index| SearchResult {
                    id: "fixture",
                    title: format!("Result {index}"),
                    subtitle: String::new(),
                    kind: ResultKind::Placeholder,
                })
                .collect(),
        );
        assert_eq!(model.results().len(), MAX_RESULTS);
        assert!(model.selected().is_some());
    }
}
