use std::rc::Rc;
use std::sync::Arc;

use flux_core::{
    HotkeyConfig, DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT,
    MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::prelude::*;

use super::everything::{self, InstallationState};
use super::history_priorities::game_mode_label;
use super::hotkeys;
use super::settings_ui::SettingsUi;
use super::startup;
use super::theme_text::stage_selection_color;
use super::ui_constants::{COMPACT_WINDOW_HEIGHT, CURRENT_VERSION};
use super::update_tasks::{
    request_update_check, request_update_install, save_settings, update_check_due,
};
use super::updater;
use super::window_geometry::{
    dimension_slider_fraction, monitor_preference_from_index, parse_dimension_input,
    request_monitor_position,
};

pub(crate) fn build_general_tab(ui: &SettingsUi) -> Element {
    let activation_display_for_ui = ui.activation_display;
    let activation_handle_for_record_button = ui.activation_handle.clone();
    let activation_recording_for_record_button = ui.activation_recording;
    let update_status_for_apply = ui.update_status;
    let update_available_for_install = ui.update_available;
    let update_installing_for_ui = ui.update_installing;
    let update_install_progress_for_ui = ui.update_install_progress;
    let update_install_sender_for_ui = ui.update_install_sender.clone();
    let update_install_in_flight_for_ui = Rc::clone(&ui.update_install_in_flight);
    let update_sender_for_apply = ui.update_sender.clone();
    let update_sender_for_check_now = update_sender_for_apply.clone();
    let update_check_in_flight_for_apply = Rc::clone(&ui.update_check_in_flight);
    let update_check_in_flight_for_check_now = Rc::clone(&ui.update_check_in_flight);
    let settings_for_clear_history = Arc::clone(&ui.shared_settings);
    let history_for_clear = Rc::clone(&ui.query_history);
    let history_cursor_for_clear = ui.history_cursor;
    let settings_for_apply = Arc::clone(&ui.shared_settings);
    let position_for_apply = ui.window_position.clone();
    let activation_handle_for_apply = ui.activation_handle.clone();
    let activation_recording_for_apply = ui.activation_recording;
    let activation_display_for_apply = ui.activation_display;
    let game_mode_status_for_apply = ui.game_mode_status;
    let settings_visible_for_apply = ui.settings_visible;
    let size_for_apply = ui.window_size.clone();
    let start_with_windows_for_apply = ui.start_with_windows;
    let update_checks_enabled_for_apply = ui.update_checks_enabled;
    let update_interval_hours_for_apply = ui.update_interval_hours;
    let auto_install_updates_for_apply = ui.auto_install_updates;
    let auto_enable_everything_for_apply = ui.auto_enable_everything;
    let obsidian_enabled_for_apply = ui.obsidian_enabled;
    let obsidian_alias_for_apply = ui.obsidian_alias;
    let google_enabled_for_apply = ui.google_enabled;
    let google_alias_for_apply = ui.google_alias;
    let everything_status_for_apply = ui.everything_status;
    let everything_installed = ui.everything_installed;
    let activation_ctrl = ui.activation_ctrl;
    let activation_alt = ui.activation_alt;
    let activation_shift = ui.activation_shift;
    let activation_meta = ui.activation_meta;
    let ignore_fullscreen = ui.ignore_fullscreen;
    let activation_key = ui.activation_key;
    let clear_query_on_activation = ui.clear_query_on_activation;
    let monitor_preference = ui.monitor_preference;
    let selection_color = ui.selection_color;
    let game_mode = ui.game_mode;
    let switch_to_english_layout = ui.switch_to_english_layout;
    let start_with_windows = ui.start_with_windows;
    let update_checks_enabled = ui.update_checks_enabled;
    let update_interval_hours = ui.update_interval_hours;
    let update_status = ui.update_status;
    let update_status_for_install = ui.update_status;
    let auto_install_updates = ui.auto_install_updates;
    let caret_duration = ui.caret_duration;
    let custom_selection_color = ui.custom_selection_color;
    let launcher_width = ui.launcher_width;
    let launcher_height = ui.launcher_height;
    let launcher_width_input = ui.launcher_width_input;
    let launcher_height_input = ui.launcher_height_input;
    let launcher_width_slider = ui.launcher_width_slider;
    let launcher_height_slider = ui.launcher_height_slider;
    let launcher_preview_text = ui.launcher_preview_text;
    let smooth_caret = ui.smooth_caret;
    let use_system_accent = ui.use_system_accent;
    let show_results = ui.show_results;
    let settings_tab = ui.settings_tab;
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
                                    stage_selection_color(
                                        use_system_accent,
                                        custom_selection_color,
                                        selection_color,
                                        &settings_for_apply,
                                        &mut *ctx,
                                    );
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
}
