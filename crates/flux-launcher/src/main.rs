#![cfg_attr(windows, windows_subsystem = "windows")]
mod accent;

mod activation_clear;
mod app_hotkeys;
mod app_identity;
mod app_scan;
mod app_shell;
mod applications;
mod background_tasks;
mod builtin;
mod builtin_calc;
mod builtin_obsidian;
mod cli;
mod color_picker;
mod everything;
mod fullscreen;
mod history_priorities;
mod host_protocol;
mod hotkeys;
mod input_keys;
mod interval_tick;
mod key_handlers;
mod keyboard_layout;
mod launch;
mod launcher_icons;
mod monitor;
mod native_host;
mod native_plugins;
mod paint_trace;
mod plugin_limits;
mod plugin_transport;
mod plugins;
mod provider_merge;
mod provider_snapshot;
mod recycle_confirm;
mod result_actions;
mod result_row;
mod result_widgets;
mod settings_general;
mod settings_plugins;
mod settings_priorities;
mod settings_shell;
mod settings_stats;
mod settings_ui;
mod settings_visual;
mod shell_icon_cache;
mod shell_icon_extract;
mod startup;
mod theme_text;
mod tray_menu;
mod ui_constants;
mod ui_dialogs;
mod ui_results;
mod ui_search;
mod update_tasks;
mod updater;
mod visual_preview;
mod window_geometry;

use flux_core::{
    MonitorPreference, SearchModel, Settings, SettingsLoadOutcome, MAX_LAUNCHER_HEIGHT,
    MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use plugins::PluginAction;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{atomic::Ordering, Arc, RwLock};
use windui::app::{CursorVisibilityHandle, WindowOpHandle, WindowSizeHandle};
use windui::prelude::*;

pub(crate) use ui_constants::*;

fn request_scroll(scroll_pending: Signal<bool>) {
    scroll_pending.set(true);
}

pub(crate) use window_geometry::*;

pub(crate) use history_priorities::*;
pub(crate) use launcher_icons::*;
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

    let (settings, settings_load) = Settings::load_or_default_from(&Settings::config_path());
    match settings_load {
        SettingsLoadOutcome::Loaded => {}
        SettingsLoadOutcome::MovedAside(backup) => eprintln!(
            "Settings could not be read, so they were moved to {} and this run starts from defaults",
            backup.display()
        ),
        SettingsLoadOutcome::Unreadable => eprintln!(
            "Settings at {} could not be read and could not be moved aside; this run starts from defaults",
            Settings::config_path().display()
        ),
    }
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
    let clear_history_confirm = signal(false);
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
            .filter(|tab| *tab < 5)
            .unwrap_or(0),
    );
    let (stats_usage_text, stats_top_text) =
        settings_stats::stats_texts(&settings.launch_counts, settings.total_queries_committed);
    let stats_usage = signal(stats_usage_text);
    let stats_top = signal(stats_top_text);
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
    let color_hsv = signal(color_picker::hsv_for_selection_u32(
        settings.custom_selection_color,
    ));
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

    let results = signal(SearchModel::new().results().to_vec());
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
        stats_usage,
        stats_top,
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
        .child(everything_install_prompt);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Start));

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
    let window_op: WindowOpHandle = app.window_op_handle();
    let theme_handle = app.theme_handle();
    let cursor_visibility: CursorVisibilityHandle = app.cursor_visibility_handle();
    let update_channels = background_tasks::register_update_channels(
        &mut app,
        update_status,
        update_available,
        update_install_progress,
        update_installing,
        Arc::clone(&shared_settings),
    );
    let update_install_sender = update_channels.install_sender.clone();
    let update_sender = update_channels.check_sender.clone();
    let update_install_in_flight = Rc::clone(&update_channels.install_in_flight);
    let update_check_in_flight = Rc::clone(&update_channels.check_in_flight);
    let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
        .map(|value| value != "1")
        .unwrap_or(true);
    if update_checks_allowed && settings.update_checks_enabled && update_check_due(&settings) {
        request_update_check(update_sender.clone(), &update_check_in_flight);
    }
    *action_window_slot.borrow_mut() = Some(window_size.clone());
    let size_for_visibility = window_size.clone();
    let application_worker = background_tasks::spawn_application_pipeline(
        &mut app,
        query,
        results,
        inline_completion,
        status,
        selected_id,
        selected_index,
        selection_touched,
        current_sequence,
        Rc::clone(&provider_results),
        priorities,
        history_mode,
    );

    let everything_worker = background_tasks::spawn_everything_pipeline(
        &mut app,
        query,
        results,
        inline_completion,
        status,
        selected_id,
        selected_index,
        selection_touched,
        current_sequence,
        Rc::clone(&provider_results),
        priorities,
        history_mode,
        auto_enable_everything,
        everything_installed,
        everything_status,
        settings.auto_enable_everything,
    );

    let plugin_worker = background_tasks::spawn_plugin_pipeline(
        &mut app,
        query,
        results,
        inline_completion,
        status,
        selected_id,
        selected_index,
        selection_touched,
        current_sequence,
        Rc::clone(&provider_results),
        priorities,
        history_mode,
        Rc::clone(&plugin_actions),
    );

    let native_plugin_worker = background_tasks::spawn_native_pipeline(
        &mut app,
        query,
        results,
        inline_completion,
        status,
        selected_id,
        selected_index,
        selection_touched,
        current_sequence,
        Rc::clone(&provider_results),
        priorities,
        history_mode,
        Rc::clone(&plugin_actions),
    );

    let activation_handle = app_hotkeys::register_activation_hotkey(
        &mut app,
        activation_hotkey,
        Arc::clone(&shared_settings),
        window_position.clone(),
        cursor_visibility.clone(),
        window_size.clone(),
        query,
        query_caret_position,
        results,
        selected_id,
        selected_index,
        selection_touched,
        show_results,
        history_mode,
        history_cursor,
        action_mode,
        action_index,
        action_items,
        inline_completion,
        scroll_request_for_rows,
        settings_visible,
        launcher_width,
        launcher_height,
    );

    app = app_hotkeys::register_game_mode_hotkey(
        app,
        Arc::clone(&shared_settings),
        game_mode,
        game_mode_status,
    );

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
        stats_usage,
        stats_top,
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

    let tray = tray_menu::build_tray(
        Arc::clone(&shared_settings),
        game_mode,
        game_mode_status,
        settings_visible,
        query,
        query_caret_position,
        results,
        selected_id,
        selected_index,
        selection_touched,
        history_mode,
        history_cursor,
        action_mode,
        action_index,
        action_items,
        inline_completion,
        scroll_request_for_rows,
        show_results,
        window_size.clone(),
        window_position.clone(),
        launcher_width,
        launcher_height,
    );

    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window.
    let settings_ui = settings_ui::SettingsUi {
        shared_settings: Arc::clone(&shared_settings),
        query_history: Rc::clone(&query_history),
        stats_usage,
        stats_top,
        clear_history_confirm,
        history_cursor,
        priorities,
        update_status,
        update_available,
        update_install_progress,
        update_installing,
        update_install_sender: update_install_sender.clone(),
        update_install_in_flight: Rc::clone(&update_install_in_flight),
        update_sender: update_sender.clone(),
        update_check_in_flight: Rc::clone(&update_check_in_flight),
        game_mode,
        game_mode_status,
        settings_visible,
        settings_tab,
        show_results,
        activation_display,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_recording,
        activation_handle: activation_handle.clone(),
        ignore_fullscreen,
        smooth_caret,
        switch_to_english_layout,
        use_system_accent,
        selection_color,
        theme_handle: theme_handle.clone(),
        custom_selection_color,
        color_hsv,
        caret_duration,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        visual_preview_generation,
        clear_query_on_activation,
        start_with_windows,
        auto_enable_everything,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        everything_status,
        everything_installed,
        monitor_preference,
        window_size: window_size.clone(),
        window_position: window_position.clone(),
        query,
        results,
        providers: Rc::clone(&provider_results),
    };
    let settings_panel = settings_shell::build_settings_panel(
        &settings_ui,
        settings_general::build_general_tab(&settings_ui),
        settings_visual::build_visual_tab(&settings_ui),
        settings_priorities::build_priorities_tab(
            &settings_ui,
            settings_priorities::build_priorities_empty(&settings_ui),
            settings_priorities::build_priority_list(&settings_ui),
        ),
        settings_plugins::build_plugins_tab(&settings_ui),
        settings_stats::build_stats_tab(&settings_ui),
    );

    let clear_history_dialog =
        ui_dialogs::build_clear_history_confirm(clear_history_confirm, &settings_ui);

    let content = app_shell::build_shell_content(
        launcher_surface,
        settings_panel,
        settings_visible,
        clear_history_dialog,
    );

    let app = interval_tick::register_interval(
        app,
        query,
        results,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        icon_refresh_generation,
        status,
        show_results,
        inline_completion,
        selection_touched,
        current_sequence,
        Rc::clone(&provider_results),
        scroll_request_for_rows,
        Rc::clone(&plugin_actions),
        auto_enable_everything,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        history_mode,
        settings_visible,
        settings_tab,
        everything_prompt_visible,
        everything_installed,
        everything_status,
        visual_preview_generation,
        selected_id,
        selected_index,
        action_mode,
        action_index,
        action_items,
        Arc::clone(&shared_settings),
        update_sender.clone(),
        Rc::clone(&update_check_in_flight),
        window_size.clone(),
        window_position.clone(),
        application_worker,
        everything_worker,
        plugin_worker,
        native_plugin_worker,
        recycle_bin_confirmation,
        window_op.clone(),
    );
    app_shell::run_launcher(
        app,
        tray,
        startup_launch,
        single_instance_disabled,
        window_op,
        content,
        settings_visible,
        query,
        query_caret_position,
        results,
        selected_id,
        selected_index,
        selection_touched,
        show_results,
        history_mode,
        history_cursor,
        action_mode,
        action_index,
        action_items,
        inline_completion,
        scroll_request_for_rows,
        launcher_width,
        launcher_height,
        clear_query_on_activation,
        size_for_visibility,
        window_size,
        cursor_visibility,
        Arc::clone(&shared_settings),
        selection_color,
        theme_handle,
    );
}
