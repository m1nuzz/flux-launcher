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
use super::ui_constants::{
    ACTION_BAR_HEIGHT, ACTION_BAR_STATUS_WIDTH, ACTION_BAR_WIDTH, RESULT_VIEWPORT_HEIGHT,
};

/// Longest token the bar can show. The bound is belt-and-braces: the label is already
/// capped to one line and to ACTION_BAR_STATUS_WIDTH, so a status nobody anticipated
/// still cannot wrap or reach the key hints. It keeps the common case free of an
/// ellipsis.
const STATUS_TOKEN_MAX: usize = 9;

/// The action bar is a single 22 px line, and AGENTS.md requires the provider status to
/// be visible there. The providers publish sentences - "16 Everything result(s)" - which
/// do not fit beside the key hints, so this shortens each status the app actually
/// publishes into a token that does. Anything unrecognised is truncated rather than
/// returned whole: a longer string would print over the hints, because a Frame does not
/// push a sibling aside and will not make room either.
pub(crate) fn compact_provider_status(status: &str) -> String {
    let trimmed = status.trim();
    if trimmed.is_empty() || trimmed == "Ready" {
        return String::new();
    }
    // "N application result(s)", "N Everything result(s)", "N Flow plugin result(s)".
    for (marker, token) in [
        ("application result", "apps"),
        ("Everything result", "files"),
        ("Flow plugin result", "plug"),
    ] {
        if let Some(at) = trimmed.find(marker) {
            // The marker already carries the "result(s)", so what precedes it is the
            // count on its own. A result list is capped well below three digits, so a
            // bare "99+" reads any larger count without widening the token.
            let count = trimmed[..at].trim();
            if !count.is_empty() && count.chars().all(|c| c.is_ascii_digit()) {
                return if count.len() > 2 {
                    String::from("99+")
                } else {
                    format!("{count} {token}")
                };
            }
        }
    }
    for (exact, token) in [
        ("Everything is not available", "no ES"),
        ("Everything query timed out", "slow"),
        ("No native Flow plugins installed", "no plug"),
        ("No native Rust plugin host installed", "no host"),
        (
            "Everything auto-enable is disabled in Flux settings",
            "ES off",
        ),
        ("Game Mode: On", "GM on"),
        ("Game Mode: Off", "GM off"),
    ] {
        if trimmed == exact {
            return String::from(token);
        }
    }
    if trimmed.starts_with("Everything query failed to send") {
        return String::from("error");
    }
    if trimmed.starts_with("Native plugin host restarted") {
        return String::from("restart");
    }
    if let Some(count) = trimmed.strip_suffix("native Flow plugin(s) did not respond") {
        let count = count.trim();
        if !count.is_empty() && count.chars().all(|c| c.is_ascii_digit()) {
            return format!("{count} dead");
        }
    }
    let mut out: String = trimmed.chars().take(STATUS_TOKEN_MAX).collect();
    if trimmed.chars().count() > STATUS_TOKEN_MAX {
        out.pop();
    }
    out
}

pub(crate) fn build_action_bar(
    show_results: Signal<bool>,
    action_mode: Signal<bool>,
    status: Signal<String>,
) -> Element {
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
        .child(action_hint("Ctrl + R", "Admin"))
        .child(action_hint("Alt + Enter", "Location"));
    Element::stack()
        .width(ACTION_BAR_WIDTH)
        .height(ACTION_BAR_HEIGHT)
        .child(action_bar_content.align(Align::Center))
        // The provider status AGENTS.md asks for. A Frame positions every child against
        // the whole content rect, so this leading-edge child overlays the bar and leaves
        // the centred hints exactly where they are. The price of that is that a Frame
        // will not make room either, so the token is bounded to the free space the
        // shortened hints leave at the leading edge, one line, vertically centred on the
        // same line as the hints. Only the token hides when there is nothing to say.
        .child(
            Element::label_signal(status)
                .font_size(9.0)
                .fg(Color::rgba(222, 233, 248, 200))
                .max_lines(1)
                .truncate(Truncate::End)
                .width(ACTION_BAR_STATUS_WIDTH)
                .height(ACTION_BAR_HEIGHT)
                .align(Align::Start)
                .visible_when(move || !status.get().is_empty()),
        )
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
        // The window is fitted to the row count, so the viewport has to take what
        // is left rather than pin six rows: a fixed 288 made the column taller than
        // the fitted window and pushed the footer out of the client area for every
        // short query.
        .weight(1.0)
        .max_height(RESULT_VIEWPORT_HEIGHT)
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

#[cfg(test)]
mod action_bar_fit {
    use super::build_action_bar;
    use windui::core::Tree;
    use windui::geometry::Size;
    use windui::text::DWriteEngine;

    /// The action bar as main.rs nests it, measured with the real font. The three key
    /// hints fill 317 px of the 340 px bar, so the free space at each edge is about 11 px.
    ///
    /// That 50 px is what the provider status token is sized against, and it is also a
    /// trap: a fourth child in that space does not get its own room, it takes the hints'
    /// width instead. AGENTS.md asks for a provider status in this bar, so anyone changing
    /// one has to make room first - by shortening the hints or by widening the window -
    /// and this test is what shows whether the bar can take it.
    #[test]
    fn the_key_hints_fit_the_action_bar() {
        let mut tree = Tree::new();
        let root = windui::ui::Element::col()
            .width_match()
            .padding_edges(10, 13, 10, 7)
            .child(build_action_bar(
                windui::signal::signal(true),
                windui::signal::signal(false),
                windui::signal::signal(String::new()),
            ))
            .build(&mut tree);
        tree.root = Some(root);
        let mut engine = DWriteEngine::new();
        tree.layout_root(Size::new(420, 382), &mut engine);
        let bar_id = tree.get(root).unwrap().children[0];
        let bar = tree.abs_bounds(bar_id);
        let hints = tree.abs_bounds(tree.get(bar_id).unwrap().children[0]);

        assert_eq!(bar.w, super::super::ui_constants::ACTION_BAR_WIDTH);
        assert!(
            hints.w <= bar.w,
            "the key hints must fit the bar: hints {} vs bar {}",
            hints.w,
            bar.w
        );
        assert_eq!(
            hints.w, 238,
            "the hints' measured width; a different value means a hint changed, and the \
             free space a status could use has to be re-measured"
        );
        assert_eq!(hints.x - bar.x, 51, "free space at the leading edge");
        assert_eq!(
            (bar.x + bar.w) - (hints.x + hints.w),
            51,
            "free space at the trailing edge"
        );
    }
}

#[cfg(test)]
mod provider_status_in_the_bar {
    use super::compact_provider_status;
    use crate::ui_constants::{ACTION_BAR_HEIGHT, ACTION_BAR_STATUS_WIDTH, ACTION_BAR_WIDTH};
    use windui::core::Tree;
    use windui::geometry::{Rect, Size};
    use windui::text::DWriteEngine;

    fn bar_rects(status: &str) -> (Rect, Rect, Rect) {
        let mut tree = Tree::new();
        let root = windui::ui::Element::col()
            .width_match()
            .padding_edges(10, 13, 10, 7)
            .child(super::build_action_bar(
                windui::signal::signal(true),
                windui::signal::signal(false),
                windui::signal::signal(String::from(status)),
            ))
            .build(&mut tree);
        tree.root = Some(root);
        let mut engine = DWriteEngine::new();
        tree.layout_root(Size::new(420, 382), &mut engine);
        let bar_id = tree.get(root).unwrap().children[0];
        let kids = tree.get(bar_id).unwrap().children.clone();
        (
            tree.abs_bounds(bar_id),
            tree.abs_bounds(kids[0]),
            tree.abs_bounds(kids[1]),
        )
    }

    /// The provider status AGENTS.md requires shares one 22 px line with the three key
    /// hints. A Frame never moves the centred hints, and it never makes room either, so
    /// the token has to fit the free space the shortened hints leave. This is the test
    /// that fails the moment a hint grows back or the token is widened.
    #[test]
    fn the_token_fits_the_gutter_and_never_reaches_a_key_hint() {
        for status in [
            "16 Everything result(s)",
            "12 application result(s)",
            "3 Flow plugin result(s)",
            "2 native Flow plugin(s) did not respond",
            "Everything is not available",
            "Everything query timed out",
            "Everything query failed to send: broken pipe",
            "No native Flow plugins installed",
            "Native plugin host restarted (attempt 2): nope",
            "a status nobody anticipated that runs on and on",
        ] {
            let (bar, hints, token) = bar_rects(status);
            assert_eq!(
                bar.w, ACTION_BAR_WIDTH,
                "the bar keeps its width for {status:?}"
            );
            assert!(
                hints.w <= bar.w,
                "{status:?}: hints {} outgrew the bar {}",
                hints.w,
                bar.w
            );
            assert!(
                token.right() <= hints.x,
                "{status:?}: token {:?} reaches into the hints at x={} (gutter {} px)",
                token,
                hints.x,
                hints.x - bar.x
            );
            assert!(
                token.w <= ACTION_BAR_STATUS_WIDTH,
                "{status:?}: token {:?} is wider than its {ACTION_BAR_STATUS_WIDTH} px bound",
                token
            );
            assert_eq!(
                token.h, ACTION_BAR_HEIGHT,
                "{status:?}: the token must share the bar's line"
            );
        }
    }

    #[test]
    fn every_status_the_app_publishes_becomes_a_short_token() {
        for (input, expected) in [
            ("16 Everything result(s)", "16 files"),
            ("12 application result(s)", "12 apps"),
            ("3 Flow plugin result(s)", "3 plug"),
            ("2 native Flow plugin(s) did not respond", "2 dead"),
            ("Everything is not available", "no ES"),
            ("Everything query timed out", "slow"),
            ("Everything query failed to send: broken pipe", "error"),
            ("No native Flow plugins installed", "no plug"),
            ("No native Rust plugin host installed", "no host"),
            ("Native plugin host restarted (attempt 2): nope", "restart"),
            (
                "Everything auto-enable is disabled in Flux settings",
                "ES off",
            ),
            ("Game Mode: On", "GM on"),
            ("Game Mode: Off", "GM off"),
            ("999 Everything result(s)", "99+"),
            ("Ready", ""),
            ("", ""),
        ] {
            assert_eq!(compact_provider_status(input), expected, "status {input:?}");
        }
    }

    /// Nothing may wrap the bar: a second line would print over the hints.
    #[test]
    fn no_token_can_wrap_the_bar() {
        let long = "a provider status nobody anticipated, going on and on for ever";
        assert!(compact_provider_status(long).chars().count() <= 9);
    }
}
