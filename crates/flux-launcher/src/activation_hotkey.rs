use crate::actions::ActionItem;
use crate::fullscreen;
use crate::window_state::{
    launcher_window_geometry_with_sizes, request_monitor_position, should_show_launcher,
};
use flux_core::{should_suppress_activation, SearchResult, Settings};
use std::sync::{Arc, RwLock};
/// 全局激活热键回调所需的句柄集合：窗口操作句柄 + 需要重置/读取的 launcher 状态。
/// 回调本身不放 hwnd（见 vendor/windui 的借用纪律），只声明窗口操作意图。
use windui::app::{CursorVisibilityHandle, WindowPositionHandle, WindowSizeHandle};
use windui::event::HotkeyCtx;
use windui::signal::Signal;
pub(crate) struct ActivationHotkeyState {
    pub(crate) settings: Arc<RwLock<Settings>>,
    pub(crate) position: WindowPositionHandle,
    pub(crate) cursor_visibility: CursorVisibilityHandle,
    pub(crate) size: WindowSizeHandle,
    pub(crate) query: Signal<String>,
    pub(crate) results: Signal<Vec<SearchResult>>,
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) selection_touched: Signal<bool>,
    pub(crate) show_results: Signal<bool>,
    pub(crate) history_mode: Signal<bool>,
    pub(crate) history_cursor: Signal<Option<usize>>,
    pub(crate) action_mode: Signal<bool>,
    pub(crate) action_index: Signal<usize>,
    pub(crate) action_items: Signal<Vec<ActionItem>>,
    pub(crate) inline_completion: Signal<String>,
    pub(crate) scroll_request: Signal<bool>,
    pub(crate) settings_visible: Signal<bool>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
}

/// 全局激活热键（默认 `Alt+Space`）的处理：清空查询 → 重算几何与显示器位置 →
/// 按窗口「实际可见性」切换显示/隐藏。
pub(crate) fn handle_activation_hotkey(ctx: &mut HotkeyCtx, state: &ActivationHotkeyState) {
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
