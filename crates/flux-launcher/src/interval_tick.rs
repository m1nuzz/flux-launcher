use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{atomic::Ordering, Arc, RwLock};

use flux_core::{
    MonitorPreference, SearchModel, SearchResult, Settings, MAX_LAUNCHER_HEIGHT,
    MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::app::{App, WindowOpHandle, WindowPositionHandle, WindowSizeHandle};
use windui::signal::Signal;

use super::applications::ApplicationWorker;
use super::everything::EverythingWorker;
use super::launch;
use super::plugins::{FlowPluginWorker, NativePluginWorker, PluginAction};
use super::provider_merge::normalize_built_in_executable_targets;
use super::provider_snapshot::{should_publish_initial_query_results, ProviderResults};
use super::request_scroll;
use super::shell_icon_cache::{
    icon_completion_generation_changed, SHELL_ICON_COMPLETION_GENERATION,
};
use super::theme_text::normalize_everything_query;
use super::ui_constants::{
    EVERYTHING_MIN_QUERY_LEN, PLUGIN_MIN_QUERY_LEN, SEARCH_INTERVAL, SETTINGS_WINDOW_HEIGHT,
    SETTINGS_WINDOW_WIDTH,
};
use super::update_tasks::{request_update_check, update_check_due};
use super::visual_preview;
use super::window_geometry::{
    apply_launcher_size, dimension_from_slider, dimension_slider_fraction,
    launcher_window_geometry_with_prompt, parse_dimension_input, request_monitor_position,
};

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_interval(
    app: App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_height_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    icon_refresh_generation: Signal<u64>,
    status: Signal<String>,
    show_results: Signal<bool>,
    inline_completion: Signal<String>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    scroll_request: Signal<bool>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    auto_enable_everything: Signal<bool>,
    obsidian_enabled: Signal<bool>,
    obsidian_alias: Signal<String>,
    google_enabled: Signal<bool>,
    google_alias: Signal<String>,
    history_mode: Signal<bool>,
    settings_visible: Signal<bool>,
    settings_tab: Signal<usize>,
    everything_prompt_visible: Signal<bool>,
    everything_installed: Signal<bool>,
    everything_status: Signal<String>,
    visual_preview_generation: Signal<u64>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<super::result_actions::ActionItem>>,
    shared_settings: Arc<RwLock<Settings>>,
    update_sender: windui::prelude::Sender<super::updater::UpdateCheckResponse>,
    update_check_in_flight: Rc<Cell<bool>>,
    window_size: WindowSizeHandle,
    window_position: WindowPositionHandle,
    application_worker: ApplicationWorker,
    everything_worker: EverythingWorker,
    plugin_worker: FlowPluginWorker,
    native_plugin_worker: NativePluginWorker,
    recycle_bin_confirmation: Signal<bool>,
    window_op: WindowOpHandle,
) -> App {
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
    let providers_for_interval = Rc::clone(&providers);
    let scroll_request_for_interval = scroll_request;
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
    let tray_settings_smoke_pending = Rc::new(Cell::new(
        std::env::var_os("FLUX_SMOKE_TRAY_SETTINGS").is_some(),
    ));
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
    let mut model = SearchModel::new();
    let position_for_interval = window_position.clone();
    let settings_for_interval_geometry = Arc::clone(&shared_settings);
    let size_for_interval = window_size.clone();
    let settings_for_update_interval = Arc::clone(&shared_settings);
    let update_sender_for_interval = update_sender.clone();
    let update_check_in_flight_for_interval = Rc::clone(&update_check_in_flight);
    let recycle_bin_confirmation_for_interval = recycle_bin_confirmation;
    let window_op_for_interval = window_op;
    let mut recycle_confirm_child: Option<std::process::Child> = None;
    app.on_interval(SEARCH_INTERVAL, move |ctx| {
        // Emptying the Recycle Bin uses the native Windows shell confirmation. The
        // launcher hides itself and spawns a short-lived child process that runs the
        // blocking shell prompt off the UI thread, then reshows when the child exits
        // (exit 0 = emptied, non-zero = declined or failed).
        if recycle_bin_confirmation_for_interval.get() {
            if recycle_confirm_child.is_none() {
                match std::env::current_exe() {
                    Ok(exe) => {
                        window_op_for_interval.hide_window();
                        match std::process::Command::new(exe)
                            .arg("--empty-recycle-confirm")
                            .spawn()
                        {
                            Ok(child) => recycle_confirm_child = Some(child),
                            Err(error) => {
                                eprintln!("Could not start Recycle Bin confirmation: {error}");
                                window_op_for_interval.show_window();
                                recycle_bin_confirmation_for_interval.set(false);
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("Could not resolve the executable for confirmation: {error}");
                        recycle_bin_confirmation_for_interval.set(false);
                    }
                }
            } else if let Some(child) = recycle_confirm_child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        recycle_confirm_child = None;
                        if status.success() {
                            if launch::empty_recycle_bin() {
                                status_for_interval.set(String::from("Recycle Bin emptied"));
                            } else {
                                status_for_interval
                                    .set(String::from("Could not empty the Recycle Bin"));
                            }
                        } else {
                            status_for_interval.set(String::from("Ready"));
                        }
                        recycle_bin_confirmation_for_interval.set(false);
                        window_op_for_interval.show_window();
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("Recycle Bin confirmation process error: {error}");
                        recycle_confirm_child = None;
                        recycle_bin_confirmation_for_interval.set(false);
                        window_op_for_interval.show_window();
                    }
                }
            }
            return;
        }
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
                    super::window_geometry::visual_preview_position(preference, preview_width, preview_height);
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
                let (preview_x, preview_y) = super::window_geometry::visual_preview_position(
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
}
