use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, SearchResult, Settings};
use windui::app::App;
use windui::prelude::Sender;
use windui::signal::Signal;

use super::applications::{ApplicationResponse, ApplicationWorker};
use super::everything::{self, EverythingResponse, EverythingWorker};
use super::plugins::{
    FlowPluginWorker, NativePluginQueryResponse, NativePluginWorker, PluginAction,
    PluginQueryResponse,
};
use super::provider_snapshot::{commit_provider_results, ProviderResults};
use super::theme_text::normalize_everything_query;
use super::ui_constants::CURRENT_VERSION;
use super::update_tasks::format_update_progress;
use super::update_tasks::{
    relaunch_mode_for_auto_install, request_update_install, save_settings, UpdateInstallResponse,
};
use super::updater;

pub(crate) struct UpdateChannels {
    pub(crate) install_sender: Sender<UpdateInstallResponse>,
    pub(crate) check_sender: Sender<updater::UpdateCheckResponse>,
    pub(crate) install_in_flight: Rc<Cell<bool>>,
    pub(crate) check_in_flight: Rc<Cell<bool>>,
}

pub(crate) fn register_update_channels(
    app: &mut App,
    update_status: Signal<String>,
    update_available: Signal<Option<updater::StableUpdate>>,
    update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    update_installing: Signal<bool>,
    shared_settings: Arc<RwLock<Settings>>,
) -> UpdateChannels {
    let update_status_for_channel = update_status;
    let update_available_for_channel = update_available;
    let update_install_progress_for_channel = update_install_progress;
    let update_installing_for_channel = update_installing;
    let update_install_in_flight = Rc::new(Cell::new(false));
    let update_install_in_flight_for_channel = Rc::clone(&update_install_in_flight);
    let update_install_sender =
        app.channel::<UpdateInstallResponse>(move |ctx, response| match response {
            UpdateInstallResponse::Progress { version, progress } => {
                update_install_progress_for_channel.set(Some((version.clone(), progress.clone())));
                update_status_for_channel.set(format_update_progress(&version, &progress));
            }
            UpdateInstallResponse::Started { version } => {
                update_install_in_flight_for_channel.set(false);
                update_installing_for_channel.set(false);
                update_install_progress_for_channel.set(None);
                update_status_for_channel.set(format!(
                    "Installing stable {version}; Flux Launcher is restarting"
                ));
                ctx.toast_ok(format!("Installing stable {version}"));
                ctx.quit();
            }
            UpdateInstallResponse::Failed { version, error } => {
                update_install_in_flight_for_channel.set(false);
                update_installing_for_channel.set(false);
                update_install_progress_for_channel.set(None);
                update_status_for_channel.set(format!("Stable {version} update failed: {error}"));
                ctx.toast_ok(format!("Update install failed: {error}"));
            }
        });
    let update_install_sender_for_channel = update_install_sender.clone();
    let settings_for_update_channel = Arc::clone(&shared_settings);
    let update_check_in_flight = Rc::new(Cell::new(false));
    let update_check_in_flight_for_channel = Rc::clone(&update_check_in_flight);
    let update_install_in_flight_for_check_channel = Rc::clone(&update_install_in_flight);
    let update_sender = app.channel::<updater::UpdateCheckResponse>(move |ctx, response| {
        update_check_in_flight_for_channel.set(false);
        if let Ok(mut settings) = settings_for_update_channel.write() {
            settings.last_update_check_unix = response.checked_at;
            let _ = save_settings(&settings);
        }
        match response.result {
            Ok(Some(update)) => {
                let message = format!("Stable {} is available", update.version);
                update_status_for_channel.set(message.clone());
                update_available_for_channel.set(Some(update.clone()));
                let auto_install = settings_for_update_channel
                    .read()
                    .map(|settings| settings.auto_install_updates)
                    .unwrap_or(false);
                if auto_install {
                    let relaunch_mode = relaunch_mode_for_auto_install();
                    update_installing_for_channel.set(true);
                    update_status_for_channel.set(format!(
                        "Preparing stable {} for installation...",
                        update.version
                    ));
                    if !request_update_install(
                        update,
                        update_install_sender_for_channel.clone(),
                        &update_install_in_flight_for_check_channel,
                        relaunch_mode,
                    ) {
                        update_installing_for_channel.set(false);
                        update_status_for_channel
                            .set(String::from("An update is already being installed"));
                    }
                } else {
                    ctx.toast_ok(message);
                }
            }
            Ok(None) => {
                update_available_for_channel.set(None);
                update_status_for_channel.set(format!("Flux {CURRENT_VERSION} is up to date"));
            }
            Err(error) => {
                update_status_for_channel.set(format!("Stable update check failed: {error}"));
            }
        }
    });
    UpdateChannels {
        install_sender: update_install_sender,
        check_sender: update_sender,
        install_in_flight: update_install_in_flight,
        check_in_flight: update_check_in_flight,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_application_pipeline(
    app: &mut App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    inline_completion: Signal<String>,
    status: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    priorities: Signal<Vec<PriorityEntry>>,
    history_mode: Signal<bool>,
) -> ApplicationWorker {
    let query_for_applications = query;
    let results_for_applications = results;
    let inline_completion_for_applications = inline_completion;
    let status_for_applications = status;
    let selected_id_for_applications = selected_id;
    let selected_index_for_applications = selected_index;
    let selection_touched_for_applications = selection_touched;
    let sequence_for_applications = current_sequence;
    let providers_for_applications = Rc::clone(&providers);
    let priorities_for_applications = priorities;
    let history_mode_for_applications = history_mode;
    let application_sender = app.channel::<ApplicationResponse>(move |_, response| {
        if response.sequence != sequence_for_applications.get()
            || response.query != query_for_applications.get()
        {
            return;
        }
        let mut providers = providers_for_applications.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.applications = response.results;
        providers.applications_ready = true;
        if providers.core_ready() {
            super::paint_trace::note(
                "commit-applications",
                &format!(
                    "query={} rows={}",
                    query_for_applications.get(),
                    providers.built_in.len() + providers.applications.len()
                ),
            );
            let priorities = priorities_for_applications
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &mut providers,
                &query_for_applications.get(),
                &priorities,
                selected_id_for_applications,
                selected_index_for_applications,
                selection_touched_for_applications,
                results_for_applications,
                history_mode_for_applications,
            );
            // Ghost completion has a single writer per query generation (this
            // pipeline). Refreshing it in every pipeline would flip the hint
            // as each provider's commit reshuffles the merged top.
            let ghost_selected = selected_id_for_applications.get();
            super::refresh_inline_completion(
                inline_completion_for_applications,
                &query_for_applications.get(),
                &providers.applications,
                &ghost_selected,
            );
        }
        status_for_applications.set(response.status);
    });
    ApplicationWorker::spawn(application_sender)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_everything_pipeline(
    app: &mut App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    inline_completion: Signal<String>,
    status: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    priorities: Signal<Vec<PriorityEntry>>,
    history_mode: Signal<bool>,
    auto_enable_everything: Signal<bool>,
    everything_installed: Signal<bool>,
    everything_status: Signal<String>,
    settings_auto_enable: bool,
) -> EverythingWorker {
    let query_for_everything = query;
    let results_for_everything = results;
    let _inline_completion_for_everything = inline_completion;
    let status_for_everything = status;
    let selected_id_for_everything = selected_id;
    let selected_index_for_everything = selected_index;
    let selection_touched_for_everything = selection_touched;
    let sequence_for_everything = current_sequence;
    let providers_for_everything = Rc::clone(&providers);
    let priorities_for_everything = priorities;
    let history_mode_for_everything = history_mode;
    let auto_enable_everything_for_response = auto_enable_everything;
    let everything_installed_for_response = everything_installed;
    let everything_status_for_response = everything_status;
    let everything_sender = app.channel::<EverythingResponse>(move |_, response| {
        if !auto_enable_everything_for_response.get() {
            everything_status_for_response.set(String::from(
                "Everything auto-enable is disabled in Flux settings",
            ));
            return;
        }
        if response.sequence != sequence_for_everything.get()
            || response.query != normalize_everything_query(&query_for_everything.get())
        {
            return;
        }
        let mut providers = providers_for_everything.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.everything_ready = true;
        if response.available {
            everything_installed_for_response.set(true);
            everything_status_for_response.set(String::from("Everything IPC is available"));
            providers.everything = response.results;
        } else if everything_installed_for_response.get() {
            everything_status_for_response.set(String::from(
                "Everything is installed but its local IPC is unavailable",
            ));
        } else {
            everything_status_for_response.set(String::from(
                "Everything is not installed. Install it with winget to enable file search.",
            ));
        }
        if providers.core_ready() {
            super::paint_trace::note(
                "commit-everything",
                &format!(
                    "query={} rows={}",
                    query_for_everything.get(),
                    providers.built_in.len() + providers.applications.len()
                ),
            );
            let priorities = priorities_for_everything
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &mut providers,
                &query_for_everything.get(),
                &priorities,
                selected_id_for_everything,
                selected_index_for_everything,
                selection_touched_for_everything,
                results_for_everything,
                history_mode_for_everything,
            );
        }
        status_for_everything.set(response.status);
    });
    let worker = EverythingWorker::spawn(everything_sender);
    if settings_auto_enable {
        // Starting the service asks tasklist whether Everything is running and
        // tries the IPC pipe: measured 170-210 ms of blocking, which used to
        // stall the first paint and the hotkey registration at startup. The
        // install state and status text already come from the cheap directory
        // scan in main, and a failed start reports itself through the status on
        // the first query, so this can run on its own thread.
        if let Err(error) = std::thread::Builder::new()
            .name(String::from("flux-everything-start"))
            .spawn(|| match everything::start_background_if_installed() {
                Ok(_) => {}
                Err(error) => eprintln!("Could not start Everything in the background: {error}"),
            })
        {
            eprintln!("Could not start Everything in the background: {error}");
        }
    } else {
        everything_status.set(String::from(
            "Everything auto-enable is disabled in Flux settings",
        ));
    }
    worker
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_plugin_pipeline(
    app: &mut App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    inline_completion: Signal<String>,
    status: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    priorities: Signal<Vec<PriorityEntry>>,
    history_mode: Signal<bool>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
) -> FlowPluginWorker {
    let query_for_plugins = query;
    let results_for_plugins = results;
    let _inline_completion_for_plugins = inline_completion;
    let status_for_plugins = status;
    let selected_id_for_plugins = selected_id;
    let selected_index_for_plugins = selected_index;
    let selection_touched_for_plugins = selection_touched;
    let sequence_for_plugins = current_sequence;
    let providers_for_plugins = Rc::clone(&providers);
    let priorities_for_plugins = priorities;
    let history_mode_for_plugins = history_mode;
    let actions_for_plugins = Rc::clone(&plugin_actions);
    let plugin_sender = app.channel::<PluginQueryResponse>(move |_, response| {
        if response.sequence != sequence_for_plugins.get()
            || response.query != query_for_plugins.get()
        {
            return;
        }
        let mut providers = providers_for_plugins.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        if response.available {
            providers.plugins = response.results;
            *actions_for_plugins.borrow_mut() = response.actions;
            if providers.core_ready() {
                super::paint_trace::note(
                    "commit-plugins",
                    &format!(
                        "query={} rows={}",
                        query_for_plugins.get(),
                        providers.built_in.len() + providers.applications.len()
                    ),
                );
                let priorities = priorities_for_plugins
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>();
                commit_provider_results(
                    &mut providers,
                    &query_for_plugins.get(),
                    &priorities,
                    selected_id_for_plugins,
                    selected_index_for_plugins,
                    selection_touched_for_plugins,
                    results_for_plugins,
                    history_mode_for_plugins,
                );
            }
        }
        status_for_plugins.set(response.status);
    });
    FlowPluginWorker::spawn(plugin_sender)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_native_pipeline(
    app: &mut App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    inline_completion: Signal<String>,
    status: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    priorities: Signal<Vec<PriorityEntry>>,
    history_mode: Signal<bool>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
) -> NativePluginWorker {
    let query_for_native_plugins = query;
    let results_for_native_plugins = results;
    let _inline_completion_for_native_plugins = inline_completion;
    let status_for_native_plugins = status;
    let selected_id_for_native_plugins = selected_id;
    let selected_index_for_native_plugins = selected_index;
    let selection_touched_for_native_plugins = selection_touched;
    let sequence_for_native_plugins = current_sequence;
    let providers_for_native_plugins = Rc::clone(&providers);
    let priorities_for_native_plugins = priorities;
    let history_mode_for_native_plugins = history_mode;
    let actions_for_native_plugins = Rc::clone(&plugin_actions);
    let native_sender = app.channel::<NativePluginQueryResponse>(move |_, response| {
        if response.sequence != sequence_for_native_plugins.get()
            || response.query != query_for_native_plugins.get()
        {
            return;
        }
        let mut providers = providers_for_native_plugins.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        let has_native_results = !response.results.is_empty();
        providers.native_plugins = response.results;
        if response.available {
            actions_for_native_plugins
                .borrow_mut()
                .extend(response.actions);
            if has_native_results {
                status_for_native_plugins.set(response.status.clone());
            }
        }
        if providers.core_ready() {
            super::paint_trace::note(
                "commit-native_plugins",
                &format!(
                    "query={} rows={}",
                    query_for_native_plugins.get(),
                    providers.built_in.len() + providers.applications.len()
                ),
            );
            let priorities = priorities_for_native_plugins
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &mut providers,
                &query_for_native_plugins.get(),
                &priorities,
                selected_id_for_native_plugins,
                selected_index_for_native_plugins,
                selection_touched_for_native_plugins,
                results_for_native_plugins,
                history_mode_for_native_plugins,
            );
        }
    });
    NativePluginWorker::spawn(native_sender)
}
