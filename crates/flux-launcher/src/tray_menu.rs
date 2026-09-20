use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::app::{WindowPositionHandle, WindowSizeHandle};
use windui::prelude::*;
use windui::signal::Signal;

use super::history_priorities::set_game_mode;
use super::launcher_icons::tray_icon;
use super::result_actions::ActionItem;
use super::ui_constants::{COMPACT_WINDOW_HEIGHT, SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH};
use super::window_geometry::request_monitor_position;

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_tray(
    shared_settings: Arc<RwLock<Settings>>,
    game_mode: Signal<bool>,
    game_mode_status: Signal<String>,
    settings_visible: Signal<bool>,
    query: Signal<String>,
    query_caret_position: Signal<usize>,
    results: Signal<Vec<flux_core::SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    show_results: Signal<bool>,
    window_size: WindowSizeHandle,
    window_position: WindowPositionHandle,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
) -> Tray {
    let settings_for_tray_toggle = Arc::clone(&shared_settings);
    let game_mode_for_tray = game_mode;
    let game_status_for_tray = game_mode_status;
    let settings_visible_for_tray = settings_visible;
    let settings_visible_for_left_click = settings_visible;
    let show_results_for_left_click = show_results;
    let size_for_left_click = window_size.clone();
    let position_for_left_click = window_position.clone();
    let settings_for_left_click = Arc::clone(&shared_settings);
    let clear_query_for_left_click = query;
    let caret_for_left_click = query_caret_position;
    let results_for_left_click = results;
    let selected_id_for_left_click = selected_id;
    let selected_index_for_left_click = selected_index;
    let selection_touched_for_left_click = selection_touched;
    let history_mode_for_left_click = history_mode;
    let history_cursor_for_left_click = history_cursor;
    let action_mode_for_left_click = action_mode;
    let action_index_for_left_click = action_index;
    let action_items_for_left_click = action_items;
    let inline_completion_for_left_click = inline_completion;
    let scroll_request_for_left_click = scroll_request;
    let settings_visible_for_left_click_clear = settings_visible;
    let launcher_width_for_left_click = launcher_width;
    let launcher_height_for_left_click = launcher_height;
    let size_for_left_click_clear = window_size.clone();
    let show_results_for_tray = show_results;
    let clear_query_for_tray = query;
    let caret_for_tray = query_caret_position;
    let results_for_tray = results;
    let selected_id_for_tray = selected_id;
    let selected_index_for_tray = selected_index;
    let selection_touched_for_tray = selection_touched;
    let history_mode_for_tray = history_mode;
    let history_cursor_for_tray = history_cursor;
    let action_mode_for_tray = action_mode;
    let action_index_for_tray = action_index;
    let action_items_for_tray = action_items;
    let inline_completion_for_tray = inline_completion;
    let scroll_request_for_tray = scroll_request;
    let settings_visible_for_tray_clear = settings_visible;
    let launcher_width_for_tray = launcher_width;
    let launcher_height_for_tray = launcher_height;
    let size_for_tray_clear = window_size.clone();
    let size_for_tray = window_size.clone();
    let position_for_tray = window_position.clone();
    let settings_for_tray_position = Arc::clone(&shared_settings);
    let size_for_settings = window_size.clone();
    let position_for_settings = window_position.clone();
    let settings_for_settings_position = Arc::clone(&shared_settings);
    Tray::new()
        .tooltip("Flux Launcher")
        .icon_rgba(16, 16, &tray_icon())
        .on_left_click(move |ctx| {
            settings_visible_for_left_click.set(false);
            // Same clear-before-show contract as the activation hotkey.
            let clear_query = settings_for_left_click
                .read()
                .map(|settings| settings.clear_query_on_activation)
                .unwrap_or(false);
            if clear_query {
                super::activation_clear::clear_query_for_activation(
                    clear_query_for_left_click,
                    caret_for_left_click,
                    results_for_left_click,
                    selected_id_for_left_click,
                    selected_index_for_left_click,
                    selection_touched_for_left_click,
                    show_results_for_left_click,
                    history_mode_for_left_click,
                    history_cursor_for_left_click,
                    action_mode_for_left_click,
                    action_index_for_left_click,
                    action_items_for_left_click,
                    inline_completion_for_left_click,
                    scroll_request_for_left_click,
                    settings_visible_for_left_click_clear,
                    launcher_width_for_left_click,
                    launcher_height_for_left_click,
                    size_for_left_click_clear.clone(),
                );
            }
            let height = if show_results_for_left_click.get() {
                launcher_height.get() as i32
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            if let Ok(settings) = settings_for_left_click.read() {
                request_monitor_position(
                    &position_for_left_click,
                    settings.monitor_preference,
                    launcher_width.get() as i32,
                    height,
                );
            }
            size_for_left_click.set(launcher_width.get() as i32, height);
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item("Show launcher", move |ctx| {
                settings_visible_for_tray.set(false);
                // Same clear-before-show contract as the activation hotkey.
                let clear_query = settings_for_tray_position
                    .read()
                    .map(|settings| settings.clear_query_on_activation)
                    .unwrap_or(false);
                if clear_query {
                    super::activation_clear::clear_query_for_activation(
                        clear_query_for_tray,
                        caret_for_tray,
                        results_for_tray,
                        selected_id_for_tray,
                        selected_index_for_tray,
                        selection_touched_for_tray,
                        show_results_for_tray,
                        history_mode_for_tray,
                        history_cursor_for_tray,
                        action_mode_for_tray,
                        action_index_for_tray,
                        action_items_for_tray,
                        inline_completion_for_tray,
                        scroll_request_for_tray,
                        settings_visible_for_tray_clear,
                        launcher_width_for_tray,
                        launcher_height_for_tray,
                        size_for_tray_clear.clone(),
                    );
                }
                let height = if show_results_for_tray.get() {
                    launcher_height.get() as i32
                } else {
                    COMPACT_WINDOW_HEIGHT
                };
                if let Ok(settings) = settings_for_tray_position.read() {
                    request_monitor_position(
                        &position_for_tray,
                        settings.monitor_preference,
                        launcher_width.get() as i32,
                        height,
                    );
                }
                size_for_tray.set(launcher_width.get() as i32, height);
                ctx.show_window();
            }),
            TrayMenuItem::item("Settings", move |ctx| {
                settings_visible.set(true);
                if let Ok(settings) = settings_for_settings_position.read() {
                    request_monitor_position(
                        &position_for_settings,
                        settings.monitor_preference,
                        SETTINGS_WINDOW_WIDTH,
                        SETTINGS_WINDOW_HEIGHT,
                    );
                }
                // Queue the Settings size before showing the hidden tray window. The
                // first frame must not use the compact 72-DIP launcher height.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                ctx.show_window();
                // Keep the request after show as well because the native show lifecycle
                // may consume a stale compact-size request from the previous hide.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::check("Game Mode", game_mode, move |_| {
                let enabled = !game_mode_for_tray.get();
                set_game_mode(
                    &settings_for_tray_toggle,
                    game_mode_for_tray,
                    game_status_for_tray,
                    enabled,
                );
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::item("Exit", |ctx| ctx.quit()),
        ])
}
