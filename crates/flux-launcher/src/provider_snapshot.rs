use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use flux_core::{rank_results_with_priorities, PriorityEntry, SearchResult};
use windui::signal::Signal;

use super::applications::canonical_application_id;
use super::provider_merge::{
    merge_application_duplicates, preserve_everything_file_order, trace_query_probe,
};
use super::ui_constants::MAX_VISIBLE_RESULTS;

/// Keep the previous result list visible while asynchronous providers compute a
/// new non-empty query. Immediate publication is safe for the home page and for
/// actionable synchronous built-in results, but publishing an empty vector for
/// every keystroke creates a visible blank frame and makes the list flicker.
pub(crate) fn should_publish_initial_query_results(
    has_query: bool,
    built_in_results_are_empty: bool,
    displayed_results_are_empty: bool,
) -> bool {
    !has_query || !built_in_results_are_empty || displayed_results_are_empty
}

#[derive(Default)]
pub(crate) struct ProviderResults {
    pub(crate) sequence: u64,
    pub(crate) built_in: Vec<SearchResult>,
    pub(crate) applications: Vec<SearchResult>,
    pub(crate) everything: Vec<SearchResult>,
    pub(crate) plugins: Vec<SearchResult>,
    pub(crate) native_plugins: Vec<SearchResult>,
    pub(crate) applications_ready: bool,
    pub(crate) everything_ready: bool,
}

impl ProviderResults {
    pub(crate) fn reset(
        &mut self,
        sequence: u64,
        built_in: Vec<SearchResult>,
        everything_expected: bool,
    ) {
        self.sequence = sequence;
        self.built_in = built_in;
        self.applications.clear();
        self.everything.clear();
        self.plugins.clear();
        self.native_plugins.clear();
        self.applications_ready = false;
        self.everything_ready = !everything_expected;
    }

    pub(crate) fn core_ready(&self) -> bool {
        // Built-in/system results must be actionable without waiting for the
        // asynchronous Everything response. When a query has no built-in result,
        // retain the atomic application+Everything snapshot behavior.
        self.applications_ready && (self.everything_ready || !self.built_in.is_empty())
    }

    pub(crate) fn merged(&self, query: &str, priorities: &[String]) -> Vec<SearchResult> {
        let mut seen = HashSet::new();
        let collected = self
            .built_in
            .iter()
            .chain(&self.applications)
            .chain(&self.everything)
            .chain(&self.plugins)
            .chain(&self.native_plugins)
            .filter(|result| seen.insert(result.id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_application_duplicates(collected);
        rank_results_with_priorities(query, &mut merged, priorities);
        preserve_everything_file_order(&mut merged, &self.everything);
        merged.truncate(MAX_VISIBLE_RESULTS);
        trace_query_probe(query, &merged);
        merged
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_provider_results(
    providers: &ProviderResults,
    query: &str,
    priorities: &[String],
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    inline_completion: Signal<String>,
    results: Signal<Vec<SearchResult>>,
) {
    let merged = providers.merged(query, priorities);
    if !selection_touched.get() {
        selected_index.set(0);
        selected_id.set(
            merged
                .first()
                .map(|result| result.id.clone())
                .unwrap_or_default(),
        );
    }
    inline_completion.set(super::inline_completion_suffix(query, &merged));
    results.set(merged);
}

pub(crate) fn refresh_merged_results(
    providers: &Rc<RefCell<ProviderResults>>,
    query: Signal<String>,
    priorities: Signal<Vec<PriorityEntry>>,
    results: Signal<Vec<SearchResult>>,
) {
    let priority_ids = priorities
        .get()
        .into_iter()
        .flat_map(|entry| {
            let mut ids = vec![entry.id];
            if let Some(canonical_id) = canonical_application_id(&entry.target) {
                ids.push(canonical_id);
            }
            ids
        })
        .collect::<Vec<_>>();
    let merged = providers.borrow().merged(&query.get(), &priority_ids);
    results.set(merged);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::{ResultKind, ResultSource};

    #[test]
    fn pending_non_empty_query_keeps_previous_result_list_visible() {
        assert!(!should_publish_initial_query_results(true, true, false));
        assert!(should_publish_initial_query_results(true, true, true));
    }

    #[test]
    fn synchronous_built_in_results_can_replace_list_immediately() {
        assert!(should_publish_initial_query_results(true, false, false));
        assert!(should_publish_initial_query_results(false, true, false));
    }

    #[test]
    fn core_provider_snapshot_waits_for_both_search_providers() {
        let mut providers = ProviderResults::default();
        providers.reset(7, Vec::new(), true);
        assert!(!providers.core_ready());

        providers.applications_ready = true;
        assert!(!providers.core_ready());

        providers.everything_ready = true;
        assert!(providers.core_ready());
    }

    #[test]
    fn disabled_everything_does_not_delay_application_snapshot() {
        let mut providers = ProviderResults::default();
        providers.reset(8, Vec::new(), false);
        providers.applications_ready = true;
        assert!(providers.core_ready());
    }

    #[test]
    fn builtin_snapshot_does_not_wait_for_everything() {
        let mut providers = ProviderResults::default();
        providers.reset(
            9,
            vec![SearchResult {
                id: String::from("system:wifi"),
                title: String::from("Wi-Fi"),
                subtitle: String::from("Windows Settings"),
                kind: ResultKind::Command,
                source: ResultSource::BuiltIn,
                target: Some(String::from("ms-settings:network-wifi")),
            }],
            true,
        );
        providers.applications_ready = true;
        assert!(providers.core_ready());
        assert!(!providers.everything_ready);
    }
}
