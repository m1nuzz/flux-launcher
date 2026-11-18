// 启动器运行时装配与 Windows 生命周期接线。
use super::*;
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::RwLock;

use std::collections::HashMap;

use crate::activation_hotkey;
use crate::everything;
use crate::hotkeys;
use crate::icons;
use crate::launch;
use crate::launcher_state::{LauncherSetup, LauncherState};
use crate::main_interval;
use crate::main_key_input;
use crate::main_layout;
use crate::provider_state::{self, ProviderChannelContext, ProviderWorkers};
use crate::query;
use crate::settings_state;
use crate::settings_view;
use crate::tray_menu;
use crate::ui_helpers;
use crate::update_state;
use crate::updater;
use crate::window_lifecycle;
use crate::window_state::WindowBootstrap;
use crate::{COMPACT_WINDOW_HEIGHT, LAUNCHER_FONT_FAMILY, SEARCH_INTERVAL, SINGLE_INSTANCE_ID};
use activation_hotkey::ActivationHotkeyState;
use flux_core::{SearchModel, SearchResult, Settings};
use windui::app::{CursorVisibilityHandle, WindowSizeHandle};
use windui::event::Hotkey;
use windui::event::KeyEvent;
use windui::prelude::*;

type SettingsCallback = Rc<dyn Fn()>;
type SettingsCallbackSlot = Rc<RefCell<Option<SettingsCallback>>>;
type SettingsDirtyCallback = Rc<dyn Fn() -> bool>;
type SettingsDirtyCallbackSlot = Rc<RefCell<Option<SettingsDirtyCallback>>>;

pub(crate) fn run_launcher(setup: LauncherSetup, single_instance_disabled: bool) {
    // ========== 1. 解构状态 ==========
    let LauncherSetup {
        settings,
        startup_launch,
        activation_hotkey,
        shared_settings,
        query_history,
        priorities,
        history_cursor,
        history_mode,
        model,
        state,
        search_state,
    } = setup;

    let LauncherState {
        query,
        selected_id,
        selected_index,
        selection_touched,
        action_mode,
        action_index,
        action_scroll_pending,
        recycle_bin_confirmation,
        action_items,
        action_window_slot,
        i18n_hub,
        status,
        update_status,
        update_available,
        update_install_progress,
        update_installing,
        current_sequence,
        game_mode,
        game_mode_status,
        settings_ui,
        settings_visible,
        settings_tab,
        language_preference,
        show_results,
        activation_key,
        activation_display,
        activation_recording,
        activation_ctrl,
        activation_alt,
        activation_shift,
        activation_meta,
        ignore_fullscreen,
        smooth_caret,
        switch_to_english_layout,
        use_system_accent,
        custom_selection_color,
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
        everything_enabled,
        everything_detection,
        update_checks_enabled,
        update_interval_hours,
        auto_install_updates,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
        monitor_preference,
        initial_monitor_preference,
        everything_installed,
        everything_prompt_visible_at_start,
        everything_prompt_visible,
        everything_status,
        selection_color,
        caret_duration,
    } = state;

    let results = search_state.results;
    let provider_results = search_state.provider_results;
    let plugin_actions = search_state.plugin_actions;
    let icon_refresh_generation = search_state.icon_refresh_generation;
    let inline_completion = search_state.inline_completion;

    // ========== 2. 布局 ==========
    let launcher_layout = build_launcher_layout(main_layout::LauncherLayoutContext {
        i18n_hub: i18n_hub.clone(),
        smooth_caret: settings.smooth_caret,
        smooth_caret_duration_ms: settings.smooth_caret_duration_ms,
        query,
        inline_completion,
        results,
        selected_id,
        selected_index,
        selection_touched,
        action_mode,
        action_index,
        action_items,
        action_scroll_pending,
        launcher_width,
        launcher_height,
        selection_color,
        show_results,
        settings_visible,
        icon_refresh_generation,
        history_mode,
        recycle_bin_confirmation,
        everything_prompt_visible,
        everything_status,
        status,
        priorities,
        plugin_actions: Rc::clone(&plugin_actions),
        provider_results: Rc::clone(&provider_results),
        query_history: Rc::clone(&query_history),
        shared_settings: Arc::clone(&shared_settings),
        action_window_slot: Rc::clone(&action_window_slot),
    });
    let scroll_request_for_rows = launcher_layout.scroll_request;
    let query_caret_position = launcher_layout.query_caret_position;
    let launcher_surface = launcher_layout.launcher_surface;

    // ========== 3. 窗口 ==========
    let window_runtime = build_window_runtime(
        settings_visible,
        everything_prompt_visible_at_start,
        launcher_width,
        initial_monitor_preference,
    );
    let mut app = window_runtime.app;
    let window_position = window_runtime.window_position;
    let cursor_visibility = window_runtime.cursor_visibility;
    let window_size = window_runtime.window_size;
    let window_op = window_runtime.window_op;

    // ========== 4. 更新通道 ==========
    let update_runtime = register_update_runtime(
        &mut app,
        Arc::clone(&shared_settings),
        update_status,
        update_available,
        update_install_progress,
        update_installing,
    );
    let update_install_sender = update_runtime.install_sender;
    let update_sender = update_runtime.update_sender;
    let update_install_in_flight = update_runtime.install_in_flight;
    let update_check_in_flight = update_runtime.check_in_flight;

    // ========== 5. Provider 通道 ==========
    let provider_workers = register_provider_workers(
        &mut app,
        ProviderChannelContext {
            query,
            results,
            inline_completion,
            status,
            selected_id,
            selected_index,
            selection_touched,
            current_sequence,
            provider_results: Rc::clone(&provider_results),
            priorities,
            plugin_actions: Rc::clone(&plugin_actions),
            everything_enabled,
            everything_installed,
            everything_status,
        },
    );

    // ========== 6. 热键 ==========
    let cancel_settings_slot: SettingsCallbackSlot = Rc::new(RefCell::new(None));
    let settings_dirty_slot: SettingsDirtyCallbackSlot = Rc::new(RefCell::new(None));
    let activation_cancel_settings = {
        let slot = Rc::clone(&cancel_settings_slot);
        Rc::new(move || {
            if let Some(cancel) = slot.borrow().as_ref() {
                cancel();
            }
        }) as Rc<dyn Fn()>
    };
    let activation_settings_dirty = {
        let slot = Rc::clone(&settings_dirty_slot);
        Rc::new(move || slot.borrow().as_ref().is_some_and(|dirty| dirty())) as Rc<dyn Fn() -> bool>
    };
    let activation_state = ActivationHotkeyState {
        settings: Arc::clone(&shared_settings),
        position: window_position.clone(),
        cursor_visibility: cursor_visibility.clone(),
        size: window_size.clone(),
        query,
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
        scroll_request: scroll_request_for_rows,
        settings_visible,
        discard_confirmation: settings_ui.discard_confirmation,
        cancel_settings: activation_cancel_settings,
        settings_dirty: activation_settings_dirty,
        launcher_width,
        launcher_height,
    };
    let activation_handle =
        register_activation_hotkey(&mut app, activation_hotkey, activation_state);

    // ========== 7. 键盘输入 ==========
    let activation_handle_for_recorder = activation_handle.clone();
    app = register_key_input(
        app,
        main_key_input::KeyInputContext {
            activation_recording_for_keys: activation_recording,
            activation_display_for_keys: activation_display,
            activation_key_for_keys: activation_key,
            activation_ctrl_for_keys: activation_ctrl,
            activation_alt_for_keys: activation_alt,
            activation_shift_for_keys: activation_shift,
            activation_meta_for_keys: activation_meta,
            activation_handle_for_recorder,
            query_for_keys: query,
            query_caret_position_for_keys: query_caret_position,
            results_for_keys: results,
            selected_id_for_keys: selected_id,
            selected_index_for_keys: selected_index,
            scroll_request_for_keys: scroll_request_for_rows,
            selection_touched_for_keys: selection_touched,
            action_mode_for_keys: action_mode,
            action_index_for_keys: action_index,
            action_items_for_keys: action_items,
            action_scroll_pending_for_keys: action_scroll_pending,
            recycle_bin_confirmation_for_keys: recycle_bin_confirmation,
            plugin_actions_for_keys: Rc::clone(&plugin_actions),
            inline_completion_for_keys: inline_completion,
            settings_visible_for_keys: settings_visible,
            query_history_for_keys: Rc::clone(&query_history),
            history_mode_for_keys: history_mode,
            history_cursor_for_keys: history_cursor,
            settings_for_history_for_keys: Arc::clone(&shared_settings),
            settings_for_priority_for_keys: Arc::clone(&shared_settings),
            priorities_for_keys: priorities,
            providers_for_keys: Rc::clone(&provider_results),
            query_for_priority_keys: query,
            window_op_for_keys: window_op.clone(),
            cursor_visibility_for_keys: cursor_visibility.clone(),
            size_for_keys: window_size.clone(),
            show_results_for_keys: show_results,
            launcher_width,
            launcher_height,
        },
    );

    // ========== 8. 托盘 ==========
    let tray = build_tray(tray_menu::TrayContext {
        i18n_hub: i18n_hub.clone(),
        settings: Arc::clone(&shared_settings),
        game_mode,
        game_mode_status,
        settings_visible,
        show_results,
        window_size: window_size.clone(),
        window_position: window_position.clone(),
        launcher_width,
        launcher_height,
    });

    // ========== 9. 设置面板 ==========
    let settings_panel_inputs = launcher_settings::LauncherSettingsPanelInputs {
        callbacks: launcher_settings::SettingsCallbacksInputs {
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
            cancel_settings_slot: Rc::clone(&cancel_settings_slot),
            settings_dirty_slot,
        },
        everything_detection: everything_detection.clone(),
        query_history: Rc::clone(&query_history),
        history_cursor,
        priorities,
        results,
        provider_results: Rc::clone(&provider_results),
        query,
        settings_tab,
        update_available,
        update_check_in_flight: update_check_in_flight.clone(),
        update_install_in_flight,
        update_install_progress,
        update_install_sender,
        update_installing,
        update_sender: update_sender.clone(),
        update_status,
        visual_preview_generation,
    };
    let settings_panel_result =
        launcher_settings::build_launcher_settings_panel(settings_panel_inputs);
    let settings_panel = settings_panel_result.panel;
    let _cancel_settings = settings_panel_result.cancel_settings;

    // ========== 10. 页面和 tick ==========
    let cancel_settings = cancel_settings_slot
        .borrow()
        .as_ref()
        .cloned()
        .unwrap_or_else(|| Rc::new(|| {}));
    let content = build_launcher_content(
        launcher_surface,
        settings_panel,
        settings_ui,
        settings_visible,
        cancel_settings,
    );

    let application_worker = provider_workers.applications;
    let everything_worker = provider_workers.everything;
    let plugin_worker = provider_workers.plugins;
    let native_plugin_worker = provider_workers.native_plugins;

    let tick_context = build_interval_tick_context(IntervalTickContextInputs {
        query_for_interval: query,
        results_for_interval: results,
        width_for_interval: launcher_width,
        height_for_interval: launcher_height,
        width_input_for_interval: launcher_width_input,
        height_input_for_interval: launcher_height_input,
        width_slider_for_interval: launcher_width_slider,
        height_slider_for_interval: launcher_height_slider,
        preview_text_for_interval: launcher_preview_text,
        icon_refresh_generation_for_interval: icon_refresh_generation,
        status_for_interval: status,
        show_results_for_interval: show_results,
        inline_completion_for_interval: inline_completion,
        selection_touched_for_interval: selection_touched,
        sequence_for_interval: current_sequence,
        scroll_request_for_interval: scroll_request_for_rows,
        everything_enabled_for_interval: everything_enabled,
        obsidian_enabled_for_interval: obsidian_enabled,
        obsidian_alias_for_interval: obsidian_alias,
        google_enabled_for_interval: google_enabled,
        google_alias_for_interval: google_alias,
        system_commands_enabled_for_interval: system_commands_enabled,
        history_mode_for_interval: history_mode,
        language_preference_for_interval: language_preference,
        language_preference,
        settings_visible_for_interval: settings_visible,
        settings_tab_for_interval: settings_tab,
        everything_prompt_visible_for_interval: everything_prompt_visible,
        everything_installed_for_interval: everything_installed,
        everything_status_for_interval: everything_status,
        visual_preview_generation_for_interval: visual_preview_generation,
        visual_preview_smoke_for_interval: std::env::var_os("FLUX_SMOKE_VISUAL_SETTINGS").is_some(),
        everything_plugins_smoke_for_interval: std::env::var_os("FLUX_SMOKE_EVERYTHING_PLUGINS")
            .is_some(),
        providers_for_interval: Rc::clone(&provider_results),
        actions_for_interval: Rc::clone(&plugin_actions),
        everything_detection_for_interval: Arc::clone(&everything_detection),
        position_for_interval: window_position.clone(),
        settings_for_interval_geometry: Arc::clone(&shared_settings),
        settings_for_update_interval: Arc::clone(&shared_settings),
        update_sender_for_interval: update_sender.clone(),
        update_check_in_flight_for_interval: Rc::clone(&update_check_in_flight),
        size_for_interval: window_size.clone(),
        launcher_width,
        launcher_height,
        selected_id,
        selected_index,
        action_mode,
        action_index,
        action_items,
        application_worker,
        everything_worker,
        plugin_worker,
        native_plugin_worker,
    });

    let tick_state = build_interval_tick_state(IntervalTickStateInputs {
        model,
        icon_refresh_generation,
        launcher_width,
        launcher_height,
        settings_visible,
        everything_prompt_visible,
        everything_enabled,
        visual_preview_generation,
    });

    // ========== 11. 启动可见性 ==========
    app = apply_startup_visibility(app, startup_launch);

    // ========== 12. 单实例 ==========
    app = register_single_instance(app, single_instance_disabled, window_op);

    // ========== 13. 运行 ==========
    register_launcher_runtime(LauncherRuntimeInputs {
        app,
        tray,
        cancel_settings: Rc::clone(&_cancel_settings),
        shared_settings,
        game_mode,
        game_mode_status,
        tick_context,
        tick_state,
        cursor_visibility,
        settings_visible,
        selection_color,
        window_size: window_size.clone(),
        everything_enabled,
        everything_installed,
        everything_status,
        clear_query_on_activation,
        query,
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
        scroll_request: scroll_request_for_rows,
        launcher_width,
        launcher_height,
        size_for_visibility: window_size.clone(),
        content,
    });
}

struct LauncherRuntimeInputs {
    app: App,
    tray: Tray,
    cancel_settings: Rc<dyn Fn()>,
    shared_settings: Arc<RwLock<Settings>>,
    game_mode: Signal<bool>,
    game_mode_status: Signal<String>,
    tick_context: main_interval::IntervalTickContext,
    tick_state: main_interval::IntervalTickState,
    cursor_visibility: CursorVisibilityHandle,
    settings_visible: Signal<bool>,
    selection_color: Signal<Color>,
    window_size: WindowSizeHandle,
    everything_enabled: Signal<bool>,
    everything_installed: Signal<bool>,
    everything_status: Signal<String>,
    clear_query_on_activation: Signal<bool>,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    show_results: Signal<bool>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<actions::ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    size_for_visibility: WindowSizeHandle,
    content: Element,
}

fn register_launcher_runtime(inputs: LauncherRuntimeInputs) {
    let LauncherRuntimeInputs {
        app,
        tray,
        cancel_settings,
        shared_settings,
        game_mode,
        game_mode_status,
        tick_context,
        mut tick_state,
        cursor_visibility,
        settings_visible,
        selection_color,
        window_size,
        everything_enabled,
        everything_installed,
        everything_status,
        clear_query_on_activation,
        query,
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
        scroll_request,
        launcher_width,
        launcher_height,
        size_for_visibility,
        content,
    } = inputs;
    let app = app
        .tray(tray)
        .hide_on_close()
        .hide_on_deactivate()
        .should_hide_on_deactivate(|| true)
        .on_window_deactivated({
            move || {
                if settings_visible.get() {
                    cancel_settings();
                }
                launch::trace_launch_event("window-deactivated");
            }
        })
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
        .theme(ui_helpers::launcher_theme())
        .content(content);
    let app = register_game_mode_hotkey(app, shared_settings.clone(), game_mode, game_mode_status)
        .on_interval(SEARCH_INTERVAL, move |_ctx| {
            main_interval::handle_interval_tick(&tick_context, &mut tick_state)
        });
    let app = register_window_show(
        app,
        shared_settings.clone(),
        cursor_visibility,
        settings_visible,
        selection_color,
        window_size.clone(),
    );
    register_window_hide(WindowHideInputs {
        app: register_window_activated(
            app,
            shared_settings.clone(),
            everything_enabled,
            everything_installed,
            everything_status,
        ),
        shared_settings,
        settings_visible,
        clear_query_on_activation,
        query,
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
        scroll_request,
        launcher_width,
        launcher_height,
        window_size: size_for_visibility,
    })
    .run();
}

fn apply_startup_visibility(app: App, startup_launch: bool) -> App {
    if startup_launch {
        app.start_hidden()
    } else {
        app
    }
}

fn register_provider_workers(app: &mut App, context: ProviderChannelContext) -> ProviderWorkers {
    let channels = provider_state::register_provider_channels(app, context);
    ProviderWorkers::new(
        channels.application_sender,
        channels.everything_sender,
        channels.plugin_sender,
        channels.native_sender,
    )
}

fn build_launcher_layout(
    context: main_layout::LauncherLayoutContext,
) -> main_layout::LauncherLayout {
    main_layout::build_launcher_layout(context)
}

fn build_window_runtime(
    settings_visible: Signal<bool>,
    everything_prompt_visible_at_start: bool,
    launcher_width: Signal<u16>,
    initial_monitor_preference: flux_core::MonitorPreference,
) -> WindowBootstrap {
    let window_icon = icons::tray_icon();
    WindowBootstrap::new(
        settings_visible.get(),
        everything_prompt_visible_at_start,
        launcher_width.get() as i32,
        initial_monitor_preference,
        &window_icon,
    )
}

fn build_tray(context: tray_menu::TrayContext) -> Tray {
    tray_menu::build_tray(context)
}

fn register_key_input(app: App, keys: main_key_input::KeyInputContext) -> App {
    app.on_key(move |event: KeyEvent| main_key_input::handle_key_input(event, &keys))
}

fn register_activation_hotkey(
    app: &mut App,
    activation_hotkey: Hotkey,
    state: ActivationHotkeyState,
) -> HotkeyHandle {
    app.hotkey_handle(activation_hotkey, move |ctx| {
        activation_hotkey::handle_activation_hotkey(ctx, &state);
    })
}

fn register_game_mode_hotkey(
    app: App,
    shared_settings: Arc<RwLock<Settings>>,
    game_mode: Signal<bool>,
    game_mode_status: Signal<String>,
) -> App {
    app.hotkey(hotkeys::game_mode_toggle_hotkey(), move |_| {
        let enabled = !game_mode.get();
        settings_state::set_game_mode(&shared_settings, game_mode, game_mode_status, enabled);
    })
}

fn register_window_show(
    app: App,
    shared_settings: Arc<RwLock<Settings>>,
    cursor_visibility: CursorVisibilityHandle,
    settings_visible: Signal<bool>,
    selection_color: Signal<Color>,
    window_size: WindowSizeHandle,
) -> App {
    app.on_window_show(window_lifecycle::on_window_show(
        shared_settings,
        cursor_visibility,
        settings_visible,
        selection_color,
        window_size,
    ))
}

fn register_window_activated(
    app: App,
    shared_settings: Arc<RwLock<Settings>>,
    everything_enabled: Signal<bool>,
    everything_installed: Signal<bool>,
    everything_status: Signal<String>,
) -> App {
    app.on_window_activated(window_lifecycle::on_window_activated(
        shared_settings,
        move || {
            everything::refresh_everything_status_cheap(
                everything_enabled,
                everything_installed,
                everything_status,
            );
        },
    ))
}

struct WindowHideInputs {
    app: App,
    shared_settings: Arc<RwLock<Settings>>,
    settings_visible: Signal<bool>,
    clear_query_on_activation: Signal<bool>,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    show_results: Signal<bool>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<actions::ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    window_size: WindowSizeHandle,
}

fn register_window_hide(inputs: WindowHideInputs) -> App {
    let WindowHideInputs {
        app,
        shared_settings,
        settings_visible,
        clear_query_on_activation,
        query,
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
        scroll_request,
        launcher_width,
        launcher_height,
        window_size,
    } = inputs;
    app.on_window_hide(window_lifecycle::on_window_hide(
        shared_settings,
        settings_visible,
        clear_query_on_activation,
        query,
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
        scroll_request,
        launcher_width,
        launcher_height,
        window_size,
    ))
}

fn build_launcher_content(
    launcher_surface: Element,
    settings_panel: Element,
    settings_ui: settings_view::SettingsUiState,
    settings_visible: Signal<bool>,
    cancel_settings: Rc<dyn Fn()>,
) -> Element {
    let launcher_page = Element::stack()
        .fill()
        .child(launcher_surface)
        .visible_when(move || !settings_visible.get());
    let settings_page = settings_view::settings_page(settings_panel, settings_ui, cancel_settings);
    Element::stack()
        .fill()
        .font_family(LAUNCHER_FONT_FAMILY)
        .child(launcher_page)
        .child(settings_page)
}

struct IntervalTickStateInputs {
    model: SearchModel,
    icon_refresh_generation: Signal<u64>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    settings_visible: Signal<bool>,
    everything_prompt_visible: Signal<bool>,
    everything_enabled: Signal<bool>,
    visual_preview_generation: Signal<u64>,
}

struct IntervalTickContextInputs {
    query_for_interval: Signal<String>,
    results_for_interval: Signal<Vec<SearchResult>>,
    width_for_interval: Signal<u16>,
    height_for_interval: Signal<u16>,
    width_input_for_interval: Signal<String>,
    height_input_for_interval: Signal<String>,
    width_slider_for_interval: Signal<f32>,
    height_slider_for_interval: Signal<f32>,
    preview_text_for_interval: Signal<String>,
    icon_refresh_generation_for_interval: Signal<u64>,
    status_for_interval: Signal<String>,
    show_results_for_interval: Signal<bool>,
    inline_completion_for_interval: Signal<String>,
    selection_touched_for_interval: Signal<bool>,
    sequence_for_interval: Signal<u64>,
    scroll_request_for_interval: Signal<bool>,
    everything_enabled_for_interval: Signal<bool>,
    obsidian_enabled_for_interval: Signal<bool>,
    obsidian_alias_for_interval: Signal<String>,
    google_enabled_for_interval: Signal<bool>,
    google_alias_for_interval: Signal<String>,
    system_commands_enabled_for_interval: Signal<bool>,
    history_mode_for_interval: Signal<bool>,
    language_preference_for_interval: Signal<usize>,
    language_preference: Signal<usize>,
    settings_visible_for_interval: Signal<bool>,
    settings_tab_for_interval: Signal<usize>,
    everything_prompt_visible_for_interval: Signal<bool>,
    everything_installed_for_interval: Signal<bool>,
    everything_status_for_interval: Signal<String>,
    visual_preview_generation_for_interval: Signal<u64>,
    visual_preview_smoke_for_interval: bool,
    everything_plugins_smoke_for_interval: bool,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<actions::ActionItem>>,
    providers_for_interval: Rc<RefCell<query::ProviderResults>>,
    actions_for_interval: Rc<RefCell<HashMap<String, plugins::PluginAction>>>,
    everything_detection_for_interval: Arc<everything::EverythingDetectionSlot>,
    position_for_interval: windui::app::WindowPositionHandle,
    settings_for_interval_geometry: Arc<RwLock<Settings>>,
    settings_for_update_interval: Arc<RwLock<Settings>>,
    update_sender_for_interval: Sender<updater::UpdateCheckResponse>,
    update_check_in_flight_for_interval: Rc<Cell<bool>>,
    size_for_interval: WindowSizeHandle,
    application_worker: applications::ApplicationWorker,
    everything_worker: everything::EverythingWorker,
    plugin_worker: plugins::FlowPluginWorker,
    native_plugin_worker: plugins::NativePluginWorker,
}

fn build_interval_tick_context(
    inputs: IntervalTickContextInputs,
) -> main_interval::IntervalTickContext {
    let IntervalTickContextInputs {
        query_for_interval,
        results_for_interval,
        width_for_interval,
        height_for_interval,
        width_input_for_interval,
        height_input_for_interval,
        width_slider_for_interval,
        height_slider_for_interval,
        preview_text_for_interval,
        icon_refresh_generation_for_interval,
        status_for_interval,
        show_results_for_interval,
        inline_completion_for_interval,
        selection_touched_for_interval,
        sequence_for_interval,
        scroll_request_for_interval,
        everything_enabled_for_interval,
        obsidian_enabled_for_interval,
        obsidian_alias_for_interval,
        google_enabled_for_interval,
        google_alias_for_interval,
        system_commands_enabled_for_interval,
        history_mode_for_interval,
        language_preference_for_interval,
        language_preference,
        settings_visible_for_interval,
        settings_tab_for_interval,
        everything_prompt_visible_for_interval,
        everything_installed_for_interval,
        everything_status_for_interval,
        visual_preview_generation_for_interval,
        visual_preview_smoke_for_interval,
        everything_plugins_smoke_for_interval,
        launcher_width,
        launcher_height,
        selected_id,
        selected_index,
        action_mode,
        action_index,
        action_items,
        providers_for_interval,
        actions_for_interval,
        everything_detection_for_interval,
        position_for_interval,
        settings_for_interval_geometry,
        settings_for_update_interval,
        update_sender_for_interval,
        update_check_in_flight_for_interval,
        size_for_interval,
        application_worker,
        everything_worker,
        plugin_worker,
        native_plugin_worker,
    } = inputs;
    main_interval::IntervalTickContext {
        query_for_interval,
        results_for_interval,
        width_for_interval,
        height_for_interval,
        width_input_for_interval,
        height_input_for_interval,
        width_slider_for_interval,
        height_slider_for_interval,
        preview_text_for_interval,
        icon_refresh_generation_for_interval,
        status_for_interval,
        show_results_for_interval,
        inline_completion_for_interval,
        selection_touched_for_interval,
        sequence_for_interval,
        scroll_request_for_interval,
        everything_enabled_for_interval,
        obsidian_enabled_for_interval,
        obsidian_alias_for_interval,
        google_enabled_for_interval,
        google_alias_for_interval,
        system_commands_enabled_for_interval,
        history_mode_for_interval,
        language_preference_for_interval,
        language_preference,
        settings_visible_for_interval,
        settings_tab_for_interval,
        everything_prompt_visible_for_interval,
        everything_installed_for_interval,
        everything_status_for_interval,
        visual_preview_generation_for_interval,
        visual_preview_smoke_for_interval,
        everything_plugins_smoke_for_interval,
        launcher_width,
        launcher_height,
        selected_id,
        selected_index,
        action_mode,
        action_index,
        action_items,
        providers_for_interval,
        actions_for_interval,
        everything_detection_for_interval,
        position_for_interval,
        settings_for_interval_geometry,
        settings_for_update_interval,
        update_sender_for_interval,
        update_check_in_flight_for_interval,
        size_for_interval,
        application_worker,
        everything_worker,
        plugin_worker,
        native_plugin_worker,
    }
}

fn build_interval_tick_state(inputs: IntervalTickStateInputs) -> main_interval::IntervalTickState {
    let IntervalTickStateInputs {
        model,
        icon_refresh_generation,
        launcher_width,
        launcher_height,
        settings_visible,
        everything_prompt_visible,
        everything_enabled,
        visual_preview_generation,
    } = inputs;
    main_interval::IntervalTickState {
        model,
        sequence: 0,
        last_icon_generation: icon_refresh_generation.get(),
        last_launcher_width: launcher_width.get(),
        last_launcher_height: launcher_height.get(),
        last_settings_visible: settings_visible.get(),
        last_everything_prompt_visible: everything_prompt_visible.get(),
        last_query: String::new(),
        last_everything_enabled: everything_enabled.get(),
        default_icon_prewarm_done: false,
        visual_preview_process: None,
        last_visual_preview_request: None,
        last_visual_preview_locale: String::new(),
        last_visual_preview_generation: visual_preview_generation.get(),
        last_visual_control_state: None,
        everything_plugins_smoke_reported: false,
    }
}

struct UpdateRuntime {
    install_sender: Sender<update_state::UpdateInstallResponse>,
    update_sender: Sender<updater::UpdateCheckResponse>,
    install_in_flight: Rc<Cell<bool>>,
    check_in_flight: Rc<Cell<bool>>,
}

fn register_update_runtime(
    app: &mut App,
    shared_settings: Arc<RwLock<Settings>>,
    update_status: Signal<String>,
    update_available: Signal<Option<updater::StableUpdate>>,
    update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    update_installing: Signal<bool>,
) -> UpdateRuntime {
    let channels = update_state::register_update_channels(
        app,
        shared_settings,
        update_status,
        update_available,
        update_install_progress,
        update_installing,
    );
    UpdateRuntime {
        install_sender: channels.install_sender,
        update_sender: channels.update_sender,
        install_in_flight: channels.install_in_flight,
        check_in_flight: channels.check_in_flight,
    }
}

fn register_single_instance(
    mut app: App,
    single_instance_disabled: bool,
    window_op: windui::app::WindowOpHandle,
) -> App {
    let second_instance_sender = app.channel::<()>(|ctx, ()| {
        ctx.show_window();
    });

    if single_instance_disabled {
        return app;
    }

    let second_instance_sender_for_callback = second_instance_sender.clone();
    let second_instance_window_op = window_op.clone();
    let shutdown_window_op = window_op.clone();

    app.single_instance(SINGLE_INSTANCE_ID, move |argv| {
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
    })
}
