use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, SearchResult, Settings};
use windui::app::{HotkeyHandle, ThemeHandle, WindowPositionHandle, WindowSizeHandle};
use windui::prelude::{Color, Sender};
use windui::signal::Signal;

use super::update_tasks::UpdateInstallResponse;
use super::updater;

/// All shared handles the settings UI needs, bundled so each settings tab
/// builder takes a single parameter instead of a thirty-signal signature.
/// Every field is a cheap `Copy` signal handle or a cloned `Rc`/`Arc`.
/// Cloning is how a button callback owns the state it needs: `move` closures
/// cannot borrow the panel builder's parameter.
#[derive(Clone)]
pub(crate) struct SettingsUi {
    pub(crate) shared_settings: Arc<RwLock<Settings>>,
    pub(crate) query_history: Rc<RefCell<Vec<String>>>,
    pub(crate) stats_usage: Signal<String>,
    pub(crate) stats_top: Signal<String>,
    pub(crate) clear_history_confirm: Signal<bool>,
    pub(crate) history_cursor: Signal<Option<usize>>,
    pub(crate) priorities: Signal<Vec<PriorityEntry>>,
    pub(crate) update_status: Signal<String>,
    pub(crate) update_available: Signal<Option<updater::StableUpdate>>,
    pub(crate) update_install_progress: Signal<Option<(String, updater::DownloadProgress)>>,
    pub(crate) update_installing: Signal<bool>,
    pub(crate) update_install_sender: Sender<UpdateInstallResponse>,
    pub(crate) update_install_in_flight: Rc<Cell<bool>>,
    pub(crate) update_sender: Sender<updater::UpdateCheckResponse>,
    pub(crate) update_check_in_flight: Rc<Cell<bool>>,
    pub(crate) game_mode: Signal<bool>,
    pub(crate) game_mode_status: Signal<String>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) settings_tab: Signal<usize>,
    /// Eased size resets: `(from_fraction, to_fraction, started)`. The frame
    /// callback drives the slider signal so the thumb and the window pair both
    /// travel on the same ease.
    pub(crate) width_ease: Rc<Cell<Option<(f32, f32, std::time::Instant)>>>,
    pub(crate) height_ease: Rc<Cell<Option<(f32, f32, std::time::Instant)>>>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) activation_display: Signal<String>,
    pub(crate) activation_key: Signal<String>,
    pub(crate) activation_ctrl: Signal<bool>,
    pub(crate) activation_alt: Signal<bool>,
    pub(crate) activation_shift: Signal<bool>,
    pub(crate) activation_meta: Signal<bool>,
    pub(crate) activation_recording: Signal<bool>,
    pub(crate) activation_handle: HotkeyHandle,
    /// Game Mode's own shortcut: drafts, the recorder flag and the live handle
    /// that Apply rebinds.
    pub(crate) game_mode_key: Signal<String>,
    pub(crate) game_mode_display: Signal<String>,
    pub(crate) game_mode_recording: Signal<bool>,
    pub(crate) game_mode_ctrl: Signal<bool>,
    pub(crate) game_mode_alt: Signal<bool>,
    pub(crate) game_mode_shift: Signal<bool>,
    pub(crate) game_mode_meta: Signal<bool>,
    pub(crate) game_mode_hotkey_enabled: Signal<bool>,
    pub(crate) game_mode_handle: HotkeyHandle,
    pub(crate) ignore_fullscreen: Signal<bool>,
    pub(crate) smooth_caret: Signal<bool>,
    pub(crate) switch_to_english_layout: Signal<bool>,
    pub(crate) use_system_accent: Signal<bool>,
    pub(crate) selection_color: Signal<Color>,
    pub(crate) theme_handle: ThemeHandle,
    pub(crate) custom_selection_color: Signal<String>,
    pub(crate) color_hsv: Signal<(f32, f32, f32)>,
    pub(crate) caret_duration: Signal<String>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) launcher_width_input: Signal<String>,
    pub(crate) launcher_height_input: Signal<String>,
    pub(crate) launcher_width_slider: Signal<f32>,
    pub(crate) launcher_height_slider: Signal<f32>,
    pub(crate) launcher_preview_text: Signal<String>,
    pub(crate) clear_query_on_activation: Signal<bool>,
    pub(crate) start_with_windows: Signal<bool>,
    pub(crate) auto_enable_everything: Signal<bool>,
    pub(crate) update_checks_enabled: Signal<bool>,
    pub(crate) update_interval_hours: Signal<String>,
    pub(crate) auto_install_updates: Signal<bool>,
    pub(crate) obsidian_enabled: Signal<bool>,
    pub(crate) obsidian_alias: Signal<String>,
    pub(crate) google_enabled: Signal<bool>,
    pub(crate) google_alias: Signal<String>,
    pub(crate) everything_status: Signal<String>,
    pub(crate) everything_installed: Signal<bool>,
    pub(crate) monitor_preference: Signal<usize>,
    pub(crate) window_size: WindowSizeHandle,
    pub(crate) window_position: WindowPositionHandle,
    /// Set by the title strip's close control; the interval tick hides the
    /// window once it has dropped the visual preview child.
    pub(crate) settings_hide_request: Signal<bool>,
    pub(crate) query: Signal<String>,
    pub(crate) results: Signal<Vec<SearchResult>>,
    pub(crate) providers: Rc<RefCell<super::provider_snapshot::ProviderResults>>,
}
