use crate::applications::{ApplicationResponse, ApplicationWorker};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::everything::{EverythingResponse, EverythingWorker};
use crate::plugins::{
    FlowPluginWorker, NativePluginQueryResponse, NativePluginWorker, PluginAction,
    PluginQueryResponse,
};
use crate::query::{commit_provider_results, ProviderResults};
use flux_core::SearchResult;
use windui::app::App;
use windui::prelude::{Sender, Signal};

pub(crate) struct ProviderChannelContext {
    pub(crate) query: Signal<String>,
    pub(crate) results: Signal<Vec<SearchResult>>,
    pub(crate) inline_completion: Signal<String>,
    pub(crate) status: Signal<String>,
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) selection_touched: Signal<bool>,
    pub(crate) current_sequence: Signal<u64>,
    pub(crate) provider_results: Rc<RefCell<ProviderResults>>,
    pub(crate) priorities: Signal<Vec<flux_core::PriorityEntry>>,
    pub(crate) plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    pub(crate) everything_enabled: Signal<bool>,
    pub(crate) everything_installed: Signal<bool>,
    pub(crate) everything_status: Signal<String>,
}

pub(crate) struct ProviderChannels {
    pub(crate) application_sender: Sender<ApplicationResponse>,
    pub(crate) everything_sender: Sender<EverythingResponse>,
    pub(crate) plugin_sender: Sender<PluginQueryResponse>,
    pub(crate) native_sender: Sender<NativePluginQueryResponse>,
}

pub(crate) fn register_provider_channels(
    app: &mut App,
    context: ProviderChannelContext,
) -> ProviderChannels {
    let application_sender = register_application_channel(app, &context);
    let everything_sender = register_everything_channel(app, &context);
    let plugin_sender = register_plugin_channel(app, &context);
    let native_sender = register_native_plugin_channel(app, &context);
    ProviderChannels {
        application_sender,
        everything_sender,
        plugin_sender,
        native_sender,
    }
}

fn register_application_channel(
    app: &mut App,
    context: &ProviderChannelContext,
) -> Sender<ApplicationResponse> {
    let query = context.query;
    let results = context.results;
    let inline_completion = context.inline_completion;
    let status = context.status;
    let selected_id = context.selected_id;
    let selected_index = context.selected_index;
    let selection_touched = context.selection_touched;
    let current_sequence = context.current_sequence;
    let provider_results = Rc::clone(&context.provider_results);
    let priorities = context.priorities;
    app.channel::<ApplicationResponse>(move |_, response| {
        if response.sequence != current_sequence.get() || response.query != query.get() {
            return;
        }
        let mut providers = provider_results.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.applications = response.results;
        providers.applications_ready = true;
        if providers.core_ready() {
            let priority_ids = priorities
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query.get(),
                &priority_ids,
                selected_id,
                selected_index,
                selection_touched,
                inline_completion,
                results,
            );
        }
        status.set(response.status);
    })
}

fn register_everything_channel(
    app: &mut App,
    context: &ProviderChannelContext,
) -> Sender<EverythingResponse> {
    let query = context.query;
    let results = context.results;
    let inline_completion = context.inline_completion;
    let status = context.status;
    let selected_id = context.selected_id;
    let selected_index = context.selected_index;
    let selection_touched = context.selection_touched;
    let current_sequence = context.current_sequence;
    let provider_results = Rc::clone(&context.provider_results);
    let priorities = context.priorities;
    let everything_enabled = context.everything_enabled;
    let everything_installed = context.everything_installed;
    let everything_status = context.everything_status;
    app.channel::<EverythingResponse>(move |_, response| {
        if !everything_enabled.get() {
            everything_status.set(t!("everything.search_disabled").into_owned());
            return;
        }
        if response.sequence != current_sequence.get()
            || response.query != crate::everything::normalize_everything_query(&query.get())
        {
            return;
        }
        let mut providers = provider_results.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.everything_ready = true;
        if response.available {
            everything_installed.set(true);
            everything_status.set(t!("everything.ipc_available").into_owned());
            providers.everything = response.results;
        } else if everything_installed.get() {
            everything_status.set(t!("everything.ipc_unavailable").into_owned());
        } else {
            everything_status.set(t!("everything.not_installed_winget").into_owned());
        }
        if providers.core_ready() {
            let priority_ids = priorities
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query.get(),
                &priority_ids,
                selected_id,
                selected_index,
                selection_touched,
                inline_completion,
                results,
            );
        }
        status.set(response.status);
    })
}

fn register_plugin_channel(
    app: &mut App,
    context: &ProviderChannelContext,
) -> Sender<PluginQueryResponse> {
    let query = context.query;
    let results = context.results;
    let inline_completion = context.inline_completion;
    let status = context.status;
    let selected_id = context.selected_id;
    let selected_index = context.selected_index;
    let selection_touched = context.selection_touched;
    let current_sequence = context.current_sequence;
    let provider_results = Rc::clone(&context.provider_results);
    let priorities = context.priorities;
    let actions = Rc::clone(&context.plugin_actions);
    app.channel::<PluginQueryResponse>(move |_, response| {
        if response.sequence != current_sequence.get() || response.query != query.get() {
            return;
        }
        let mut providers = provider_results.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        if response.available {
            providers.plugins = response.results;
            *actions.borrow_mut() = response.actions;
            if providers.core_ready() {
                let priority_ids = priorities
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>();
                commit_provider_results(
                    &providers,
                    &query.get(),
                    &priority_ids,
                    selected_id,
                    selected_index,
                    selection_touched,
                    inline_completion,
                    results,
                );
            }
        }
        status.set(response.status);
    })
}

fn register_native_plugin_channel(
    app: &mut App,
    context: &ProviderChannelContext,
) -> Sender<NativePluginQueryResponse> {
    let query = context.query;
    let results = context.results;
    let inline_completion = context.inline_completion;
    let status = context.status;
    let selected_id = context.selected_id;
    let selected_index = context.selected_index;
    let selection_touched = context.selection_touched;
    let current_sequence = context.current_sequence;
    let provider_results = Rc::clone(&context.provider_results);
    let priorities = context.priorities;
    let actions = Rc::clone(&context.plugin_actions);
    app.channel::<NativePluginQueryResponse>(move |_, response| {
        if response.sequence != current_sequence.get() || response.query != query.get() {
            return;
        }
        let mut providers = provider_results.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        let has_native_results = !response.results.is_empty();
        providers.native_plugins = response.results;
        if response.available {
            actions.borrow_mut().extend(response.actions);
            if has_native_results {
                status.set(response.status.clone());
            }
        }
        if providers.core_ready() {
            let priority_ids = priorities
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query.get(),
                &priority_ids,
                selected_id,
                selected_index,
                selection_touched,
                inline_completion,
                results,
            );
        }
    })
}

pub(crate) struct ProviderWorkers {
    pub(crate) applications: ApplicationWorker,
    pub(crate) everything: EverythingWorker,
    pub(crate) plugins: FlowPluginWorker,
    pub(crate) native_plugins: NativePluginWorker,
}

impl ProviderWorkers {
    pub(crate) fn new(
        application_sender: Sender<ApplicationResponse>,
        everything_sender: Sender<EverythingResponse>,
        plugin_sender: Sender<PluginQueryResponse>,
        native_plugin_sender: Sender<NativePluginQueryResponse>,
    ) -> Self {
        Self {
            applications: ApplicationWorker::spawn(application_sender),
            everything: EverythingWorker::spawn(everything_sender),
            plugins: FlowPluginWorker::spawn(plugin_sender),
            native_plugins: NativePluginWorker::spawn(native_plugin_sender),
        }
    }
}
