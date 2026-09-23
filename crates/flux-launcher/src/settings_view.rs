use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use crate::accent::{parse_selection_color, selection_color_for_settings};
use crate::everything::{self, EverythingDetectionSlot};
use crate::hotkeys;
use crate::i18n::{apply_configured_locale, language_preference_from_index, I18nHub};
use crate::plugins::native_plugin_install_path;
use crate::query::ProviderResults;
use crate::settings_state::save_settings;
use crate::startup;
use crate::ui_helpers::{game_mode_label, priority_row, selection_color_hex, selection_palette};
use crate::update_state::{request_update_check, request_update_install, update_check_due};
use crate::updater;
use crate::window_state::{
    dimension_slider_fraction, monitor_preference_from_index, parse_dimension_input,
    request_monitor_position,
};
use flux_core::{
    HotkeyConfig, PriorityEntry, SearchResult, Settings, DEFAULT_LAUNCHER_HEIGHT,
    DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT,
    MIN_LAUNCHER_WIDTH,
};
use windui::app::{HotkeyHandle, WindowPositionHandle, WindowSizeHandle};
use windui::prelude::*;

/// Shared reactive state for the Settings surface.
#[derive(Clone, Copy)]
pub(crate) struct SettingsUiState {
    pub(crate) visible: Signal<bool>,
    pub(crate) tab: Signal<usize>,
}

pub(crate) fn priority_list(
    priorities: Signal<Vec<PriorityEntry>>,
    results: Signal<Vec<SearchResult>>,
    providers: Rc<RefCell<ProviderResults>>,
    query: Signal<String>,
    settings: Arc<RwLock<Settings>>,
) -> Element {
    Element::list_signal(
        priorities,
        |entry| entry.id.clone(),
        move |entry| {
            let rank = priorities
                .get()
                .iter()
                .position(|candidate| candidate.id == entry.id)
                .map(|index| index + 1)
                .unwrap_or_default();
            priority_row(
                &entry,
                rank,
                priorities,
                results,
                Rc::clone(&providers),
                query,
                Arc::clone(&settings),
            )
        },
    )
}

pub(crate) fn launcher_width_reset_button(
    i18n_hub: I18nHub,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    visual_preview_generation: Signal<u64>,
) -> Element {
    Element::button(i18n_hub.tr(|| t!("settings.visual.reset").into_owned()))
        .neutral()
        .on_click(move |_| {
            let width = flux_core::DEFAULT_LAUNCHER_WIDTH;
            let height = launcher_height.get();
            eprintln!("Visual width reset clicked: {}x{}", width, height);
            launcher_width.set(width);
            launcher_width_input.set(width.to_string());
            launcher_width_slider.set(crate::dimension_slider_fraction(
                width,
                flux_core::MIN_LAUNCHER_WIDTH,
                flux_core::MAX_LAUNCHER_WIDTH,
            ));
            launcher_preview_text.set(
                t!(
                    "settings.visual.client_area",
                    width = width,
                    height = height
                )
                .into_owned(),
            );
            visual_preview_generation.set(visual_preview_generation.get().saturating_add(1));
        })
}

pub(crate) fn launcher_height_reset_button(
    i18n_hub: I18nHub,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_height_input: Signal<String>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    visual_preview_generation: Signal<u64>,
) -> Element {
    Element::button(i18n_hub.tr(|| t!("settings.visual.reset").into_owned()))
        .neutral()
        .on_click(move |_| {
            let width = launcher_width.get();
            let height = flux_core::DEFAULT_LAUNCHER_HEIGHT;
            eprintln!("Visual height reset clicked: {}x{}", width, height);
            launcher_height.set(height);
            launcher_height_input.set(height.to_string());
            launcher_height_slider.set(crate::dimension_slider_fraction(
                height,
                flux_core::MIN_LAUNCHER_HEIGHT,
                flux_core::MAX_LAUNCHER_HEIGHT,
            ));
            launcher_preview_text.set(
                t!(
                    "settings.visual.client_area",
                    width = width,
                    height = height
                )
                .into_owned(),
            );
            visual_preview_generation.set(visual_preview_generation.get().saturating_add(1));
        })
}

pub(crate) struct VisualApplyContext {
    pub(crate) i18n_hub: I18nHub,
    pub(crate) settings: Arc<RwLock<Settings>>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) launcher_width_input: Signal<String>,
    pub(crate) launcher_height_input: Signal<String>,
    pub(crate) launcher_width_slider: Signal<f32>,
    pub(crate) launcher_height_slider: Signal<f32>,
    pub(crate) launcher_preview_text: Signal<String>,
    pub(crate) smooth_caret: Signal<bool>,
    pub(crate) caret_duration: Signal<String>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) window_size: windui::app::WindowSizeHandle,
    pub(crate) window_position: windui::app::WindowPositionHandle,
}

pub(crate) fn visual_apply_button(context: VisualApplyContext) -> Element {
    let VisualApplyContext {
        i18n_hub,
        settings,
        launcher_width,
        launcher_height,
        launcher_width_input,
        launcher_height_input,
        launcher_width_slider,
        launcher_height_slider,
        launcher_preview_text,
        smooth_caret,
        caret_duration,
        settings_visible,
        show_results,
        window_size,
        window_position,
    } = context;

    Element::button(i18n_hub.tr(|| t!("settings.visual.apply").into_owned())).on_click(move |ctx| {
        let mut width = crate::parse_dimension_input(
            &launcher_width_input.get(),
            flux_core::MIN_LAUNCHER_WIDTH,
            flux_core::MAX_LAUNCHER_WIDTH,
        )
        .unwrap_or(flux_core::DEFAULT_LAUNCHER_WIDTH);
        let mut height = crate::parse_dimension_input(
            &launcher_height_input.get(),
            flux_core::MIN_LAUNCHER_HEIGHT,
            flux_core::MAX_LAUNCHER_HEIGHT,
        )
        .unwrap_or(flux_core::DEFAULT_LAUNCHER_HEIGHT);
        let duration = caret_duration
            .get()
            .trim()
            .parse::<u16>()
            .unwrap_or(95)
            .clamp(60, 160);
        let Ok(mut settings) = settings.write() else {
            ctx.toast_ok(t!("settings.lock_failed"));
            return;
        };
        settings.launcher_width = width;
        settings.launcher_height = height;
        settings.smooth_caret = smooth_caret.get();
        settings.smooth_caret_duration_ms = duration;
        settings.normalize();
        width = settings.launcher_width;
        height = settings.launcher_height;
        let preference = settings.monitor_preference;
        if !crate::settings_state::save_settings(&settings) {
            ctx.toast_ok(t!("settings.visual.save_failed"));
            return;
        }
        launcher_width.set(width);
        launcher_height.set(height);
        launcher_width_input.set(width.to_string());
        launcher_height_input.set(height.to_string());
        launcher_width_slider.set(crate::dimension_slider_fraction(
            width,
            flux_core::MIN_LAUNCHER_WIDTH,
            flux_core::MAX_LAUNCHER_WIDTH,
        ));
        launcher_height_slider.set(crate::dimension_slider_fraction(
            height,
            flux_core::MIN_LAUNCHER_HEIGHT,
            flux_core::MAX_LAUNCHER_HEIGHT,
        ));
        launcher_preview_text.set(
            t!(
                "settings.visual.client_area",
                width = width,
                height = height
            )
            .into_owned(),
        );
        eprintln!("Visual Apply dimensions clicked: {}x{}", width, height);
        settings_visible.set(false);
        let target_height = if show_results.get() {
            i32::from(height)
        } else {
            crate::COMPACT_WINDOW_HEIGHT
        };
        crate::window_state::request_monitor_position(
            &window_position,
            preference,
            i32::from(width),
            target_height,
        );
        window_size.set(i32::from(width), target_height);
        ctx.show_window();
        ctx.toast_ok(t!("settings.visual.applied"));
    })
}

pub(crate) struct EverythingSettingsContext {
    pub(crate) i18n_hub: I18nHub,
    pub(crate) settings: Arc<RwLock<Settings>>,
    pub(crate) auto_enable: Signal<bool>,
    pub(crate) installed: Signal<bool>,
    pub(crate) status: Signal<String>,
    pub(crate) detection: Arc<EverythingDetectionSlot>,
    pub(crate) obsidian_enabled: Signal<bool>,
    pub(crate) obsidian_alias: Signal<String>,
    pub(crate) google_enabled: Signal<bool>,
    pub(crate) google_alias: Signal<String>,
    pub(crate) system_commands_enabled: Signal<bool>,
}

pub(crate) fn everything_settings(context: EverythingSettingsContext) -> Element {
    let EverythingSettingsContext {
        i18n_hub,
        settings,
        auto_enable,
        installed,
        status,
        detection,
        obsidian_enabled,
        obsidian_alias,
        google_enabled,
        google_alias,
        system_commands_enabled,
    } = context;
    let settings_for_toggle = Arc::clone(&settings);
    let auto_enable_for_toggle = auto_enable;
    let status_for_toggle = status;
    let installed_for_ui = installed;
    let status_for_install = status;
    let detection_for_toggle = Arc::clone(&detection);

    Element::col()
        .width_match()
        .spacing(12)
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.everything").into_owned()))
                .font_size(17.0)
                .fg(Color::WHITE),
        )
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.everything_tab_desc").into_owned()))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 180))
                .max_lines(3)
                .truncate(Truncate::End),
        )
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.everything").into_owned()),
            Element::checkbox(
                i18n_hub.tr(|| t!("settings.everything_desc").into_owned()),
                auto_enable,
            )
            .on_toggle(move |_| {
                let enabled = auto_enable_for_toggle.get();
                if let Ok(mut settings) = settings_for_toggle.write() {
                    settings.auto_enable_everything = enabled;
                    settings.normalize();
                    let _ = save_settings(&settings);
                }
                if !enabled {
                    status_for_toggle.set(t!("everything.auto_enable_disabled").into_owned());
                    return;
                }
                // 立即给出反馈，检测放到后台线程；结果由 on_interval 轮询回填，
                // 避免在 UI 线程上同步等待注册表扫描 / tasklist / IPC 连接。
                status_for_toggle.set(t!("everything.detecting").into_owned());
                detection_for_toggle.spawn_detection();
            }),
        ))
        .child(
            Element::label(i18n_hub.tr(|| t!("everything.is_installed").into_owned()))
                .font_size(12.0)
                .fg(Color::rgba(180, 255, 205, 235))
                .visible_when(move || installed_for_ui.get()),
        )
        .child(
            Element::label(i18n_hub.tr(|| t!("everything.is_not_installed").into_owned()))
                .font_size(12.0)
                .fg(Color::rgba(255, 225, 175, 235))
                .visible_when(move || !installed.get()),
        )
        .child(
            Element::label_signal(status)
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 190))
                .max_lines(2)
                .truncate(Truncate::End)
                .width_match(),
        )
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.everything_command").into_owned()))
                .font_size(10.0)
                .fg(Color::rgba(235, 241, 255, 155))
                .visible_when(move || !installed_for_ui.get())
                .width_match(),
        )
        .child(
            Element::button(i18n_hub.tr(|| t!("everything.install").into_owned()))
                .visible_when(move || !installed_for_ui.get())
                .on_click(
                    move |ctx| match crate::everything::launch_winget_install() {
                        Ok(()) => {
                            status_for_install
                                .set(t!("everything.winget_started_restart").into_owned());
                            ctx.toast_ok(t!("everything.winget_started_toast"));
                        }
                        Err(error) => {
                            status_for_install.set(error.clone());
                            ctx.toast_ok(error);
                        }
                    },
                ),
        )
        .child(plugin_settings(
            i18n_hub,
            obsidian_enabled,
            obsidian_alias,
            google_enabled,
            google_alias,
            system_commands_enabled,
        ))
}

impl SettingsUiState {
    pub(crate) fn new(visible: Signal<bool>, tab: Signal<usize>) -> Self {
        Self { visible, tab }
    }
}

/// Wrap the Settings panel without changing its transparent Acrylic surface or
/// its visibility binding. Page contents remain assembled by the launcher while
/// their lifecycle callbacks are migrated incrementally.
pub(crate) fn settings_page(panel: Element, state: SettingsUiState) -> Element {
    Element::col()
        .fill()
        .padding(18)
        .child(panel)
        .visible_signal(state.visible)
}

/// Build the Settings header while keeping tab state and the Back callback explicit.
pub(crate) fn settings_header(
    settings_tab: Signal<usize>,
    i18n_hub: I18nHub,
    cancel_settings: Rc<dyn Fn()>,
) -> Element {
    Element::row()
        .width_match()
        .child(
            Element::col()
                .weight(1.0)
                .spacing(3)
                .child(
                    Element::label(i18n_hub.tr(|| t!("settings.title").into_owned()))
                        .font_size(25.0)
                        .fg(Color::WHITE),
                )
                .child(
                    Element::label(i18n_hub.tr(|| t!("settings.apply_immediate").into_owned()))
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 180)),
                ),
        )
        .child(Element::segmented_signal(
            i18n_hub.tr_vec(|| {
                vec![
                    t!("settings.tab.general").to_string(),
                    t!("settings.tab.visual").to_string(),
                    t!("settings.tab.priorities").to_string(),
                    t!("settings.tab.plugins").to_string(),
                ]
            }),
            settings_tab,
        ))
        .child(
            Element::button(i18n_hub.tr(|| t!("settings.back").into_owned()))
                .neutral()
                .on_click({
                    let cancel_settings = Rc::clone(&cancel_settings);
                    move |ctx| {
                        cancel_settings();
                        ctx.show_window();
                    }
                }),
        )
}

/// Build the native plugin configuration section.
pub(crate) fn plugin_settings(
    i18n_hub: I18nHub,
    obsidian_enabled: Signal<bool>,
    obsidian_alias: Signal<String>,
    google_enabled: Signal<bool>,
    google_alias: Signal<String>,
    system_commands_enabled: Signal<bool>,
) -> Element {
    Element::col()
        .width_match()
        .spacing(12)
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.plugins.title").into_owned()))
                .font_size(17.0)
                .fg(Color::WHITE),
        )
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.plugins.description").into_owned()))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 180))
                .max_lines(3)
                .truncate(Truncate::End),
        )
        .child(
            Element::label(t!(
                "settings.plugins.folder",
                path = native_plugin_install_path()
            ))
            .font_size(10.0)
            .fg(Color::rgba(235, 241, 255, 150))
            .max_lines(2)
            .truncate(Truncate::End),
        )
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.plugins.config_hint").into_owned()))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 180))
                .max_lines(2)
                .truncate(Truncate::End),
        )
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.plugins.obsidian").into_owned()),
            Element::checkbox(
                i18n_hub.tr(|| t!("settings.plugins.obsidian_desc").into_owned()),
                obsidian_enabled,
            ),
        ))
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.plugins.action_keyword").into_owned()),
            Element::text_input(obsidian_alias, "ob").width_match(),
        ))
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.plugins.obsidian_hint").into_owned()))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 175))
                .max_lines(3)
                .truncate(Truncate::End),
        )
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.plugins.google").into_owned()),
            Element::checkbox(
                i18n_hub.tr(|| t!("settings.plugins.google_desc").into_owned()),
                google_enabled,
            ),
        ))
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.plugins.action_keyword").into_owned()),
            Element::text_input(google_alias, "g").width_match(),
        ))
        .child(
            Element::label(i18n_hub.tr(|| t!("settings.plugins.google_hint").into_owned()))
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 175))
                .max_lines(3)
                .truncate(Truncate::End),
        )
        .child(Element::field_signal(
            i18n_hub.tr(|| t!("settings.plugins.system_commands").into_owned()),
            Element::checkbox(
                i18n_hub.tr(|| t!("settings.plugins.system_commands_desc").into_owned()),
                system_commands_enabled,
            ),
        ))
}

/// Settings 面板整棵元素树的构建上下文。
///
/// 字段名与 `main()` 中的局部变量逐一对应，函数内以同名解构引入，
/// 因此面板体代码与原来完全一致（零标识符改写）。
/// 字段均为 `Signal`/`Rc`/`Arc` 句柄或已构建好的 `Element`，克隆成本可控。
pub(crate) struct SettingsPanelContext {
    pub(crate) activation_alt: Signal<bool>,
    pub(crate) activation_ctrl: Signal<bool>,
    pub(crate) activation_display_for_apply: Signal<String>,
    pub(crate) activation_display_for_ui: Signal<String>,
    pub(crate) activation_handle_for_apply: HotkeyHandle,
    pub(crate) activation_handle_for_record_button: HotkeyHandle,
    pub(crate) activation_key: Signal<String>,
    pub(crate) activation_meta: Signal<bool>,
    pub(crate) activation_recording_for_apply: Signal<bool>,
    pub(crate) activation_recording_for_record_button: Signal<bool>,
    pub(crate) activation_shift: Signal<bool>,
    pub(crate) auto_enable_everything: Signal<bool>,
    pub(crate) auto_enable_everything_for_apply: Signal<bool>,
    pub(crate) auto_install_updates: Signal<bool>,
    pub(crate) auto_install_updates_for_apply: Signal<bool>,
    pub(crate) cancel_settings: Rc<dyn Fn()>,
    pub(crate) caret_duration: Signal<String>,
    pub(crate) clear_query_on_activation: Signal<bool>,
    pub(crate) custom_selection_color: Signal<String>,
    pub(crate) everything_detection: Arc<EverythingDetectionSlot>,
    pub(crate) everything_installed: Signal<bool>,
    pub(crate) everything_status: Signal<String>,
    pub(crate) everything_status_for_apply: Signal<String>,
    pub(crate) game_mode: Signal<bool>,
    pub(crate) game_mode_status_for_apply: Signal<String>,
    pub(crate) google_alias: Signal<String>,
    pub(crate) google_alias_for_apply: Signal<String>,
    pub(crate) google_enabled: Signal<bool>,
    pub(crate) google_enabled_for_apply: Signal<bool>,
    pub(crate) history_cursor_for_clear: Signal<Option<usize>>,
    pub(crate) history_for_clear: Rc<RefCell<Vec<String>>>,
    pub(crate) i18n_hub: I18nHub,
    pub(crate) i18n_hub_for_apply: I18nHub,
    pub(crate) ignore_fullscreen: Signal<bool>,
    pub(crate) language_preference: Signal<usize>,
    pub(crate) language_preference_for_apply: Signal<usize>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) launcher_height_input: Signal<String>,
    pub(crate) launcher_height_slider: Signal<f32>,
    pub(crate) launcher_preview_text: Signal<String>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_width_input: Signal<String>,
    pub(crate) launcher_width_slider: Signal<f32>,
    pub(crate) monitor_preference: Signal<usize>,
    pub(crate) obsidian_alias: Signal<String>,
    pub(crate) obsidian_alias_for_apply: Signal<String>,
    pub(crate) obsidian_enabled: Signal<bool>,
    pub(crate) obsidian_enabled_for_apply: Signal<bool>,
    pub(crate) position_for_apply: WindowPositionHandle,
    pub(crate) priorities_empty: Element,
    pub(crate) priority_list: Element,
    pub(crate) selection_color: Signal<Color>,
    pub(crate) settings_for_apply: Arc<RwLock<Settings>>,
    pub(crate) settings_for_clear_history: Arc<RwLock<Settings>>,
    pub(crate) settings_tab: Signal<usize>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) settings_visible_for_apply: Signal<bool>,
    pub(crate) shared_settings: Arc<RwLock<Settings>>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) size_for_apply: WindowSizeHandle,
    pub(crate) smooth_caret: Signal<bool>,
    pub(crate) start_with_windows: Signal<bool>,
    pub(crate) start_with_windows_for_apply: Signal<bool>,
    pub(crate) switch_to_english_layout: Signal<bool>,
    pub(crate) system_commands_enabled: Signal<bool>,
    pub(crate) system_commands_enabled_for_apply: Signal<bool>,
    pub(crate) update_available_for_install: Signal<Option<crate::updater::StableUpdate>>,
    pub(crate) update_check_in_flight_for_apply: Rc<Cell<bool>>,
    pub(crate) update_check_in_flight_for_check_now: Rc<Cell<bool>>,
    pub(crate) update_checks_enabled: Signal<bool>,
    pub(crate) update_checks_enabled_for_apply: Signal<bool>,
    pub(crate) update_install_in_flight_for_ui: Rc<Cell<bool>>,
    pub(crate) update_install_progress_for_ui:
        Signal<Option<(String, crate::updater::DownloadProgress)>>,
    pub(crate) update_install_sender_for_ui: Sender<crate::update_state::UpdateInstallResponse>,
    pub(crate) update_installing_for_ui: Signal<bool>,
    pub(crate) update_interval_hours: Signal<String>,
    pub(crate) update_interval_hours_for_apply: Signal<String>,
    pub(crate) update_sender_for_apply: Sender<crate::updater::UpdateCheckResponse>,
    pub(crate) update_sender_for_check_now: Sender<crate::updater::UpdateCheckResponse>,
    pub(crate) update_status: Signal<String>,
    pub(crate) update_status_for_apply: Signal<String>,
    pub(crate) update_status_for_install: Signal<String>,
    pub(crate) use_system_accent: Signal<bool>,
    pub(crate) visual_preview_generation_for_height_reset: Signal<u64>,
    pub(crate) visual_preview_generation_for_width_reset: Signal<u64>,
    pub(crate) window_position: WindowPositionHandle,
    pub(crate) window_size: WindowSizeHandle,
}

/// Back/Cancel 时把已保存设置还原到各 UI 信号的信号集合。
///
/// windui 的 `Signal` 是 Copy 句柄，构造时按字段直拷即可；`I18nHub`、
/// `HotkeyHandle` 为 Clone。还原逻辑集中在 [`restore_saved_settings`]，
/// `main.rs` 的 cancel 闭包只负责读取共享设置并调用本函数，再处理窗口几何。
pub(crate) struct SettingsRestoreSignals {
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) language_preference: Signal<usize>,
    pub(crate) i18n_hub: I18nHub,
    pub(crate) activation_key: Signal<String>,
    pub(crate) activation_ctrl: Signal<bool>,
    pub(crate) activation_alt: Signal<bool>,
    pub(crate) activation_shift: Signal<bool>,
    pub(crate) activation_meta: Signal<bool>,
    pub(crate) activation_display: Signal<String>,
    pub(crate) activation_recording: Signal<bool>,
    pub(crate) activation_handle: HotkeyHandle,
    pub(crate) ignore_fullscreen: Signal<bool>,
    pub(crate) game_mode: Signal<bool>,
    pub(crate) game_mode_status: Signal<String>,
    pub(crate) smooth_caret: Signal<bool>,
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
    pub(crate) auto_enable_everything: Signal<bool>,
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
}

/// 按已保存设置还原全部设置页 UI 信号（不含窗口几何，几何由调用方处理）。
pub(crate) fn restore_saved_settings(saved: &Settings, s: &SettingsRestoreSignals) {
    s.settings_visible.set(false);
    s.language_preference
        .set(crate::i18n::language_preference_index(saved.language));
    apply_configured_locale(saved.language);
    s.i18n_hub.refresh();

    // 裸键配置回退为默认热键，保证显示、勾选与注册一致。
    let activation = hotkeys::sanitize_activation_config(&saved.activation_hotkey);
    s.activation_key.set(activation.key.clone());
    s.activation_ctrl.set(activation.ctrl);
    s.activation_alt.set(activation.alt);
    s.activation_shift.set(activation.shift);
    s.activation_meta.set(activation.meta);
    s.activation_display
        .set(hotkeys::display_config(&activation));
    s.activation_recording.set(false);
    s.activation_handle
        .set(hotkeys::activation_hotkey(&activation));
    s.activation_handle.set_enabled(true);

    s.ignore_fullscreen.set(saved.ignore_hotkeys_in_fullscreen);
    s.game_mode.set(saved.game_mode);
    s.game_mode_status.set(game_mode_label(saved.game_mode));
    s.smooth_caret.set(saved.smooth_caret);
    s.switch_to_english_layout
        .set(saved.switch_to_english_layout);
    s.use_system_accent.set(saved.use_system_accent);
    s.custom_selection_color
        .set(selection_color_hex(saved.custom_selection_color));
    s.selection_color.set(selection_color_for_settings(saved));

    s.launcher_width.set(saved.launcher_width);
    s.launcher_height.set(saved.launcher_height);
    s.launcher_width_input.set(saved.launcher_width.to_string());
    s.launcher_height_input
        .set(saved.launcher_height.to_string());
    s.launcher_width_slider.set(dimension_slider_fraction(
        saved.launcher_width,
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    ));
    s.launcher_height_slider.set(dimension_slider_fraction(
        saved.launcher_height,
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    ));
    s.launcher_preview_text.set(
        t!(
            "settings.visual.client_area",
            width = saved.launcher_width,
            height = saved.launcher_height
        )
        .into_owned(),
    );

    s.clear_query_on_activation
        .set(saved.clear_query_on_activation);
    s.start_with_windows.set(saved.start_with_windows);
    s.auto_enable_everything.set(saved.auto_enable_everything);
    s.update_checks_enabled.set(saved.update_checks_enabled);
    s.update_interval_hours
        .set(saved.update_interval_hours.to_string());
    s.auto_install_updates.set(saved.auto_install_updates);
    s.obsidian_enabled.set(saved.obsidian_enabled);
    s.obsidian_alias.set(saved.obsidian_alias.clone());
    s.google_enabled.set(saved.google_enabled);
    s.google_alias.set(saved.google_alias.clone());
    s.system_commands_enabled.set(saved.system_commands_enabled);
    s.monitor_preference
        .set(crate::window_state::monitor_preference_index(
            saved.monitor_preference,
        ));
    // 设置变更（例如切换语言）后刷新状态文案；这里同样使用低成本刷新，
    // 如实反映 Everything 当前的安装与 IPC 状态。
    everything::refresh_everything_status_cheap(
        s.auto_enable_everything,
        s.everything_installed,
        s.everything_status,
    );
}

pub(crate) fn settings_panel(context: SettingsPanelContext) -> Element {
    let SettingsPanelContext {
        activation_alt,
        activation_ctrl,
        activation_display_for_apply,
        activation_display_for_ui,
        activation_handle_for_apply,
        activation_handle_for_record_button,
        activation_key,
        activation_meta,
        activation_recording_for_apply,
        activation_recording_for_record_button,
        activation_shift,
        auto_enable_everything,
        auto_enable_everything_for_apply,
        auto_install_updates,
        auto_install_updates_for_apply,
        cancel_settings,
        caret_duration,
        clear_query_on_activation,
        custom_selection_color,
        everything_detection,
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
        history_for_clear,
        i18n_hub,
        i18n_hub_for_apply,
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
        position_for_apply,
        priorities_empty,
        priority_list,
        selection_color,
        settings_for_apply,
        settings_for_clear_history,
        settings_tab,
        settings_visible,
        settings_visible_for_apply,
        shared_settings,
        show_results,
        size_for_apply,
        smooth_caret,
        start_with_windows,
        start_with_windows_for_apply,
        switch_to_english_layout,
        system_commands_enabled,
        system_commands_enabled_for_apply,
        update_available_for_install,
        update_check_in_flight_for_apply,
        update_check_in_flight_for_check_now,
        update_checks_enabled,
        update_checks_enabled_for_apply,
        update_install_in_flight_for_ui,
        update_install_progress_for_ui,
        update_install_sender_for_ui,
        update_installing_for_ui,
        update_interval_hours,
        update_interval_hours_for_apply,
        update_sender_for_apply,
        update_sender_for_check_now,
        update_status,
        update_status_for_apply,
        update_status_for_install,
        use_system_accent,
        visual_preview_generation_for_height_reset,
        visual_preview_generation_for_width_reset,
        window_position,
        window_size,
    } = context;

    Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(0, 0, 0, 0))
        .border(Color::rgba(0, 0, 0, 0), 0)
        .child(settings_header(
            settings_tab,
            i18n_hub.clone(),
            Rc::clone(&cancel_settings),
        ))
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 0)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.activation_key").into_owned()),
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
                                            Element::button(
                                                i18n_hub
                                                    .tr(|| t!("settings.record_key").into_owned()),
                                            )
                                            .neutral()
                                            .on_click(
                                                move |ctx| {
                                                    activation_recording_for_record_button
                                                        .set(true);
                                                    activation_handle_for_record_button
                                                        .set_enabled(false);
                                                    ctx.toast_ok(t!("settings.press_desired_key"));
                                                },
                                            ),
                                        ),
                                )
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.record_hint").into_owned()),
                                    )
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 170))
                                    .visible_when(move || {
                                        activation_recording_for_record_button.get()
                                    }),
                                )
                                .child(
                                    Element::label(
                                        i18n_hub
                                            .tr(|| t!("settings.modifier_required").into_owned()),
                                    )
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 170)),
                                ),
                        ))
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.ctrl").into_owned()),
                                    activation_ctrl,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.alt").into_owned()),
                                    activation_alt,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.shift").into_owned()),
                                    activation_shift,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.windows").into_owned()),
                                    activation_meta,
                                )),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.fullscreen_protection").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.fullscreen_protection_desc").into_owned()),
                                ignore_fullscreen,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.game_mode").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.game_mode_desc").into_owned()),
                                game_mode,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.keyboard_layout").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.keyboard_layout_desc").into_owned()),
                                switch_to_english_layout,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.query_on_activation").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.query_on_activation_desc").into_owned()),
                                clear_query_on_activation,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.windows_startup").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.windows_startup_desc").into_owned()),
                                start_with_windows,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.language").into_owned()),
                            Element::dropdown_signal(
                                i18n_hub.tr_vec(|| {
                                    vec![
                                        t!("settings.language_options.follow_system").into_owned(),
                                        t!("settings.language_options.english").into_owned(),
                                        t!("settings.language_options.chinese").into_owned(),
                                    ]
                                }),
                                language_preference,
                            )
                            .on_dropdown_change({
                                let i18n_hub = i18n_hub.clone();
                                move |ctx, index| {
                                    let target_lang = language_preference_from_index(index);
                                    apply_configured_locale(target_lang);
                                    i18n_hub.refresh();
                                    ctx.mark_dirty_all();
                                }
                            }),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.open_launcher_on").into_owned()),
                            Element::col()
                                .spacing(6)
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.primary").into_owned()),
                                    monitor_preference,
                                    0,
                                ))
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.cursor").into_owned()),
                                    monitor_preference,
                                    1,
                                ))
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.foreground").into_owned()),
                                    monitor_preference,
                                    2,
                                )),
                        ))
                        .child(
                            Element::col()
                                .width_match()
                                .spacing(8)
                                .child(Element::field_signal(
                                    i18n_hub.tr(|| t!("settings.updates").into_owned()),
                                    Element::checkbox(
                                        i18n_hub.tr(|| t!("settings.updates_desc").into_owned()),
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
                                        .child(
                                            Element::label(i18n_hub.tr(|| {
                                                t!("settings.hours_between_checks").into_owned()
                                            }))
                                            .font_size(11.0),
                                        ),
                                )
                                .child(
                                    Element::row()
                                        .width_match()
                                        .spacing(8)
                                        .child(
                                            Element::label(
                                                i18n_hub.tr(|| {
                                                    t!("settings.update_action").into_owned()
                                                }),
                                            )
                                            .width_match(),
                                        )
                                        .child(
                                            Element::label(t!(
                                                "settings.current_version",
                                                version = crate::CURRENT_VERSION
                                            ))
                                            .font_size(11.0)
                                            .fg(Color::rgba(235, 241, 255, 190)),
                                        ),
                                )
                                .child(Element::checkbox(
                                    i18n_hub
                                        .tr(|| t!("settings.auto_install_updates").into_owned()),
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
                                        .child(
                                            Element::button(
                                                i18n_hub.tr(|| {
                                                    t!("settings.check_updates").into_owned()
                                                }),
                                            )
                                            .on_click(
                                                move |ctx| {
                                                    update_status_for_apply.set(
                                                        t!("updater.checking_github").into_owned(),
                                                    );
                                                    request_update_check(
                                                        update_sender_for_check_now.clone(),
                                                        &update_check_in_flight_for_check_now,
                                                    );
                                                    ctx.toast_ok(t!("updater.checking_toast"));
                                                },
                                            ),
                                        )
                                        .child(
                                            Element::button(
                                                i18n_hub
                                                    .tr(|| t!("settings.install_now").into_owned()),
                                            )
                                            .visible_when(move || {
                                                update_available_for_install.get().is_some()
                                                    && !update_installing_for_ui.get()
                                            })
                                            .on_click(
                                                move |ctx| {
                                                    if update_installing_for_ui.get() {
                                                        return;
                                                    }
                                                    if let Some(update) =
                                                        update_available_for_install.get()
                                                    {
                                                        update_installing_for_ui.set(true);
                                                        update_install_progress_for_ui.set(None);
                                                        update_status_for_install.set(
                                                            t!(
                                                                "updater.preparing_download",
                                                                version = update.version
                                                            )
                                                            .into_owned(),
                                                        );
                                                        if !request_update_install(
                                                            update,
                                                            update_install_sender_for_ui.clone(),
                                                            &update_install_in_flight_for_ui,
                                                            updater::RelaunchMode::Visible,
                                                        ) {
                                                            update_installing_for_ui.set(false);
                                                            update_status_for_install.set(
                                                                t!("updater.already_installing")
                                                                    .into_owned(),
                                                            );
                                                            ctx.toast_ok(t!(
                                                                "updater.already_installing"
                                                            ));
                                                        }
                                                    }
                                                },
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .child(
                                    Element::label(
                                        i18n_hub
                                            .tr(|| t!("settings.query_history_hint").into_owned()),
                                    )
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 175))
                                    .width_match(),
                                )
                                .child(
                                    Element::button(
                                        i18n_hub.tr(|| t!("settings.clear_history").into_owned()),
                                    )
                                    .on_click(move |ctx| {
                                        if let Ok(mut settings) = settings_for_clear_history.write()
                                        {
                                            settings.clear_query_history();
                                            let _ = save_settings(&settings);
                                        }
                                        history_for_clear.borrow_mut().clear();
                                        history_cursor_for_clear.set(None);
                                        ctx.toast_ok(t!("settings.history_cleared"));
                                    }),
                                ),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.native_plugins_hint").into_owned()),
                            )
                            .font_size(12.0)
                            .fg(Color::rgba(235, 241, 255, 160)),
                        )
                        .child(
                            Element::button(i18n_hub.tr(|| t!("settings.apply").into_owned()))
                                .on_click(move |ctx| {
                                    let duration = caret_duration
                                        .get()
                                        .trim()
                                        .parse::<u16>()
                                        .unwrap_or(95)
                                        .clamp(60, 160);
                                    // 裸键配置回退为默认热键（Alt+Space），写入前规范化。
                                    let configuration =
                                        hotkeys::sanitize_activation_config(&HotkeyConfig {
                                            ctrl: activation_ctrl.get(),
                                            alt: activation_alt.get(),
                                            shift: activation_shift.get(),
                                            meta: activation_meta.get(),
                                            key: activation_key.get(),
                                        });
                                    let custom_color =
                                        parse_selection_color(&custom_selection_color.get())
                                            .unwrap_or(0x4c8bf4);
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
                                        let previous_language = settings.language;
                                        settings.activation_hotkey = configuration;
                                        settings.ignore_hotkeys_in_fullscreen =
                                            ignore_fullscreen.get();
                                        settings.game_mode = game_mode.get();
                                        settings.smooth_caret = smooth_caret.get();
                                        settings.switch_to_english_layout =
                                            switch_to_english_layout.get();
                                        settings.use_system_accent = use_system_accent.get();
                                        settings.custom_selection_color = custom_color;
                                        settings.launcher_width = configured_width;
                                        settings.launcher_height = configured_height;
                                        settings.clear_query_on_activation =
                                            clear_query_on_activation.get();
                                        settings.start_with_windows =
                                            start_with_windows_for_apply.get();
                                        settings.update_checks_enabled =
                                            update_checks_enabled_for_apply.get();
                                        settings.update_interval_hours =
                                            update_interval_hours_for_apply
                                                .get()
                                                .trim()
                                                .parse::<u32>()
                                                .unwrap_or(24)
                                                .clamp(1, 168);
                                        settings.auto_install_updates =
                                            auto_install_updates_for_apply.get();
                                        update_interval_hours_for_apply
                                            .set(settings.update_interval_hours.to_string());
                                        settings.auto_enable_everything =
                                            auto_enable_everything_for_apply.get();
                                        settings.obsidian_enabled =
                                            obsidian_enabled_for_apply.get();
                                        settings.obsidian_alias = obsidian_alias_for_apply.get();
                                        settings.google_enabled = google_enabled_for_apply.get();
                                        settings.google_alias = google_alias_for_apply.get();
                                        settings.system_commands_enabled =
                                            system_commands_enabled_for_apply.get();
                                        settings.monitor_preference =
                                            monitor_preference_from_index(monitor_preference.get());
                                        settings.language = language_preference_from_index(
                                            language_preference_for_apply.get(),
                                        );
                                        settings.smooth_caret_duration_ms = duration;
                                        settings.normalize();
                                        activation_recording_for_apply.set(false);
                                        activation_display_for_apply.set(hotkeys::display_config(
                                            &settings.activation_hotkey,
                                        ));
                                        // 裸键已回退为默认热键，同步勾选框与键名信号，
                                        // 避免显示值与复选状态不一致。
                                        activation_ctrl.set(settings.activation_hotkey.ctrl);
                                        activation_alt.set(settings.activation_hotkey.alt);
                                        activation_shift.set(settings.activation_hotkey.shift);
                                        activation_meta.set(settings.activation_hotkey.meta);
                                        activation_key.set(settings.activation_hotkey.key.clone());
                                        selection_color
                                            .set(selection_color_for_settings(&settings));
                                        custom_selection_color.set(selection_color_hex(
                                            settings.custom_selection_color,
                                        ));
                                        launcher_width.set(settings.launcher_width);
                                        launcher_height.set(settings.launcher_height);
                                        launcher_width_input
                                            .set(settings.launcher_width.to_string());
                                        launcher_height_input
                                            .set(settings.launcher_height.to_string());
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
                                        launcher_preview_text.set(
                                            t!(
                                                "settings.visual.client_area",
                                                width = settings.launcher_width,
                                                height = settings.launcher_height
                                            )
                                            .into_owned(),
                                        );
                                        activation_handle_for_apply.set(
                                            hotkeys::activation_hotkey(&settings.activation_hotkey),
                                        );
                                        activation_handle_for_apply.set_enabled(true);
                                        game_mode_status_for_apply
                                            .set(game_mode_label(settings.game_mode));
                                        if settings.auto_enable_everything {
                                            match everything::start_background_if_installed() {
                                                Ok(outcome) => {
                                                    everything_installed
                                                        .set(outcome.is_installed());
                                                    everything_status_for_apply
                                                        .set(outcome.status_message());
                                                }
                                                Err(error) => {
                                                    everything_status_for_apply.set(error)
                                                }
                                            }
                                        } else {
                                            everything_status_for_apply.set(
                                                t!("everything.auto_enable_disabled").into_owned(),
                                            );
                                        }
                                        let _ = save_settings(&settings);
                                        apply_configured_locale(settings.language);
                                        if previous_language != settings.language {
                                            launcher_preview_text.set(
                                                t!(
                                                    "settings.visual.client_area",
                                                    width = launcher_width.get(),
                                                    height = launcher_height.get()
                                                )
                                                .into_owned(),
                                            );
                                            i18n_hub_for_apply.refresh();
                                        }
                                        if let Err(error) =
                                            startup::set_enabled(settings.start_with_windows)
                                        {
                                            ctx.toast_ok(t!(
                                                "settings.startup_failed",
                                                error = error
                                            ));
                                        }
                                        if settings.update_checks_enabled
                                            && update_check_due(&settings)
                                        {
                                            update_status_for_apply
                                                .set(t!("updater.checking_github").into_owned());
                                            request_update_check(
                                                update_sender_for_apply.clone(),
                                                &update_check_in_flight_for_apply,
                                            );
                                        }
                                    }
                                    settings_visible_for_apply.set(false);
                                    let selected_preference =
                                        monitor_preference_from_index(monitor_preference.get());
                                    let applied_width = launcher_width.get() as i32;
                                    let applied_height = launcher_height.get() as i32;
                                    let target_height = if show_results.get() {
                                        applied_height
                                    } else {
                                        crate::COMPACT_WINDOW_HEIGHT
                                    };
                                    request_monitor_position(
                                        &position_for_apply,
                                        selected_preference,
                                        applied_width,
                                        target_height,
                                    );
                                    size_for_apply.set(applied_width, target_height);
                                    ctx.show_window();
                                    ctx.toast_ok(t!("settings.applied"));
                                }),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 3)
                .child(everything_settings(EverythingSettingsContext {
                    i18n_hub: i18n_hub.clone(),
                    settings: Arc::clone(&shared_settings),
                    auto_enable: auto_enable_everything,
                    installed: everything_installed,
                    status: everything_status,
                    detection: Arc::clone(&everything_detection),
                    obsidian_enabled,
                    obsidian_alias,
                    google_enabled,
                    google_alias,
                    system_commands_enabled,
                })),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 1)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.title").into_owned()),
                            )
                            .font_size(17.0)
                            .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.preview_desc").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.smooth_caret").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::checkbox(
                                        i18n_hub
                                            .tr(|| t!("settings.smooth_caret_desc").into_owned()),
                                        smooth_caret,
                                    )
                                    .width_match(),
                                )
                                .child(Element::text_input(caret_duration, "95").width(76))
                                .child(Element::label("ms").font_size(11.0)),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.selection_color").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.visual.use_system_accent").into_owned()),
                                use_system_accent,
                            ),
                        ))
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.accent_hint").into_owned()),
                            )
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.preview_hint").into_owned()),
                            )
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 170))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::col()
                                .spacing(8)
                                .visible_when(move || !use_system_accent.get())
                                .child(
                                    Element::text_input(custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(custom_selection_color)),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.launcher_width").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::slider(launcher_width_slider)
                                        .width(crate::VISUAL_SLIDER_WIDTH),
                                )
                                .child(Element::text_input(launcher_width_input, "420").width(76))
                                .child(launcher_width_reset_button(
                                    i18n_hub.clone(),
                                    launcher_width,
                                    launcher_height,
                                    launcher_width_input,
                                    launcher_width_slider,
                                    launcher_preview_text,
                                    visual_preview_generation_for_width_reset,
                                ))
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.visual.dip").into_owned()),
                                    )
                                    .font_size(11.0),
                                ),
                        ))
                        .child(
                            Element::label(i18n_hub.tr(|| {
                                t!(
                                    "settings.visual.safe_range",
                                    min = MIN_LAUNCHER_WIDTH,
                                    max = MAX_LAUNCHER_WIDTH
                                )
                                .into_owned()
                            }))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.results_height").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::slider(launcher_height_slider)
                                        .width(crate::VISUAL_SLIDER_WIDTH),
                                )
                                .child(Element::text_input(launcher_height_input, "382").width(76))
                                .child(launcher_height_reset_button(
                                    i18n_hub.clone(),
                                    launcher_width,
                                    launcher_height,
                                    launcher_height_input,
                                    launcher_height_slider,
                                    launcher_preview_text,
                                    visual_preview_generation_for_height_reset,
                                ))
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.visual.dip").into_owned()),
                                    )
                                    .font_size(11.0),
                                ),
                        ))
                        .child(
                            Element::label(i18n_hub.tr(|| {
                                t!(
                                    "settings.visual.safe_range",
                                    min = MIN_LAUNCHER_HEIGHT,
                                    max = MAX_LAUNCHER_HEIGHT
                                )
                                .into_owned()
                            }))
                            .font_size(10.0)
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(
                            Element::label_signal(launcher_preview_text)
                                .font_size(12.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub
                                    .tr(|| t!("settings.visual.native_preview_hint").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 175))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(visual_apply_button(VisualApplyContext {
                            i18n_hub: i18n_hub.clone(),
                            settings: Arc::clone(&shared_settings),
                            launcher_width,
                            launcher_height,
                            launcher_width_input,
                            launcher_height_input,
                            launcher_width_slider,
                            launcher_height_slider,
                            launcher_preview_text,
                            smooth_caret,
                            caret_duration,
                            settings_visible,
                            show_results,
                            window_size: window_size.clone(),
                            window_position: window_position.clone(),
                        })),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 2)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(10)
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.priorities.title").into_owned()),
                            )
                            .font_size(17.0)
                            .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.priorities.description").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(priorities_empty)
                        .child(priority_list),
                ),
        )
}
