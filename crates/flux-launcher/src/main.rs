#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
mod app_identity;
mod app_scan;
mod applications;
mod builtin;
mod builtin_calc;
mod builtin_obsidian;
mod cli;
mod everything;
mod fullscreen;
mod history_priorities;
mod host_protocol;
mod hotkeys;
mod input_keys;
mod key_handlers;
mod keyboard_layout;
mod launch;
mod launcher_icons;
mod monitor;
mod native_host;
mod native_plugins;
mod plugin_limits;
mod plugin_transport;
mod plugins;
mod provider_merge;
mod provider_snapshot;
mod result_actions;
mod result_row;
mod result_widgets;
mod shell_icon_cache;
mod shell_icon_extract;
mod startup;
mod theme_text;
mod ui_constants;
mod ui_dialogs;
mod ui_results;
mod ui_search;
mod update_tasks;
mod updater;
mod visual_preview;
mod window_geometry;

use applications::{ApplicationResponse, ApplicationWorker};
use everything::{EverythingResponse, EverythingWorker, InstallationState};
use flux_core::{
    should_suppress_activation, HotkeyConfig, MonitorPreference, SearchModel, Settings,
    DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use plugins::{
    native_plugin_install_path, FlowPluginWorker, NativePluginQueryResponse, NativePluginWorker,
    PluginAction, PluginQueryResponse,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{atomic::Ordering, Arc, RwLock};
use windui::app::{CursorVisibilityHandle, WindowOpHandle, WindowSizeHandle};
use windui::prelude::*;

pub(crate) use ui_constants::*;

fn should_claim_single_instance(mode: Option<&std::ffi::OsStr>) -> bool {
    !matches!(
        mode,
        Some(mode)
            if mode == std::ffi::OsStr::new("--plugin-host")
                || mode == std::ffi::OsStr::new("--folder-launch-smoke")
                || mode == std::ffi::OsStr::new("--shortcut-icon-smoke")
    )
}
fn is_shutdown_mode(mode: Option<&std::ffi::OsStr>) -> bool {
    mode == Some(std::ffi::OsStr::new("--shutdown"))
}

fn request_scroll(scroll_pending: Signal<bool>) {
    scroll_pending.set(true);
}

pub(crate) use window_geometry::*;

pub(crate) use history_priorities::*;
pub(crate) use input_keys::*;
pub(crate) use launcher_icons::*;
pub(crate) use provider_merge::*;
pub(crate) use provider_snapshot::*;
pub(crate) use result_actions::*;
pub(crate) use shell_icon_cache::*;
pub(crate) use theme_text::*;
pub(crate) use update_tasks::*;

fn main() {
    let (startup_launch, single_instance_disabled) = match cli::handle_cli_modes() {
        cli::StartupAction::Exit => return,
        cli::StartupAction::Launch {
            startup,
            single_instance_disabled,
        } => (startup, single_instance_disabled),
    };

    let settings = Settings::load_or_default();
    if let Err(error) = startup::set_enabled(settings.start_with_windows) {
        eprintln!("Could not synchronize Windows startup setting: {error}");
    }
    let activation_hotkey = hotkeys::activation_hotkey(&settings.activation_hotkey);
    let shared_settings = Arc::new(RwLock::new(settings.clone()));
    let query_history = Rc::new(RefCell::new(settings.query_history.clone()));
    let priorities = signal(settings.priority_entries.clone());
    let history_cursor = signal(None::<usize>);
    let history_mode = signal(false);

    let query = signal(String::new());
    let selected_id = signal(String::new());
    let selected_index = signal(0_usize);
    let selection_touched = signal(false);
    let action_mode = signal(false);
    let action_index = signal(0_usize);
    let action_scroll_pending = signal(false);
    let recycle_bin_confirmation = signal(false);
    let action_items = signal(Vec::<ActionItem>::new());
    let action_window_slot = Rc::new(RefCell::new(None::<WindowSizeHandle>));
    let status = signal(String::from("Ready"));
    let update_status = signal(String::from("Stable updates are checked automatically"));
    let update_available = signal(None::<updater::StableUpdate>);
    let update_install_progress = signal(None::<(String, updater::DownloadProgress)>);
    let update_installing = signal(false);
    let current_sequence = signal(0_u64);
    let game_mode = signal(settings.game_mode);
    let game_mode_status = signal(game_mode_label(settings.game_mode));
    let settings_visible = signal(std::env::var_os("FLUX_OPEN_SETTINGS").is_some());
    let settings_tab = signal(
        std::env::var("FLUX_SMOKE_SETTINGS_TAB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|tab| *tab < 4)
            .unwrap_or(0),
    );
    let tray_settings_smoke_pending = Rc::new(Cell::new(
        std::env::var_os("FLUX_SMOKE_TRAY_SETTINGS").is_some(),
    ));
    let show_results = signal(false);
    let activation_key = signal(settings.activation_hotkey.key.clone());
    let activation_display = signal(hotkeys::display_config(&settings.activation_hotkey));
    let activation_recording = signal(false);
    let activation_ctrl = signal(settings.activation_hotkey.ctrl);
    let activation_alt = signal(settings.activation_hotkey.alt);
    let activation_shift = signal(settings.activation_hotkey.shift);
    let activation_meta = signal(settings.activation_hotkey.meta);
    let ignore_fullscreen = signal(settings.ignore_hotkeys_in_fullscreen);
    let smooth_caret = signal(settings.smooth_caret);
    let switch_to_english_layout = signal(settings.switch_to_english_layout);
    let use_system_accent = signal(settings.use_system_accent);
    let custom_selection_color = signal(selection_color_hex(settings.custom_selection_color));
    let launcher_width = signal(settings.launcher_width);
    let launcher_height = signal(settings.launcher_height);
    let launcher_width_input = signal(settings.launcher_width.to_string());
    let launcher_height_input = signal(settings.launcher_height.to_string());
    let launcher_width_slider = signal(dimension_slider_fraction(
        settings.launcher_width,
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    ));
    let launcher_height_slider = signal(dimension_slider_fraction(
        settings.launcher_height,
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    ));
    let launcher_preview_text = signal(format!(
        "Current launcher client area: {} × {} logical px (DIP)",
        settings.launcher_width, settings.launcher_height
    ));
    let visual_preview_generation = signal(0_u64);
    let clear_query_on_activation = signal(settings.clear_query_on_activation);
    let start_with_windows = signal(settings.start_with_windows);
    let auto_enable_everything = signal(settings.auto_enable_everything);
    let update_checks_enabled = signal(settings.update_checks_enabled);
    let update_interval_hours = signal(settings.update_interval_hours.to_string());
    let auto_install_updates = signal(settings.auto_install_updates);
    let obsidian_enabled = signal(settings.obsidian_enabled);
    let obsidian_alias = signal(settings.obsidian_alias.clone());
    let google_enabled = signal(settings.google_enabled);
    let google_alias = signal(settings.google_alias.clone());
    let initial_monitor_preference = std::env::var("FLUX_SMOKE_MONITOR_PREFERENCE")
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "primary" => Some(MonitorPreference::Primary),
            "cursor" => Some(MonitorPreference::Cursor),
            "foreground" => Some(MonitorPreference::Foreground),
            _ => None,
        })
        .unwrap_or(settings.monitor_preference);
    let monitor_preference = signal(monitor_preference_index(initial_monitor_preference));
    let initial_everything_state = everything::installation_state();
    let everything_installed = signal(initial_everything_state.is_installed());
    let everything_prompt_disabled = std::env::var("FLUX_DISABLE_EVERYTHING_PROMPT")
        .ok()
        .as_deref()
        == Some("1");
    let everything_prompt_visible_at_start = should_show_everything_install_prompt(
        initial_everything_state.is_installed(),
        settings.auto_enable_everything,
        settings.everything_install_prompt_seen,
        everything_prompt_disabled,
    );
    let everything_prompt_visible = signal(everything_prompt_visible_at_start);
    let everything_status = signal(if everything_installed.get() {
        String::from("Everything is already installed; Flux will enable IPC automatically")
    } else {
        String::from("Everything is not installed. Install it with winget to enable file search.")
    });
    let selection_color = signal(selection_color_for_settings(&settings));
    let caret_duration = signal(settings.smooth_caret_duration_ms.to_string());

    let mut model = SearchModel::new();
    let results = signal(model.results().to_vec());
    let provider_results = Rc::new(RefCell::new(ProviderResults::default()));
    let plugin_actions = Rc::new(RefCell::new(HashMap::<String, PluginAction>::new()));
    let result_source = results;
    let scroll_request_for_rows = signal(false);
    let icon_refresh_generation = signal(SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire));
    let inline_completion = signal(String::new());
    let query_caret_position = signal(query.with(|text| text.chars().count()));

    let search_box = ui_search::build_search_box(
        query,
        query_caret_position,
        inline_completion,
        settings.smooth_caret,
        settings.smooth_caret_duration_ms,
    );

    let action_bar = ui_results::build_action_bar(show_results, action_mode);
    let result_list = ui_results::build_result_list(
        result_source,
        selected_id,
        selected_index,
        selection_touched,
        icon_refresh_generation,
        Rc::clone(&plugin_actions),
        action_items,
        action_index,
        action_scroll_pending,
        action_mode,
        launcher_width,
        query,
        scroll_request_for_rows,
        selection_color,
        Arc::clone(&shared_settings),
        Rc::clone(&query_history),
        history_mode,
        recycle_bin_confirmation,
        settings_visible,
        Rc::clone(&action_window_slot),
        show_results,
    );

    let everything_install_prompt = ui_dialogs::build_everything_install_prompt(
        everything_prompt_visible,
        everything_status,
        Arc::clone(&shared_settings),
    );

    let recycle_bin_dialog = ui_dialogs::build_recycle_bin_dialog(recycle_bin_confirmation, status);

    let action_list = ui_results::build_action_list(
        action_items,
        Arc::clone(&shared_settings),
        priorities,
        Rc::clone(&provider_results),
        query,
        result_source,
        selected_id,
        selected_index,
        action_index,
        action_scroll_pending,
        Rc::clone(&action_window_slot),
        action_mode,
        launcher_width,
        launcher_height,
    );

    // The HWND itself owns the system Acrylic surface. Keep this root transparent so
    // the blur fills the complete client area instead of becoming an inset card. The
    // content must match the live window width so result rows expand with resizing.
    // Keep the empty search strip and the results palette intrinsically sized. A
    // full-height column plus a weighted spacer made the compact state look too
    // tall and left an oversized gap between the last result and the footer.
    // Pin the content to the top edge: the window grows downward when results
    // appear, so a vertically centered column would move the Search strip down
    // by half the slack difference between the compact and expanded states
    // (the Search icon, text, and caret visibly jumped on the first keystroke).
    // Top-anchoring keeps the strip at the same offset in every state. The top
    // inset is 13 so the 31px strip sits centered in the 56px compact window
    // ((56 - 31) / 2 = 12.5); padding lives inside the content, so the expanded
    // strip lands at the same offset and the action-bar bottom inset stays
    // within the smoke contract.
    let launcher_content = Element::col()
        .width_match()
        .padding_edges(10, 13, 10, 7)
        .spacing(4)
        .child(search_box)
        .child(result_list)
        // The result viewport now ends immediately before the fixed footer. Do not
        // add a weighted spacer: it creates a visible blank band for short queries.
        .child(action_bar)
        .child(action_list)
        .child(recycle_bin_dialog)
        .child(everything_install_prompt);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Start));

    let query_for_interval = query;
    let results_for_interval = results;
    let width_for_interval = launcher_width;
    let height_for_interval = launcher_height;
    let width_input_for_interval = launcher_width_input;
    let height_input_for_interval = launcher_height_input;
    let width_slider_for_interval = launcher_width_slider;
    let height_slider_for_interval = launcher_height_slider;
    let preview_text_for_interval = launcher_preview_text;
    let icon_refresh_generation_for_interval = icon_refresh_generation;
    let status_for_interval = status;
    let show_results_for_interval = show_results;
    let inline_completion_for_interval = inline_completion;
    let selection_touched_for_interval = selection_touched;
    let sequence_for_interval = current_sequence;
    let providers_for_interval = Rc::clone(&provider_results);
    let scroll_request_for_interval = scroll_request_for_rows;
    let actions_for_interval = Rc::clone(&plugin_actions);
    let auto_enable_everything_for_interval = auto_enable_everything;
    let obsidian_enabled_for_interval = obsidian_enabled;
    let obsidian_alias_for_interval = obsidian_alias;
    let google_enabled_for_interval = google_enabled;
    let google_alias_for_interval = google_alias;
    let history_mode_for_interval = history_mode;
    let settings_visible_for_interval = settings_visible;
    let settings_tab_for_interval = settings_tab;
    let everything_prompt_visible_for_interval = everything_prompt_visible;
    let everything_installed_for_interval = everything_installed;
    let everything_status_for_interval = everything_status;
    let visual_preview_generation_for_interval = visual_preview_generation;
    let visual_preview_smoke_for_interval =
        std::env::var_os("FLUX_SMOKE_VISUAL_SETTINGS").is_some();
    let everything_plugins_smoke_for_interval =
        std::env::var_os("FLUX_SMOKE_EVERYTHING_PLUGINS").is_some();
    let tray_settings_smoke_pending_for_interval = Rc::clone(&tray_settings_smoke_pending);
    let mut last_icon_generation = icon_refresh_generation.get();
    let mut last_launcher_width = launcher_width.get();
    let mut last_launcher_height = launcher_height.get();
    let mut last_settings_visible = settings_visible.get();
    let mut last_everything_prompt_visible = everything_prompt_visible.get();
    let mut last_query = String::new();
    let mut visual_preview_process: Option<visual_preview::PreviewProcess> = None;
    let mut last_visual_preview_request: Option<(u16, u16)> = None;
    let mut last_visual_preview_generation = visual_preview_generation.get();
    let mut last_visual_control_state: Option<(u16, u16, u32, u32)> = None;
    let mut everything_plugins_smoke_reported = false;
    let mut sequence = 0_u64;

    let settings_at_start = settings_visible.get();
    let initial_height = if settings_at_start {
        SETTINGS_WINDOW_HEIGHT
    } else if everything_prompt_visible_at_start {
        EVERYTHING_PROMPT_WINDOW_HEIGHT
    } else {
        COMPACT_WINDOW_HEIGHT
    };
    let initial_width = if settings_at_start {
        SETTINGS_WINDOW_WIDTH
    } else if everything_prompt_visible_at_start {
        EVERYTHING_PROMPT_WINDOW_WIDTH
    } else {
        launcher_width.get() as i32
    };
    let window_icon = tray_icon();
    let mut app =
        App::new("Flux Launcher", initial_width, initial_height).icon_rgba(16, 16, &window_icon);
    if everything_prompt_visible_at_start
        && std::env::var_os("FLUX_SMOKE_EVERYTHING_PROMPT").is_some()
    {
        eprintln!("Everything install prompt: visible at startup");
        eprintln!(
            "Everything install prompt style: glass-transparent panel_fill=none modal_scrim=none window_background=transparent"
        );
    }
    if let Some((x, y)) =
        monitor::centered_position(initial_monitor_preference, initial_width, initial_height)
    {
        app = app.position(x, y);
    }
    let window_size = app.window_size_handle();
    let window_position = app.window_position_handle();
    let position_for_interval = window_position.clone();
    let settings_for_interval_geometry = Arc::clone(&shared_settings);
    let window_op: WindowOpHandle = app.window_op_handle();
    let cursor_visibility: CursorVisibilityHandle = app.cursor_visibility_handle();
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
    let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
        .map(|value| value != "1")
        .unwrap_or(true);
    if update_checks_allowed && settings.update_checks_enabled && update_check_due(&settings) {
        request_update_check(update_sender.clone(), &update_check_in_flight);
    }
    let settings_for_update_interval = Arc::clone(&shared_settings);
    let update_sender_for_interval = update_sender.clone();
    let update_check_in_flight_for_interval = Rc::clone(&update_check_in_flight);
    *action_window_slot.borrow_mut() = Some(window_size.clone());
    let size_for_interval = window_size.clone();
    let size_for_visibility = window_size.clone();
    let query_for_applications = query;
    let results_for_applications = results;
    let inline_completion_for_applications = inline_completion;
    let status_for_applications = status;
    let selected_id_for_applications = selected_id;
    let selected_index_for_applications = selected_index;
    let selection_touched_for_applications = selection_touched;
    let sequence_for_applications = current_sequence;
    let providers_for_applications = Rc::clone(&provider_results);
    let priorities_for_applications = priorities;
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
            let priorities = priorities_for_applications
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_applications.get(),
                &priorities,
                selected_id_for_applications,
                selected_index_for_applications,
                selection_touched_for_applications,
                inline_completion_for_applications,
                results_for_applications,
            );
        }
        status_for_applications.set(response.status);
    });
    let application_worker = ApplicationWorker::spawn(application_sender);

    let query_for_everything = query;
    let results_for_everything = results;
    let inline_completion_for_everything = inline_completion;
    let status_for_everything = status;
    let selected_id_for_everything = selected_id;
    let selected_index_for_everything = selected_index;
    let selection_touched_for_everything = selection_touched;
    let sequence_for_everything = current_sequence;
    let providers_for_everything = Rc::clone(&provider_results);
    let priorities_for_everything = priorities;
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
            let priorities = priorities_for_everything
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_everything.get(),
                &priorities,
                selected_id_for_everything,
                selected_index_for_everything,
                selection_touched_for_everything,
                inline_completion_for_everything,
                results_for_everything,
            );
        }
        status_for_everything.set(response.status);
    });
    let everything_worker = EverythingWorker::spawn(everything_sender);
    if settings.auto_enable_everything {
        match everything::start_background_if_installed() {
            Ok(InstallationState::Installed(_)) => {
                everything_installed.set(true);
                everything_status.set(String::from(
                    "Everything is already installed; Flux is enabling local IPC automatically",
                ));
            }
            Ok(InstallationState::Missing) => {
                everything_installed.set(false);
                everything_status.set(String::from(
                    "Everything is not installed. Install it with winget to enable file search.",
                ));
            }
            Err(error) => {
                everything_status.set(error);
            }
        }
    } else {
        everything_status.set(String::from(
            "Everything auto-enable is disabled in Flux settings",
        ));
    }

    let query_for_plugins = query;
    let results_for_plugins = results;
    let inline_completion_for_plugins = inline_completion;
    let status_for_plugins = status;
    let selected_id_for_plugins = selected_id;
    let selected_index_for_plugins = selected_index;
    let selection_touched_for_plugins = selection_touched;
    let sequence_for_plugins = current_sequence;
    let providers_for_plugins = Rc::clone(&provider_results);
    let priorities_for_plugins = priorities;
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
                let priorities = priorities_for_plugins
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>();
                commit_provider_results(
                    &providers,
                    &query_for_plugins.get(),
                    &priorities,
                    selected_id_for_plugins,
                    selected_index_for_plugins,
                    selection_touched_for_plugins,
                    inline_completion_for_plugins,
                    results_for_plugins,
                );
            }
        }
        status_for_plugins.set(response.status);
    });
    let plugin_worker = FlowPluginWorker::spawn(plugin_sender);

    let query_for_native_plugins = query;
    let results_for_native_plugins = results;
    let inline_completion_for_native_plugins = inline_completion;
    let status_for_native_plugins = status;
    let selected_id_for_native_plugins = selected_id;
    let selected_index_for_native_plugins = selected_index;
    let selection_touched_for_native_plugins = selection_touched;
    let sequence_for_native_plugins = current_sequence;
    let providers_for_native_plugins = Rc::clone(&provider_results);
    let priorities_for_native_plugins = priorities;
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
            let priorities = priorities_for_native_plugins
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_native_plugins.get(),
                &priorities,
                selected_id_for_native_plugins,
                selected_index_for_native_plugins,
                selection_touched_for_native_plugins,
                inline_completion_for_native_plugins,
                results_for_native_plugins,
            );
        }
    });
    let native_plugin_worker = NativePluginWorker::spawn(native_sender);

    let settings_for_activation = Arc::clone(&shared_settings);
    let position_for_activation = window_position.clone();
    let cursor_visibility_for_activation = cursor_visibility.clone();
    let size_for_activation = window_size.clone();
    let query_for_activation = query;
    let results_for_activation = results;
    let selected_id_for_activation = selected_id;
    let selected_index_for_activation = selected_index;
    let selection_touched_for_activation = selection_touched;
    let show_results_for_activation = show_results;
    let history_mode_for_activation = history_mode;
    let history_cursor_for_activation = history_cursor;
    let action_mode_for_activation = action_mode;
    let action_index_for_activation = action_index;
    let action_items_for_activation = action_items;
    let inline_completion_for_activation = inline_completion;
    let scroll_request_for_activation = scroll_request_for_rows;
    let settings_visible_for_activation = settings_visible;
    let activation_handle = app.hotkey_handle(activation_hotkey, move |ctx| {
        let settings = settings_for_activation
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
            // Clear before toggling visibility. The previous implementation did
            // this only from on_window_hide, which allowed the old query frame to
            // survive in the compositor until the next repaint after re-show.
            if settings.clear_query_on_activation {
                query_for_activation.set(String::new());
                results_for_activation.set(Vec::new());
                selected_id_for_activation.set(String::new());
                selected_index_for_activation.set(0);
                selection_touched_for_activation.set(false);
                show_results_for_activation.set(false);
                history_mode_for_activation.set(false);
                history_cursor_for_activation.set(None);
                action_mode_for_activation.set(false);
                action_index_for_activation.set(0);
                action_items_for_activation.set(Vec::new());
                inline_completion_for_activation.set(String::new());
                scroll_request_for_activation.set(false);
                let (compact_width, compact_height) = launcher_window_geometry_with_sizes(
                    settings_visible_for_activation.get(),
                    false,
                    i32::from(launcher_width.get()),
                    i32::from(launcher_height.get()),
                );
                size_for_activation.set(compact_width, compact_height);
            }
            let (width, height) = launcher_window_geometry_with_sizes(
                settings_visible_for_activation.get(),
                show_results_for_activation.get(),
                i32::from(launcher_width.get()),
                i32::from(launcher_height.get()),
            );
            request_monitor_position(
                &position_for_activation,
                settings.monitor_preference,
                width,
                height,
            );
            cursor_visibility_for_activation.show();
            if should_show_launcher(launcher_is_foreground()) {
                ctx.show_window();
            } else {
                ctx.hide_window();
            }
        }
    });

    let settings_for_game_hotkey = Arc::clone(&shared_settings);
    let game_mode_for_hotkey = game_mode;
    let game_mode_status_for_hotkey = game_mode_status;
    app = app.hotkey(hotkeys::game_mode_toggle_hotkey(), move |_| {
        let enabled = !game_mode_for_hotkey.get();
        set_game_mode(
            &settings_for_game_hotkey,
            game_mode_for_hotkey,
            game_mode_status_for_hotkey,
            enabled,
        );
    });

    app = key_handlers::register_key_handlers(
        app,
        activation_recording,
        activation_display,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_handle.clone(),
        query,
        query_caret_position,
        results,
        selected_id,
        selected_index,
        scroll_request_for_rows,
        selection_touched,
        action_mode,
        action_index,
        action_items,
        action_scroll_pending,
        recycle_bin_confirmation,
        Rc::clone(&plugin_actions),
        inline_completion,
        settings_visible,
        Rc::clone(&query_history),
        history_mode,
        history_cursor,
        Arc::clone(&shared_settings),
        priorities,
        Rc::clone(&provider_results),
        window_op.clone(),
        cursor_visibility.clone(),
        window_size.clone(),
        show_results,
        launcher_width,
        launcher_height,
    );

    let settings_for_tray_toggle = Arc::clone(&shared_settings);
    let game_mode_for_tray = game_mode;
    let game_status_for_tray = game_mode_status;
    let settings_visible_for_tray = settings_visible;
    let settings_visible_for_left_click = settings_visible;
    let show_results_for_left_click = show_results;
    let size_for_left_click = window_size.clone();
    let position_for_left_click = window_position.clone();
    let settings_for_left_click = Arc::clone(&shared_settings);
    let show_results_for_tray = show_results;
    let size_for_tray = window_size.clone();
    let position_for_tray = window_position.clone();
    let settings_for_tray_position = Arc::clone(&shared_settings);
    let size_for_settings = window_size.clone();
    let position_for_settings = window_position.clone();
    let settings_for_settings_position = Arc::clone(&shared_settings);
    let tray = Tray::new()
        .tooltip("Flux Launcher")
        .icon_rgba(16, 16, &tray_icon())
        .on_left_click(move |ctx| {
            settings_visible_for_left_click.set(false);
            let height = if show_results_for_left_click.get() {
                launcher_height.get() as i32
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            if let Ok(settings) = settings_for_left_click.read() {
                request_monitor_position(
                    &position_for_left_click,
                    settings.monitor_preference,
                    launcher_width.get() as i32,
                    height,
                );
            }
            size_for_left_click.set(launcher_width.get() as i32, height);
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item("Show launcher", move |ctx| {
                settings_visible_for_tray.set(false);
                let height = if show_results_for_tray.get() {
                    launcher_height.get() as i32
                } else {
                    COMPACT_WINDOW_HEIGHT
                };
                if let Ok(settings) = settings_for_tray_position.read() {
                    request_monitor_position(
                        &position_for_tray,
                        settings.monitor_preference,
                        launcher_width.get() as i32,
                        height,
                    );
                }
                size_for_tray.set(launcher_width.get() as i32, height);
                ctx.show_window();
            }),
            TrayMenuItem::item("Settings", move |ctx| {
                settings_visible.set(true);
                if let Ok(settings) = settings_for_settings_position.read() {
                    request_monitor_position(
                        &position_for_settings,
                        settings.monitor_preference,
                        SETTINGS_WINDOW_WIDTH,
                        SETTINGS_WINDOW_HEIGHT,
                    );
                }
                // Queue the Settings size before showing the hidden tray window. The
                // first frame must not use the compact 72-DIP launcher height.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                ctx.show_window();
                // Keep the request after show as well because the native show lifecycle
                // may consume a stale compact-size request from the previous hide.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::check("Game Mode", game_mode, move |_| {
                let enabled = !game_mode_for_tray.get();
                set_game_mode(
                    &settings_for_tray_toggle,
                    game_mode_for_tray,
                    game_status_for_tray,
                    enabled,
                );
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::item("Exit", |ctx| ctx.quit()),
        ]);

    let settings_for_apply = Arc::clone(&shared_settings);
    let position_for_apply = window_position.clone();
    let activation_handle_for_apply = activation_handle.clone();
    let activation_handle_for_record_button = activation_handle.clone();
    let activation_recording_for_record_button = activation_recording;
    let activation_recording_for_apply = activation_recording;
    let activation_display_for_ui = activation_display;
    let activation_display_for_apply = activation_display;
    let game_mode_status_for_apply = game_mode_status;
    let settings_visible_for_apply = settings_visible;
    let show_results_for_back = show_results;
    let position_for_back = window_position.clone();
    let settings_for_back_position = Arc::clone(&shared_settings);
    let size_for_back = window_size.clone();
    let size_for_apply = window_size.clone();
    let settings_for_clear_history = Arc::clone(&shared_settings);
    let history_for_clear = Rc::clone(&query_history);
    let history_cursor_for_clear = history_cursor;
    let start_with_windows_for_apply = start_with_windows;
    let update_checks_enabled_for_apply = update_checks_enabled;
    let update_interval_hours_for_apply = update_interval_hours;
    let auto_install_updates_for_apply = auto_install_updates;
    let update_status_for_apply = update_status;
    let update_available_for_install = update_available;
    let update_status_for_install = update_status;
    let update_installing_for_ui = update_installing;
    let update_install_progress_for_ui = update_install_progress;
    let update_install_sender_for_ui = update_install_sender.clone();
    let update_install_in_flight_for_ui = Rc::clone(&update_install_in_flight);
    let update_sender_for_apply = update_sender.clone();
    let update_sender_for_check_now = update_sender_for_apply.clone();
    let update_check_in_flight_for_apply = Rc::clone(&update_check_in_flight);
    let update_check_in_flight_for_check_now = Rc::clone(&update_check_in_flight);
    let auto_enable_everything_for_apply = auto_enable_everything;
    let obsidian_enabled_for_apply = obsidian_enabled;
    let obsidian_alias_for_apply = obsidian_alias;
    let google_enabled_for_apply = google_enabled;
    let google_alias_for_apply = google_alias;
    let everything_status_for_apply = everything_status;
    let everything_installed_for_ui = everything_installed;
    let settings_for_everything_toggle = Arc::clone(&shared_settings);
    let auto_enable_everything_for_toggle = auto_enable_everything;
    let everything_installed_for_toggle = everything_installed;
    let everything_status_for_toggle = everything_status;
    let settings_for_priority_ui = Arc::clone(&shared_settings);
    let providers_for_priority_ui = Rc::clone(&provider_results);
    let query_for_priority_ui = query;
    let priority_list = Element::list_signal(
        priorities,
        |entry| entry.id.clone(),
        move |entry| {
            let entry_id = entry.id.clone();
            let rank = priorities
                .get()
                .iter()
                .position(|candidate| candidate.id == entry_id)
                .map(|index| index + 1)
                .unwrap_or_default();
            let title = entry.title.clone();
            let target = entry.target.clone();
            let settings_for_up = Arc::clone(&settings_for_priority_ui);
            let settings_for_down = Arc::clone(&settings_for_priority_ui);
            let settings_for_remove = Arc::clone(&settings_for_priority_ui);
            let providers_for_up = Rc::clone(&providers_for_priority_ui);
            let providers_for_down = Rc::clone(&providers_for_priority_ui);
            let providers_for_remove = Rc::clone(&providers_for_priority_ui);
            let query_for_up = query_for_priority_ui;
            let query_for_down = query_for_priority_ui;
            let query_for_remove = query_for_priority_ui;
            let id_for_up = entry_id.clone();
            let id_for_down = entry_id.clone();
            let id_for_remove = entry_id.clone();
            Element::row()
                .width_match()
                .height(58)
                .padding_xy(10, 5)
                .spacing(8)
                .corner(9.0)
                .bg(Color::rgba(255, 255, 255, 12))
                .child(
                    Element::label(format!("{rank}"))
                        .font_size(16.0)
                        .fg(Color::rgba(170, 204, 255, 245))
                        .width(24)
                        .align(Align::Center),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(1)
                        .child(
                            Element::label(title)
                                .font_size(13.0)
                                .fg(Color::WHITE)
                                .max_lines(1)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(target)
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 170))
                                .max_lines(1)
                                .truncate(Truncate::End),
                        ),
                )
                .child(
                    Element::button("↑")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if move_priority_entry(&settings_for_up, priorities, &id_for_up, -1) {
                                refresh_merged_results(
                                    &providers_for_up,
                                    query_for_up,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority moved up");
                            }
                        }),
                )
                .child(
                    Element::button("↓")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if move_priority_entry(&settings_for_down, priorities, &id_for_down, 1)
                            {
                                refresh_merged_results(
                                    &providers_for_down,
                                    query_for_down,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority moved down");
                            }
                        }),
                )
                .child(
                    Element::button("Remove")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if remove_priority_entry(
                                &settings_for_remove,
                                priorities,
                                &id_for_remove,
                            ) {
                                refresh_merged_results(
                                    &providers_for_remove,
                                    query_for_remove,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority removed");
                            }
                        }),
                )
        },
    );
    let priorities_empty = Element::label(
        "No explicit priorities yet. Select an application, press Right, then choose Set as priority.",
    )
    .font_size(12.0)
    .fg(Color::rgba(235, 241, 255, 185))
    .max_lines(2)
    .truncate(Truncate::End)
    .visible_when(move || priorities.get().is_empty());

    let visual_preview_generation_for_width_reset = visual_preview_generation;
    let visual_preview_generation_for_height_reset = visual_preview_generation;
    let settings_for_visual_apply = Arc::clone(&shared_settings);
    let size_for_visual_apply = window_size.clone();
    let position_for_visual_apply = window_position.clone();
    let settings_visible_for_visual_apply = settings_visible;
    let show_results_for_visual_apply = show_results;

    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window.
    let settings_panel = Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(0, 0, 0, 0))
        .border(Color::rgba(0, 0, 0, 0), 0)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(3)
                        .child(Element::label("Settings").font_size(25.0).fg(Color::WHITE))
                        .child(
                            Element::label("Changes apply immediately and are saved atomically")
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 180)),
                        ),
                )
                .child(Element::segmented(
                    vec!["General", "Visual", "Priorities", "Plugins"],
                    settings_tab,
                ))
                .child(
                    Element::button("Back")
                        .neutral()
                        .on_click(move |_| {
                            settings_visible.set(false);
                            let height = if show_results_for_back.get() {
                                launcher_height.get() as i32
                            } else {
                                COMPACT_WINDOW_HEIGHT
                            };
                            if let Ok(settings) = settings_for_back_position.read() {
                                request_monitor_position(
                                    &position_for_back,
                                    settings.monitor_preference,
                                    launcher_width.get() as i32,
                                    height,
                                );
                            }
                            size_for_back.set(launcher_width.get() as i32, height);
                        }),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 0)
                .child(
                Element::col()
                    .width_match()
                    .spacing(12)
                    .child(Element::field(
                        "Activation key",
                        Element::col()
                            .width_match()
                            .spacing(6)
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::label_signal(activation_display_for_ui)
                                            .width_match()
                                            .padding_xy(10, 8)
                                            .bg(Color::rgba(255, 255, 255, 24))
                                            .corner(8.0),
                                    )
                                    .child(
                                        Element::button("Record key")
                                            .neutral()
                                            .on_click(move |ctx| {
                                                activation_recording_for_record_button.set(true);
                                                activation_handle_for_record_button.set_enabled(false);
                                                ctx.toast_ok("Press the desired activation key");
                                            }),
                                    ),
                            )
                            .child(
                                Element::label("Click Record key, then press one key or a key combination")
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 170))
                                    .visible_when(move || activation_recording_for_record_button.get()),
                            ),
                    ))
                    .child(
                        Element::row()
                            .width_match()
                            .spacing(10)
                            .child(Element::checkbox("Ctrl", activation_ctrl))
                            .child(Element::checkbox("Alt", activation_alt))
                            .child(Element::checkbox("Shift", activation_shift))
                            .child(Element::checkbox("Windows", activation_meta)),
                    )
                    .child(Element::field(
                        "Fullscreen protection",
                        Element::checkbox("Ignore activation while another app is fullscreen", ignore_fullscreen),
                    ))
                    .child(Element::field(
                        "Game Mode",
                        Element::checkbox("Suppress the launcher until manually disabled", game_mode),
                    ))
                    .child(Element::field(
                        "Keyboard layout",
                        Element::checkbox(
                            "Start typing in English and restore the previous layout on hide",
                            switch_to_english_layout,
                        ),
                    ))
                    .child(Element::field(
                        "Query on activation",
                        Element::checkbox(
                            "Clear the previous query when opened with the global hotkey",
                            clear_query_on_activation,
                        ),
                    ))
                    .child(Element::field(
                        "Windows startup",
                        Element::checkbox(
                            "Start Flux automatically with Windows",
                            start_with_windows,
                        ),
                    ))
                    .child(Element::field(
                        "Open launcher on",
                        Element::col()
                            .spacing(6)
                            .child(Element::radio(
                                "Primary display",
                                monitor_preference,
                                0,
                            ))
                            .child(Element::radio(
                                "Display with the mouse cursor",
                                monitor_preference,
                                1,
                            ))
                            .child(Element::radio(
                                "Display with the focused window",
                                monitor_preference,
                                2,
                            )),
                    ))
                    .child(
                        Element::col()
                            .width_match()
                            .spacing(8)
                            .child(Element::field(
                                "Updates",
                                Element::checkbox(
                                    "Check stable GitHub releases automatically",
                                    update_checks_enabled,
                                ),
                            ))
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::text_input(update_interval_hours, "24")
                                            .width_match(),
                                    )
                                    .child(Element::label("hours between checks").font_size(11.0)),
                            )
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(Element::label("Update action").width_match())
                                    .child(
                                        Element::label(format!("Current version: {CURRENT_VERSION}"))
                                            .font_size(11.0)
                                            .fg(Color::rgba(235, 241, 255, 190)),
                                    ),
                            )
                            .child(Element::checkbox(
                                "Install stable updates automatically",
                                auto_install_updates,
                            ))
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::label_signal(update_status)
                                            .font_size(11.0)
                                            .fg(Color::rgba(235, 241, 255, 190))
                                            .max_lines(2)
                                            .truncate(Truncate::End)
                                            .width_match(),
                                    )
                                    .child(Element::button("Check for updates").on_click(move |ctx| {
                                        update_status_for_apply.set(String::from(
                                            "Checking stable GitHub releases...",
                                        ));
                                        request_update_check(
                                            update_sender_for_check_now.clone(),
                                            &update_check_in_flight_for_check_now,
                                        );
                                        ctx.toast_ok("Checking stable updates");
                                    }))
                                    .child(
                                        Element::button("Install now")
                                            .visible_when(move || {
                                                update_available_for_install.get().is_some()
                                                    && !update_installing_for_ui.get()
                                            })
                                            .on_click(move |ctx| {
                                                if update_installing_for_ui.get() {
                                                    return;
                                                }
                                                if let Some(update) = update_available_for_install.get() {
                                                    update_installing_for_ui.set(true);
                                                    update_install_progress_for_ui.set(None);
                                                    update_status_for_install.set(format!(
                                                        "Preparing stable {} for download...",
                                                        update.version
                                                    ));
                                                    if !request_update_install(
                                                        update,
                                                        update_install_sender_for_ui.clone(),
                                                        &update_install_in_flight_for_ui,
                                                        updater::RelaunchMode::Visible,
                                                    ) {
                                                        update_installing_for_ui.set(false);
                                                        update_status_for_install.set(String::from(
                                                            "An update is already being installed",
                                                        ));
                                                        ctx.toast_ok("An update is already being installed");
                                                    }
                                                }
                                            }),
                                    ),
                            )
                    )
                    .child(
                        Element::row()
                            .width_match()
                            .spacing(10)
                            .child(
                                Element::label("Query history: Ctrl+H recalls committed searches")
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 175))
                                    .width_match(),
                            )
                            .child(Element::button("Clear history").on_click(move |ctx| {
                                if let Ok(mut settings) = settings_for_clear_history.write() {
                                    settings.clear_query_history();
                                    let _ = save_settings(&settings);
                                }
                                history_for_clear.borrow_mut().clear();
                                history_cursor_for_clear.set(None);
                                ctx.toast_ok("Query history cleared");
                            })),
                    )
                    .child(
                        Element::label("Native Flow plugins: %APPDATA%\\FluxLauncher\\Plugins or FLUX_PLUGIN_DIR")
                            .font_size(12.0)
                            .fg(Color::rgba(235, 241, 255, 160)),
                    )
                    .child(
                        Element::button("Apply settings").on_click(move |ctx| {
                            let duration = caret_duration
                                .get()
                                .trim()
                                .parse::<u16>()
                                .unwrap_or(95)
                                .clamp(60, 160);
                            let configuration = HotkeyConfig {
                                ctrl: activation_ctrl.get(),
                                alt: activation_alt.get(),
                                shift: activation_shift.get(),
                                meta: activation_meta.get(),
                                key: activation_key.get(),
                            };
                            let custom_color = parse_selection_color(&custom_selection_color.get())
                                .unwrap_or(0x4c8bf4);
                            let configured_width = parse_dimension_input(
                                &launcher_width_input.get(),
                                MIN_LAUNCHER_WIDTH,
                                MAX_LAUNCHER_WIDTH,
                            )
                            .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                            let configured_height = parse_dimension_input(
                                &launcher_height_input.get(),
                                MIN_LAUNCHER_HEIGHT,
                                MAX_LAUNCHER_HEIGHT,
                            )
                            .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                            if let Ok(mut settings) = settings_for_apply.write() {
                                settings.activation_hotkey = configuration;
                                settings.ignore_hotkeys_in_fullscreen = ignore_fullscreen.get();
                                settings.game_mode = game_mode.get();
                                settings.smooth_caret = smooth_caret.get();
                                settings.switch_to_english_layout = switch_to_english_layout.get();
                                settings.use_system_accent = use_system_accent.get();
                                settings.custom_selection_color = custom_color;
                                settings.launcher_width = configured_width;
                                settings.launcher_height = configured_height;
                                settings.clear_query_on_activation = clear_query_on_activation.get();
                                settings.start_with_windows = start_with_windows_for_apply.get();
                                settings.update_checks_enabled = update_checks_enabled_for_apply.get();
                                settings.update_interval_hours = update_interval_hours_for_apply
                                    .get()
                                    .trim()
                                    .parse::<u32>()
                                    .unwrap_or(24)
                                    .clamp(1, 168);
                                settings.auto_install_updates = auto_install_updates_for_apply.get();
                                update_interval_hours_for_apply
                                    .set(settings.update_interval_hours.to_string());
                                settings.auto_enable_everything = auto_enable_everything_for_apply.get();
                                settings.obsidian_enabled = obsidian_enabled_for_apply.get();
                                settings.obsidian_alias = obsidian_alias_for_apply.get();
                                settings.google_enabled = google_enabled_for_apply.get();
                                settings.google_alias = google_alias_for_apply.get();
                                settings.monitor_preference = monitor_preference_from_index(monitor_preference.get());
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                activation_recording_for_apply.set(false);
                                activation_display_for_apply
                                    .set(hotkeys::display_config(&settings.activation_hotkey));
                                selection_color.set(selection_color_for_settings(&settings));
                                custom_selection_color.set(selection_color_hex(settings.custom_selection_color));
                                launcher_width.set(settings.launcher_width);
                                launcher_height.set(settings.launcher_height);
                                launcher_width_input.set(settings.launcher_width.to_string());
                                launcher_height_input.set(settings.launcher_height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    settings.launcher_width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    settings.launcher_height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(format!(
                                    "Current launcher client area: {} × {} logical px (DIP)",
                                    settings.launcher_width, settings.launcher_height
                                ));
                                activation_handle_for_apply
                                    .set(hotkeys::activation_hotkey(&settings.activation_hotkey));
                                activation_handle_for_apply.set_enabled(true);
                                game_mode_status_for_apply.set(game_mode_label(settings.game_mode));
                                if settings.auto_enable_everything {
                                    match everything::start_background_if_installed() {
                                        Ok(InstallationState::Installed(_)) => {
                                            everything_installed.set(true);
                                            everything_status_for_apply.set(String::from(
                                                "Everything is already installed; Flux will enable IPC automatically",
                                            ));
                                        }
                                        Ok(InstallationState::Missing) => {
                                            everything_installed.set(false);
                                            everything_status_for_apply.set(String::from(
                                                "Everything is not installed. Install it with winget to enable file search.",
                                            ));
                                        }
                                        Err(error) => everything_status_for_apply.set(error),
                                    }
                                } else {
                                    everything_status_for_apply.set(String::from(
                                        "Everything auto-enable is disabled in Flux settings",
                                    ));
                                }
                                let _ = save_settings(&settings);
                                if let Err(error) = startup::set_enabled(settings.start_with_windows) {
                                    ctx.toast_ok(format!("Startup setting failed: {error}"));
                                }
                                if settings.update_checks_enabled && update_check_due(&settings)
                                {
                                    update_status_for_apply
                                        .set(String::from("Checking stable GitHub releases..."));
                                    request_update_check(
                                        update_sender_for_apply.clone(),
                                        &update_check_in_flight_for_apply,
                                    );
                                }
                            }
                            settings_visible_for_apply.set(false);
                            let selected_preference = monitor_preference_from_index(monitor_preference.get());
                            let applied_width = launcher_width.get() as i32;
                            let applied_height = launcher_height.get() as i32;
                            let target_height = if show_results.get() {
                                applied_height
                            } else {
                                COMPACT_WINDOW_HEIGHT
                            };
                            request_monitor_position(
                                &position_for_apply,
                                selected_preference,
                                applied_width,
                                target_height,
                            );
                            size_for_apply.set(applied_width, target_height);
                            ctx.toast_ok("Settings applied");
                        }),
                    ),
            ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 3)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::label("Everything").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("Everything provides fast indexed file and folder search. Configure its automatic use here, alongside the other plugins.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Everything",
                            Element::checkbox(
                                "Auto-enable Everything when installed",
                                auto_enable_everything,
                            )
                            .on_toggle(move |_| {
                                let enabled = auto_enable_everything_for_toggle.get();
                                if let Ok(mut settings) = settings_for_everything_toggle.write() {
                                    settings.auto_enable_everything = enabled;
                                    settings.normalize();
                                    let _ = save_settings(&settings);
                                }
                                if !enabled {
                                    everything_status_for_toggle.set(String::from(
                                        "Everything auto-enable is disabled in Flux settings",
                                    ));
                                    return;
                                }
                                match everything::start_background_if_installed() {
                                    Ok(InstallationState::Installed(_)) => {
                                        everything_installed_for_toggle.set(true);
                                        everything_status_for_toggle.set(String::from(
                                            "Everything is already installed; Flux will enable IPC automatically",
                                        ));
                                    }
                                    Ok(InstallationState::Missing) => {
                                        everything_installed_for_toggle.set(false);
                                        everything_status_for_toggle.set(String::from(
                                            "Everything is not installed. Install it with winget to enable file search.",
                                        ));
                                    }
                                    Err(error) => everything_status_for_toggle.set(error),
                                }
                            }),
                        ))
                        .child(
                            Element::label("Everything is already installed")
                                .font_size(12.0)
                                .fg(Color::rgba(180, 255, 205, 235))
                                .visible_when(move || everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label("Everything is not installed")
                                .font_size(12.0)
                                .fg(Color::rgba(255, 225, 175, 235))
                                .visible_when(move || !everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label_signal(everything_status)
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 190))
                                .max_lines(2)
                                .truncate(Truncate::End)
                                .width_match(),
                        )
                        .child(
                            Element::label("Command: winget install -e --id voidtools.Everything")
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 155))
                                .visible_when(move || !everything_installed_for_ui.get())
                                .width_match(),
                        )
                        .child(
                            Element::button("Install Everything")
                                .visible_when(move || !everything_installed_for_ui.get())
                                .on_click(move |ctx| {
                                    match everything::launch_winget_install() {
                                        Ok(()) => {
                                            everything_status.set(String::from(
                                                "winget install started. Restart Flux after Everything is installed.",
                                            ));
                                            ctx.toast_ok("winget install started");
                                        }
                                        Err(error) => {
                                            everything_status.set(error.clone());
                                            ctx.toast_ok(error);
                                        }
                                    }
                                }),
                        )
                        .child(Element::label("Native plugins").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("Built-in providers run inside Flux. Community Rust DLL plugins run in one isolated shared worker spawned from this same flux-launcher.exe.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(format!("Community plugin folder: {}", native_plugin_install_path()))
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 150))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label("Configure built-in native Rust plugins without Python or C# runtimes.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Obsidian",
                            Element::checkbox("Enable Obsidian vault search", obsidian_enabled),
                        ))
                        .child(Element::field(
                            "Action keyword",
                            Element::text_input(obsidian_alias, "ob").width_match(),
                        ))
                        .child(
                            Element::label("Search notes and vault files with the configured keyword, for example: ob meeting. Use `ob create project` to create a new note.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Google Search",
                            Element::checkbox("Enable Google web search", google_enabled),
                        ))
                        .child(Element::field(
                            "Action keyword",
                            Element::text_input(google_alias, "g").width_match(),
                        ))
                        .child(
                            Element::label("Search the web with the configured keyword, for example: g space exploration. The result opens in your default browser.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 1)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::label("Visual appearance").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("The live preview is a separate native windui window. Its client area is resized directly in realtime; the Settings window stays centered and stable while dragging.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Smooth Caret",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::checkbox("Animate search caret movement", smooth_caret)
                                        .width_match(),
                                )
                                .child(Element::text_input(caret_duration, "95").width(76))
                                .child(Element::label("ms").font_size(11.0)),
                        ))
                        .child(Element::field(
                            "Selection color",
                            Element::checkbox(
                                "Use the Windows 11 system accent color when available",
                                use_system_accent,
                            ),
                        ))
                        .child(Element::label("Windows accent is read from the current user profile; the custom color is used as a safe fallback.").font_size(10.0).fg(Color::rgba(235, 241, 255, 150)).max_lines(2).truncate(Truncate::End))
                        .child(Element::label("The exact native preview window opens beside Settings when this Visual tab is active."
).font_size(10.0).fg(Color::rgba(235, 241, 255, 170)).max_lines(2).truncate(Truncate::End))
                        .child(
                            Element::col()
                                .spacing(8)
                                .visible_when(move || !use_system_accent.get())
                                .child(
                                    Element::text_input(custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(custom_selection_color)),
                        )
                        .child(Element::field(
                            "Launcher width",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_width_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_width_input, "420")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = DEFAULT_LAUNCHER_WIDTH;
                                            let height = launcher_height.get();
                                            eprintln!("Visual width reset clicked: {}x{}", width, height);
                                            launcher_width.set(width);
                                            launcher_width_input.set(width.to_string());
                                            launcher_width_slider.set(dimension_slider_fraction(
                                                width,
                                                MIN_LAUNCHER_WIDTH,
                                                MAX_LAUNCHER_WIDTH,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_width_reset.set(
                                                visual_preview_generation_for_width_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::field(
                            "Results height",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_height_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_height_input, "382")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = launcher_width.get();
                                            let height = DEFAULT_LAUNCHER_HEIGHT;
                                            eprintln!("Visual height reset clicked: {}x{}", width, height);
                                            launcher_height.set(height);
                                            launcher_height_input.set(height.to_string());
                                            launcher_height_slider.set(dimension_slider_fraction(
                                                height,
                                                MIN_LAUNCHER_HEIGHT,
                                                MAX_LAUNCHER_HEIGHT,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_height_reset.set(
                                                visual_preview_generation_for_height_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::label_signal(launcher_preview_text).font_size(12.0).fg(Color::WHITE))
                        .child(
                            Element::label("The native preview uses the exact requested logical client dimensions. Physical GetClientRect pixels scale with the preview monitor DPI; Apply saves the values.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::button("Apply dimensions").on_click(move |ctx| {
                                let mut width = parse_dimension_input(
                                    &launcher_width_input.get(),
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                                let mut height = parse_dimension_input(
                                    &launcher_height_input.get(),
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                                let duration = caret_duration
                                    .get()
                                    .trim()
                                    .parse::<u16>()
                                    .unwrap_or(95)
                                    .clamp(60, 160);
                                let Ok(mut settings) = settings_for_visual_apply.write() else {
                                    ctx.toast_ok("Could not lock Flux settings");
                                    return;
                                };
                                settings.launcher_width = width;
                                settings.launcher_height = height;
                                settings.smooth_caret = smooth_caret.get();
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                width = settings.launcher_width;
                                height = settings.launcher_height;
                                let preference = settings.monitor_preference;
                                if !save_settings(&settings) {
                                    ctx.toast_ok("Could not save visual dimensions");
                                    return;
                                }
                                launcher_width.set(width);
                                launcher_height.set(height);
                                launcher_width_input.set(width.to_string());
                                launcher_height_input.set(height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(format!(
                                    "Current launcher client area: {} × {} logical px (DIP)",
                                    width, height
                                ));
                                eprintln!("Visual Apply dimensions clicked: {}x{}", width, height);
                                settings_visible_for_visual_apply.set(false);
                                let target_height = if show_results_for_visual_apply.get() {
                                    i32::from(height)
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                };
                                request_monitor_position(
                                    &position_for_visual_apply,
                                    preference,
                                    i32::from(width),
                                    target_height,
                                );
                                size_for_visual_apply.set(i32::from(width), target_height);
                                ctx.toast_ok("Visual dimensions applied");
                            }),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 2)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(10)
                        .child(
                            Element::label("Explicit application priorities")
                                .font_size(17.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label("Only applications explicitly added with Set as priority appear here. Rank 1 is searched first.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(priorities_empty)
                        .child(priority_list),
                ),
        );

    let launcher_page = Element::stack()
        .fill()
        .child(launcher_surface)
        .visible_when(move || !settings_visible.get());
    let settings_page = Element::col()
        .fill()
        .padding(18)
        .child(settings_panel)
        .visible_signal(settings_visible);
    if std::env::var_os("FLUX_SMOKE_SETTINGS_UI").is_some() {
        eprintln!(
            "Settings UI contract: UpdateActionVersionLabel=Current version: {CURRENT_VERSION}; SmoothCaretTab=Visual; SmoothCaretGeneral=false"
        );
    }

    let content = Element::stack()
        .fill()
        .font_family(LAUNCHER_FONT_FAMILY)
        .child(launcher_page)
        .child(settings_page);

    let mut app = if startup_launch {
        app.start_hidden()
    } else {
        app
    };
    let second_instance_sender = app.channel::<()>(|ctx, ()| {
        ctx.show_window();
    });
    let second_instance_sender_for_callback = second_instance_sender.clone();
    let second_instance_window_op = window_op.clone();
    let shutdown_window_op = window_op.clone();
    if !single_instance_disabled {
        app = app.single_instance(SINGLE_INSTANCE_ID, move |argv| {
            if argv.iter().any(|arg| arg == "--shutdown") {
                // Uninstall is an application-controlled handoff: destroy the native
                // window and exit the event loop instead of applying hide_on_close.
                shutdown_window_op.quit();
                return;
            }
            // The native windui listener activates the window; queue Show as well so a
            // tray-hidden startup is made visible before the channel callback is drained.
            second_instance_window_op.show_window();
            let _ = second_instance_sender_for_callback.send(());
        });
    }
    app.tray(tray)
        .hide_on_close()
        .hide_on_deactivate()
        .focus_first_control_on_show()
        // Keep the HWND background transparent so Acrylic/DWM remains visible
        // through the launcher and its install prompt instead of adding a solid slab.
        .bg(Color::TRANSPARENT)
        .centered()
        .frameless()
        .resizable(false)
        .min_size(MIN_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        .theme(launcher_theme())
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |ctx| {
            let current_width = width_for_interval.get();
            let current_height = height_for_interval.get();
            let settings_is_visible = settings_visible_for_interval.get();
            let prompt_is_visible = everything_prompt_visible_for_interval.get();
            let visual_tab_is_visible = settings_tab_for_interval.get() == 1;
            let plugins_tab_is_visible = settings_tab_for_interval.get() == 3;
            let visual_preview_is_visible = settings_is_visible && visual_tab_is_visible;
            if everything_plugins_smoke_for_interval
                && settings_is_visible
                && plugins_tab_is_visible
                && !everything_plugins_smoke_reported
            {
                let installed = everything_installed_for_interval.get();
                let auto_enable = auto_enable_everything_for_interval.get();
                let status = everything_status_for_interval.get();
                eprintln!(
                    "Everything Plugins UI: tab_visible=true everything_section=true auto_enable_checkbox=true status_label=true install_button_label=Install_Everything already_installed_label=Everything_is_already_installed auto_enable={} installed={} install_button_visible={} already_installed_visible={} status={}",
                    auto_enable,
                    installed,
                    !installed,
                    installed,
                    status.replace(' ', "_")
                );
                everything_plugins_smoke_reported = true;
            }
            if settings_is_visible && !last_settings_visible {
                if let Ok(settings) = settings_for_interval_geometry.read() {
                    request_monitor_position(
                        &position_for_interval,
                        settings.monitor_preference,
                        SETTINGS_WINDOW_WIDTH,
                        SETTINGS_WINDOW_HEIGHT,
                    );
                }
                size_for_interval.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            }
            if prompt_is_visible != last_everything_prompt_visible {
                last_everything_prompt_visible = prompt_is_visible;
                let (prompt_width, prompt_height) = launcher_window_geometry_with_prompt(
                    settings_is_visible,
                    prompt_is_visible,
                    show_results_for_interval.get(),
                    width_for_interval.get() as i32,
                    height_for_interval.get() as i32,
                );
                if let Ok(settings) = settings_for_interval_geometry.read() {
                    request_monitor_position(
                        &position_for_interval,
                        settings.monitor_preference,
                        prompt_width,
                        prompt_height,
                    );
                }
                size_for_interval.set(prompt_width, prompt_height);
            }
            if visual_preview_is_visible {
                let preference = settings_for_interval_geometry
                    .read()
                    .map(|settings| settings.monitor_preference)
                    .unwrap_or(MonitorPreference::Cursor);
                let child_exited = visual_preview_process
                    .as_mut()
                    .is_some_and(|preview| !preview.is_alive());
                if child_exited {
                    visual_preview_process.take();
                    last_visual_preview_request = None;
                }
                if let Some(preview) = visual_preview_process.as_mut() {
                    match preview.poll_ready() {
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("Could not ready visual preview: {error}");
                            visual_preview_process.take();
                            last_visual_preview_request = None;
                        }
                    }
                }
                if visual_preview_process.is_none() {
                    let preview_width = i32::from(current_width);
                    let preview_height = i32::from(current_height);
                    let (preview_x, preview_y) =
                        visual_preview_position(preference, preview_width, preview_height);
                    match visual_preview::PreviewProcess::start(
                        preview_width,
                        preview_height,
                        preview_x,
                        preview_y,
                    ) {
                        Ok(preview) => {
                            visual_preview_process = Some(preview);
                            last_visual_preview_request = None;
                        }
                        Err(error) => eprintln!("Could not start visual preview: {error}"),
                    }
                }
            } else if let Some(preview) = visual_preview_process.as_mut() {
                eprintln!(
                    "Visual preview closing because Settings/Visual is hidden: pid={}",
                    preview.pid()
                );
                visual_preview_process.take();
                last_visual_preview_request = None;
            }
            last_settings_visible = settings_is_visible;
            let slider_width = dimension_from_slider(
                width_slider_for_interval.get(),
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH,
            );
            let slider_height = dimension_from_slider(
                height_slider_for_interval.get(),
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT,
            );
            if visual_preview_smoke_for_interval {
                let control_state = (
                    width_for_interval.get(),
                    height_for_interval.get(),
                    (width_slider_for_interval.get() * 10_000.0).round() as u32,
                    (height_slider_for_interval.get() * 10_000.0).round() as u32,
                );
                if last_visual_control_state != Some(control_state) {
                    eprintln!(
                        "Visual control state: width={} height={} width_slider={} height_slider={}",
                        control_state.0, control_state.1, control_state.2, control_state.3
                    );
                    last_visual_control_state = Some(control_state);
                }
            }
            let typed_width = parse_dimension_input(
                &width_input_for_interval.get(),
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH,
            );
            let typed_height = parse_dimension_input(
                &height_input_for_interval.get(),
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT,
            );
            let next_width = typed_width
                .filter(|value| *value != current_width)
                .unwrap_or(if slider_width != current_width {
                    slider_width
                } else {
                    current_width
                });
            let next_height = typed_height
                .filter(|value| *value != current_height)
                .unwrap_or(if slider_height != current_height {
                    slider_height
                } else {
                    current_height
                });
            if next_width != current_width || next_height != current_height {
                width_for_interval.set(next_width);
                height_for_interval.set(next_height);
                width_input_for_interval.set(next_width.to_string());
                height_input_for_interval.set(next_height.to_string());
                width_slider_for_interval.set(dimension_slider_fraction(
                    next_width,
                    MIN_LAUNCHER_WIDTH,
                    MAX_LAUNCHER_WIDTH,
                ));
                height_slider_for_interval.set(dimension_slider_fraction(
                    next_height,
                    MIN_LAUNCHER_HEIGHT,
                    MAX_LAUNCHER_HEIGHT,
                ));
                preview_text_for_interval.set(format!(
                    "Current launcher client area: {} × {} logical px (DIP)",
                    next_width, next_height
                ));
                if !(settings_visible_for_interval.get() && settings_tab_for_interval.get() == 1) {
                    apply_launcher_size(
                        &size_for_interval,
                        &position_for_interval,
                        &settings_for_interval_geometry,
                        next_width,
                        next_height,
                        false,
                        show_results_for_interval.get(),
                    );
                }
                if visual_preview_smoke_for_interval {
                    eprintln!(
                        "Visual preview dimension state: {}x{} logical px",
                        next_width, next_height
                    );
                }
                last_launcher_width = next_width;
                last_launcher_height = next_height;
            } else if current_width != last_launcher_width || current_height != last_launcher_height
            {
                last_launcher_width = current_width;
                last_launcher_height = current_height;
            }

            let preview_generation = visual_preview_generation_for_interval.get();
            if visual_preview_is_visible {
                let requested = (width_for_interval.get(), height_for_interval.get());
                let must_dispatch = last_visual_preview_request != Some(requested)
                    || last_visual_preview_generation != preview_generation;
                if must_dispatch {
                    let preference = settings_for_interval_geometry
                        .read()
                        .map(|settings| settings.monitor_preference)
                        .unwrap_or(MonitorPreference::Cursor);
                    let (preview_x, preview_y) = visual_preview_position(
                        preference,
                        i32::from(requested.0),
                        i32::from(requested.1),
                    );
                    let dispatch_result = if let Some(preview) = visual_preview_process.as_mut() {
                        match preview.poll_ready() {
                            Ok(true) => Some(preview.resize(
                                i32::from(requested.0),
                                i32::from(requested.1),
                                preview_x,
                                preview_y,
                            )),
                            Ok(false) => None,
                            Err(error) => Some(Err(error)),
                        }
                    } else {
                        None
                    };
                    match dispatch_result {
                        Some(Ok(())) => {
                            last_visual_preview_request = Some(requested);
                            last_visual_preview_generation = preview_generation;
                            eprintln!(
                                "Visual preview IPC resize dispatched: {}x{}",
                                requested.0, requested.1
                            );
                        }
                        Some(Err(error)) => {
                            eprintln!("Could not update visual preview: {error}");
                            visual_preview_process.take();
                            last_visual_preview_request = None;
                        }
                        None => {}
                    }
                }
            } else {
                last_visual_preview_request = None;
                last_visual_preview_generation = preview_generation;
            }

            if let Ok(settings) = settings_for_update_interval.read() {
                let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
                    .map(|value| value != "1")
                    .unwrap_or(true);
                if update_checks_allowed
                    && settings.update_checks_enabled
                    && update_check_due(&settings)
                {
                    request_update_check(
                        update_sender_for_interval.clone(),
                        &update_check_in_flight_for_interval,
                    );
                }
            }
            let completed_icon_generation =
                SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire);
            if icon_completion_generation_changed(last_icon_generation, completed_icon_generation) {
                last_icon_generation = completed_icon_generation;
                icon_refresh_generation_for_interval.set(completed_icon_generation);
            }
            if tray_settings_smoke_pending_for_interval.replace(false) {
                // Exercise the same lifecycle order as the tray Settings item,
                // without relying on brittle screen-coordinate tray automation.
                settings_visible_for_interval.set(true);
                ctx.show_window();
                size_for_interval.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                return;
            }
            let next_query = query_for_interval.get();
            if next_query == last_query {
                return;
            }

            let has_query = !next_query.trim().is_empty();
            history_mode_for_interval.set(false);
            show_results_for_interval.set(has_query);
            // Query cleanup also happens when hide-on-deactivate hides the
            // launcher. Do not let that asynchronous query transition resize
            // an already-open Settings panel back to the compact search strip.
            let (target_width, target_height) = launcher_window_geometry_with_prompt(
                settings_visible_for_interval.get(),
                everything_prompt_visible_for_interval.get(),
                has_query,
                launcher_width.get() as i32,
                launcher_height.get() as i32,
            );
            size_for_interval.set(target_width, target_height);
            sequence = sequence.wrapping_add(1);
            sequence_for_interval.set(sequence);
            model.set_query(&next_query);
            {
                let mut built_in_results = model.results().to_vec();
                normalize_built_in_executable_targets(&mut built_in_results);
                let everything_expected = auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN;
                let mut providers = providers_for_interval.borrow_mut();
                providers.reset(sequence, built_in_results.clone(), everything_expected);
                let publish_initial_results = should_publish_initial_query_results(
                    has_query,
                    built_in_results.is_empty(),
                    results_for_interval.get().is_empty(),
                );
                if publish_initial_results {
                    selection_touched_for_interval.set(false);
                    selected_index.set(0);
                    selected_id.set(
                        built_in_results
                            .first()
                            .map(|result| result.id.clone())
                            .unwrap_or_default(),
                    );
                    // Built-in/system commands are synchronous and must be actionable
                    // immediately. External providers still replace this snapshot once
                    // their responses arrive for the same query sequence.
                    results_for_interval.set(built_in_results);
                }
                // Do not derive or display completion from the previous query while
                // the current provider generation is still pending.
                inline_completion_for_interval.set(String::new());
            }
            request_scroll(scroll_request_for_interval);
            action_mode.set(false);
            action_index.set(0);
            action_items.set(Vec::new());
            actions_for_interval.borrow_mut().clear();
            if !has_query {
                inline_completion_for_interval.set(String::new());
                status_for_interval.set(String::from("Ready"));
            } else {
                status_for_interval.set(String::from(
                    "Searching applications, Everything and native Flow plugins...",
                ));
                application_worker.request(sequence, next_query.clone());
                // Everything is the always-on file provider for every non-empty
                // query. Native Everything syntax such as `ext:zip`, `parent:`,
                // `file:`, and `dm:today` stays unchanged; a leading `.ext`
                // shorthand is normalized only for this provider.
                if auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN
                {
                    everything_worker.request(sequence, normalize_everything_query(&next_query));
                }
                if next_query.trim().len() >= PLUGIN_MIN_QUERY_LEN {
                    plugin_worker.request(
                        sequence,
                        next_query.clone(),
                        obsidian_enabled_for_interval.get(),
                        obsidian_alias_for_interval.get(),
                        google_enabled_for_interval.get(),
                        google_alias_for_interval.get(),
                    );
                    native_plugin_worker.request(sequence, next_query.clone());
                }
            }
            last_query = next_query;
        })
        .on_window_show({
            let settings = Arc::clone(&shared_settings);
            let cursor_visibility_for_show = cursor_visibility.clone();
            let settings_visible_for_show = settings_visible;
            let size_for_show = window_size.clone();
            move || {
                cursor_visibility_for_show.show();
                if let Ok(settings) = settings.read() {
                    selection_color.set(selection_color_for_settings(&settings));
                }
                // Tray activation can show the HWND before the first interval pass.
                // Apply the Settings client size in this lifecycle callback too, so
                // the initial frame is the full panel rather than a 72-DIP strip.
                if settings_visible_for_show.get() {
                    size_for_show.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                }
            }
        })
        .on_window_activated({
            let settings = Arc::clone(&shared_settings);
            move || {
                let layout_enabled = settings
                    .read()
                    .map(|settings| settings.switch_to_english_layout)
                    .unwrap_or(true);
                if layout_enabled {
                    keyboard_layout::switch_to_english();
                }
            }
        })
        .on_window_deactivated(|| {
            launch::trace_launch_event("window-deactivated");
        })
        .on_window_hide({
            let settings = Arc::clone(&shared_settings);
            move || {
                launch::trace_launch_event("window-hide");
                let (enabled, clear_query) = settings
                    .read()
                    .map(|settings| {
                        (
                            settings.switch_to_english_layout,
                            settings.clear_query_on_activation,
                        )
                    })
                    .unwrap_or((true, clear_query_on_activation.get()));
                if enabled {
                    keyboard_layout::restore_previous();
                }
                if clear_query {
                    query.set(String::new());
                    results.set(Vec::new());
                    selected_id.set(String::new());
                    selected_index.set(0);
                    selection_touched.set(false);
                    show_results.set(false);
                    history_mode.set(false);
                    history_cursor.set(None);
                    action_mode.set(false);
                    action_index.set(0);
                    action_items.set(Vec::new());
                    inline_completion.set(String::new());
                    scroll_request_for_rows.set(false);
                    let (width, height) = launcher_window_geometry_with_sizes(
                        settings_visible.get(),
                        false,
                        launcher_width.get() as i32,
                        launcher_height.get() as i32,
                    );
                    size_for_visibility.set(width, height);
                }
            }
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::{is_shutdown_mode, should_claim_single_instance};

    #[test]
    fn plugin_host_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--plugin-host"
        ))));
    }

    #[test]
    fn folder_launch_smoke_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--folder-launch-smoke"
        ))));
    }

    #[test]
    fn normal_and_startup_modes_use_main_single_instance_guard() {
        assert!(should_claim_single_instance(None));
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--startup"
        ))));
    }

    #[test]
    fn shutdown_mode_is_a_single_instance_command() {
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--shutdown"
        ))));
        assert!(is_shutdown_mode(Some(std::ffi::OsStr::new("--shutdown"))));
        assert!(!is_shutdown_mode(Some(std::ffi::OsStr::new("--startup"))));
        assert!(!is_shutdown_mode(None));
    }
}
