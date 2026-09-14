use super::search::{ResultKind, ResultSource, SearchResult, MAX_RESULTS};
use super::search_match::rank_results;
use super::system_results::{built_in_results, system_results};

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
        let mut built_ins = built_in_results(&self.query);
        built_ins.extend(system_results(&self.query));
        rank_results(&self.query, &mut built_ins);
        built_ins.truncate(MAX_RESULTS);
        self.results = built_ins;
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
