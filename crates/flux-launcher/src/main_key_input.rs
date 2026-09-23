//! 主窗口键盘事件处理。
//!
//! 从 `main.rs` 的 `on_key` 闭包整体外移（阶段十一 步骤 2）。闭包原本捕获 30+
//! 个句柄，因此这里用一个 `KeyInputContext` 结构聚合：`Signal` 是 Copy 句柄，
//! `Rc`/`Arc`/窗口句柄按引用访问，函数体内按同名字段解构，保持原实现逐行可对照。
//!
//! 行为约束（§3）：本模块只读取与写入信号，不调用 `show_window` / `hide_window`
//! 之外的生命周期入口；窗口显示/隐藏仍由调用方与既有回调负责。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, SearchResult, Settings};
use windui::app::{CursorVisibilityHandle, WindowOpHandle, WindowSizeHandle};
use windui::event::{Key, KeyEvent};
use windui::prelude::Signal;

use crate::hotkeys;
use crate::keyboard::{
    alt_key_is_down, cycle_query_history, handle_action_entry, handle_action_mode,
    handle_copy_shortcut, handle_enter_key, handle_open_location_shortcut,
    handle_result_navigation, handle_run_as_admin_shortcut, is_run_as_admin_key, open_history_mode,
    shift_key_is_down, ActionKeyContext, EnterKeyContext,
};
use crate::query::ProviderResults;
use crate::window_state::request_scroll;

/// `on_key` 闭包所需的全部句柄。
///
/// 字段名与 `main.rs` 中原有的 `*_for_keys` 别名一致，便于对照历史实现。
pub(crate) struct KeyInputContext {
    pub(crate) activation_recording_for_keys: Signal<bool>,
    pub(crate) activation_display_for_keys: Signal<String>,
    pub(crate) activation_key_for_keys: Signal<String>,
    pub(crate) activation_ctrl_for_keys: Signal<bool>,
    pub(crate) activation_alt_for_keys: Signal<bool>,
    pub(crate) activation_shift_for_keys: Signal<bool>,
    pub(crate) activation_meta_for_keys: Signal<bool>,
    pub(crate) activation_handle_for_recorder: windui::app::HotkeyHandle,
    pub(crate) query_for_keys: Signal<String>,
    pub(crate) query_caret_position_for_keys: Signal<usize>,
    pub(crate) results_for_keys: Signal<Vec<SearchResult>>,
    pub(crate) selected_id_for_keys: Signal<String>,
    pub(crate) selected_index_for_keys: Signal<usize>,
    pub(crate) scroll_request_for_keys: Signal<bool>,
    pub(crate) selection_touched_for_keys: Signal<bool>,
    pub(crate) action_mode_for_keys: Signal<bool>,
    pub(crate) action_index_for_keys: Signal<usize>,
    pub(crate) action_items_for_keys: Signal<Vec<crate::actions::ActionItem>>,
    pub(crate) action_scroll_pending_for_keys: Signal<bool>,
    pub(crate) recycle_bin_confirmation_for_keys: Signal<bool>,
    pub(crate) inline_completion_for_keys: Signal<String>,
    pub(crate) settings_visible_for_keys: Signal<bool>,
    pub(crate) history_mode_for_keys: Signal<bool>,
    pub(crate) history_cursor_for_keys: Signal<Option<usize>>,
    pub(crate) priorities_for_keys: Signal<Vec<PriorityEntry>>,
    pub(crate) show_results_for_keys: Signal<bool>,
    pub(crate) query_for_priority_keys: Signal<String>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) launcher_height: Signal<u16>,
    pub(crate) plugin_actions_for_keys: Rc<RefCell<HashMap<String, crate::plugins::PluginAction>>>,
    pub(crate) query_history_for_keys: Rc<RefCell<Vec<String>>>,
    pub(crate) settings_for_history_for_keys: Arc<RwLock<Settings>>,
    pub(crate) settings_for_priority_for_keys: Arc<RwLock<Settings>>,
    pub(crate) providers_for_keys: Rc<RefCell<ProviderResults>>,
    pub(crate) window_op_for_keys: WindowOpHandle,
    pub(crate) cursor_visibility_for_keys: CursorVisibilityHandle,
    pub(crate) size_for_keys: WindowSizeHandle,
}

/// 处理主窗口的一次键盘事件，返回是否已消费该事件。
///
/// 与 `main.rs` 原 `on_key` 闭包逐行等价：录制激活键、复制快捷键、历史模式、
/// 内联补全、Tab 选择、动作模式、以管理员运行、以及上/下/右/回车导航。
pub(crate) fn handle_key_input(event: KeyEvent, keys: &KeyInputContext) -> bool {
    let KeyInputContext {
        activation_recording_for_keys,
        activation_display_for_keys,
        activation_key_for_keys,
        activation_ctrl_for_keys,
        activation_alt_for_keys,
        activation_shift_for_keys,
        activation_meta_for_keys,
        ref activation_handle_for_recorder,
        query_for_keys,
        query_caret_position_for_keys,
        results_for_keys,
        selected_id_for_keys,
        selected_index_for_keys,
        scroll_request_for_keys,
        selection_touched_for_keys,
        action_mode_for_keys,
        action_index_for_keys,
        action_items_for_keys,
        action_scroll_pending_for_keys,
        recycle_bin_confirmation_for_keys,
        inline_completion_for_keys,
        settings_visible_for_keys,
        history_mode_for_keys,
        history_cursor_for_keys,
        priorities_for_keys,
        show_results_for_keys,
        query_for_priority_keys,
        launcher_width,
        launcher_height,
        ref plugin_actions_for_keys,
        ref query_history_for_keys,
        ref settings_for_history_for_keys,
        ref settings_for_priority_for_keys,
        ref providers_for_keys,
        ref window_op_for_keys,
        ref cursor_visibility_for_keys,
        ref size_for_keys,
    } = *keys;
    if activation_recording_for_keys.get() {
        if event.pressed {
            if let Some(configuration) =
                hotkeys::capture_config(&event, alt_key_is_down(), hotkeys::meta_key_is_down())
            {
                activation_key_for_keys.set(configuration.key.clone());
                activation_ctrl_for_keys.set(configuration.ctrl);
                activation_alt_for_keys.set(configuration.alt);
                activation_shift_for_keys.set(configuration.shift);
                activation_meta_for_keys.set(configuration.meta);
                activation_display_for_keys.set(hotkeys::display_config(&configuration));
                activation_recording_for_keys.set(false);
                activation_handle_for_recorder.set_enabled(true);
            }
        }
        return true;
    }
    if !event.pressed || settings_visible_for_keys.get() {
        return false;
    }
    let alt_down = alt_key_is_down();
    if !event.ctrl && !alt_down && matches!(event.key, Key::Char(_) | Key::Backspace | Key::Delete)
    {
        history_cursor_for_keys.set(None);
        cursor_visibility_for_keys.hide();
    }
    if event.ctrl
        && (event.shift || shift_key_is_down())
        && matches!(
            event.key,
            Key::Other(0x43) | Key::Char('c') | Key::Char('C')
        )
    {
        return handle_copy_shortcut(
            true,
            event.shift,
            shift_key_is_down(),
            &results_for_keys.get(),
            selected_id_for_keys,
            selected_index_for_keys,
        );
    }
    if event.ctrl
        && !event.shift
        && !shift_key_is_down()
        && matches!(
            event.key,
            Key::Other(0x43) | Key::Char('c') | Key::Char('C')
        )
    {
        return handle_copy_shortcut(
            false,
            event.shift,
            shift_key_is_down(),
            &results_for_keys.get(),
            selected_id_for_keys,
            selected_index_for_keys,
        );
    }
    if event.ctrl && matches!(event.key, Key::Char('h') | Key::Char('H')) {
        let history = query_history_for_keys.borrow();
        return open_history_mode(
            &history,
            query_for_keys,
            history_mode_for_keys,
            history_cursor_for_keys,
            action_mode_for_keys,
            action_items_for_keys,
            inline_completion_for_keys,
            selected_index_for_keys,
            selected_id_for_keys,
            results_for_keys,
            show_results_for_keys,
            size_for_keys,
            launcher_width,
            launcher_height,
        );
    }
    let query = query_for_keys.get();
    let history = query_history_for_keys.borrow();
    if alt_down && !event.ctrl && !event.shift && matches!(event.key, Key::Up | Key::Down) {
        return cycle_query_history(
            &history,
            event.key,
            history_cursor_for_keys,
            history_mode_for_keys,
            query_for_keys,
        );
    }
    if !history_mode_for_keys.get()
        && event.key == Key::Up
        && !alt_down
        && !event.ctrl
        && !event.shift
        && query.trim().is_empty()
    {
        if let Some(latest) = history.last() {
            history_cursor_for_keys.set(Some(history.len() - 1));
            query_for_keys.set(latest.clone());
            return true;
        }
    }
    drop(history);
    if query.trim().is_empty() {
        return false;
    }
    let current_results = results_for_keys.get();
    if current_results.is_empty() {
        return false;
    }

    if event.ctrl && event.key == Key::Tab {
        let suffix = inline_completion_for_keys.get();
        if !suffix.is_empty() {
            query_for_keys.set(format!("{query}{suffix}"));
            return true;
        }
    }

    // Match Flow Launcher: plain Tab selects the next result, while
    // Shift+Tab selects the previous result. Ctrl+Tab remains reserved
    // for inline completion above.
    if !event.ctrl && !alt_down && event.key == Key::Tab {
        let count = current_results.len();
        let next = if event.shift {
            selected_index_for_keys
                .get()
                .checked_sub(1)
                .unwrap_or(count - 1)
        } else {
            (selected_index_for_keys.get() + 1) % count
        };
        selection_touched_for_keys.set(true);
        selected_index_for_keys.set(next);
        if let Some(result) = current_results.get(next) {
            selected_id_for_keys.set(result.id.clone());
        }
        // Keep the existing row tree intact while changing only selection.
        // Rebuilding the DynList here resets row geometry and prevents the
        // pending scroll request from bringing the next result into view.
        request_scroll(scroll_request_for_keys);
        return true;
    }

    if event.key == Key::Enter && alt_key_is_down() {
        return handle_open_location_shortcut(
            history_mode_for_keys,
            query_for_keys,
            query_history_for_keys,
            settings_for_history_for_keys,
            &current_results,
            selected_id_for_keys,
            selected_index_for_keys,
        );
    }

    if action_mode_for_keys.get() {
        return handle_action_mode(
            event.key,
            &ActionKeyContext {
                action_mode: action_mode_for_keys,
                action_index: action_index_for_keys,
                action_items: action_items_for_keys,
                action_scroll_pending: action_scroll_pending_for_keys,
                history_mode: history_mode_for_keys,
                query: query_for_keys,
                query_history: Rc::clone(query_history_for_keys),
                current_results: current_results.clone(),
                selected_id: selected_id_for_keys,
                selected_index: selected_index_for_keys,
                settings: Arc::clone(settings_for_priority_for_keys),
                priorities: priorities_for_keys,
                providers: Rc::clone(providers_for_keys),
                priority_query: query_for_priority_keys,
                results: results_for_keys,
                window_op: window_op_for_keys.clone(),
                window_size: size_for_keys.clone(),
                launcher_width,
                launcher_height,
            },
        );
    }

    if is_run_as_admin_key(&event) {
        return handle_run_as_admin_shortcut(
            query_for_keys,
            query_history_for_keys,
            settings_for_history_for_keys,
            &current_results,
            selected_id_for_keys,
            selected_index_for_keys,
            window_op_for_keys,
        );
    }
    match event.key {
        Key::Up | Key::Down => handle_result_navigation(
            event.key,
            &current_results,
            selected_id_for_keys,
            selected_index_for_keys,
            selection_touched_for_keys,
            scroll_request_for_keys,
        ),
        Key::Right => handle_action_entry(
            query_for_keys,
            query_caret_position_for_keys,
            &current_results,
            selected_id_for_keys,
            selected_index_for_keys,
            plugin_actions_for_keys,
            action_items_for_keys,
            action_index_for_keys,
            action_scroll_pending_for_keys,
            action_mode_for_keys,
            show_results_for_keys,
            size_for_keys,
            launcher_width,
        ),
        Key::Enter => handle_enter_key(&EnterKeyContext {
            history_mode: history_mode_for_keys,
            query: query_for_keys,
            query_history: Rc::clone(query_history_for_keys),
            settings: Arc::clone(settings_for_history_for_keys),
            current_results: current_results.clone(),
            selected_id: selected_id_for_keys,
            selected_index: selected_index_for_keys,
            recycle_bin_confirmation: recycle_bin_confirmation_for_keys,
            settings_visible: settings_visible_for_keys,
            window_size: size_for_keys.clone(),
            window_op: window_op_for_keys.clone(),
            plugin_actions: Rc::clone(plugin_actions_for_keys),
        }),
        _ => false,
    }
}
