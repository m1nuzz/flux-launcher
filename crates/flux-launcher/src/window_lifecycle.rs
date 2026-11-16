use std::sync::{Arc, RwLock};

use flux_core::{SearchResult, Settings};
use windui::app::{CursorVisibilityHandle, WindowSizeHandle};
use windui::prelude::{Color, Signal};

use crate::accent::selection_color_for_settings;
use crate::actions::ActionItem;
use crate::keyboard_layout;
use crate::window_state::launcher_window_geometry_with_sizes;

pub(crate) fn on_window_show(
    settings: Arc<RwLock<Settings>>,
    cursor_visibility: CursorVisibilityHandle,
    settings_visible: Signal<bool>,
    selection_color: Signal<Color>,
    window_size: WindowSizeHandle,
) -> impl FnMut() + 'static {
    move || {
        cursor_visibility.show();
        if let Ok(settings) = settings.read() {
            selection_color.set(selection_color_for_settings(&settings));
        }
        if settings_visible.get() {
            window_size.set(crate::SETTINGS_WINDOW_WIDTH, crate::SETTINGS_WINDOW_HEIGHT);
        }
    }
}

/// 窗口激活/显示时：按设置切换键盘布局，并刷新 Everything 的真实运行状态。
///
/// `refresh_everything_state` 必须是低成本操作（注册表 + IPC 探测，见
/// `everything::refresh_state_cheap()`）；它不能枚举进程或拉起 Everything，
/// 否则会把毫秒级回调变成数百毫秒阻塞。
pub(crate) fn on_window_activated(
    settings: Arc<RwLock<Settings>>,
    mut refresh_everything_state: impl FnMut() + 'static,
) -> impl FnMut() + 'static {
    move || {
        let layout_enabled = settings
            .read()
            .map(|settings| settings.switch_to_english_layout)
            .unwrap_or(false);
        if layout_enabled {
            keyboard_layout::switch_to_english();
        }
        // 用户可能在两次激活之间退出（或启动）了 Everything；此处刷新状态，
        // 避免设置页继续显示与事实不符的「IPC 可用」。
        refresh_everything_state();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn on_window_hide(
    settings: Arc<RwLock<Settings>>,
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
    action_items: Signal<Vec<ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    window_size: WindowSizeHandle,
) -> impl FnMut() + 'static {
    move || {
        let (enabled, clear_query) = settings
            .read()
            .map(|settings| {
                (
                    settings.switch_to_english_layout,
                    settings.clear_query_on_activation,
                )
            })
            .unwrap_or((false, clear_query_on_activation.get()));
        if enabled {
            keyboard_layout::restore_previous();
        }
        if clear_query {
            query.set(String::new());
            results.set(Vec::new());
            selected_id.set(String::new());
            selected_index.set(0);
            selection_touched.set(false);
            show_results.set(false);
            history_mode.set(false);
            history_cursor.set(None);
            action_mode.set(false);
            action_index.set(0);
            action_items.set(Vec::new());
            inline_completion.set(String::new());
            scroll_request.set(false);
            let (width, height) = launcher_window_geometry_with_sizes(
                settings_visible.get(),
                false,
                launcher_width.get() as i32,
                launcher_height.get() as i32,
            );
            window_size.set(width, height);
        }
    }
}
