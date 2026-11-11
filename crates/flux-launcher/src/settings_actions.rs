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
    parse_selection_color, push_selection_appearance, rgb_to_hsv, selection_color_hex,
    selection_rgb_for_settings, stage_selection_color,
};
use super::ui_constants::COMPACT_WINDOW_HEIGHT;
use super::update_tasks::{
    request_update_check, request_update_install, save_settings_async, update_check_due,
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
    let mut check_due = false;
    if let Ok(mut settings) = ui.shared_settings.write() {
        let auto_enable_previous = settings.auto_enable_everything;
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
        // Starting Everything walks its process and service state, which is the
        // slowest thing this click could do, so it runs only when the switch
        // itself moved. An unrelated Apply keeps the status text it already has.
        if settings.auto_enable_everything != auto_enable_previous {
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
        }
        eprintln!(
            "Settings apply dispatched: {}x{}",
            settings.launcher_width, settings.launcher_height
        );
        check_due = settings.update_checks_enabled && update_check_due(&settings);
    }
    // Persist off the UI thread and leave the window exactly where it is: Apply
    // used to write the file, sync the registry and shrink back to the search
    // strip on the click, which read as a freeze and threw away the page.
    save_settings_async(&ui.shared_settings);
    let start_with_windows = ui.start_with_windows.get();
    let _ = std::thread::Builder::new()
        .name(String::from("flux-startup-sync"))
        .spawn(move || {
            if let Err(error) = startup::set_enabled(start_with_windows) {
                eprintln!("Could not synchronize Windows startup setting: {error}");
            }
        });
    if check_due {
        ui.update_status
            .set(String::from("Checking stable GitHub releases..."));
        request_update_check(ui.update_sender.clone(), &ui.update_check_in_flight);
    }
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

/// Discard every pending edit and put the draft signals back on the values that
/// are actually stored, without touching the settings file or the live hotkey
/// registrations (those only change on Apply). The size pair travels back on the
/// same eased curve as Reset tab defaults instead of snapping.
pub(crate) fn revert_settings(ui: &SettingsUi, ctx: &mut EventCtx) {
    let stored = match ui.shared_settings.read() {
        Ok(settings) => (*settings).clone(),
        Err(_) => {
            ctx.toast_ok("Could not lock Flux settings");
            return;
        }
    };
    reset_general(ui, &stored);
    reset_visual(ui, &stored);
    reset_plugins(ui, &stored);
    ui.priorities.set(stored.priority_entries.clone());
    // A recorder still armed has its global hotkey unregistered and swallows
    // every keystroke, so leaving recording is part of reverting the drafts.
    ui.activation_recording.set(false);
    ui.activation_handle.set_enabled(true);
    ui.game_mode_recording.set(false);
    ui.game_mode_handle
        .set_enabled(stored.game_mode_hotkey_enabled);
    ui.game_mode_status.set(game_mode_label(stored.game_mode));
    // The color controls stage the live selection appearance as they are used;
    // push the stored appearance back so Revert is visible now, not at restart.
    push_selection_appearance(
        &ui.theme_handle,
        ui.selection_color,
        selection_rgb_for_settings(&stored),
    );
    eprintln!("Settings revert dispatched");
    ctx.toast_ok("Changes discarded");
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
    // Let the frame callback ease the sliders instead of snapping: the thumb and
    // the Settings/preview pair both travel on the same ease-in-out curve.
    let now = std::time::Instant::now();
    ui.width_ease.set(Some((
        ui.launcher_width_slider.get(),
        dimension_slider_fraction(
            defaults.launcher_width,
            MIN_LAUNCHER_WIDTH,
            MAX_LAUNCHER_WIDTH,
        ),
        now,
    )));
    ui.height_ease.set(Some((
        ui.launcher_height_slider.get(),
        dimension_slider_fraction(
            defaults.launcher_height,
            MIN_LAUNCHER_HEIGHT,
            MAX_LAUNCHER_HEIGHT,
        ),
        now,
    )));
}

fn reset_plugins(ui: &SettingsUi, defaults: &Settings) {
    ui.auto_enable_everything
        .set(defaults.auto_enable_everything);
    ui.obsidian_enabled.set(defaults.obsidian_enabled);
    ui.obsidian_alias.set(defaults.obsidian_alias.clone());
    ui.google_enabled.set(defaults.google_enabled);
    ui.google_alias.set(defaults.google_alias.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_snapshot::ProviderResults;
    use crate::update_tasks::UpdateInstallResponse;
    use flux_core::{MonitorPreference, PriorityEntry, SearchResult};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::{Arc, RwLock};
    use windui::prelude::*;

    /// The Settings panel's draft signals, built the way `main` builds them but
    /// without a window: the signals and handles are plain shared state, so the
    /// footer actions can run against them under `windui::testing`.
    fn fixture(stored: &Settings) -> SettingsUi {
        let mut app = App::new("Flux settings test", 320, 200);
        let theme_handle = app.theme_handle();
        let window_size = app.window_size_handle();
        let window_position = app.window_position_handle();
        let activation_handle = app.hotkey_handle(Hotkey::new(Key::Space).alt(), |_| {});
        let game_mode_handle = app.hotkey_handle(Hotkey::new(Key::Char('G')).ctrl(), |_| {});
        let update_install_sender = app.channel::<UpdateInstallResponse>(|_, _| {});
        let update_sender = app.channel::<updater::UpdateCheckResponse>(|_, _| {});
        SettingsUi {
            shared_settings: Arc::new(RwLock::new(stored.clone())),
            query_history: Rc::new(RefCell::new(Vec::new())),
            stats_usage: signal(String::new()),
            stats_top: signal(String::new()),
            clear_history_confirm: signal(false),
            history_cursor: signal(None::<usize>),
            priorities: signal(stored.priority_entries.clone()),
            update_status: signal(String::new()),
            update_available: signal(None::<updater::StableUpdate>),
            update_install_progress: signal(None::<(String, updater::DownloadProgress)>),
            update_installing: signal(false),
            update_install_sender,
            update_install_in_flight: Rc::new(Cell::new(false)),
            update_sender,
            update_check_in_flight: Rc::new(Cell::new(false)),
            game_mode: signal(stored.game_mode),
            game_mode_status: signal(game_mode_label(stored.game_mode)),
            settings_visible: signal(false),
            settings_tab: signal(0),
            width_ease: Rc::new(Cell::new(None)),
            height_ease: Rc::new(Cell::new(None)),
            show_results: signal(false),
            activation_display: signal(hotkeys::display_config(&stored.activation_hotkey)),
            activation_key: signal(stored.activation_hotkey.key.clone()),
            activation_ctrl: signal(stored.activation_hotkey.ctrl),
            activation_alt: signal(stored.activation_hotkey.alt),
            activation_shift: signal(stored.activation_hotkey.shift),
            activation_meta: signal(stored.activation_hotkey.meta),
            activation_recording: signal(false),
            activation_handle,
            game_mode_key: signal(stored.game_mode_hotkey.key.clone()),
            game_mode_display: signal(hotkeys::display_config(&stored.game_mode_hotkey)),
            game_mode_recording: signal(false),
            game_mode_ctrl: signal(stored.game_mode_hotkey.ctrl),
            game_mode_alt: signal(stored.game_mode_hotkey.alt),
            game_mode_shift: signal(stored.game_mode_hotkey.shift),
            game_mode_meta: signal(stored.game_mode_hotkey.meta),
            game_mode_hotkey_enabled: signal(stored.game_mode_hotkey_enabled),
            game_mode_handle,
            ignore_fullscreen: signal(stored.ignore_hotkeys_in_fullscreen),
            smooth_caret: signal(stored.smooth_caret),
            switch_to_english_layout: signal(stored.switch_to_english_layout),
            use_system_accent: signal(stored.use_system_accent),
            selection_color: signal(crate::theme_text::selection_color_for_settings(stored)),
            theme_handle,
            custom_selection_color: signal(selection_color_hex(stored.custom_selection_color)),
            color_hsv: signal(rgb_to_hsv(
                ((stored.custom_selection_color >> 16) & 0xff) as u8,
                ((stored.custom_selection_color >> 8) & 0xff) as u8,
                (stored.custom_selection_color & 0xff) as u8,
            )),
            caret_duration: signal(stored.smooth_caret_duration_ms.to_string()),
            launcher_width: signal(stored.launcher_width),
            launcher_height: signal(stored.launcher_height),
            launcher_width_input: signal(stored.launcher_width.to_string()),
            launcher_height_input: signal(stored.launcher_height.to_string()),
            launcher_width_slider: signal(dimension_slider_fraction(
                stored.launcher_width,
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH,
            )),
            launcher_height_slider: signal(dimension_slider_fraction(
                stored.launcher_height,
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT,
            )),
            launcher_preview_text: signal(String::new()),
            clear_query_on_activation: signal(stored.clear_query_on_activation),
            start_with_windows: signal(stored.start_with_windows),
            auto_enable_everything: signal(stored.auto_enable_everything),
            update_checks_enabled: signal(stored.update_checks_enabled),
            update_interval_hours: signal(stored.update_interval_hours.to_string()),
            auto_install_updates: signal(stored.auto_install_updates),
            obsidian_enabled: signal(stored.obsidian_enabled),
            obsidian_alias: signal(stored.obsidian_alias.clone()),
            google_enabled: signal(stored.google_enabled),
            google_alias: signal(stored.google_alias.clone()),
            everything_status: signal(String::new()),
            everything_installed: signal(true),
            monitor_preference: signal(monitor_preference_index(stored.monitor_preference)),
            window_size,
            window_position,
            settings_hide_request: signal(false),
            query: signal(String::new()),
            results: signal(Vec::<SearchResult>::new()),
            providers: Rc::new(RefCell::new(ProviderResults::default())),
        }
    }

    /// A stored configuration that differs from the defaults on every field the
    /// revert has to put back, so a stray write of defaults cannot pass.
    fn customized_settings() -> Settings {
        Settings {
            activation_hotkey: HotkeyConfig {
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
                key: String::from("K"),
            },
            ignore_hotkeys_in_fullscreen: false,
            game_mode: true,
            game_mode_hotkey_enabled: false,
            smooth_caret: false,
            switch_to_english_layout: false,
            use_system_accent: false,
            custom_selection_color: 0x10_7c_10,
            launcher_width: 512,
            launcher_height: 640,
            clear_query_on_activation: false,
            start_with_windows: false,
            auto_enable_everything: false,
            update_checks_enabled: false,
            update_interval_hours: 6,
            auto_install_updates: true,
            obsidian_enabled: false,
            obsidian_alias: String::from("notes"),
            google_enabled: false,
            google_alias: String::from("web"),
            monitor_preference: MonitorPreference::Primary,
            smooth_caret_duration_ms: 120,
            priority_entries: vec![PriorityEntry {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                target: String::from("C:/Steam.lnk"),
            }],
            ..Settings::default()
        }
    }

    fn edit_every_tab(ui: &SettingsUi) {
        ui.launcher_width_input.set(String::from("700"));
        ui.launcher_height_input.set(String::from("520"));
        ui.launcher_width_slider.set(0.9);
        ui.launcher_height_slider.set(0.9);
        ui.width_ease
            .set(Some((0.9, 0.2, std::time::Instant::now())));
        ui.height_ease
            .set(Some((0.9, 0.2, std::time::Instant::now())));
        ui.smooth_caret.set(true);
        ui.caret_duration.set(String::from("60"));
        ui.use_system_accent.set(true);
        ui.custom_selection_color.set(String::from("#FF0000"));
        ui.clear_query_on_activation.set(true);
        ui.start_with_windows.set(true);
        ui.auto_enable_everything.set(true);
        ui.update_checks_enabled.set(true);
        ui.update_interval_hours.set(String::from("48"));
        ui.auto_install_updates.set(false);
        ui.obsidian_enabled.set(true);
        ui.obsidian_alias.set(String::from("x"));
        ui.google_enabled.set(true);
        ui.google_alias.set(String::from("y"));
        ui.monitor_preference.set(2);
        ui.game_mode.set(false);
        ui.game_mode_hotkey_enabled.set(true);
        ui.ignore_fullscreen.set(true);
        ui.switch_to_english_layout.set(true);
        ui.activation_ctrl.set(false);
        ui.activation_alt.set(true);
        ui.activation_shift.set(false);
        ui.activation_key.set(String::from("Space"));
        ui.priorities.set(Vec::new());
    }

    /// Emulate the tick + frame callback once the size ease has landed: the
    /// slider reaches its target and the tick writes the width fields back.
    fn land_size_ease(ui: &SettingsUi, stored: &Settings) {
        ui.launcher_width_slider.set(dimension_slider_fraction(
            stored.launcher_width,
            MIN_LAUNCHER_WIDTH,
            MAX_LAUNCHER_WIDTH,
        ));
        ui.launcher_height_slider.set(dimension_slider_fraction(
            stored.launcher_height,
            MIN_LAUNCHER_HEIGHT,
            MAX_LAUNCHER_HEIGHT,
        ));
        ui.launcher_width.set(stored.launcher_width);
        ui.launcher_height.set(stored.launcher_height);
        ui.launcher_width_input
            .set(stored.launcher_width.to_string());
        ui.launcher_height_input
            .set(stored.launcher_height.to_string());
    }

    #[test]
    fn revert_puts_every_draft_back_on_the_stored_values() {
        let stored = customized_settings();
        let ui = fixture(&stored);
        edit_every_tab(&ui);
        assert!(has_unsaved_changes(&ui));

        windui::testing::run_with_ctx(|ctx| revert_settings(&ui, ctx));

        assert!(!ui.smooth_caret.get());
        assert_eq!(ui.caret_duration.get(), "120");
        assert!(!ui.use_system_accent.get());
        assert_eq!(ui.custom_selection_color.get(), "#107C10");
        assert_eq!(
            ui.selection_color.get(),
            Color::rgba(16, 124, 16, 84),
            "the live selection color must follow the restored draft"
        );
        assert!(!ui.clear_query_on_activation.get());
        assert!(!ui.start_with_windows.get());
        assert!(!ui.auto_enable_everything.get());
        assert!(!ui.update_checks_enabled.get());
        assert_eq!(ui.update_interval_hours.get(), "6");
        assert!(ui.auto_install_updates.get());
        assert!(!ui.obsidian_enabled.get());
        assert_eq!(ui.obsidian_alias.get(), "notes");
        assert!(!ui.google_enabled.get());
        assert_eq!(ui.google_alias.get(), "web");
        assert_eq!(ui.monitor_preference.get(), 0);
        assert!(ui.game_mode.get());
        assert!(!ui.game_mode_hotkey_enabled.get());
        assert!(!ui.ignore_fullscreen.get());
        assert!(!ui.switch_to_english_layout.get());
        assert_eq!(ui.activation_key.get(), "K");
        assert_eq!(ui.activation_display.get(), "Ctrl + Shift + K");
        assert_eq!(ui.priorities.get(), stored.priority_entries);
        assert_eq!(
            *ui.shared_settings.read().unwrap(),
            stored,
            "Revert is a draft action; Apply remains the only persist point"
        );

        // The size pair travels back on the eased curve instead of snapping;
        // once the ease lands the footer has nothing left to save.
        land_size_ease(&ui, &stored);
        assert_eq!(ui.launcher_width_input.get(), "512");
        assert_eq!(ui.launcher_height_input.get(), "640");
        assert!(!has_unsaved_changes(&ui));
    }

    #[test]
    fn revert_eases_the_size_back_instead_of_snapping() {
        let stored = Settings::default();
        let ui = fixture(&stored);
        ui.launcher_width_input.set(String::from("700"));
        ui.launcher_height_input.set(String::from("520"));
        ui.launcher_width_slider.set(0.9);
        ui.launcher_height_slider.set(0.9);
        assert!(has_unsaved_changes(&ui));

        windui::testing::run_with_ctx(|ctx| revert_settings(&ui, ctx));

        // Same contract as Reset tab defaults: the slider keeps its current
        // position and the frame callback eases it to the stored fraction.
        let (from, to, _) = ui.width_ease.get().expect("width ease starts");
        assert_eq!(from, 0.9);
        assert_eq!(
            to,
            dimension_slider_fraction(
                stored.launcher_width,
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH
            )
        );
        assert_eq!(ui.launcher_width_slider.get(), 0.9);
        let (from, to, _) = ui.height_ease.get().expect("height ease starts");
        assert_eq!(from, 0.9);
        assert_eq!(
            to,
            dimension_slider_fraction(
                stored.launcher_height,
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT
            )
        );

        // A second revert with the glide still running re-targets the same
        // stored value from wherever the thumb is now, and nothing is persisted.
        windui::testing::run_with_ctx(|ctx| revert_settings(&ui, ctx));
        assert!(ui.width_ease.get().is_some());
        assert_eq!(*ui.shared_settings.read().unwrap(), stored);

        land_size_ease(&ui, &stored);
        assert!(!has_unsaved_changes(&ui));
    }
}
