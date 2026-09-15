use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::app::{
    App, CursorVisibilityHandle, HotkeyHandle, WindowPositionHandle, WindowSizeHandle,
};
use windui::event::Hotkey;
use windui::signal::Signal;

use super::fullscreen;
use super::history_priorities::set_game_mode;
use super::hotkeys;
use super::input_keys::{launcher_is_foreground, should_show_launcher};
use super::window_geometry::{launcher_window_geometry_with_sizes, request_monitor_position};
use flux_core::should_suppress_activation;

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_activation_hotkey(
    app: &mut App,
    activation_hotkey: Hotkey,
    shared_settings: Arc<RwLock<Settings>>,
    window_position: WindowPositionHandle,
    cursor_visibility: CursorVisibilityHandle,
    window_size: WindowSizeHandle,
    query: Signal<String>,
    results: Signal<Vec<flux_core::SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    show_results: Signal<bool>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<super::result_actions::ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    settings_visible: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
) -> HotkeyHandle {
    let settings_for_activation = Arc::clone(&shared_settings);
    let position_for_activation = window_position.clone();
    let cursor_visibility_for_activation = cursor_visibility.clone();
    let size_for_activation = window_size.clone();
    let query_for_activation = query;
    let results_for_activation = results;
    let selected_id_for_activation = selected_id;
    let selected_index_for_activation = selected_index;
    let selection_touched_for_activation = selection_touched;
    let show_results_for_activation = show_results;
    let history_mode_for_activation = history_mode;
    let history_cursor_for_activation = history_cursor;
    let action_mode_for_activation = action_mode;
    let action_index_for_activation = action_index;
    let action_items_for_activation = action_items;
    let inline_completion_for_activation = inline_completion;
    let scroll_request_for_activation = scroll_request;
    let settings_visible_for_activation = settings_visible;
    app.hotkey_handle(activation_hotkey, move |ctx| {
        let settings = settings_for_activation
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
            // Clear before toggling visibility. The previous implementation did
            // this only from on_window_hide, which allowed the old query frame to
            // survive in the compositor until the next repaint after re-show.
            if settings.clear_query_on_activation {
                query_for_activation.set(String::new());
                results_for_activation.set(Vec::new());
                selected_id_for_activation.set(String::new());
                selected_index_for_activation.set(0);
                selection_touched_for_activation.set(false);
                show_results_for_activation.set(false);
                history_mode_for_activation.set(false);
                history_cursor_for_activation.set(None);
                action_mode_for_activation.set(false);
                action_index_for_activation.set(0);
                action_items_for_activation.set(Vec::new());
                inline_completion_for_activation.set(String::new());
                scroll_request_for_activation.set(false);
                let (compact_width, compact_height) = launcher_window_geometry_with_sizes(
                    settings_visible_for_activation.get(),
                    false,
                    i32::from(launcher_width.get()),
                    i32::from(launcher_height.get()),
                );
                size_for_activation.set(compact_width, compact_height);
            }
            let (width, height) = launcher_window_geometry_with_sizes(
                settings_visible_for_activation.get(),
                show_results_for_activation.get(),
                i32::from(launcher_width.get()),
                i32::from(launcher_height.get()),
            );
            request_monitor_position(
                &position_for_activation,
                settings.monitor_preference,
                width,
                height,
            );
            cursor_visibility_for_activation.show();
            if should_show_launcher(launcher_is_foreground()) {
                ctx.show_window();
            } else {
                ctx.hide_window();
            }
        }
    })
}

pub(crate) fn register_game_mode_hotkey(
    app: App,
    shared_settings: Arc<RwLock<Settings>>,
    game_mode: Signal<bool>,
    game_mode_status: Signal<String>,
) -> App {
    let settings_for_game_hotkey = Arc::clone(&shared_settings);
    let game_mode_for_hotkey = game_mode;
    let game_mode_status_for_hotkey = game_mode_status;
    app.hotkey(hotkeys::game_mode_toggle_hotkey(), move |_| {
        let enabled = !game_mode_for_hotkey.get();
        set_game_mode(
            &settings_for_game_hotkey,
            game_mode_for_hotkey,
            game_mode_status_for_hotkey,
            enabled,
        );
    })
}
