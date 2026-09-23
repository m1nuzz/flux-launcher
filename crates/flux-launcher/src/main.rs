#![cfg_attr(windows, windows_subsystem = "windows")]

#[macro_use]
extern crate rust_i18n;
i18n!("locales", fallback = "en");

mod accent;
mod action_panel;
mod actions;
mod applications;
mod builtin;
mod entry;
mod everything;
mod fullscreen;
mod hotkeys;
mod i18n;
mod icons;
mod interval_state;
mod keyboard;
mod keyboard_layout;
mod launch;
mod launcher_dialogs;
mod main_interval;
mod main_key_input;
mod main_layout;
mod monitor;
mod native_host;
mod plugin_limits;
mod plugin_transport;
mod plugins;
mod provider_state;
mod query;
mod result_row;
mod settings_state;
mod settings_view;
mod startup;
mod tray_menu;
mod ui_helpers;
mod update_state;
mod updater;
mod visual_preview;
mod window_lifecycle;
mod window_state;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::icons::tray_icon;
use crate::launch::trace_launch_event;
use accent::selection_color_for_settings;
use actions::ActionItem;
#[cfg(test)]
pub(crate) use actions::ActionKind;
use everything::{refresh_everything_status_cheap, EverythingRuntimeState};
use flux_core::{
    should_suppress_activation, MonitorPreference, SearchModel, SearchResult, Settings,
    MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use i18n::{apply_system_locale, I18nHub};
#[cfg(test)]
pub(crate) use keyboard::history_cursor_step;
use provider_state::{register_provider_channels, ProviderChannelContext, ProviderWorkers};
#[cfg(test)]
pub(crate) use query::should_publish_initial_query_results;
use query::SearchRuntimeState;
use settings_state::{set_game_mode, LauncherSettingsState};
use settings_view::SettingsUiState;
use ui_helpers::{launcher_theme, priorities_empty, selection_color_hex};
use update_state::{register_update_channels, request_update_check, update_check_due};
use window_state::{
    dimension_slider_fraction, launcher_window_geometry_with_sizes, monitor_preference_index,
    parse_dimension_input, request_monitor_position, should_show_launcher, WindowBootstrap,
};
use windui::app::{CursorVisibilityHandle, WindowPositionHandle, WindowSizeHandle};
use windui::event::{HotkeyCtx, KeyEvent};
use windui::prelude::*;

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SINGLE_INSTANCE_ID: &str = "lvxingqi.flux-launcher-cn";
pub(crate) const SETTINGS_WINDOW_WIDTH: i32 = 720;
const EVERYTHING_PROMPT_WINDOW_WIDTH: i32 = 440;
const EVERYTHING_PROMPT_WINDOW_HEIGHT: i32 = 242;
// The empty launcher is a compact search strip; the results state keeps the user-configured height.
pub(crate) const COMPACT_WINDOW_HEIGHT: i32 = 56;
const VISUAL_SLIDER_WIDTH: i32 = 200;
// The action group stays narrower than the minimum launcher content width so it
// can be centered between the same left/right content insets at every size.
const ACTION_BAR_WIDTH: i32 = 340;
const ACTION_BAR_HEIGHT: i32 = 22;
// Keep the result palette compact like the reference while exposing a six-row
// viewport; additional results remain available through the native wheel scroll.
const ACTION_WINDOW_HEIGHT: i32 = 250;
// Six 46-DIP result rows plus local scroll padding keep the footer close to the results.
const RESULT_VIEWPORT_HEIGHT: i32 = 288;
// 结果行行高（DIP），与 result_row 中的行高保持一致，用于推导图标请求窗口。
pub(crate) const RESULT_ROW_HEIGHT: i32 = 46;
/// 图标请求「立即提取」的行数上限：可见行（RESULT_VIEWPORT_HEIGHT / RESULT_ROW_HEIGHT）
/// 加两行余量。其余行在滚到选区附近时由响应式回调补发请求，避免一次结果集变化
/// 把几十个 Everything 文件全部塞进唯一的图标提取线程。
pub(crate) const EAGER_ICON_ROW_COUNT: usize =
    (RESULT_VIEWPORT_HEIGHT / RESULT_ROW_HEIGHT + 2) as usize;
pub(crate) const SETTINGS_WINDOW_HEIGHT: i32 = 520;
const LAUNCHER_FONT_FAMILY: &str = "Segoe UI Variable";
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const EVERYTHING_MIN_QUERY_LEN: usize = 1;
const PLUGIN_MIN_QUERY_LEN: usize = 2;

/// `main()` 启动阶段创建的全部 Signal 与共享状态。Signal 创建顺序敏感，
/// 因此统一收敛到 `init_launcher_state` 内按原顺序创建，再由 `main()` 解构为
/// 与原先同名的局部变量，保证后续组装代码零改动。
struct LauncherState {
    query: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_scroll_pending: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    action_items: Signal<Vec<ActionItem>>,
    action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    i18n_hub: I18nHub,
    status: Signal<String>,
    update_status: Signal<String>,
    update_available: Signal<Option<updater::StableUpdate>>,
    update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    update_installing: Signal<bool>,
    current_sequence: Signal<u64>,
    game_mode: Signal<bool>,
    game_mode_status: Signal<String>,
    settings_ui: SettingsUiState,
    settings_visible: Signal<bool>,
    settings_tab: Signal<usize>,
    language_preference: Signal<usize>,
    show_results: Signal<bool>,
    activation_key: Signal<String>,
    activation_display: Signal<String>,
    activation_recording: Signal<bool>,
    activation_ctrl: Signal<bool>,
    activation_alt: Signal<bool>,
    activation_shift: Signal<bool>,
    activation_meta: Signal<bool>,
    ignore_fullscreen: Signal<bool>,
    smooth_caret: Signal<bool>,
    switch_to_english_layout: Signal<bool>,
    use_system_accent: Signal<bool>,
    custom_selection_color: Signal<String>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_height_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    visual_preview_generation: Signal<u64>,
    clear_query_on_activation: Signal<bool>,
    start_with_windows: Signal<bool>,
    auto_enable_everything: Signal<bool>,
    everything_detection: Arc<everything::EverythingDetectionSlot>,
    update_checks_enabled: Signal<bool>,
    update_interval_hours: Signal<String>,
    auto_install_updates: Signal<bool>,
    obsidian_enabled: Signal<bool>,
    obsidian_alias: Signal<String>,
    google_enabled: Signal<bool>,
    google_alias: Signal<String>,
    system_commands_enabled: Signal<bool>,
    monitor_preference: Signal<usize>,
    initial_monitor_preference: MonitorPreference,
    everything_installed: Signal<bool>,
    everything_prompt_visible_at_start: bool,
    everything_prompt_visible: Signal<bool>,
    everything_status: Signal<String>,
    selection_color: Signal<Color>,
    caret_duration: Signal<String>,
}

fn init_launcher_state(
    settings: &Settings,
    settings_state: &LauncherSettingsState,
) -> LauncherState {
    let i18n_hub = settings_state.i18n_hub.clone();
    let game_mode = settings_state.game_mode;
    let game_mode_status = settings_state.game_mode_status;
    let settings_ui = settings_state.settings_ui;
    let language_preference = settings_state.language_preference;
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
    let status = i18n_hub.tr(|| t!("status.ready").into_owned());
    let update_status = i18n_hub.tr(|| t!("updater.checked_automatically").into_owned());
    let update_available = signal(None::<updater::StableUpdate>);
    let update_install_progress = signal(None::<(String, updater::DownloadProgress)>);
    let update_installing = signal(false);
    let current_sequence = signal(0_u64);
    let settings_visible = settings_ui.visible;
    let settings_tab = settings_ui.tab;
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
    let launcher_preview_text = signal(
        t!(
            "settings.visual.client_area",
            width = settings.launcher_width,
            height = settings.launcher_height
        )
        .into_owned(),
    );
    let visual_preview_generation = signal(0_u64);
    let clear_query_on_activation = signal(settings.clear_query_on_activation);
    let start_with_windows = signal(settings.start_with_windows);
    let auto_enable_everything = signal(settings.auto_enable_everything);
    let everything_detection = Arc::new(everything::EverythingDetectionSlot::default());
    let update_checks_enabled = signal(settings.update_checks_enabled);
    let update_interval_hours = signal(settings.update_interval_hours.to_string());
    let auto_install_updates = signal(settings.auto_install_updates);
    let obsidian_enabled = signal(settings.obsidian_enabled);
    let obsidian_alias = signal(settings.obsidian_alias.clone());
    let google_enabled = signal(settings.google_enabled);
    let google_alias = signal(settings.google_alias.clone());
    let system_commands_enabled = signal(settings.system_commands_enabled);
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
    let everything_state = EverythingRuntimeState::new(settings);
    let everything_installed = everything_state.installed;
    let everything_prompt_visible_at_start = everything_state.prompt_visible_at_start;
    let everything_prompt_visible = everything_state.prompt_visible;
    let everything_status = everything_state.status;
    let selection_color = signal(selection_color_for_settings(settings));
    let caret_duration = signal(settings.smooth_caret_duration_ms.to_string());

    LauncherState {
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
        auto_enable_everything,
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
    }
}

/// 全局激活热键回调所需的句柄集合：窗口操作句柄 + 需要重置/读取的 launcher 状态。
/// 回调本身不放 hwnd（见 vendor/windui 的借用纪律），只声明窗口操作意图。
struct ActivationHotkeyState {
    settings: Arc<RwLock<Settings>>,
    position: WindowPositionHandle,
    cursor_visibility: CursorVisibilityHandle,
    size: WindowSizeHandle,
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
    action_items: Signal<Vec<ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    settings_visible: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
}

/// 全局激活热键（默认 `Alt+Space`）的处理：清空查询 → 重算几何与显示器位置 →
/// 按窗口「实际可见性」切换显示/隐藏。
fn handle_activation_hotkey(ctx: &mut HotkeyCtx, state: &ActivationHotkeyState) {
    let settings = state
        .settings
        .read()
        .map(|settings| settings.clone())
        .unwrap_or_default();
    if should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
        return;
    }
    // Clear before toggling visibility. The previous implementation did
    // this only from on_window_hide, which allowed the old query frame to
    // survive in the compositor until the next repaint after re-show.
    if settings.clear_query_on_activation {
        state.query.set(String::new());
        state.results.set(Vec::new());
        state.selected_id.set(String::new());
        state.selected_index.set(0);
        state.selection_touched.set(false);
        state.show_results.set(false);
        state.history_mode.set(false);
        state.history_cursor.set(None);
        state.action_mode.set(false);
        state.action_index.set(0);
        state.action_items.set(Vec::new());
        state.inline_completion.set(String::new());
        state.scroll_request.set(false);
        let (compact_width, compact_height) = launcher_window_geometry_with_sizes(
            state.settings_visible.get(),
            false,
            i32::from(state.launcher_width.get()),
            i32::from(state.launcher_height.get()),
        );
        state.size.set(compact_width, compact_height);
    }
    let (width, height) = launcher_window_geometry_with_sizes(
        state.settings_visible.get(),
        state.show_results.get(),
        i32::from(state.launcher_width.get()),
        i32::from(state.launcher_height.get()),
    );
    request_monitor_position(&state.position, settings.monitor_preference, width, height);
    state.cursor_visibility.show();
    // 切换方向按窗口「实际可见性」判定，而不是按前台归属：launcher 自隐藏后
    // （例如刚启动一个不前置窗口或瞬间退出的程序）系统可能仍把它视为前台
    // 窗口，此时按前台判断会误判为「已显示」而只发 hide，用户再按 Alt+Space
    // 就唤不起窗口。可见性快照由 windui 平台层在派发 WM_HOTKEY 时提供
    // （`HotkeyCtx` 刻意不持有 hwnd，见 vendor/windui 的借用纪律）。
    if should_show_launcher(ctx.window_visible()) {
        ctx.show_window();
    } else {
        ctx.hide_window();
    }
}

fn main() {
    #[cfg(windows)]
    {
        // Monitor coordinates are queried before windui creates the HWND. Set
        // per-monitor awareness first so Windows does not virtualize the
        // 4K/mixed-DPI work area used for the initial center position.
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }
    apply_system_locale();
    let mut args = std::env::args_os();
    let _executable = args.next();
    let mode = args.next();
    if entry::run_special_mode(mode.as_deref(), &mut args) {
        return;
    }
    let single_instance_disabled = std::env::var_os("FLUX_DISABLE_SINGLE_INSTANCE").is_some();
    if !single_instance_disabled
        && entry::should_claim_single_instance(mode.as_deref())
        && matches!(
            windui::claim_instance(SINGLE_INSTANCE_ID),
            windui::InstanceRole::Handoff
        )
    {
        return;
    }
    // The uninstaller uses this one-shot mode only to reach the already-running
    // instance through the single-instance listener. Never create a new UI if
    // there is no instance left to shut down.
    if entry::is_shutdown_mode(mode.as_deref()) {
        return;
    }
    let (settings, startup_launch) = entry::load_settings(mode.as_deref());
    let activation_hotkey = hotkeys::activation_hotkey(&settings.activation_hotkey);
    let settings_state = LauncherSettingsState::from_settings(&settings);
    let shared_settings = Arc::clone(&settings_state.shared_settings);
    let query_history = Rc::clone(&settings_state.query_history);
    let priorities = settings_state.priorities;
    let history_cursor = signal(None::<usize>);
    let history_mode = signal(false);

    let model = SearchModel::new();
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
        auto_enable_everything,
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
    } = init_launcher_state(&settings, &settings_state);
    let search_state = SearchRuntimeState::new(model.results().to_vec());
    let results = search_state.results;
    let provider_results = search_state.provider_results;
    let plugin_actions = search_state.plugin_actions;
    let icon_refresh_generation = search_state.icon_refresh_generation;
    let inline_completion = search_state.inline_completion;
    let launcher_layout = main_layout::build_launcher_layout(main_layout::LauncherLayoutContext {
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

    let settings_at_start = settings_visible.get();
    let window_icon = tray_icon();
    let window_bootstrap = WindowBootstrap::new(
        settings_at_start,
        everything_prompt_visible_at_start,
        launcher_width.get() as i32,
        initial_monitor_preference,
        &window_icon,
    );
    let mut app = window_bootstrap.app;
    if everything_prompt_visible_at_start
        && std::env::var_os("FLUX_SMOKE_EVERYTHING_PROMPT").is_some()
    {
        eprintln!("Everything install prompt: visible at startup");
        eprintln!(
            "Everything install prompt style: glass-transparent panel_fill=none modal_scrim=none window_background=transparent"
        );
    }
    let window_size = window_bootstrap.window_size;
    let window_position = window_bootstrap.window_position;
    let window_op = window_bootstrap.window_op;
    let cursor_visibility = window_bootstrap.cursor_visibility;
    let update_channels = register_update_channels(
        &mut app,
        Arc::clone(&shared_settings),
        update_status,
        update_available,
        update_install_progress,
        update_installing,
    );
    let update_install_sender = update_channels.install_sender;
    let update_sender = update_channels.update_sender;
    let update_install_in_flight = update_channels.install_in_flight;
    let update_check_in_flight = update_channels.check_in_flight;
    let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
        .map(|value| value != "1")
        .unwrap_or(true);
    if update_checks_allowed && settings.update_checks_enabled && update_check_due(&settings) {
        request_update_check(update_sender.clone(), &update_check_in_flight);
    }
    *action_window_slot.borrow_mut() = Some(window_size.clone());
    let size_for_visibility = window_size.clone();
    let provider_channels = register_provider_channels(
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
            auto_enable_everything,
            everything_installed,
            everything_status,
        },
    );
    let application_sender = provider_channels.application_sender;
    let everything_sender = provider_channels.everything_sender;
    let plugin_sender = provider_channels.plugin_sender;
    let native_sender = provider_channels.native_sender;
    if settings.auto_enable_everything {
        match everything::start_background_if_installed() {
            Ok(outcome) => {
                everything_installed.set(outcome.is_installed());
                everything_status.set(outcome.status_message());
            }
            Err(error) => {
                everything_status.set(error);
            }
        }
    } else {
        everything_status.set(t!("everything.auto_enable_disabled").into_owned());
    }
    let provider_workers = ProviderWorkers::new(
        application_sender,
        everything_sender,
        plugin_sender,
        native_sender,
    );
    let application_worker = provider_workers.applications;
    let everything_worker = provider_workers.everything;
    let plugin_worker = provider_workers.plugins;
    let native_plugin_worker = provider_workers.native_plugins;

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
        launcher_width,
        launcher_height,
    };
    let activation_handle = app.hotkey_handle(activation_hotkey, move |ctx| {
        handle_activation_hotkey(ctx, &activation_state);
    });

    let activation_handle_for_recorder = activation_handle.clone();
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

    app = app.on_key({
        let keys = main_key_input::KeyInputContext {
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
        };
        move |event: KeyEvent| main_key_input::handle_key_input(event, &keys)
    });

    let tray = tray_menu::build_tray(tray_menu::TrayContext {
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

    let i18n_hub_for_apply = i18n_hub.clone();
    let settings_for_apply = Arc::clone(&shared_settings);
    let language_preference_for_apply = language_preference;
    let position_for_apply = window_position.clone();
    let activation_handle_for_apply = activation_handle.clone();
    let activation_handle_for_record_button = activation_handle.clone();
    let activation_recording_for_record_button = activation_recording;
    let activation_recording_for_apply = activation_recording;
    let activation_display_for_ui = activation_display;
    let activation_display_for_apply = activation_display;
    let game_mode_status_for_apply = game_mode_status;
    let settings_visible_for_apply = settings_visible;
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
    let system_commands_enabled_for_apply = system_commands_enabled;
    let everything_status_for_apply = everything_status;
    let cancel_settings = {
        let settings = Arc::clone(&shared_settings);
        let i18n_hub = i18n_hub.clone();
        let activation_handle = activation_handle.clone();
        let window_size = window_size.clone();
        let window_position = window_position.clone();
        let restore = settings_view::SettingsRestoreSignals {
            settings_visible,
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
            auto_enable_everything,
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
        Rc::new(move || {
            let Ok(saved) = settings.read() else {
                return;
            };
            let saved = saved.clone();
            settings_view::restore_saved_settings(&saved, &restore);

            let target_height = if show_results.get() {
                i32::from(saved.launcher_height)
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            request_monitor_position(
                &window_position,
                saved.monitor_preference,
                i32::from(saved.launcher_width),
                target_height,
            );
            window_size.set(i32::from(saved.launcher_width), target_height);
        }) as Rc<dyn Fn()>
    };
    let priority_list = settings_view::priority_list(
        priorities,
        results,
        Rc::clone(&provider_results),
        query,
        Arc::clone(&shared_settings),
    );
    let priorities_empty = priorities_empty(priorities);

    let visual_preview_generation_for_width_reset = visual_preview_generation;
    let visual_preview_generation_for_height_reset = visual_preview_generation;

    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window.
    let settings_panel = settings_view::settings_panel(settings_view::SettingsPanelContext {
        activation_alt,
        activation_ctrl,
        activation_display_for_apply,
        activation_display_for_ui,
        activation_handle_for_apply: activation_handle_for_apply.clone(),
        activation_handle_for_record_button: activation_handle_for_record_button.clone(),
        activation_key,
        activation_meta,
        activation_recording_for_apply,
        activation_recording_for_record_button,
        activation_shift,
        auto_enable_everything,
        auto_enable_everything_for_apply,
        auto_install_updates,
        auto_install_updates_for_apply,
        cancel_settings: cancel_settings.clone(),
        caret_duration,
        clear_query_on_activation,
        custom_selection_color,
        everything_detection: everything_detection.clone(),
        everything_installed,
        everything_status,
        everything_status_for_apply,
        game_mode,
        game_mode_status_for_apply,
        google_alias,
        google_alias_for_apply,
        google_enabled,
        google_enabled_for_apply,
        history_cursor_for_clear,
        history_for_clear: history_for_clear.clone(),
        i18n_hub: i18n_hub.clone(),
        i18n_hub_for_apply: i18n_hub_for_apply.clone(),
        ignore_fullscreen,
        language_preference,
        language_preference_for_apply,
        launcher_height,
        launcher_height_input,
        launcher_height_slider,
        launcher_preview_text,
        launcher_width,
        launcher_width_input,
        launcher_width_slider,
        monitor_preference,
        obsidian_alias,
        obsidian_alias_for_apply,
        obsidian_enabled,
        obsidian_enabled_for_apply,
        position_for_apply: position_for_apply.clone(),
        priorities_empty,
        priority_list,
        selection_color,
        settings_for_apply: settings_for_apply.clone(),
        settings_for_clear_history: settings_for_clear_history.clone(),
        settings_tab,
        settings_visible,
        settings_visible_for_apply,
        shared_settings: shared_settings.clone(),
        show_results,
        size_for_apply: size_for_apply.clone(),
        smooth_caret,
        start_with_windows,
        start_with_windows_for_apply,
        switch_to_english_layout,
        system_commands_enabled,
        system_commands_enabled_for_apply,
        update_available_for_install,
        update_check_in_flight_for_apply: update_check_in_flight_for_apply.clone(),
        update_check_in_flight_for_check_now: update_check_in_flight_for_check_now.clone(),
        update_checks_enabled,
        update_checks_enabled_for_apply,
        update_install_in_flight_for_ui: update_install_in_flight_for_ui.clone(),
        update_install_progress_for_ui,
        update_install_sender_for_ui: update_install_sender_for_ui.clone(),
        update_installing_for_ui,
        update_interval_hours,
        update_interval_hours_for_apply,
        update_sender_for_apply: update_sender_for_apply.clone(),
        update_sender_for_check_now: update_sender_for_check_now.clone(),
        update_status,
        update_status_for_apply,
        update_status_for_install,
        use_system_accent,
        visual_preview_generation_for_height_reset,
        visual_preview_generation_for_width_reset,
        window_position: window_position.clone(),
        window_size: window_size.clone(),
    });

    let launcher_page = Element::stack()
        .fill()
        .child(launcher_surface)
        .visible_when(move || !settings_visible.get());
    let settings_page = settings_view::settings_page(settings_panel, settings_ui);
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

    let tick_context = main_interval::IntervalTickContext {
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
        auto_enable_everything_for_interval: auto_enable_everything,
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
    };
    let mut tick_state = main_interval::IntervalTickState {
        model,
        sequence: 0,
        last_icon_generation: icon_refresh_generation.get(),
        last_launcher_width: launcher_width.get(),
        last_launcher_height: launcher_height.get(),
        last_settings_visible: settings_visible.get(),
        last_everything_prompt_visible: everything_prompt_visible.get(),
        last_query: String::new(),
        default_icon_prewarm_done: false,
        visual_preview_process: None,
        last_visual_preview_request: None,
        last_visual_preview_locale: String::new(),
        last_visual_preview_generation: visual_preview_generation.get(),
        last_visual_control_state: None,
        everything_plugins_smoke_reported: false,
    };

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
        .on_window_deactivated(|| {
            trace_launch_event("window-deactivated");
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
        .theme(launcher_theme())
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |_ctx| {
            main_interval::handle_interval_tick(&tick_context, &mut tick_state)
        })
        .on_window_show(window_lifecycle::on_window_show(
            Arc::clone(&shared_settings),
            cursor_visibility.clone(),
            settings_visible,
            selection_color,
            window_size.clone(),
        ))
        .on_window_activated(window_lifecycle::on_window_activated(
            Arc::clone(&shared_settings),
            move || {
                refresh_everything_status_cheap(
                    auto_enable_everything,
                    everything_installed,
                    everything_status,
                );
            },
        ))
        .on_window_hide(window_lifecycle::on_window_hide(
            Arc::clone(&shared_settings),
            Rc::clone(&cancel_settings),
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
            scroll_request_for_rows,
            launcher_width,
            launcher_height,
            size_for_visibility,
        ))
        .run();
}

#[cfg(test)]
mod tests;
