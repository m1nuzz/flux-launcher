use flux_core::{
    HotkeyConfig, Settings, DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT,
    MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::core::EventCtx;

use super::everything::{self, InstallationState};
use super::history_priorities::game_mode_label;
use super::hotkeys;
use super::settings_stats::STATS_TAB_INDEX;
use super::settings_ui::SettingsUi;
use super::startup;
use super::theme_text::{
    parse_selection_color, rgb_to_hsv, selection_color_hex, stage_selection_color,
};
use super::ui_constants::COMPACT_WINDOW_HEIGHT;
use super::update_tasks::{
    request_update_check, request_update_install, save_settings, update_check_due,
};
use super::updater;
use super::window_geometry::{
    dimension_slider_fraction, monitor_preference_from_index, monitor_preference_index,
    parse_dimension_input, request_monitor_position,
};

/// One persist point for the whole Settings window. Every tab edits the same
/// draft signals, so the footer's Apply writes all of them at once instead of
/// each tab carrying its own save button.
pub(crate) fn apply_settings(ui: &SettingsUi, ctx: &mut EventCtx) {
    let duration = ui
        .caret_duration
        .get()
        .trim()
        .parse::<u16>()
        .unwrap_or(95)
        .clamp(60, 160);
    let configuration = HotkeyConfig {
        ctrl: ui.activation_ctrl.get(),
        alt: ui.activation_alt.get(),
        shift: ui.activation_shift.get(),
        meta: ui.activation_meta.get(),
        key: ui.activation_key.get(),
    };
    stage_selection_color(
        ui.use_system_accent,
        ui.custom_selection_color,
        ui.selection_color,
        &ui.theme_handle,
        &ui.shared_settings,
        ctx,
    );
    let configured_width = parse_dimension_input(
        &ui.launcher_width_input.get(),
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    )
    .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
    let configured_height = parse_dimension_input(
        &ui.launcher_height_input.get(),
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    )
    .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
    if let Ok(mut settings) = ui.shared_settings.write() {
        settings.activation_hotkey = configuration;
        settings.game_mode_hotkey = HotkeyConfig {
            ctrl: ui.game_mode_ctrl.get(),
            alt: ui.game_mode_alt.get(),
            shift: ui.game_mode_shift.get(),
            meta: ui.game_mode_meta.get(),
            key: ui.game_mode_key.get(),
        };
        settings.game_mode_hotkey_enabled = ui.game_mode_hotkey_enabled.get();
        settings.ignore_hotkeys_in_fullscreen = ui.ignore_fullscreen.get();
        settings.game_mode = ui.game_mode.get();
        settings.smooth_caret = ui.smooth_caret.get();
        settings.switch_to_english_layout = ui.switch_to_english_layout.get();
        settings.launcher_width = configured_width;
        settings.launcher_height = configured_height;
        settings.clear_query_on_activation = ui.clear_query_on_activation.get();
        settings.start_with_windows = ui.start_with_windows.get();
        settings.update_checks_enabled = ui.update_checks_enabled.get();
        settings.update_interval_hours = ui
            .update_interval_hours
            .get()
            .trim()
            .parse::<u32>()
            .unwrap_or(24)
            .clamp(1, 168);
        settings.auto_install_updates = ui.auto_install_updates.get();
        ui.update_interval_hours
            .set(settings.update_interval_hours.to_string());
        settings.auto_enable_everything = ui.auto_enable_everything.get();
        settings.obsidian_enabled = ui.obsidian_enabled.get();
        settings.obsidian_alias = ui.obsidian_alias.get();
        settings.google_enabled = ui.google_enabled.get();
        settings.google_alias = ui.google_alias.get();
        settings.monitor_preference = monitor_preference_from_index(ui.monitor_preference.get());
        settings.smooth_caret_duration_ms = duration;
        settings.priority_entries = ui.priorities.get();
        settings.normalize();
        ui.activation_recording.set(false);
        ui.activation_display
            .set(hotkeys::display_config(&settings.activation_hotkey));
        ui.launcher_width.set(settings.launcher_width);
        ui.launcher_height.set(settings.launcher_height);
        ui.launcher_width_input
            .set(settings.launcher_width.to_string());
        ui.launcher_height_input
            .set(settings.launcher_height.to_string());
        ui.launcher_width_slider.set(dimension_slider_fraction(
            settings.launcher_width,
            MIN_LAUNCHER_WIDTH,
            MAX_LAUNCHER_WIDTH,
        ));
        ui.launcher_height_slider.set(dimension_slider_fraction(
            settings.launcher_height,
            MIN_LAUNCHER_HEIGHT,
            MAX_LAUNCHER_HEIGHT,
        ));
        ui.launcher_preview_text.set(format!(
            "Current launcher client area: {} × {} logical px (DIP)",
            settings.launcher_width, settings.launcher_height
        ));
        ui.activation_handle
            .set(hotkeys::activation_hotkey(&settings.activation_hotkey));
        ui.activation_handle.set_enabled(true);
        ui.game_mode_display
            .set(hotkeys::display_config(&settings.game_mode_hotkey));
        ui.game_mode_handle
            .set(hotkeys::game_mode_hotkey(&settings.game_mode_hotkey));
        ui.game_mode_handle
            .set_enabled(settings.game_mode_hotkey_enabled);
        ui.game_mode_recording.set(false);
        ui.game_mode_status.set(game_mode_label(settings.game_mode));
        if settings.auto_enable_everything {
            match everything::start_background_if_installed() {
                Ok(InstallationState::Installed(_)) => {
                    ui.everything_installed.set(true);
                    ui.everything_status.set(String::from(
                        "Everything is already installed; Flux will enable IPC automatically",
                    ));
                }
                Ok(InstallationState::Missing) => {
                    ui.everything_installed.set(false);
                    ui.everything_status.set(String::from(
                        "Everything is not installed. Install it with winget to enable file search.",
                    ));
                }
                Err(error) => ui.everything_status.set(error),
            }
        } else {
            ui.everything_status.set(String::from(
                "Everything auto-enable is disabled in Flux settings",
            ));
        }
        let _ = save_settings(&settings);
        eprintln!(
            "Settings apply dispatched: {}x{}",
            settings.launcher_width, settings.launcher_height
        );
        if let Err(error) = startup::set_enabled(settings.start_with_windows) {
            ctx.toast_ok(format!("Startup setting failed: {error}"));
        }
        if settings.update_checks_enabled && update_check_due(&settings) {
            ui.update_status
                .set(String::from("Checking stable GitHub releases..."));
            request_update_check(ui.update_sender.clone(), &ui.update_check_in_flight);
        }
    }
    ui.settings_visible.set(false);
    let selected_preference = monitor_preference_from_index(ui.monitor_preference.get());
    let applied_width = i32::from(ui.launcher_width.get());
    let applied_height = i32::from(ui.launcher_height.get());
    let target_height = if ui.show_results.get() {
        applied_height
    } else {
        COMPACT_WINDOW_HEIGHT
    };
    request_monitor_position(
        &ui.window_position,
        selected_preference,
        applied_width,
        target_height,
    );
    ui.window_size.set(applied_width, target_height);
    ctx.toast_ok("Settings applied");
}

/// Close the Settings window without quitting: the process stays resident in
/// the tray and the activation hotkey brings the search strip back.
///
/// Dismissing Settings also releases the global key recorder. Left armed it
/// would swallow every keystroke and keep the hotkey unregistered, which is
/// why the title strip cannot simply hide the HWND through the framework.
pub(crate) fn close_settings(ui: &SettingsUi) {
    hotkeys::end_recording(
        ui.activation_recording,
        ui.activation_display,
        ui.activation_key,
        ui.activation_ctrl,
        ui.activation_alt,
        ui.activation_shift,
        ui.activation_meta,
        &ui.activation_handle,
    );
    hotkeys::end_recording(
        ui.game_mode_recording,
        ui.game_mode_display,
        ui.game_mode_key,
        ui.game_mode_ctrl,
        ui.game_mode_alt,
        ui.game_mode_shift,
        ui.game_mode_meta,
        &ui.game_mode_handle,
    );
    ui.settings_visible.set(false);
    let height = if ui.show_results.get() {
        i32::from(ui.launcher_height.get())
    } else {
        COMPACT_WINDOW_HEIGHT
    };
    if let Ok(settings) = ui.shared_settings.read() {
        request_monitor_position(
            &ui.window_position,
            settings.monitor_preference,
            i32::from(ui.launcher_width.get()),
            height,
        );
    }
    ui.window_size
        .set(i32::from(ui.launcher_width.get()), height);
    // The hide belongs to the interval tick: it is where the visual preview
    // child gets dropped, and the tick stops running once the window is hidden.
    ui.settings_hide_request.set(true);
}

/// Whether the draft signals differ from the stored configuration.
///
/// The footer's Apply is the only persist point, so it is shown exactly when
/// pressing it would write something. A window where nothing was touched has no
/// primary action at all, which is what the reference does and what keeps the
/// reader from looking for a save that would do nothing.
pub(crate) fn has_unsaved_changes(ui: &SettingsUi) -> bool {
    let Ok(settings) = ui.shared_settings.read() else {
        return false;
    };
    let duration = ui
        .caret_duration
        .get()
        .trim()
        .parse::<u16>()
        .unwrap_or(95)
        .clamp(60, 160);
    let hotkey = HotkeyConfig {
        ctrl: ui.activation_ctrl.get(),
        alt: ui.activation_alt.get(),
        shift: ui.activation_shift.get(),
        meta: ui.activation_meta.get(),
        key: ui.activation_key.get(),
    };
    let game_hotkey = HotkeyConfig {
        ctrl: ui.game_mode_ctrl.get(),
        alt: ui.game_mode_alt.get(),
        shift: ui.game_mode_shift.get(),
        meta: ui.game_mode_meta.get(),
        key: ui.game_mode_key.get(),
    };
    let width = parse_dimension_input(
        &ui.launcher_width_input.get(),
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    )
    .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
    let height = parse_dimension_input(
        &ui.launcher_height_input.get(),
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    )
    .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
    let interval = ui
        .update_interval_hours
        .get()
        .trim()
        .parse::<u32>()
        .unwrap_or(24)
        .clamp(1, 168);
    let accent = parse_selection_color(&ui.custom_selection_color.get()).unwrap_or(0x4c_8b_f4);
    settings.activation_hotkey != hotkey
        || settings.game_mode_hotkey != game_hotkey
        || settings.game_mode_hotkey_enabled != ui.game_mode_hotkey_enabled.get()
        || settings.ignore_hotkeys_in_fullscreen != ui.ignore_fullscreen.get()
        || settings.game_mode != ui.game_mode.get()
        || settings.smooth_caret != ui.smooth_caret.get()
        || settings.switch_to_english_layout != ui.switch_to_english_layout.get()
        || settings.use_system_accent != ui.use_system_accent.get()
        || settings.custom_selection_color != accent
        || settings.launcher_width != width
        || settings.launcher_height != height
        || settings.clear_query_on_activation != ui.clear_query_on_activation.get()
        || settings.start_with_windows != ui.start_with_windows.get()
        || settings.auto_enable_everything != ui.auto_enable_everything.get()
        || settings.update_checks_enabled != ui.update_checks_enabled.get()
        || settings.update_interval_hours != interval
        || settings.auto_install_updates != ui.auto_install_updates.get()
        || settings.obsidian_enabled != ui.obsidian_enabled.get()
        || settings.obsidian_alias != ui.obsidian_alias.get()
        || settings.google_enabled != ui.google_enabled.get()
        || settings.google_alias != ui.google_alias.get()
        || settings.monitor_preference != monitor_preference_from_index(ui.monitor_preference.get())
        || settings.smooth_caret_duration_ms != duration
        || settings.priority_entries != ui.priorities.get()
}

/// Hand an available update to the installer without leaving Settings.
pub(crate) fn install_update(ui: &SettingsUi, ctx: &mut EventCtx) {
    if ui.update_installing.get() {
        return;
    }
    let Some(update) = ui.update_available.get() else {
        return;
    };
    ui.update_installing.set(true);
    ui.update_install_progress.set(None);
    ui.update_status.set(format!(
        "Preparing stable {} for download...",
        update.version
    ));
    if !request_update_install(
        update,
        ui.update_install_sender.clone(),
        &ui.update_install_in_flight,
        updater::RelaunchMode::Visible,
    ) {
        ui.update_installing.set(false);
        ui.update_status
            .set(String::from("An update is already being installed"));
        ctx.toast_ok("An update is already being installed");
    }
}

pub(crate) fn check_for_updates_now(ui: &SettingsUi, ctx: &mut EventCtx) {
    ui.update_status
        .set(String::from("Checking stable GitHub releases..."));
    request_update_check(ui.update_sender.clone(), &ui.update_check_in_flight);
    ctx.toast_ok("Checking stable updates");
}

/// Restore the defaults of the visible tab into the draft signals only.
///
/// Deliberately field-by-field rather than replacing the stored settings with
/// `Settings::default()`: the same file also carries query history, remembered
/// history rows and launch counts, and a tab reset must not erase them. Nothing
/// reaches disk here - Apply is still the only persist point.
pub(crate) fn reset_active_tab(ui: &SettingsUi, ctx: &mut EventCtx) {
    let defaults = Settings::default();
    let tab = ui.settings_tab.get();
    match tab {
        0 => reset_general(ui, &defaults),
        1 => reset_visual(ui, &defaults),
        2 => ui.priorities.set(Vec::new()),
        3 => reset_plugins(ui, &defaults),
        _ => {}
    }
    eprintln!("Settings tab reset: {tab}");
    ctx.toast_ok("Defaults restored for this tab. Press Apply settings to save.");
}

/// The Stats tab holds counters rather than settings, so it has nothing to
/// reset; its own Clear history stays the explicit data action.
pub(crate) fn tab_is_resettable(tab: usize) -> bool {
    tab != STATS_TAB_INDEX
}

fn reset_general(ui: &SettingsUi, defaults: &Settings) {
    ui.activation_ctrl.set(defaults.activation_hotkey.ctrl);
    ui.activation_alt.set(defaults.activation_hotkey.alt);
    ui.activation_shift.set(defaults.activation_hotkey.shift);
    ui.activation_meta.set(defaults.activation_hotkey.meta);
    ui.activation_key
        .set(defaults.activation_hotkey.key.clone());
    ui.activation_display
        .set(hotkeys::display_config(&defaults.activation_hotkey));
    ui.ignore_fullscreen
        .set(defaults.ignore_hotkeys_in_fullscreen);
    ui.game_mode.set(defaults.game_mode);
    ui.game_mode_ctrl.set(defaults.game_mode_hotkey.ctrl);
    ui.game_mode_alt.set(defaults.game_mode_hotkey.alt);
    ui.game_mode_shift.set(defaults.game_mode_hotkey.shift);
    ui.game_mode_meta.set(defaults.game_mode_hotkey.meta);
    ui.game_mode_key.set(defaults.game_mode_hotkey.key.clone());
    ui.game_mode_display
        .set(hotkeys::display_config(&defaults.game_mode_hotkey));
    ui.game_mode_hotkey_enabled
        .set(defaults.game_mode_hotkey_enabled);
    ui.switch_to_english_layout
        .set(defaults.switch_to_english_layout);
    ui.clear_query_on_activation
        .set(defaults.clear_query_on_activation);
    ui.start_with_windows.set(defaults.start_with_windows);
    ui.monitor_preference
        .set(monitor_preference_index(defaults.monitor_preference));
    ui.update_checks_enabled.set(defaults.update_checks_enabled);
    ui.update_interval_hours
        .set(defaults.update_interval_hours.to_string());
    ui.auto_install_updates.set(defaults.auto_install_updates);
}

fn reset_visual(ui: &SettingsUi, defaults: &Settings) {
    ui.smooth_caret.set(defaults.smooth_caret);
    ui.caret_duration
        .set(defaults.smooth_caret_duration_ms.to_string());
    ui.use_system_accent.set(defaults.use_system_accent);
    ui.custom_selection_color
        .set(selection_color_hex(defaults.custom_selection_color));
    ui.color_hsv.set(rgb_to_hsv(
        ((defaults.custom_selection_color >> 16) & 0xff) as u8,
        ((defaults.custom_selection_color >> 8) & 0xff) as u8,
        (defaults.custom_selection_color & 0xff) as u8,
    ));
    ui.launcher_width.set(defaults.launcher_width);
    ui.launcher_height.set(defaults.launcher_height);
    ui.launcher_width_input
        .set(defaults.launcher_width.to_string());
    ui.launcher_height_input
        .set(defaults.launcher_height.to_string());
    ui.launcher_width_slider.set(dimension_slider_fraction(
        defaults.launcher_width,
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    ));
    ui.launcher_height_slider.set(dimension_slider_fraction(
        defaults.launcher_height,
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    ));
    ui.launcher_preview_text.set(format!(
        "Current launcher client area: {} × {} logical px (DIP)",
        defaults.launcher_width, defaults.launcher_height
    ));
}

fn reset_plugins(ui: &SettingsUi, defaults: &Settings) {
    ui.auto_enable_everything
        .set(defaults.auto_enable_everything);
    ui.obsidian_enabled.set(defaults.obsidian_enabled);
    ui.obsidian_alias.set(defaults.obsidian_alias.clone());
    ui.google_enabled.set(defaults.google_enabled);
    ui.google_alias.set(defaults.google_alias.clone());
}
