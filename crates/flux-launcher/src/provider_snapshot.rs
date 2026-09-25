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
/// new non-empty query. Immediate publication is safe for the home page (empty
/// query clears) and for the first paint (nothing displayed yet), but swapping
/// in a builtins-only list on every keystroke and replacing it again on the
/// applications commit flashes the whole list twice per keystroke. The
/// applications scan is local and lands milliseconds later, so holding the
/// previous list is strictly calmer.
pub(crate) fn should_publish_initial_query_results(
    has_query: bool,
    displayed_results_are_empty: bool,
) -> bool {
    !has_query || displayed_results_are_empty
}

#[derive(Default)]
pub(crate) struct ProviderResults {
    pub(crate) sequence: u64,
    /// The query the visible list was last published for. The list is held back
    /// while a new generation is in flight, so this is what lets a key handler
    /// notice that the rows on screen belong to an earlier keystroke. The Ctrl+H
    /// list clears it: those rows belong to no search generation.
    pub(crate) published_query: String,
    pub(crate) built_in: Vec<SearchResult>,
    pub(crate) applications: Vec<SearchResult>,
    pub(crate) everything: Vec<SearchResult>,
    pub(crate) plugins: Vec<SearchResult>,
    pub(crate) native_plugins: Vec<SearchResult>,
    pub(crate) applications_ready: bool,
    pub(crate) everything_ready: bool,
    /// Set while the user is actively typing (the tick clears it once the query has
    /// been quiet). A commit that arrives during typing for the query already on
    /// screen is deferred instead of published, so one keystroke never rebuilds the
    /// whole row tree twice.
    pub(crate) typing_active: bool,
    /// A complete snapshot is waiting in the provider vectors for the next quiet
    /// tick, which publishes it.
    pub(crate) pending_publish: bool,
    /// The list on screen for `published_query` already contains provider rows.
    /// A built-in-only publish does not count: holding back the first real results
    /// for a keystroke would make a dead-end prefix show system commands until the
    /// next pause, which is a delay, not a saved repaint.
    pub(crate) published_providers: bool,
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
        // A reset only happens on a keystroke, and any deferred snapshot belongs
        // to the query that was just replaced.
        self.typing_active = true;
        self.pending_publish = false;
        self.published_providers = false;
    }

    pub(crate) fn core_ready(&self) -> bool {
        if !self.applications_ready {
            return false;
        }
        // Built-in/system results must be actionable without waiting for the
        // asynchronous Everything response. When a query has no built-in result,
        // retain the atomic application+Everything snapshot behavior: publishing
        // the applications half first means two row-tree rebuilds per keystroke,
        // and the second one repaints every row under the stable top hit.
        self.everything_ready || !self.built_in.is_empty()
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
    providers: &mut ProviderResults,
    query: &str,
    priorities: &[String],
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    results: Signal<Vec<SearchResult>>,
    history_mode: Signal<bool>,
) {
    if history_mode.get() {
        // The Ctrl+H list owns the visible rows. Publishing here would swap the
        // history rows out from under the arrow keys while the user is still
        // navigating them, so only record that the screen no longer matches this
        // query: the providers keep their data and the next key handler
        // republishes it once history mode ends.
        providers.published_query.clear();
        return;
    }
    let merged = providers.merged(query, priorities);
    if providers.typing_active
        && providers.published_query == query
        && providers.published_providers
    {
        // This keystroke is already on screen and the user is still typing. A
        // second publish for the same query would rebuild every row again - the
        // visible full-list flash - so the snapshot waits in the provider vectors
        // and the next quiet tick publishes it once.
        providers.pending_publish = true;
        super::paint_trace::note(
            "list-deferred",
            &format!("query={query} rows={}", merged.len()),
        );
        return;
    }
    providers.pending_publish = false;
    providers.published_query = query.to_owned();
    providers.published_providers = true;
    // distinctUntilChanged: an identical snapshot must not rebuild the row
    // tree or jump the highlight, so write each signal only when its value
    // would actually change. Element and order both matter: a reorder alone
    // still needs a publish.
    let first_id = merged
        .first()
        .map(|result| result.id.clone())
        .unwrap_or_default();
    if !selection_touched.get() {
        if selected_index.get() != 0 {
            selected_index.set(0);
        }
        if selected_id.get() != first_id {
            selected_id.set(first_id);
        }
    } else if let Some(position) = merged
        .iter()
        .position(|result| result.id == selected_id.get())
    {
        // A recalled row is chosen by id, so keep the index pointing at it: the
        // viewport and Enter must agree with what the highlight shows.
        if selected_index.get() != position {
            selected_index.set(position);
        }
    } else {
        // The remembered id is gone - a provider dropped it or the list was
        // truncated. Follow the clamped index instead of leaving a highlight
        // that matches no visible row.
        let index = if merged.is_empty() {
            0
        } else {
            selected_index.get().min(merged.len() - 1)
        };
        if selected_index.get() != index {
            selected_index.set(index);
        }
        let id = merged
            .get(index)
            .map(|result| result.id.clone())
            .unwrap_or_default();
        if selected_id.get() != id {
            selected_id.set(id);
        }
    }
    if merged != results.get() {
        super::paint_trace::note(
            "list-write",
            &format!(
                "query={query} rows={} shown={}",
                merged.len(),
                results.get().len()
            ),
        );
        results.set(merged);
    }
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
    // This is a publish too: recording it keeps a later keystroke from treating
    // the freshly written list as stale, and it consumes any deferred snapshot.
    let mut providers = providers.borrow_mut();
    providers.published_query = query.get();
    providers.pending_publish = false;
    drop(providers);
    results.set(merged);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::{ResultKind, ResultSource};

    #[test]
    fn pending_non_empty_query_keeps_previous_result_list_visible() {
        assert!(!should_publish_initial_query_results(true, false));
        assert!(should_publish_initial_query_results(true, true));
        assert!(should_publish_initial_query_results(false, false));
    }

    #[test]
    fn builtin_only_snapshot_waits_for_apps_commit_when_list_shown() {
        // Even with synchronous built-in results ready, a shown list is left
        // alone: the applications commit lands milliseconds later and swaps
        // once instead of flashing builtins-then-full.
        assert!(!should_publish_initial_query_results(true, false));
        assert!(should_publish_initial_query_results(false, true));
    }

    #[test]
    fn core_provider_snapshot_waits_for_both_search_providers() {
        let mut providers = ProviderResults::default();
        providers.reset(7, Vec::new(), true);
        assert!(!providers.core_ready());

        providers.applications_ready = true;
        // Held even while the shown list belongs to an earlier keystroke:
        // publishing the applications half means two row-tree rebuilds for this
        // query, one when applications land and one when Everything does.
        providers.published_query = String::from("older-keystroke");
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
    fn commit_records_the_query_the_published_list_belongs_to() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:perplexity"),
            title: String::from("Perplexity"),
            subtitle: String::new(),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(1, Vec::new(), false);
        providers.applications = vec![app];
        providers.applications_ready = true;
        // Nothing published yet: a key handler must treat any shown list as
        // belonging to a different query.
        assert!(providers.published_query.is_empty());
        commit_provider_results(
            &mut providers,
            "perp",
            &[],
            signal(String::new()),
            signal(0_usize),
            signal(false),
            signal(Vec::new()),
            signal(false),
        );
        assert_eq!(providers.published_query, "perp");
    }

    #[test]
    fn history_list_survives_a_provider_commit() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:perplexity"),
            title: String::from("Perplexity"),
            subtitle: String::new(),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let history_row = SearchResult {
            id: String::from("history:perp"),
            title: String::from("perp"),
            subtitle: String::new(),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(1, Vec::new(), false);
        providers.applications = vec![app];
        providers.applications_ready = true;
        providers.published_query = String::from("perp");
        let results = signal(vec![history_row]);
        let version = results.version();
        commit_provider_results(
            &mut providers,
            "perp",
            &[],
            signal(String::from("history:perp")),
            signal(0_usize),
            signal(false),
            results,
            signal(true),
        );
        // The rows the arrow keys are walking must not be swapped out mid-list,
        // and the screen must be marked as belonging to no search generation so
        // the real list returns as soon as history mode closes.
        assert_eq!(results.version(), version, "history rows must stay");
        assert!(providers.published_query.is_empty());
    }

    #[test]
    fn a_second_publish_for_the_same_query_waits_while_the_user_types() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:chatgpt"),
            title: String::from("ChatGPT"),
            subtitle: String::from("Application"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let file = SearchResult {
            id: String::from("everything:chatgpt.md"),
            title: String::from("chatgpt.md"),
            subtitle: String::from("Documents"),
            kind: ResultKind::File,
            source: ResultSource::Everything,
            target: Some(String::from("C:\\Docs\\chatgpt.md")),
        };
        let mut providers = ProviderResults::default();
        providers.reset(1, Vec::new(), true);
        providers.applications = vec![app.clone()];
        providers.applications_ready = true;
        // The keystroke is already on screen with the application snapshot.
        providers.published_query = String::from("chatgpt");
        providers.published_providers = true;
        let results = signal(vec![app]);
        let version = results.version();
        commit_provider_results(
            &mut providers,
            "chatgpt",
            &[],
            signal(String::new()),
            signal(0_usize),
            signal(false),
            results,
            signal(false),
        );
        assert_eq!(
            results.version(),
            version,
            "the row tree must not be rebuilt a second time for one keystroke"
        );
        assert!(providers.pending_publish, "the file rows stay queued");

        // The quiet tick clears the typing flag, and the same commit now lands.
        providers.typing_active = false;
        providers.everything = vec![file];
        providers.everything_ready = true;
        commit_provider_results(
            &mut providers,
            "chatgpt",
            &[],
            signal(String::new()),
            signal(0_usize),
            signal(false),
            results,
            signal(false),
        );
        assert_ne!(
            results.version(),
            version,
            "the queued snapshot is published"
        );
        assert!(!providers.pending_publish);
    }

    #[test]
    fn a_builtin_only_publish_does_not_hold_back_the_first_provider_results() {
        use windui::signal::signal;
        let command = SearchResult {
            id: String::from("system:clear"),
            title: String::from("Clear"),
            subtitle: String::new(),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: None,
        };
        let app = SearchResult {
            id: String::from("app:chatgpt"),
            title: String::from("ChatGPT"),
            subtitle: String::from("Application"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(5, vec![command.clone()], true);
        providers.applications = vec![app];
        providers.applications_ready = true;
        // The tick published the built-in list for this very keystroke, which is
        // what the list looked like after a prefix that returned nothing.
        providers.published_query = String::from("chatgpt");
        providers.published_providers = false;
        providers.typing_active = true;
        let results = signal(vec![command]);
        let version = results.version();
        commit_provider_results(
            &mut providers,
            "chatgpt",
            &[],
            signal(String::new()),
            signal(0_usize),
            signal(false),
            results,
            signal(false),
        );
        assert_ne!(
            results.version(),
            version,
            "real results must appear on this keystroke, not after a pause"
        );
        assert!(providers.published_providers);
        assert!(!providers.pending_publish);
    }

    #[test]
    fn the_first_snapshot_of_a_generation_publishes_even_mid_typing() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:chatgpt"),
            title: String::from("ChatGPT"),
            subtitle: String::from("Application"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(2, Vec::new(), true);
        providers.applications = vec![app.clone()];
        providers.applications_ready = true;
        providers.published_query = String::from("chatgp");
        providers.typing_active = true;
        let results = signal(Vec::new());
        let version = results.version();
        commit_provider_results(
            &mut providers,
            "chatgpt",
            &[],
            signal(String::new()),
            signal(0_usize),
            signal(false),
            results,
            signal(false),
        );
        assert_ne!(
            results.version(),
            version,
            "the new keystroke must show at once"
        );
        assert_eq!(providers.published_query, "chatgpt");
    }

    #[test]
    fn a_new_keystroke_drops_a_snapshot_deferred_by_the_previous_one() {
        let mut providers = ProviderResults::default();
        providers.reset(3, Vec::new(), true);
        providers.pending_publish = true;
        providers.reset(4, Vec::new(), true);
        assert!(!providers.pending_publish);
        assert!(providers.typing_active);
    }

    #[test]
    fn identical_commit_writes_no_signals() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:perplexity"),
            title: String::from("Perplexity"),
            subtitle: String::new(),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(1, Vec::new(), false);
        providers.applications = vec![app.clone()];
        providers.applications_ready = true;
        let selected_id = signal(String::from("app:perplexity"));
        let selected_index = signal(0_usize);
        let selection_touched = signal(false);
        let results = signal(vec![app]);
        let (v_results, v_id, v_idx) = (
            results.version(),
            selected_id.version(),
            selected_index.version(),
        );
        commit_provider_results(
            &mut providers,
            "perp",
            &[],
            selected_id,
            selected_index,
            selection_touched,
            results,
            signal(false),
        );
        // Same elements, same order, same selection: no signal may be
        // re-set, otherwise the row tree rebuilds and the frame shimmers.
        assert_eq!(results.version(), v_results, "results must not re-set");
        assert_eq!(selected_id.version(), v_id, "selection must not re-set");
        assert_eq!(selected_index.version(), v_idx, "index must not re-set");
    }

    #[test]
    fn changed_commit_reselects_first_result() {
        use windui::signal::signal;
        let app = SearchResult {
            id: String::from("app:perplexity"),
            title: String::from("Perplexity"),
            subtitle: String::new(),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: None,
        };
        let mut providers = ProviderResults::default();
        providers.reset(1, Vec::new(), false);
        providers.applications = vec![app.clone()];
        providers.applications_ready = true;
        let selected_id = signal(String::from("stale-id"));
        let selected_index = signal(3_usize);
        let selection_touched = signal(false);
        let results = signal(Vec::new());
        commit_provider_results(
            &mut providers,
            "perp",
            &[],
            selected_id,
            selected_index,
            selection_touched,
            results,
            signal(false),
        );
        assert_eq!(selected_id.get(), "app:perplexity");
        assert_eq!(selected_index.get(), 0);
        assert_eq!(results.get(), vec![app]);
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
