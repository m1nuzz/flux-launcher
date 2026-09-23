use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::applications::ApplicationWorker;
use crate::everything::EverythingWorker;
use crate::plugins::{FlowPluginWorker, NativePluginWorker, PluginAction};
use crate::query::{
    normalize_built_in_executable_targets, should_publish_initial_query_results, ProviderResults,
};
use flux_core::{SearchModel, SearchResult};
use rust_i18n::t;
use windui::prelude::Signal;

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_query(
    model: &mut SearchModel,
    sequence: &mut u64,
    query: &str,
    has_query: bool,
    auto_enable_everything: Signal<bool>,
    obsidian_enabled: Signal<bool>,
    obsidian_alias: Signal<String>,
    google_enabled: Signal<bool>,
    google_alias: Signal<String>,
    system_commands_enabled: Signal<bool>,
    current_results: Signal<Vec<SearchResult>>,
    provider_results: Rc<RefCell<ProviderResults>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    inline_completion: Signal<String>,
    scroll_pending: Signal<bool>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<crate::actions::ActionItem>>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    status: Signal<String>,
    ready_status: Signal<String>,
    application_worker: &ApplicationWorker,
    everything_worker: &EverythingWorker,
    plugin_worker: &FlowPluginWorker,
    native_plugin_worker: &NativePluginWorker,
) {
    *sequence = sequence.wrapping_add(1);
    let current_sequence = *sequence;
    model.set_query(query);
    let mut built_in_results = model.results().to_vec();
    normalize_built_in_executable_targets(&mut built_in_results);
    let everything_expected =
        auto_enable_everything.get() && query.trim().len() >= crate::EVERYTHING_MIN_QUERY_LEN;
    {
        let mut providers = provider_results.borrow_mut();
        providers.reset(
            current_sequence,
            built_in_results.clone(),
            everything_expected,
        );
        let publish_initial_results = should_publish_initial_query_results(
            has_query,
            built_in_results.is_empty(),
            current_results.get().is_empty(),
        );
        if publish_initial_results {
            selection_touched.set(false);
            selected_index.set(0);
            selected_id.set(
                built_in_results
                    .first()
                    .map(|result| result.id.clone())
                    .unwrap_or_default(),
            );
            current_results.set(built_in_results);
        }
        inline_completion.set(String::new());
    }
    scroll_pending.set(true);
    action_mode.set(false);
    action_index.set(0);
    action_items.set(Vec::new());
    plugin_actions.borrow_mut().clear();
    if !has_query {
        inline_completion.set(String::new());
        status.set(ready_status.get());
        return;
    }
    status.set(t!("status.searching").into_owned());
    application_worker.request(current_sequence, query.to_owned());
    if auto_enable_everything.get() && query.trim().len() >= crate::EVERYTHING_MIN_QUERY_LEN {
        everything_worker.request(
            current_sequence,
            crate::everything::normalize_everything_query(query),
        );
    }
    if query.trim().len() >= crate::PLUGIN_MIN_QUERY_LEN {
        plugin_worker.request(
            current_sequence,
            query.to_owned(),
            crate::plugins::BuiltinPluginFlags {
                obsidian_enabled: obsidian_enabled.get(),
                obsidian_keyword: obsidian_alias.get(),
                google_enabled: google_enabled.get(),
                google_keyword: google_alias.get(),
                system_commands_enabled: system_commands_enabled.get(),
            },
        );
        native_plugin_worker.request(current_sequence, query.to_owned());
    }
}
