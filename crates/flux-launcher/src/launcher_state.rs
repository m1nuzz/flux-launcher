use crate::accent::selection_color_for_settings;
use crate::actions::ActionItem;
use crate::everything::{self, EverythingRuntimeState};
use crate::i18n::I18nHub;
use crate::query::SearchRuntimeState;
use crate::settings_state::LauncherSettingsState;
use crate::settings_view::SettingsUiState;
use crate::ui_helpers::selection_color_hex;
use crate::window_state::{dimension_slider_fraction, monitor_preference_index};
use crate::{hotkeys, updater};
use flux_core::{
    MonitorPreference, SearchModel, Settings, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use windui::app::WindowSizeHandle;
use windui::geometry::Color;
use windui::signal::{signal, Signal};
/// `main()` 启动阶段创建的全部 Signal 与共享状态。Signal 创建顺序敏感，
/// 因此统一收敛到 `init_launcher_state` 内按原顺序创建，再由 `main()` 解构为
/// 与原先同名的局部变量，保证后续组装代码零改动。
pub(crate) struct LauncherState {
    pub query: Signal<String>,
    pub selected_id: Signal<String>,
    pub selected_index: Signal<usize>,
    pub selection_touched: Signal<bool>,
    pub action_mode: Signal<bool>,
    pub action_index: Signal<usize>,
    pub action_scroll_pending: Signal<bool>,
    pub recycle_bin_confirmation: Signal<bool>,
    pub action_items: Signal<Vec<ActionItem>>,
    pub action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    pub i18n_hub: I18nHub,
    pub status: Signal<String>,
    pub update_status: Signal<String>,
    pub update_available: Signal<Option<updater::StableUpdate>>,
    pub update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    pub update_installing: Signal<bool>,
    pub current_sequence: Signal<u64>,
    pub game_mode: Signal<bool>,
    pub game_mode_status: Signal<String>,
    pub settings_ui: SettingsUiState,
    pub settings_visible: Signal<bool>,
    pub settings_tab: Signal<usize>,
    pub language_preference: Signal<usize>,
    pub show_results: Signal<bool>,
    pub activation_key: Signal<String>,
    pub activation_display: Signal<String>,
    pub activation_recording: Signal<bool>,
    pub activation_ctrl: Signal<bool>,
    pub activation_alt: Signal<bool>,
    pub activation_shift: Signal<bool>,
    pub activation_meta: Signal<bool>,
    pub ignore_fullscreen: Signal<bool>,
    pub smooth_caret: Signal<bool>,
    pub switch_to_english_layout: Signal<bool>,
    pub use_system_accent: Signal<bool>,
    pub custom_selection_color: Signal<String>,
    pub launcher_width: Signal<u16>,
    pub launcher_height: Signal<u16>,
    pub launcher_width_input: Signal<String>,
    pub launcher_height_input: Signal<String>,
    pub launcher_width_slider: Signal<f32>,
    pub launcher_height_slider: Signal<f32>,
    pub launcher_preview_text: Signal<String>,
    pub visual_preview_generation: Signal<u64>,
    pub clear_query_on_activation: Signal<bool>,
    pub start_with_windows: Signal<bool>,
    pub auto_enable_everything: Signal<bool>,
    pub everything_detection: Arc<everything::EverythingDetectionSlot>,
    pub update_checks_enabled: Signal<bool>,
    pub update_interval_hours: Signal<String>,
    pub auto_install_updates: Signal<bool>,
    pub obsidian_enabled: Signal<bool>,
    pub obsidian_alias: Signal<String>,
    pub google_enabled: Signal<bool>,
    pub google_alias: Signal<String>,
    pub system_commands_enabled: Signal<bool>,
    pub monitor_preference: Signal<usize>,
    pub initial_monitor_preference: MonitorPreference,
    pub everything_installed: Signal<bool>,
    pub everything_prompt_visible_at_start: bool,
    pub everything_prompt_visible: Signal<bool>,
    pub everything_status: Signal<String>,
    pub selection_color: Signal<Color>,
    pub caret_duration: Signal<String>,
}

pub(crate) fn init_launcher_state(
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
    // 裸键配置回退为默认热键（Alt+Space），显示与勾选状态保持一致。
    let activation_config = hotkeys::sanitize_activation_config(&settings.activation_hotkey);
    let activation_key = signal(activation_config.key.clone());
    let activation_display = signal(hotkeys::display_config(&activation_config));
    let activation_recording = signal(false);
    let activation_ctrl = signal(activation_config.ctrl);
    let activation_alt = signal(activation_config.alt);
    let activation_shift = signal(activation_config.shift);
    let activation_meta = signal(activation_config.meta);
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

pub(crate) struct LauncherSetup {
    pub(crate) settings: flux_core::Settings,
    pub(crate) startup_launch: bool,
    pub(crate) activation_hotkey: windui::event::Hotkey,
    pub(crate) shared_settings: Arc<std::sync::RwLock<flux_core::Settings>>,
    pub(crate) query_history: Rc<std::cell::RefCell<Vec<String>>>,
    pub(crate) priorities: Signal<Vec<flux_core::PriorityEntry>>,
    pub(crate) history_cursor: Signal<Option<usize>>,
    pub(crate) history_mode: Signal<bool>,
    pub(crate) model: SearchModel,
    pub(crate) state: LauncherState,
    pub(crate) search_state: SearchRuntimeState,
}

pub(crate) fn load_launcher_setup(mode: Option<&std::ffi::OsStr>) -> LauncherSetup {
    // 1.加载配置
    let (settings, startup_launch) = crate::entry::load_settings(mode);
    // 2.计算热键
    let activation_hotkey = hotkeys::activation_hotkey(&settings.activation_hotkey);
    // 3.创建setting_state
    let settings_state = LauncherSettingsState::from_settings(&settings);
    // 4.提取共享句柄
    let shared_settings = Arc::clone(&settings_state.shared_settings);
    let query_history = Rc::clone(&settings_state.query_history);
    let priorities = settings_state.priorities;
    // 5.创建history_cursor / history_mode
    let history_cursor = signal(None::<usize>);
    let history_mode = signal(false);
    // 6.创建model
    let model = SearchModel::new();
    // 7.初始化init_launcher_state（60+ signal）
    let state = init_launcher_state(&settings, &settings_state);
    // 8.创建search_state
    let search_state = SearchRuntimeState::new(model.results().to_vec());

    LauncherSetup {
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
    }
}
