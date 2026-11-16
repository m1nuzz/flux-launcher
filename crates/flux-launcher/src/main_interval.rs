//! 主窗口 40ms 定时回调（`on_interval`）。
//!
//! 从 `main.rs` 整体外移（阶段十一 步骤 5 的 interval 部分）。回调内含一批
//! 只在闭包内部演化的可变状态（预览进程、图标代次、上一次查询等），
//! 集中在 [`IntervalTickState`]；跨帧共享的信号与句柄在 [`IntervalTickContext`]。
//! 行为约束（§3）：本回调只做信号读写、尺寸/位置请求与后台探测，
//! 不直接调用 `show_window` / `hide_window`。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};

use flux_core::{MonitorPreference, SearchModel, SearchResult, Settings};
use rust_i18n::t;
use windui::app::{WindowPositionHandle, WindowSizeHandle};
use windui::prelude::{Sender, Signal};

use crate::i18n::{configured_locale, language_preference_from_index};
use crate::icons::{
    icon_completion_generation_changed, prewarm_icon_targets, request_shell_icon,
    SHELL_ICON_COMPLETION_GENERATION,
};
use crate::interval_state::dispatch_query;
use crate::plugins::PluginAction;
use crate::query::ProviderResults;
use crate::update_state::maybe_request_update_check;
use crate::window_state::{
    apply_launcher_size, dimension_from_slider, dimension_slider_fraction,
    launcher_window_geometry_with_prompt, parse_dimension_input, request_monitor_position,
    visual_preview_position,
};

use crate::{SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH};
use flux_core::{MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH};

/// `on_interval` 回调跨帧共享的信号与句柄。
pub(crate) struct IntervalTickContext {
    pub(crate) query_for_interval: Signal<String>,
    pub(crate) results_for_interval: Signal<Vec<SearchResult>>,
    pub(crate) width_for_interval: Signal<u16>,
    pub(crate) height_for_interval: Signal<u16>,
    pub(crate) width_input_for_interval: Signal<String>,
    pub(crate) height_input_for_interval: Signal<String>,
    pub(crate) width_slider_for_interval: Signal<f32>,
    pub(crate) height_slider_for_interval: Signal<f32>,
    pub(crate) preview_text_for_interval: Signal<String>,
    pub(crate) icon_refresh_generation_for_interval: Signal<u64>,
    pub(crate) status_for_interval: Signal<String>,
    pub(crate) show_results_for_interval: Signal<bool>,
    pub(crate) inline_completion_for_interval: Signal<String>,
    pub(crate) selection_touched_for_interval: Signal<bool>,
    pub(crate) sequence_for_interval: Signal<u64>,
    pub(crate) scroll_request_for_interval: Signal<bool>,
    pub(crate) everything_enabled_for_interval: Signal<bool>,
    pub(crate) obsidian_enabled_for_interval: Signal<bool>,
    pub(crate) obsidian_alias_for_interval: Signal<String>,
    pub(crate) google_enabled_for_interval: Signal<bool>,
    pub(crate) google_alias_for_interval: Signal<String>,
    pub(crate) system_commands_enabled_for_interval: Signal<bool>,
    pub(crate) history_mode_for_interval: Signal<bool>,
    pub(crate) language_preference_for_interval: Signal<usize>,
    pub(crate) language_preference: Signal<usize>,
    pub(crate) settings_visible_for_interval: Signal<bool>,
    pub(crate) settings_tab_for_interval: Signal<usize>,
    pub(crate) everything_prompt_visible_for_interval: Signal<bool>,
    pub(crate) everything_installed_for_interval: Signal<bool>,
    pub(crate) everything_status_for_interval: Signal<String>,
    pub(crate) visual_preview_generation_for_interval: Signal<u64>,
    pub(crate) visual_preview_smoke_for_interval: bool,
    pub(crate) everything_plugins_smoke_for_interval: bool,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) action_mode: Signal<bool>,
    pub(crate) action_index: Signal<usize>,
    pub(crate) action_items: Signal<Vec<crate::actions::ActionItem>>,
    pub(crate) providers_for_interval: Rc<RefCell<ProviderResults>>,
    pub(crate) actions_for_interval: Rc<RefCell<HashMap<String, PluginAction>>>,
    pub(crate) everything_detection_for_interval: Arc<crate::everything::EverythingDetectionSlot>,
    pub(crate) position_for_interval: WindowPositionHandle,
    pub(crate) settings_for_interval_geometry: Arc<RwLock<Settings>>,
    pub(crate) settings_for_update_interval: Arc<RwLock<Settings>>,
    pub(crate) update_sender_for_interval: Sender<crate::updater::UpdateCheckResponse>,
    pub(crate) update_check_in_flight_for_interval: Rc<Cell<bool>>,
    pub(crate) size_for_interval: WindowSizeHandle,
    pub(crate) application_worker: crate::applications::ApplicationWorker,
    pub(crate) everything_worker: crate::everything::EverythingWorker,
    pub(crate) plugin_worker: crate::plugins::FlowPluginWorker,
    pub(crate) native_plugin_worker: crate::plugins::NativePluginWorker,
}

/// `on_interval` 回调闭包私有的可变演化状态。
pub(crate) struct IntervalTickState {
    pub(crate) model: SearchModel,
    pub(crate) sequence: u64,
    pub(crate) last_icon_generation: u64,
    pub(crate) last_launcher_width: u16,
    pub(crate) last_launcher_height: u16,
    pub(crate) last_settings_visible: bool,
    pub(crate) last_everything_prompt_visible: bool,
    pub(crate) last_query: String,
    pub(crate) last_everything_enabled: bool,
    pub(crate) default_icon_prewarm_done: bool,
    pub(crate) visual_preview_process: Option<crate::visual_preview::PreviewProcess>,
    pub(crate) last_visual_preview_request: Option<(u16, u16)>,
    pub(crate) last_visual_preview_locale: String,
    pub(crate) last_visual_preview_generation: u64,
    pub(crate) last_visual_control_state: Option<(u16, u16, u32, u32)>,
    pub(crate) everything_plugins_smoke_reported: bool,
}

/// 处理一次 40ms 定时 tick，与 `main.rs` 原 `on_interval` 闭包逐行等价。
pub(crate) fn handle_interval_tick(
    context: &IntervalTickContext,
    tick_state: &mut IntervalTickState,
) {
    let IntervalTickContext {
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
        ref providers_for_interval,
        ref actions_for_interval,
        ref everything_detection_for_interval,
        ref position_for_interval,
        ref settings_for_interval_geometry,
        ref settings_for_update_interval,
        ref update_sender_for_interval,
        ref update_check_in_flight_for_interval,
        ref size_for_interval,
        ref application_worker,
        ref everything_worker,
        ref plugin_worker,
        ref native_plugin_worker,
    } = *context;
    let current_width = width_for_interval.get();
    let current_height = height_for_interval.get();
    let everything_enabled = everything_enabled_for_interval.get();
    if everything_enabled != tick_state.last_everything_enabled {
        // 开关切换后立即丢弃旧的 Everything 结果，避免关闭搜索后旧结果继续留在界面。
        providers_for_interval.borrow_mut().everything.clear();
        results_for_interval.set(Vec::new());
        tick_state.last_query.clear();
        tick_state.last_everything_enabled = everything_enabled;
    }
    let settings_is_visible = settings_visible_for_interval.get();
    let prompt_is_visible = everything_prompt_visible_for_interval.get();
    let visual_tab_is_visible = settings_tab_for_interval.get() == 1;
    let plugins_tab_is_visible = settings_tab_for_interval.get() == 3;
    let visual_preview_is_visible = settings_is_visible && visual_tab_is_visible;
    if everything_plugins_smoke_for_interval
        && settings_is_visible
        && plugins_tab_is_visible
        && !tick_state.everything_plugins_smoke_reported
    {
        let installed = everything_installed_for_interval.get();
        let everything_enabled = everything_enabled_for_interval.get();
        let status = everything_status_for_interval.get();
        eprintln!(
                    "Everything Plugins UI: tab_visible=true everything_section=true everything_enabled_checkbox=true status_label=true install_button_label=Install_Everything already_installed_label=Everything_is_already_installed everything_enabled={} installed={} install_button_visible={} already_installed_visible={} status={}",
                    everything_enabled,
                    installed,
                    !installed,
                    installed,
                    status.replace(' ', "_")
                );
        tick_state.everything_plugins_smoke_reported = true;
    }
    if let Some(result) = everything_detection_for_interval.take_result() {
        // Everything 检测在后台线程完成；此处仅在 UI 线程回填信号。
        // 若用户在此期间关闭了文件搜索，则以关闭文案为准，不回填陈旧检测结果。
        if !everything_enabled_for_interval.get() {
            everything_status_for_interval.set(t!("everything.search_disabled").into_owned());
        } else {
            match result {
                Ok(outcome) => {
                    everything_installed_for_interval.set(outcome.is_installed());
                    everything_status_for_interval.set(outcome.status_message());
                }
                Err(error) => everything_status_for_interval.set(error),
            }
        }
    }
    if settings_is_visible && !tick_state.last_settings_visible {
        if let Ok(settings) = settings_for_interval_geometry.read() {
            request_monitor_position(
                position_for_interval,
                settings.monitor_preference,
                SETTINGS_WINDOW_WIDTH,
                SETTINGS_WINDOW_HEIGHT,
            );
        }
        size_for_interval.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
    }
    if prompt_is_visible != tick_state.last_everything_prompt_visible {
        tick_state.last_everything_prompt_visible = prompt_is_visible;
        let (prompt_width, prompt_height) = launcher_window_geometry_with_prompt(
            settings_is_visible,
            prompt_is_visible,
            show_results_for_interval.get(),
            width_for_interval.get() as i32,
            height_for_interval.get() as i32,
        );
        if let Ok(settings) = settings_for_interval_geometry.read() {
            request_monitor_position(
                position_for_interval,
                settings.monitor_preference,
                prompt_width,
                prompt_height,
            );
        }
        size_for_interval.set(prompt_width, prompt_height);
    }
    if visual_preview_is_visible {
        let preference = settings_for_interval_geometry
            .read()
            .map(|settings| settings.monitor_preference)
            .unwrap_or(MonitorPreference::Cursor);
        let child_exited = tick_state
            .visual_preview_process
            .as_mut()
            .is_some_and(|preview| !preview.is_alive());
        if child_exited {
            tick_state.visual_preview_process.take();
            tick_state.last_visual_preview_request = None;
            tick_state.last_visual_preview_locale.clear();
        }
        if let Some(preview) = tick_state.visual_preview_process.as_mut() {
            match preview.poll_ready() {
                Ok(_) => {}
                Err(error) => {
                    eprintln!("Could not ready visual preview: {error}");
                    tick_state.visual_preview_process.take();
                    tick_state.last_visual_preview_request = None;
                    tick_state.last_visual_preview_locale.clear();
                }
            }
        }
        if tick_state.visual_preview_process.is_none() {
            let preview_width = i32::from(current_width);
            let preview_height = i32::from(current_height);
            let (preview_x, preview_y) =
                visual_preview_position(preference, preview_width, preview_height);
            match crate::visual_preview::PreviewProcess::start(
                preview_width,
                preview_height,
                preview_x,
                preview_y,
                &configured_locale(language_preference_from_index(language_preference.get())),
            ) {
                Ok(preview) => {
                    tick_state.visual_preview_process = Some(preview);
                    tick_state.last_visual_preview_request = None;
                    tick_state.last_visual_preview_locale = configured_locale(
                        language_preference_from_index(language_preference.get()),
                    );
                }
                Err(error) => eprintln!("Could not start visual preview: {error}"),
            }
        }
    } else if let Some(preview) = tick_state.visual_preview_process.as_mut() {
        eprintln!(
            "Visual preview closing because Settings/Visual is hidden: pid={}",
            preview.pid()
        );
        tick_state.visual_preview_process.take();
        tick_state.last_visual_preview_request = None;
        tick_state.last_visual_preview_locale.clear();
    }
    tick_state.last_settings_visible = settings_is_visible;
    let slider_width = dimension_from_slider(
        width_slider_for_interval.get(),
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    );
    let slider_height = dimension_from_slider(
        height_slider_for_interval.get(),
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    );
    if visual_preview_smoke_for_interval {
        let control_state = (
            width_for_interval.get(),
            height_for_interval.get(),
            (width_slider_for_interval.get() * 10_000.0).round() as u32,
            (height_slider_for_interval.get() * 10_000.0).round() as u32,
        );
        if tick_state.last_visual_control_state != Some(control_state) {
            eprintln!(
                "Visual control state: width={} height={} width_slider={} height_slider={}",
                control_state.0, control_state.1, control_state.2, control_state.3
            );
            tick_state.last_visual_control_state = Some(control_state);
        }
    }
    let typed_width = parse_dimension_input(
        &width_input_for_interval.get(),
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    );
    let typed_height = parse_dimension_input(
        &height_input_for_interval.get(),
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    );
    let next_width = typed_width
        .filter(|value| *value != current_width)
        .unwrap_or(if slider_width != current_width {
            slider_width
        } else {
            current_width
        });
    let next_height = typed_height
        .filter(|value| *value != current_height)
        .unwrap_or(if slider_height != current_height {
            slider_height
        } else {
            current_height
        });
    if next_width != current_width || next_height != current_height {
        width_for_interval.set(next_width);
        height_for_interval.set(next_height);
        width_input_for_interval.set(next_width.to_string());
        height_input_for_interval.set(next_height.to_string());
        width_slider_for_interval.set(dimension_slider_fraction(
            next_width,
            MIN_LAUNCHER_WIDTH,
            MAX_LAUNCHER_WIDTH,
        ));
        height_slider_for_interval.set(dimension_slider_fraction(
            next_height,
            MIN_LAUNCHER_HEIGHT,
            MAX_LAUNCHER_HEIGHT,
        ));
        preview_text_for_interval.set(
            t!(
                "settings.visual.client_area",
                width = next_width,
                height = next_height
            )
            .into_owned(),
        );
        if !(settings_visible_for_interval.get() && settings_tab_for_interval.get() == 1) {
            apply_launcher_size(
                size_for_interval,
                position_for_interval,
                settings_for_interval_geometry,
                next_width,
                next_height,
                false,
                show_results_for_interval.get(),
            );
        }
        if visual_preview_smoke_for_interval {
            eprintln!(
                "Visual preview dimension state: {}x{} logical px",
                next_width, next_height
            );
        }
        tick_state.last_launcher_width = next_width;
        tick_state.last_launcher_height = next_height;
    } else if current_width != tick_state.last_launcher_width
        || current_height != tick_state.last_launcher_height
    {
        tick_state.last_launcher_width = current_width;
        tick_state.last_launcher_height = current_height;
    }

    let preview_generation = visual_preview_generation_for_interval.get();
    if visual_preview_is_visible {
        let requested = (width_for_interval.get(), height_for_interval.get());
        let preview_locale = configured_locale(language_preference_from_index(
            language_preference_for_interval.get(),
        ));
        let must_dispatch = tick_state.last_visual_preview_request != Some(requested)
            || tick_state.last_visual_preview_generation != preview_generation
            || tick_state.last_visual_preview_locale != preview_locale;
        if must_dispatch {
            let preference = settings_for_interval_geometry
                .read()
                .map(|settings| settings.monitor_preference)
                .unwrap_or(MonitorPreference::Cursor);
            let (preview_x, preview_y) =
                visual_preview_position(preference, i32::from(requested.0), i32::from(requested.1));
            let dispatch_result = if let Some(preview) = tick_state.visual_preview_process.as_mut()
            {
                match preview.poll_ready() {
                    Ok(true) => Some(preview.set_locale(&preview_locale).and_then(|_| {
                        preview.resize(
                            i32::from(requested.0),
                            i32::from(requested.1),
                            preview_x,
                            preview_y,
                        )
                    })),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            } else {
                None
            };
            match dispatch_result {
                Some(Ok(())) => {
                    tick_state.last_visual_preview_request = Some(requested);
                    tick_state.last_visual_preview_generation = preview_generation;
                    tick_state.last_visual_preview_locale = preview_locale.clone();
                    eprintln!(
                        "Visual preview IPC resize dispatched: {}x{}",
                        requested.0, requested.1
                    );
                }
                Some(Err(error)) => {
                    eprintln!("Could not update visual preview: {error}");
                    tick_state.visual_preview_process.take();
                    tick_state.last_visual_preview_request = None;
                    tick_state.last_visual_preview_locale.clear();
                }
                None => {}
            }
        }
    } else {
        tick_state.last_visual_preview_request = None;
        tick_state.last_visual_preview_generation = preview_generation;
    }

    if let Ok(settings) = settings_for_update_interval.read() {
        maybe_request_update_check(
            &settings,
            update_sender_for_interval.clone(),
            update_check_in_flight_for_interval,
        );
    }
    // 空查询默认结果集预热：启动后首个空闲 tick 把首屏默认目标的 shell 图标
    // 排入后台提取，首次显示结果时即命中缓存，不再出现图标逐个弹入。
    if !tick_state.default_icon_prewarm_done {
        tick_state.default_icon_prewarm_done = true;
        for target in prewarm_icon_targets(tick_state.model.results(), crate::EAGER_ICON_ROW_COUNT)
        {
            request_shell_icon(&target);
        }
    }
    let completed_icon_generation = SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire);
    if icon_completion_generation_changed(
        tick_state.last_icon_generation,
        completed_icon_generation,
    ) {
        tick_state.last_icon_generation = completed_icon_generation;
        icon_refresh_generation_for_interval.set(completed_icon_generation);
    }
    let next_query = query_for_interval.get();
    if next_query == tick_state.last_query {
        return;
    }

    let has_query = !next_query.trim().is_empty();
    history_mode_for_interval.set(false);
    show_results_for_interval.set(has_query);
    // Query cleanup also happens when hide-on-deactivate hides the
    // launcher. Do not let that asynchronous query transition resize
    // an already-open Settings panel back to the compact search strip.
    let (target_width, target_height) = launcher_window_geometry_with_prompt(
        settings_visible_for_interval.get(),
        everything_prompt_visible_for_interval.get(),
        has_query,
        launcher_width.get() as i32,
        launcher_height.get() as i32,
    );
    size_for_interval.set(target_width, target_height);
    dispatch_query(
        &mut tick_state.model,
        &mut tick_state.sequence,
        &next_query,
        has_query,
        everything_enabled_for_interval,
        obsidian_enabled_for_interval,
        obsidian_alias_for_interval,
        google_enabled_for_interval,
        google_alias_for_interval,
        system_commands_enabled_for_interval,
        results_for_interval,
        Rc::clone(providers_for_interval),
        selected_id,
        selected_index,
        selection_touched_for_interval,
        inline_completion_for_interval,
        scroll_request_for_interval,
        action_mode,
        action_index,
        action_items,
        Rc::clone(actions_for_interval),
        status_for_interval,
        application_worker,
        everything_worker,
        plugin_worker,
        native_plugin_worker,
    );
    sequence_for_interval.set(tick_state.sequence);
    tick_state.last_query = next_query;
}
