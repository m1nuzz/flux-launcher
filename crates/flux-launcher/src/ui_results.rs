use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, SearchResult, Settings};
use windui::app::WindowSizeHandle;
use windui::prelude::*;
use windui::signal::Signal;

use super::history_priorities::set_result_priority;
use super::plugins::PluginAction;
use super::provider_snapshot::{refresh_merged_results, ProviderResults};
use super::result_actions::{execute_result_action, selected_result, ActionItem, ActionKind};
use super::result_row::{result_row, ActionBarGeometryProbe};
use super::result_widgets::ActionRowAnchor;
use super::ui_constants::{ACTION_BAR_HEIGHT, ACTION_BAR_WIDTH, RESULT_VIEWPORT_HEIGHT};

pub(crate) fn build_action_bar(show_results: Signal<bool>, action_mode: Signal<bool>) -> Element {
    let action_hint = |key: &'static str, label: &'static str| {
        Element::row()
            .height(22)
            .cross(Align::Center)
            .spacing(4)
            .child(
                Element::label(key)
                    .font_size(9.0)
                    .fg(Color::rgba(235, 243, 255, 235))
                    .bg(Color::rgba(255, 255, 255, 24))
                    .corner(5.0)
                    .padding_xy(4, 2),
            )
            .child(
                Element::label(label)
                    .font_size(10.0)
                    .fg(Color::rgba(222, 233, 248, 220)),
            )
    };
    // Use a bounded frame plus an explicitly centered content row instead of
    // full-width spacer children. This keeps the three hints visually centered
    // between the launcher content insets while the window width changes.
    let action_bar_content = Element::row()
        .height(22)
        .spacing(8)
        .child(action_hint("↵", "Open"))
        .child(action_hint("Ctrl + R", "Run as admin"))
        .child(action_hint("Alt + Enter", "Open file location"));
    Element::stack()
        .width(ACTION_BAR_WIDTH)
        .height(ACTION_BAR_HEIGHT)
        .child(action_bar_content.align(Align::Center))
        // Keep the probe inside the same real frame so its telemetry describes
        // the exact slot that is centered between the launcher insets.
        .child(
            Element::leaf()
                .widget(ActionBarGeometryProbe::default())
                .fill(),
        )
        .align(Align::Center)
        .visible_when(move || show_results.get() && !action_mode.get())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_result_list(
    result_source: Signal<Vec<SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    icon_refresh_generation: Signal<u64>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    action_items: Signal<Vec<ActionItem>>,
    action_index: Signal<usize>,
    action_scroll_pending: Signal<bool>,
    action_mode: Signal<bool>,
    launcher_width: Signal<u16>,
    query: Signal<String>,
    scroll_request: Signal<bool>,
    selection_color: Signal<Color>,
    settings: Arc<RwLock<Settings>>,
    query_history: Rc<RefCell<Vec<String>>>,
    stats_usage: Signal<String>,
    stats_top: Signal<String>,
    history_mode: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    settings_visible: Signal<bool>,
    window_size_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    show_results: Signal<bool>,
) -> Element {
    let result_list_body = Element::host_signal(result_source, move |result| {
        result_row(
            result,
            selected_id,
            selected_index,
            selection_touched,
            result_source,
            icon_refresh_generation,
            Rc::clone(&plugin_actions),
            action_items,
            action_index,
            action_scroll_pending,
            action_mode,
            launcher_width,
            query,
            scroll_request,
            selection_color,
            Arc::clone(&settings),
            Rc::clone(&query_history),
            stats_usage,
            stats_top,
            history_mode,
            recycle_bin_confirmation,
            settings_visible,
            Rc::clone(&window_size_slot),
        )
    })
    .width_match()
    // Keep the result body transparent so the window remains one continuous
    // Acrylic surface. Only individual result rows draw controls. The extra
    // right inset is local to the scroll content: it keeps the thumb clear of
    // row cards without changing the launcher window width.
    .padding_edges(6, 6, 18, 6);
    Element::scroll()
        .width_match()
        .height(RESULT_VIEWPORT_HEIGHT)
        .child(result_list_body)
        .visible_when(move || show_results.get() && !action_mode.get())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_action_list(
    action_items: Signal<Vec<ActionItem>>,
    settings: Arc<RwLock<Settings>>,
    priorities: Signal<Vec<PriorityEntry>>,
    providers: Rc<RefCell<ProviderResults>>,
    query: Signal<String>,
    result_source: Signal<Vec<SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    action_index: Signal<usize>,
    action_scroll_pending: Signal<bool>,
    action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    action_mode: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
) -> Element {
    let settings_for_action_list = Arc::clone(&settings);
    let priorities_for_action_list = priorities;
    let providers_for_action_list = Rc::clone(&providers);
    let query_for_action_list = query;
    Element::list_signal(
        action_items,
        |item| item.id.clone(),
        move |item| {
            let item_id = item.id.clone();
            let item_label = item.label.clone();
            let item_kind = item.kind.clone();
            let settings_for_item_action = Arc::clone(&settings_for_action_list);
            let priorities_for_item_action = priorities_for_action_list;
            let providers_for_item_action = Rc::clone(&providers_for_action_list);
            let query_for_item_action = query_for_action_list;
            Element::row()
                .widget(ActionRowAnchor {
                    item_index: action_items
                        .get()
                        .iter()
                        .position(|candidate| candidate.id == item_id)
                        .unwrap_or_default(),
                    action_index,
                    scroll_pending: action_scroll_pending,
                    last_pointer: None,
                    pressed: false,
                    on_click: None,
                })
                .reactive()
                .width_match()
                .height(36)
                .padding_xy(10, 4)
                .corner(9.0)
                .child(
                    Element::label(item_label)
                        .font_size(13.0)
                        .fg(Color::rgba(250, 252, 255, 255))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                )
                .on_click({
                    let action_window_slot = action_window_slot.clone();
                    move |ctx| {
                        let executed = selected_result(
                            &result_source.get(),
                            &selected_id.get(),
                            selected_index.get(),
                        )
                        .is_some_and(|result| {
                            if matches!(item_kind, ActionKind::SetPriority) {
                                let saved = set_result_priority(
                                    &settings_for_item_action,
                                    priorities_for_item_action,
                                    &result,
                                );
                                if saved {
                                    refresh_merged_results(
                                        &providers_for_item_action,
                                        query_for_item_action,
                                        priorities_for_item_action,
                                        result_source,
                                    );
                                }
                                saved
                            } else {
                                execute_result_action(&result, &item_kind)
                            }
                        });
                        if executed {
                            ctx.hide_window();
                        }
                        action_mode.set(false);
                        if let Some(handle) = action_window_slot.borrow().as_ref() {
                            handle.set(
                                i32::from(launcher_width.get()),
                                i32::from(launcher_height.get()),
                            );
                        }
                    }
                })
        },
    )
    .height(174)
    .corner(12.0)
    .visible_signal(action_mode)
}
