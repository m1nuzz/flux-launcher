use flux_core::SearchResult;
use windui::app::WindowSizeHandle;
use windui::signal::Signal;

use crate::result_actions::ActionItem;
use crate::window_geometry::launcher_window_geometry_with_sizes;

/// Clear search state before showing the launcher.
///
/// Every show path (activation hotkey, second-instance handoff, tray
/// actions) must call this FIRST. Clearing before the visibility change lets
/// the empty state repaint while the window is still visible, so a stale
/// query frame can never survive in the compositor until the next repaint
/// after re-show (which users perceive as an old-query flash with the caret
/// sweeping right-to-left).
#[allow(clippy::too_many_arguments)]
pub(crate) fn clear_query_for_activation(
    query: Signal<String>,
    query_caret_position: Signal<usize>,
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
    size: WindowSizeHandle,
) {
    query.set(String::new());
    query_caret_position.set(0);
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
    let (compact_width, compact_height) = launcher_window_geometry_with_sizes(
        settings_visible.get(),
        false,
        i32::from(launcher_width.get()),
        i32::from(launcher_height.get()),
    );
    size.set(compact_width, compact_height);
}
