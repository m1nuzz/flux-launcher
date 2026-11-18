// 设置页运行时装配。
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use crate::everything;
use crate::i18n;
use crate::query;
use crate::settings_view;
use crate::ui_helpers;
use crate::update_state;
use crate::updater;
use crate::window_state;
use crate::COMPACT_WINDOW_HEIGHT;
use flux_core::{SearchResult, Settings};
use windui::app::WindowSizeHandle;
use windui::prelude::*;

type SettingsCallback = Rc<dyn Fn()>;
type SettingsCallbackSlot = Rc<RefCell<Option<SettingsCallback>>>;
type SettingsDirtyCallback = Rc<dyn Fn() -> bool>;
type SettingsDirtyCallbackSlot = Rc<RefCell<Option<SettingsDirtyCallback>>>;
type SettingsCallbackPair = (SettingsCallback, SettingsCallback);

fn build_priority_list(
    priorities: Signal<Vec<flux_core::PriorityEntry>>,
    results: Signal<Vec<SearchResult>>,
    provider_results: &Rc<RefCell<query::ProviderResults>>,
    query: Signal<String>,
    shared_settings: &Arc<RwLock<Settings>>,
) -> Element {
    settings_view::priority_list(
        priorities,
        results,
        Rc::clone(provider_results),
        query,
        Arc::clone(shared_settings),
    )
}

struct SettingsUpdateInputs {
    update_available_for_install: Signal<Option<updater::StableUpdate>>,
    update_check_in_flight_for_apply: Rc<Cell<bool>>,
    update_check_in_flight_for_check_now: Rc<Cell<bool>>,
    update_checks_enabled_for_apply: Signal<bool>,
    update_install_in_flight_for_ui: Rc<Cell<bool>>,
    update_install_progress_for_ui: Signal<Option<(String, updater::DownloadProgress)>>,
    update_install_sender_for_ui: Sender<update_state::UpdateInstallResponse>,
    update_installing_for_ui: Signal<bool>,
    update_interval_hours_for_apply: Signal<String>,
    update_sender_for_apply: Sender<updater::UpdateCheckResponse>,
    update_sender_for_check_now: Sender<updater::UpdateCheckResponse>,
    update_status_for_apply: Signal<String>,
    update_status_for_install: Signal<String>,
}

struct SettingsUpdateInputsSource {
    update_available: Signal<Option<updater::StableUpdate>>,
    update_check_in_flight: Rc<Cell<bool>>,
    update_checks_enabled: Signal<bool>,
    update_install_in_flight: Rc<Cell<bool>>,
    update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    update_install_sender: Sender<update_state::UpdateInstallResponse>,
    update_installing: Signal<bool>,
    update_interval_hours: Signal<String>,
    update_sender: Sender<updater::UpdateCheckResponse>,
    update_status: Signal<String>,
}

fn build_settings_update_inputs(source: SettingsUpdateInputsSource) -> SettingsUpdateInputs {
    let SettingsUpdateInputsSource {
        update_available,
        update_check_in_flight,
        update_checks_enabled,
        update_install_in_flight,
        update_install_progress,
        update_install_sender,
        update_installing,
        update_interval_hours,
        update_sender,
        update_status,
    } = source;
    let update_sender_for_apply = update_sender.clone();
    SettingsUpdateInputs {
        update_available_for_install: update_available,
        update_check_in_flight_for_apply: update_check_in_flight.clone(),
        update_check_in_flight_for_check_now: update_check_in_flight,
        update_checks_enabled_for_apply: update_checks_enabled,
        update_install_in_flight_for_ui: update_install_in_flight,
        update_install_progress_for_ui: update_install_progress,
        update_install_sender_for_ui: update_install_sender,
        update_installing_for_ui: update_installing,
        update_interval_hours_for_apply: update_interval_hours,
        update_sender_for_apply: update_sender_for_apply.clone(),
        update_sender_for_check_now: update_sender_for_apply,
        update_status_for_apply: update_status,
        update_status_for_install: update_status,
    }
}

struct SettingsApplyInputs {
    everything_status_for_apply: Signal<String>,
    game_mode_status_for_apply: Signal<String>,
    google_alias_for_apply: Signal<String>,
    google_enabled_for_apply: Signal<bool>,
    history_cursor_for_clear: Signal<Option<usize>>,
    history_for_clear: Rc<RefCell<Vec<String>>>,
    i18n_hub_for_apply: i18n::I18nHub,
    language_preference_for_apply: Signal<usize>,
    obsidian_alias_for_apply: Signal<String>,
    obsidian_enabled_for_apply: Signal<bool>,
    position_for_apply: windui::app::WindowPositionHandle,
    settings_for_apply: Arc<RwLock<Settings>>,
    settings_for_clear_history: Arc<RwLock<Settings>>,
    settings_visible_for_apply: Signal<bool>,
    size_for_apply: WindowSizeHandle,
    start_with_windows_for_apply: Signal<bool>,
    system_commands_enabled_for_apply: Signal<bool>,
}

struct SettingsApplyInputsSource {
    shared_settings: Arc<RwLock<Settings>>,
    window_position: windui::app::WindowPositionHandle,
    window_size: WindowSizeHandle,
    query_history: Rc<RefCell<Vec<String>>>,
    everything_status: Signal<String>,
    game_mode_status: Signal<String>,
    google_alias: Signal<String>,
    google_enabled: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    i18n_hub: i18n::I18nHub,
    language_preference: Signal<usize>,
    obsidian_alias: Signal<String>,
    obsidian_enabled: Signal<bool>,
    settings_visible: Signal<bool>,
    start_with_windows: Signal<bool>,
    system_commands_enabled: Signal<bool>,
}

struct ActivationPanelInputs {
    activation_display_for_apply: Signal<String>,
    activation_display_for_ui: Signal<String>,
    activation_handle_for_apply: HotkeyHandle,
    activation_handle_for_record_button: HotkeyHandle,
    activation_recording_for_apply: Signal<bool>,
    activation_recording_for_record_button: Signal<bool>,
}

struct LauncherAppearanceInputs {
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_height_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    visual_preview_generation_for_height_reset: Signal<u64>,
    visual_preview_generation_for_width_reset: Signal<u64>,
}

struct LauncherAppearanceInputsSource {
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_height_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    visual_preview_generation: Signal<u64>,
}

struct EverythingPanelInputs {
    enabled: Signal<bool>,
    enabled_for_apply: Signal<bool>,
    detection: Arc<everything::EverythingDetectionSlot>,
    installed: Signal<bool>,
    status: Signal<String>,
    status_for_apply: Signal<String>,
}

struct EverythingPanelInputsSource {
    enabled: Signal<bool>,
    detection: Arc<everything::EverythingDetectionSlot>,
    installed: Signal<bool>,
    status: Signal<String>,
    status_for_apply: Signal<String>,
}

fn build_everything_panel_inputs(source: EverythingPanelInputsSource) -> EverythingPanelInputs {
    EverythingPanelInputs {
        enabled: source.enabled,
        enabled_for_apply: source.enabled,
        detection: source.detection,
        installed: source.installed,
        status: source.status,
        status_for_apply: source.status_for_apply,
    }
}

fn build_launcher_appearance_inputs(
    source: LauncherAppearanceInputsSource,
) -> LauncherAppearanceInputs {
    LauncherAppearanceInputs {
        launcher_width: source.launcher_width,
        launcher_height: source.launcher_height,
        launcher_width_input: source.launcher_width_input,
        launcher_height_input: source.launcher_height_input,
        launcher_width_slider: source.launcher_width_slider,
        launcher_height_slider: source.launcher_height_slider,
        launcher_preview_text: source.launcher_preview_text,
        visual_preview_generation_for_height_reset: source.visual_preview_generation,
        visual_preview_generation_for_width_reset: source.visual_preview_generation,
    }
}

fn build_activation_panel_inputs(
    activation_display: Signal<String>,
    activation_handle: HotkeyHandle,
    activation_recording: Signal<bool>,
) -> ActivationPanelInputs {
    ActivationPanelInputs {
        activation_display_for_apply: activation_display,
        activation_display_for_ui: activation_display,
        activation_handle_for_apply: activation_handle.clone(),
        activation_handle_for_record_button: activation_handle,
        activation_recording_for_apply: activation_recording,
        activation_recording_for_record_button: activation_recording,
    }
}

fn build_settings_apply_inputs(source: SettingsApplyInputsSource) -> SettingsApplyInputs {
    SettingsApplyInputs {
        everything_status_for_apply: source.everything_status,
        game_mode_status_for_apply: source.game_mode_status,
        google_alias_for_apply: source.google_alias,
        google_enabled_for_apply: source.google_enabled,
        history_cursor_for_clear: source.history_cursor,
        history_for_clear: source.query_history,
        i18n_hub_for_apply: source.i18n_hub,
        language_preference_for_apply: source.language_preference,
        obsidian_alias_for_apply: source.obsidian_alias,
        obsidian_enabled_for_apply: source.obsidian_enabled,
        position_for_apply: source.window_position,
        settings_for_apply: Arc::clone(&source.shared_settings),
        settings_for_clear_history: source.shared_settings,
        settings_visible_for_apply: source.settings_visible,
        size_for_apply: source.window_size,
        start_with_windows_for_apply: source.start_with_windows,
        system_commands_enabled_for_apply: source.system_commands_enabled,
    }
}

pub(crate) struct SettingsCallbacksInputs {
    pub(crate) shared_settings: Arc<RwLock<Settings>>,
    pub(crate) i18n_hub: i18n::I18nHub,
    pub(crate) activation_handle: HotkeyHandle,
    pub(crate) window_size: WindowSizeHandle,
    pub(crate) window_position: windui::app::WindowPositionHandle,
    pub(crate) settings_ui: settings_view::SettingsUiState,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) language_preference: Signal<usize>,
    pub(crate) activation_key: Signal<String>,
    pub(crate) activation_ctrl: Signal<bool>,
    pub(crate) activation_alt: Signal<bool>,
    pub(crate) activation_shift: Signal<bool>,
    pub(crate) activation_meta: Signal<bool>,
    pub(crate) activation_display: Signal<String>,
    pub(crate) activation_recording: Signal<bool>,
    pub(crate) ignore_fullscreen: Signal<bool>,
    pub(crate) game_mode: Signal<bool>,
    pub(crate) game_mode_status: Signal<String>,
    pub(crate) smooth_caret: Signal<bool>,
    pub(crate) caret_duration: Signal<String>,
    pub(crate) switch_to_english_layout: Signal<bool>,
    pub(crate) use_system_accent: Signal<bool>,
    pub(crate) custom_selection_color: Signal<String>,
    pub(crate) selection_color: Signal<Color>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) launcher_width_input: Signal<String>,
    pub(crate) launcher_height_input: Signal<String>,
    pub(crate) launcher_width_slider: Signal<f32>,
    pub(crate) launcher_height_slider: Signal<f32>,
    pub(crate) launcher_preview_text: Signal<String>,
    pub(crate) clear_query_on_activation: Signal<bool>,
    pub(crate) start_with_windows: Signal<bool>,
    pub(crate) everything_enabled: Signal<bool>,
    pub(crate) update_checks_enabled: Signal<bool>,
    pub(crate) update_interval_hours: Signal<String>,
    pub(crate) auto_install_updates: Signal<bool>,
    pub(crate) obsidian_enabled: Signal<bool>,
    pub(crate) obsidian_alias: Signal<String>,
    pub(crate) google_enabled: Signal<bool>,
    pub(crate) google_alias: Signal<String>,
    pub(crate) system_commands_enabled: Signal<bool>,
    pub(crate) monitor_preference: Signal<usize>,
    pub(crate) everything_installed: Signal<bool>,
    pub(crate) everything_status: Signal<String>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) cancel_settings_slot: SettingsCallbackSlot,
    pub(crate) settings_dirty_slot: SettingsDirtyCallbackSlot,
}

fn build_settings_callbacks(inputs: SettingsCallbacksInputs) -> SettingsCallbackPair {
    let SettingsCallbacksInputs {
        shared_settings,
        i18n_hub,
        activation_handle,
        window_size,
        window_position,
        settings_ui,
        settings_visible,
        language_preference,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_display,
        activation_recording,
        ignore_fullscreen,
        game_mode,
        game_mode_status,
        smooth_caret,
        caret_duration,
        switch_to_english_layout,
        use_system_accent,
        custom_selection_color,
        selection_color,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        clear_query_on_activation,
        start_with_windows,
        everything_enabled,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
        monitor_preference,
        everything_installed,
        everything_status,
        show_results,
        cancel_settings_slot,
        settings_dirty_slot,
    } = inputs;
    let settings = Arc::clone(&shared_settings);
    let restore = settings_view::SettingsRestoreSignals {
        settings_visible,
        discard_confirmation: settings_ui.discard_confirmation,
        language_preference,
        i18n_hub,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_display,
        activation_recording,
        activation_handle,
        ignore_fullscreen,
        game_mode,
        game_mode_status,
        smooth_caret,
        caret_duration,
        switch_to_english_layout,
        use_system_accent,
        custom_selection_color,
        selection_color,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        clear_query_on_activation,
        start_with_windows,
        everything_enabled,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
        monitor_preference,
        everything_installed,
        everything_status,
    };
    let restore_for_reset = restore.clone();
    let settings_for_cancel = Arc::clone(&settings);
    let restore_for_cancel = restore.clone();
    let cancel_settings = Rc::new(move || {
        let Ok(saved) = settings_for_cancel.read() else {
            return;
        };
        let saved = saved.clone();
        settings_view::restore_saved_settings(&saved, &restore_for_cancel);

        let target_height = if show_results.get() {
            i32::from(saved.launcher_height)
        } else {
            COMPACT_WINDOW_HEIGHT
        };
        window_state::request_monitor_position(
            &window_position,
            saved.monitor_preference,
            i32::from(saved.launcher_width),
            target_height,
        );
        window_size.set(i32::from(saved.launcher_width), target_height);
    }) as Rc<dyn Fn()>;
    let reset_settings = {
        let settings = Arc::clone(&shared_settings);
        Rc::new(move || {
            let Ok(saved) = settings.read() else {
                return;
            };
            let defaults = saved.default_editable_template();
            settings_view::apply_settings_to_signals(&defaults, &restore_for_reset, false);
        }) as Rc<dyn Fn()>
    };
    *cancel_settings_slot.borrow_mut() = Some(Rc::clone(&cancel_settings));
    let settings_for_dirty = Arc::clone(&settings);
    let dirty_signals = restore;
    *settings_dirty_slot.borrow_mut() = Some(Rc::new(move || {
        settings_for_dirty
            .read()
            .map(|saved| settings_view::settings_draft_is_dirty(&saved, &dirty_signals))
            .unwrap_or(false)
    }));
    (cancel_settings, reset_settings)
}

fn build_settings_panel(context: settings_view::SettingsPanelContext) -> Element {
    settings_view::settings_panel(context)
}

pub(crate) struct LauncherSettingsPanelResult {
    pub(crate) panel: Element,
    pub(crate) cancel_settings: SettingsCallback,
}

pub(crate) struct LauncherSettingsPanelInputs {
    pub(crate) callbacks: SettingsCallbacksInputs,
    pub(crate) everything_detection: Arc<everything::EverythingDetectionSlot>,
    pub(crate) query_history: Rc<RefCell<Vec<String>>>,
    pub(crate) history_cursor: Signal<Option<usize>>,
    pub(crate) priorities: Signal<Vec<flux_core::PriorityEntry>>,
    pub(crate) results: Signal<Vec<SearchResult>>,
    pub(crate) provider_results: Rc<RefCell<query::ProviderResults>>,
    pub(crate) query: Signal<String>,
    pub(crate) settings_tab: Signal<usize>,
    pub(crate) update_available: Signal<Option<updater::StableUpdate>>,
    pub(crate) update_check_in_flight: Rc<Cell<bool>>,
    pub(crate) update_install_in_flight: Rc<Cell<bool>>,
    pub(crate) update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    pub(crate) update_install_sender: Sender<update_state::UpdateInstallResponse>,
    pub(crate) update_installing: Signal<bool>,
    pub(crate) update_sender: Sender<updater::UpdateCheckResponse>,
    pub(crate) update_status: Signal<String>,
    pub(crate) visual_preview_generation: Signal<u64>,
}

pub(crate) fn build_launcher_settings_panel(
    inputs: LauncherSettingsPanelInputs,
) -> LauncherSettingsPanelResult {
    let LauncherSettingsPanelInputs {
        callbacks,
        everything_detection,
        query_history,
        history_cursor,
        priorities,
        results,
        provider_results,
        query,
        settings_tab,
        update_available,
        update_check_in_flight,
        update_install_in_flight,
        update_install_progress,
        update_install_sender,
        update_installing,
        update_sender,
        update_status,
        visual_preview_generation,
    } = inputs;
    let SettingsCallbacksInputs {
        shared_settings,
        i18n_hub,
        activation_handle,
        window_size,
        window_position,
        settings_ui,
        settings_visible,
        language_preference,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_display,
        activation_recording,
        ignore_fullscreen,
        game_mode,
        game_mode_status,
        smooth_caret,
        caret_duration,
        switch_to_english_layout,
        use_system_accent,
        custom_selection_color,
        selection_color,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        clear_query_on_activation,
        start_with_windows,
        everything_enabled,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
        monitor_preference,
        everything_installed,
        everything_status,
        show_results,
        cancel_settings_slot,
        settings_dirty_slot,
    } = callbacks;
    let (cancel_settings, reset_settings) = build_settings_callbacks(SettingsCallbacksInputs {
        shared_settings: Arc::clone(&shared_settings),
        i18n_hub: i18n_hub.clone(),
        activation_handle: activation_handle.clone(),
        window_size: window_size.clone(),
        window_position: window_position.clone(),
        settings_ui,
        settings_visible,
        language_preference,
        activation_key,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        activation_display,
        activation_recording,
        ignore_fullscreen,
        game_mode,
        game_mode_status,
        smooth_caret,
        caret_duration,
        switch_to_english_layout,
        use_system_accent,
        custom_selection_color,
        selection_color,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        clear_query_on_activation,
        start_with_windows,
        everything_enabled,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
        monitor_preference,
        everything_installed,
        everything_status,
        show_results,
        cancel_settings_slot,
        settings_dirty_slot,
    });
    let settings_apply_inputs = build_settings_apply_inputs(SettingsApplyInputsSource {
        shared_settings: Arc::clone(&shared_settings),
        window_position: window_position.clone(),
        window_size: window_size.clone(),
        query_history,
        everything_status,
        game_mode_status,
        google_alias,
        google_enabled,
        history_cursor,
        i18n_hub: i18n_hub.clone(),
        language_preference,
        obsidian_alias,
        obsidian_enabled,
        settings_visible,
        start_with_windows,
        system_commands_enabled,
    });
    let activation_panel_inputs =
        build_activation_panel_inputs(activation_display, activation_handle, activation_recording);
    let update_panel_inputs = build_settings_update_inputs(SettingsUpdateInputsSource {
        update_available,
        update_check_in_flight,
        update_checks_enabled,
        update_install_in_flight,
        update_install_progress,
        update_install_sender,
        update_installing,
        update_interval_hours,
        update_sender,
        update_status,
    });
    let appearance_panel_inputs =
        build_launcher_appearance_inputs(LauncherAppearanceInputsSource {
            launcher_width,
            launcher_height,
            launcher_width_input,
            launcher_height_input,
            launcher_width_slider,
            launcher_height_slider,
            launcher_preview_text,
            visual_preview_generation,
        });
    let everything_panel_inputs = build_everything_panel_inputs(EverythingPanelInputsSource {
        enabled: everything_enabled,
        detection: everything_detection,
        installed: everything_installed,
        status: everything_status,
        status_for_apply: settings_apply_inputs.everything_status_for_apply,
    });
    let priorities_empty = ui_helpers::priorities_empty(priorities);
    let priority_list = build_priority_list(
        priorities,
        results,
        &provider_results,
        query,
        &shared_settings,
    );
    let panel = build_settings_panel(settings_view::SettingsPanelContext {
        activation_alt,
        activation_ctrl,
        activation_display_for_apply: activation_panel_inputs.activation_display_for_apply,
        activation_display_for_ui: activation_panel_inputs.activation_display_for_ui,
        activation_handle_for_apply: activation_panel_inputs.activation_handle_for_apply.clone(),
        activation_handle_for_record_button: activation_panel_inputs
            .activation_handle_for_record_button
            .clone(),
        activation_key,
        activation_meta,
        activation_recording_for_apply: activation_panel_inputs.activation_recording_for_apply,
        activation_recording_for_record_button: activation_panel_inputs
            .activation_recording_for_record_button,
        activation_shift,
        everything_enabled: everything_panel_inputs.enabled,
        everything_enabled_for_apply: everything_panel_inputs.enabled_for_apply,
        auto_install_updates,
        auto_install_updates_for_apply: auto_install_updates,
        reset_settings,
        caret_duration,
        clear_query_on_activation,
        custom_selection_color,
        everything_detection: everything_panel_inputs.detection,
        everything_installed: everything_panel_inputs.installed,
        everything_status: everything_panel_inputs.status,
        everything_status_for_apply: everything_panel_inputs.status_for_apply,
        game_mode,
        game_mode_status_for_apply: settings_apply_inputs.game_mode_status_for_apply,
        google_alias,
        google_alias_for_apply: settings_apply_inputs.google_alias_for_apply,
        google_enabled,
        google_enabled_for_apply: settings_apply_inputs.google_enabled_for_apply,
        history_cursor_for_clear: settings_apply_inputs.history_cursor_for_clear,
        history_for_clear: settings_apply_inputs.history_for_clear.clone(),
        i18n_hub,
        i18n_hub_for_apply: settings_apply_inputs.i18n_hub_for_apply.clone(),
        ignore_fullscreen,
        language_preference,
        language_preference_for_apply: settings_apply_inputs.language_preference_for_apply,
        launcher_height: appearance_panel_inputs.launcher_height,
        launcher_height_input: appearance_panel_inputs.launcher_height_input,
        launcher_height_slider: appearance_panel_inputs.launcher_height_slider,
        launcher_preview_text: appearance_panel_inputs.launcher_preview_text,
        launcher_width: appearance_panel_inputs.launcher_width,
        launcher_width_input: appearance_panel_inputs.launcher_width_input,
        launcher_width_slider: appearance_panel_inputs.launcher_width_slider,
        monitor_preference,
        obsidian_alias,
        obsidian_alias_for_apply: settings_apply_inputs.obsidian_alias_for_apply,
        obsidian_enabled,
        obsidian_enabled_for_apply: settings_apply_inputs.obsidian_enabled_for_apply,
        position_for_apply: settings_apply_inputs.position_for_apply.clone(),
        priorities_empty,
        priority_list,
        selection_color,
        settings_for_apply: settings_apply_inputs.settings_for_apply.clone(),
        settings_for_clear_history: settings_apply_inputs.settings_for_clear_history.clone(),
        settings_tab,
        settings_visible_for_apply: settings_apply_inputs.settings_visible_for_apply,
        shared_settings,
        show_results,
        size_for_apply: settings_apply_inputs.size_for_apply.clone(),
        smooth_caret,
        start_with_windows,
        start_with_windows_for_apply: settings_apply_inputs.start_with_windows_for_apply,
        switch_to_english_layout,
        system_commands_enabled,
        system_commands_enabled_for_apply: settings_apply_inputs.system_commands_enabled_for_apply,
        update_available_for_install: update_panel_inputs.update_available_for_install,
        update_check_in_flight_for_apply: update_panel_inputs
            .update_check_in_flight_for_apply
            .clone(),
        update_check_in_flight_for_check_now: update_panel_inputs
            .update_check_in_flight_for_check_now
            .clone(),
        update_checks_enabled,
        update_checks_enabled_for_apply: update_panel_inputs.update_checks_enabled_for_apply,
        update_install_in_flight_for_ui: update_panel_inputs
            .update_install_in_flight_for_ui
            .clone(),
        update_install_progress_for_ui: update_panel_inputs.update_install_progress_for_ui,
        update_install_sender_for_ui: update_panel_inputs.update_install_sender_for_ui.clone(),
        update_installing_for_ui: update_panel_inputs.update_installing_for_ui,
        update_interval_hours,
        update_interval_hours_for_apply: update_panel_inputs.update_interval_hours_for_apply,
        update_sender_for_apply: update_panel_inputs.update_sender_for_apply.clone(),
        update_sender_for_check_now: update_panel_inputs.update_sender_for_check_now.clone(),
        update_status,
        update_status_for_apply: update_panel_inputs.update_status_for_apply,
        update_status_for_install: update_panel_inputs.update_status_for_install,
        use_system_accent,
        visual_preview_generation_for_height_reset: appearance_panel_inputs
            .visual_preview_generation_for_height_reset,
        visual_preview_generation_for_width_reset: appearance_panel_inputs
            .visual_preview_generation_for_width_reset,
    });
    LauncherSettingsPanelResult {
        panel,
        cancel_settings,
    }
}
