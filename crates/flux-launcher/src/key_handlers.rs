use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{history_results, PriorityEntry, SearchResult, Settings};
use windui::app::{App, CursorVisibilityHandle, HotkeyHandle, WindowOpHandle, WindowSizeHandle};
use windui::event::{Key, KeyEvent};
use windui::signal::Signal;

use super::history_priorities::{record_launch, record_query_history, set_result_priority};
use super::hotkeys;
use super::input_keys::{alt_key_is_down, is_history_key, is_run_as_admin_key, shift_key_is_down};
use super::launch;
use super::plugins::PluginAction;
use super::provider_snapshot::{refresh_merged_results, settle_results_for_query, ProviderResults};
use super::result_actions::{
    actions_for_result, copy_result_file, copy_result_path, execute_result_action, selected_result,
    ActionItem, ActionKind,
};
use super::theme_text::{
    history_cursor_step, list_cursor_step, recall_history_row, refresh_inline_completion,
};
use super::ui_constants::{ACTION_WINDOW_HEIGHT, SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH};
use super::{plugins, request_scroll};

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_key_handlers(
    app: App,
    activation_recording: Signal<bool>,
    activation_display: Signal<String>,
    activation_key: Signal<String>,
    activation_ctrl: Signal<bool>,
    activation_alt: Signal<bool>,
    activation_shift: Signal<bool>,
    activation_meta: Signal<bool>,
    activation_handle: HotkeyHandle,
    query: Signal<String>,
    query_caret_position: Signal<usize>,
    results: Signal<Vec<SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    scroll_request: Signal<bool>,
    selection_touched: Signal<bool>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<ActionItem>>,
    action_scroll_pending: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    inline_completion: Signal<String>,
    settings_visible: Signal<bool>,
    query_history: Rc<RefCell<Vec<String>>>,
    stats_usage: Signal<String>,
    stats_top: Signal<String>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    shared_settings: Arc<RwLock<Settings>>,
    priorities: Signal<Vec<PriorityEntry>>,
    providers: Rc<RefCell<ProviderResults>>,
    window_op: WindowOpHandle,
    cursor_visibility: CursorVisibilityHandle,
    window_size: WindowSizeHandle,
    show_results: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
) -> App {
    let activation_handle_for_recorder = activation_handle.clone();
    let activation_recording_for_keys = activation_recording;
    let activation_display_for_keys = activation_display;
    let activation_key_for_keys = activation_key;
    let activation_ctrl_for_keys = activation_ctrl;
    let activation_alt_for_keys = activation_alt;
    let activation_shift_for_keys = activation_shift;
    let activation_meta_for_keys = activation_meta;
    let query_for_keys = query;
    let query_caret_position_for_keys = query_caret_position;
    let results_for_keys = results;
    let selected_id_for_keys = selected_id;
    let selected_index_for_keys = selected_index;
    let scroll_request_for_keys = scroll_request;
    let selection_touched_for_keys = selection_touched;
    let action_mode_for_keys = action_mode;
    let action_index_for_keys = action_index;
    let action_items_for_keys = action_items;
    let action_scroll_pending_for_keys = action_scroll_pending;
    let recycle_bin_confirmation_for_keys = recycle_bin_confirmation;
    let plugin_actions_for_keys = Rc::clone(&plugin_actions);
    let inline_completion_for_keys = inline_completion;
    let settings_visible_for_keys = settings_visible;
    let query_history_for_keys = Rc::clone(&query_history);
    let stats_usage_for_keys = stats_usage;
    let stats_top_for_keys = stats_top;
    let history_mode_for_keys = history_mode;
    let history_cursor_for_keys = history_cursor;
    let settings_for_history_for_keys = Arc::clone(&shared_settings);
    let settings_for_priority_for_keys = Arc::clone(&shared_settings);
    let priorities_for_keys = priorities;
    let providers_for_keys = Rc::clone(&providers);
    let query_for_priority_keys = query;
    let window_op_for_keys = window_op.clone();
    let cursor_visibility_for_keys = cursor_visibility.clone();
    let size_for_keys = window_size.clone();
    let show_results_for_keys = show_results;
    app.on_key(move |event: KeyEvent| {
        if activation_recording_for_keys.get() {
            // Recording is only meaningful while the dialog that started it is on
            // screen. Exiting Settings any other way (tray toggle, the Visual
            // tab's Apply) leaves the flag armed, and then the launcher swallows
            // every keystroke with the global hotkey still disabled. Repair that
            // orphaned state on the first key that arrives without the dialog.
            if !settings_visible_for_keys.get() {
                hotkeys::end_recording(
                    activation_recording_for_keys,
                    activation_display_for_keys,
                    activation_key_for_keys,
                    activation_ctrl_for_keys,
                    activation_alt_for_keys,
                    activation_shift_for_keys,
                    activation_meta_for_keys,
                    &activation_handle_for_recorder,
                );
                return false;
            }
            if event.pressed {
                let recorder_alt = alt_key_is_down();
                let recorder_meta = hotkeys::meta_key_is_down();
                // Escape is Flux's dismiss key everywhere else, so it cancels
                // recording rather than becoming the global activation hotkey.
                if event.key == Key::Escape
                    && !event.ctrl
                    && !event.shift
                    && !recorder_alt
                    && !recorder_meta
                {
                    hotkeys::end_recording(
                        activation_recording_for_keys,
                        activation_display_for_keys,
                        activation_key_for_keys,
                        activation_ctrl_for_keys,
                        activation_alt_for_keys,
                        activation_shift_for_keys,
                        activation_meta_for_keys,
                        &activation_handle_for_recorder,
                    );
                    return true;
                }
                if let Some(configuration) =
                    hotkeys::capture_config(&event, recorder_alt, recorder_meta)
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
        if !event.ctrl
            && !alt_down
            && matches!(event.key, Key::Char(_) | Key::Backspace | Key::Delete)
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
            if let Some(result) = selected_result(
                &results_for_keys.get(),
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if copy_result_file(&result) {
                    // Copying a hit is as much a committed search as running
                    // it, so the typed query has to come back on Alt+Up.
                    record_query_history(
                        &settings_for_history_for_keys,
                        &query_history_for_keys,
                        &query_for_keys.get(),
                        &selected_id_for_keys.get(),
                        stats_usage_for_keys,
                        stats_top_for_keys,
                    );
                    return true;
                }
            }
            return false;
        }
        if event.ctrl
            && !event.shift
            && !shift_key_is_down()
            && matches!(
                event.key,
                Key::Other(0x43) | Key::Char('c') | Key::Char('C')
            )
        {
            if let Some(result) = selected_result(
                &results_for_keys.get(),
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if copy_result_path(&result) {
                    record_query_history(
                        &settings_for_history_for_keys,
                        &query_history_for_keys,
                        &query_for_keys.get(),
                        &selected_id_for_keys.get(),
                        stats_usage_for_keys,
                        stats_top_for_keys,
                    );
                    return true;
                }
            }
            return false;
        }
        if is_history_key(&event) {
            let history = query_history_for_keys.borrow();
            if history.is_empty() {
                return false;
            }
            let filtered = history_results(&history, &query_for_keys.get());
            history_mode_for_keys.set(true);
            history_cursor_for_keys.set(None);
            action_mode_for_keys.set(false);
            action_items_for_keys.set(Vec::new());
            inline_completion_for_keys.set(String::new());
            selected_index_for_keys.set(0);
            selected_id_for_keys.set(
                filtered
                    .first()
                    .map(|result| result.id.clone())
                    .unwrap_or_default(),
            );
            results_for_keys.set(filtered);
            // These rows belong to no search generation, so the published list
            // is no longer what is on screen. Providers keep filling in for the
            // typed query and republish it as soon as history mode ends.
            providers_for_keys.borrow_mut().published_query.clear();
            show_results_for_keys.set(true);
            size_for_keys.set(
                i32::from(launcher_width.get()),
                i32::from(launcher_height.get()),
            );
            return true;
        }
        let query = query_for_keys.get();
        let history = query_history_for_keys.borrow();
        if alt_down && !event.ctrl && !event.shift && matches!(event.key, Key::Up | Key::Down) {
            if history.is_empty() {
                return false;
            }
            let Some(next) =
                history_cursor_step(history.len(), history_cursor_for_keys.get(), event.key)
            else {
                return false;
            };
            history_cursor_for_keys.set(Some(next));
            let recalled = history[next].clone();
            recall_history_row(
                &recalled,
                &settings_for_history_for_keys,
                query_for_keys,
                query_caret_position_for_keys,
                history_mode_for_keys,
                selected_index_for_keys,
                selected_id_for_keys,
                selection_touched_for_keys,
            );
            return true;
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
                let recalled = latest.clone();
                recall_history_row(
                    &recalled,
                    &settings_for_history_for_keys,
                    query_for_keys,
                    query_caret_position_for_keys,
                    history_mode_for_keys,
                    selected_index_for_keys,
                    selected_id_for_keys,
                    selection_touched_for_keys,
                );
                return true;
            }
        }
        drop(history);
        // An empty field is how the Ctrl+H list is normally opened, so it must
        // not bail out here: that guard used to swallow the arrow keys and
        // Enter for the history rows.
        if query.trim().is_empty() && !history_mode_for_keys.get() {
            return false;
        }
        let mut current_results = results_for_keys.get();
        // The panel is held back on purpose while a new query generation is still
        // answering, so a fast typist can be looking at rows from an earlier
        // keystroke: typing "chat" and pressing Enter within Everything's round
        // trip would otherwise launch the top hit of "cha". Acting on the panel
        // settles the generation - the rows, the highlight and this keystroke all
        // move to the text the user typed. History rows are excluded: there the
        // list on screen is the point of the keystroke.
        if !history_mode_for_keys.get() && providers_for_keys.borrow().published_query != query {
            let shown_head = current_results
                .first()
                .map(|result| result.id.clone())
                .unwrap_or_default();
            settle_results_for_query(
                &providers_for_keys,
                query_for_keys,
                priorities_for_keys,
                selected_id_for_keys,
                selected_index_for_keys,
                selection_touched_for_keys,
                results_for_keys,
                history_mode_for_keys,
            );
            current_results = results_for_keys.get();
            super::paint_trace::note(
                "resolve",
                &format!(
                    "query={query} resolved={} shown={shown_head} rows={}",
                    current_results
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                    current_results.len()
                ),
            );
        }
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
                let providers = providers_for_keys.borrow();
                refresh_inline_completion(
                    inline_completion_for_keys,
                    &query,
                    &providers.applications,
                    &result.id,
                );
            }
            // Keep the existing row tree intact while changing only selection.
            // Rebuilding the DynList here resets row geometry and prevents the
            // pending scroll request from bringing the next result into view.
            request_scroll(scroll_request_for_keys);
            return true;
        }

        if event.key == Key::Enter && alt_key_is_down() {
            if history_mode_for_keys.get() {
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    recall_history_row(
                        &result.title,
                        &settings_for_history_for_keys,
                        query_for_keys,
                        query_caret_position_for_keys,
                        history_mode_for_keys,
                        selected_index_for_keys,
                        selected_id_for_keys,
                        selection_touched_for_keys,
                    );
                    request_scroll(scroll_request_for_keys);
                }
                return true;
            }
            record_query_history(
                &settings_for_history_for_keys,
                &query_history_for_keys,
                &query,
                &selected_id_for_keys.get(),
                stats_usage_for_keys,
                stats_top_for_keys,
            );
            if let Some(result) = selected_result(
                &current_results,
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if let Some(target) = result.target.as_deref() {
                    record_launch(
                        &settings_for_history_for_keys,
                        &result.id,
                        &result.title,
                        stats_top_for_keys,
                    );
                    let _ = launch::open_file_location(target);
                }
            }
            return true;
        }

        if action_mode_for_keys.get() {
            let count = action_items_for_keys.get().len();
            if count == 0 {
                action_mode_for_keys.set(false);
                return true;
            }
            match event.key {
                Key::Up => {
                    action_index_for_keys.set(
                        action_index_for_keys
                            .get()
                            .checked_sub(1)
                            .unwrap_or(count - 1),
                    );
                    action_scroll_pending_for_keys.set(true);
                    return true;
                }
                Key::Down => {
                    action_index_for_keys.set((action_index_for_keys.get() + 1) % count);
                    action_scroll_pending_for_keys.set(true);
                    return true;
                }
                Key::Left | Key::Escape => {
                    action_mode_for_keys.set(false);
                    action_index_for_keys.set(0);
                    size_for_keys.set(
                        i32::from(launcher_width.get()),
                        i32::from(launcher_height.get()),
                    );
                    return true;
                }
                Key::Enter | Key::Space => {
                    if history_mode_for_keys.get() {
                        if let Some(result) = selected_result(
                            &current_results,
                            &selected_id_for_keys.get(),
                            selected_index_for_keys.get(),
                        ) {
                            recall_history_row(
                                &result.title,
                                &settings_for_history_for_keys,
                                query_for_keys,
                                query_caret_position_for_keys,
                                history_mode_for_keys,
                                selected_index_for_keys,
                                selected_id_for_keys,
                                selection_touched_for_keys,
                            );
                            request_scroll(scroll_request_for_keys);
                        }
                        return true;
                    }
                    record_query_history(
                        &settings_for_history_for_keys,
                        &query_history_for_keys,
                        &query,
                        &selected_id_for_keys.get(),
                        stats_usage_for_keys,
                        stats_top_for_keys,
                    );
                    if let Some(result) = selected_result(
                        &current_results,
                        &selected_id_for_keys.get(),
                        selected_index_for_keys.get(),
                    ) {
                        if let Some(action) = action_items_for_keys
                            .get()
                            .get(action_index_for_keys.get())
                            .cloned()
                        {
                            let executed = if matches!(action.kind, ActionKind::SetPriority) {
                                let saved = set_result_priority(
                                    &settings_for_priority_for_keys,
                                    priorities_for_keys,
                                    &result,
                                );
                                if saved {
                                    refresh_merged_results(
                                        &providers_for_keys,
                                        query_for_priority_keys,
                                        priorities_for_keys,
                                        results_for_keys,
                                    );
                                }
                                saved
                            } else {
                                execute_result_action(&result, &action.kind)
                            };
                            if executed {
                                // Priority assignment opens nothing; every other
                                // executed action runs its result.
                                if !matches!(action.kind, ActionKind::SetPriority) {
                                    record_launch(
                                        &settings_for_history_for_keys,
                                        &result.id,
                                        &result.title,
                                        stats_top_for_keys,
                                    );
                                }
                                window_op_for_keys.hide_window();
                            }
                        }
                    }
                    action_mode_for_keys.set(false);
                    size_for_keys.set(
                        i32::from(launcher_width.get()),
                        i32::from(launcher_height.get()),
                    );
                    return true;
                }
                _ => return true,
            }
        }

        if is_run_as_admin_key(&event) {
            record_query_history(
                &settings_for_history_for_keys,
                &query_history_for_keys,
                &query,
                &selected_id_for_keys.get(),
                stats_usage_for_keys,
                stats_top_for_keys,
            );
            if let Some(result) = selected_result(
                &current_results,
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if let Some(target) = result.target.as_deref() {
                    if launch::run_as_admin(target) {
                        record_launch(
                            &settings_for_history_for_keys,
                            &result.id,
                            &result.title,
                            stats_top_for_keys,
                        );
                        window_op_for_keys.hide_window();
                    }
                }
            }
            return true;
        }
        match event.key {
            Key::Up | Key::Down => {
                let count = current_results.len();
                let next = list_cursor_step(count, selected_index_for_keys.get(), event.key);
                selection_touched_for_keys.set(true);
                selected_index_for_keys.set(next);
                if let Some(result) = current_results.get(next) {
                    selected_id_for_keys.set(result.id.clone());
                    let providers = providers_for_keys.borrow();
                    refresh_inline_completion(
                        inline_completion_for_keys,
                        &query,
                        &providers.applications,
                        &result.id,
                    );
                }
                // Preserve the current row geometry so scroll_into_view can
                // move the viewport after the selected result changes.
                request_scroll(scroll_request_for_keys);
                true
            }
            Key::Right => {
                // Ctrl+Right is the input's word-jump: never open the action
                // bar with it, even when the caret sits at the query end.
                if event.ctrl {
                    return false;
                }
                // History rows carry no actions, and opening the bar over them
                // would leave the arrows driving action items while Enter still
                // commits a query.
                if history_mode_for_keys.get() {
                    return false;
                }
                if query_caret_position_for_keys.get() != query_for_keys.get().chars().count() {
                    return false;
                }
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    let actions = actions_for_result(&result, &plugin_actions_for_keys.borrow());
                    if !actions.is_empty() {
                        action_items_for_keys.set(actions);
                        action_index_for_keys.set(0);
                        action_scroll_pending_for_keys.set(true);
                        action_mode_for_keys.set(true);
                        show_results_for_keys.set(true);
                        size_for_keys.set(i32::from(launcher_width.get()), ACTION_WINDOW_HEIGHT);
                    }
                }
                true
            }
            Key::Enter => {
                if history_mode_for_keys.get() {
                    if let Some(result) = selected_result(
                        &current_results,
                        &selected_id_for_keys.get(),
                        selected_index_for_keys.get(),
                    ) {
                        recall_history_row(
                            &result.title,
                            &settings_for_history_for_keys,
                            query_for_keys,
                            query_caret_position_for_keys,
                            history_mode_for_keys,
                            selected_index_for_keys,
                            selected_id_for_keys,
                            selection_touched_for_keys,
                        );
                        request_scroll(scroll_request_for_keys);
                    }
                    return true;
                }
                record_query_history(
                    &settings_for_history_for_keys,
                    &query_history_for_keys,
                    &query,
                    &selected_id_for_keys.get(),
                    stats_usage_for_keys,
                    stats_top_for_keys,
                );
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    if result.id == "empty-recycle-bin" {
                        recycle_bin_confirmation_for_keys.set(true);
                    } else if result.id == "flux-settings" {
                        settings_visible_for_keys.set(true);
                        size_for_keys.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                    } else if result.id == "open-recycle-bin" {
                        record_launch(
                            &settings_for_history_for_keys,
                            &result.id,
                            &result.title,
                            stats_top_for_keys,
                        );
                        launch::open_recycle_bin_async();
                        window_op_for_keys.hide_window();
                    } else if let Some(target) = result.target.as_deref() {
                        record_launch(
                            &settings_for_history_for_keys,
                            &result.id,
                            &result.title,
                            stats_top_for_keys,
                        );
                        launch::open_path_async(target);
                        window_op_for_keys.hide_window();
                    } else if let Some(action) =
                        plugin_actions_for_keys.borrow().get(&result.id).cloned()
                    {
                        record_launch(
                            &settings_for_history_for_keys,
                            &result.id,
                            &result.title,
                            stats_top_for_keys,
                        );
                        plugins::execute_async(action);
                        window_op_for_keys.hide_window();
                    }
                }
                true
            }
            _ => false,
        }
    })
}
