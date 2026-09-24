use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{SearchResult, Settings};
use windui::app::WindowSizeHandle;
use windui::core::Widget;
use windui::geometry::Rect;
use windui::prelude::*;
use windui::render::Canvas;
use windui::style::Style;

use super::applications::resolve_bare_executable_path;
use super::launcher_icons::bundled_icon_rgba;
use super::plugins::{execute_async, PluginAction};
use super::result_actions::{actions_for_result, ActionItem};
use super::result_widgets::{ResultIconView, ResultRowAnchor};
use super::ui_constants::{LAUNCHER_FONT_FAMILY, SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH};
use super::{
    launch, record_launch, record_query_history, request_shell_icon, title_match_doc,
    trace_result_icon_probe,
};

#[derive(Default)]
pub(crate) struct ActionBarGeometryProbe {
    last: Cell<Option<(i32, i32, i32, i32)>>,
}

impl Widget for ActionBarGeometryProbe {
    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        _canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        if std::env::var_os("FLUX_SMOKE_ACTION_BAR").is_none() {
            return;
        }
        let geometry = (bounds.x, bounds.y, bounds.w, bounds.h);
        if self.last.get() != Some(geometry) {
            eprintln!(
                "ActionBarGeometry: x={} y={} width={} height={}",
                geometry.0, geometry.1, geometry.2, geometry.3
            );
            self.last.set(Some(geometry));
        }
    }
}

pub(crate) fn icon_target_for_path(target: &str) -> String {
    resolve_bare_executable_path(target).unwrap_or_else(|| target.to_owned())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn result_row(
    result: SearchResult,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    icon_refresh_generation: Signal<u64>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    action_items: Signal<Vec<ActionItem>>,
    action_index: Signal<usize>,
    action_scroll_pending: Signal<bool>,
    action_mode: Signal<bool>,
    launcher_width: Signal<u16>,
    query: Signal<String>,
    scroll_pending: Signal<bool>,
    selection_color: Signal<Color>,
    settings: Arc<RwLock<Settings>>,
    query_history: Rc<RefCell<Vec<String>>>,
    stats_usage: Signal<String>,
    stats_top: Signal<String>,
    history_mode: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    settings_visible: Signal<bool>,
    window_size_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
) -> Element {
    let result_for_actions = result.clone();
    let id = result.id;
    let target = result.target;
    let title = result.title;
    let subtitle = result.subtitle;
    let icon_target = target.as_deref().map(icon_target_for_path);
    let (glyph, glyph_font) = match id.as_str() {
        "empty-recycle-bin" => (String::from("\u{ea99}"), "Segoe Fluent Icons"),
        "open-recycle-bin" => (String::from("\u{e74d}"), "Segoe Fluent Icons"),
        // Recalling a previous search is what these rows do, so mark them with an
        // hourglass instead of the generic placeholder square. The family is named
        // explicitly: U+231B has an emoji presentation by default, and the rows are
        // painted monochrome.
        _ if id.starts_with("history:") => (String::from("⏳"), "Segoe UI Symbol"),
        _ if subtitle.contains("Application") => (String::from("◉"), LAUNCHER_FONT_FAMILY),
        _ => (String::from("▣"), LAUNCHER_FONT_FAMILY),
    };
    let icon =
        bundled_icon_rgba(&id).or_else(|| icon_target.as_deref().and_then(request_shell_icon));
    let actions = actions_for_result(&result_for_actions, &plugin_actions.borrow());
    trace_result_icon_probe(
        &title,
        target.as_deref(),
        icon_target.as_deref(),
        icon.is_some(),
    );
    let icon_element = Element::leaf()
        .widget(ResultIconView::new(
            icon_target,
            glyph,
            glyph_font,
            icon,
            icon_refresh_generation,
        ))
        .reactive()
        .width(32)
        .height(32)
        .corner(7.0);
    let selected = selected_id.get() == id;
    let title_doc_signal = signal(title_match_doc(&title, &query.get()));
    let trailing_signal = signal(if selected {
        String::from("↵")
    } else {
        String::new()
    });
    Element::row()
        .widget(ResultRowAnchor {
            result_id: id.clone(),
            title: title.clone(),
            title_doc_signal,
            trailing_signal,
            selected_id,
            selected_index,
            selection_touched,
            rows_refresh,
            query,
            scroll_pending,
            selection_color,
            action_items,
            action_index,
            action_scroll_pending,
            action_mode,
            launcher_width,
            action_window_slot: Rc::clone(&window_size_slot),
            actions,
            on_click: None,
            pressed: false,
            last_pointer: None,
            last_selected: None,
            last_query: query.get(),
        })
        .reactive()
        .width_match()
        .height(46)
        .padding_xy(12, 3)
        .spacing(10)
        .corner(10.0)
        // Selection background is owned exclusively by ResultRowAnchor. Keeping
        // a static background here leaves stale highlights after selection moves.
        .child(icon_element)
        .child(
            Element::col()
                .weight(1.0)
                .spacing(1)
                .child(
                    Element::rich_signal(title_doc_signal)
                        .selection_requires_ctrl(true)
                        .copy_menu(false)
                        .font_family(LAUNCHER_FONT_FAMILY)
                        .font_size(14.0)
                        .width_match(),
                )
                .child(
                    Element::label(subtitle)
                        .font_family(LAUNCHER_FONT_FAMILY)
                        .font_size(12.0)
                        .fg(Color::rgba(248, 251, 255, 255))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                ),
        )
        .child(
            Element::label_signal(trailing_signal)
                .font_size(17.0)
                .fg(Color::rgba(238, 246, 255, 230))
                .width(22)
                .align(Align::Center),
        )
        .on_click(move |ctx| {
            if history_mode.get() {
                query.set(title.clone());
                history_mode.set(false);
                return;
            }
            record_query_history(
                &settings,
                &query_history,
                &query.get(),
                &id,
                stats_usage,
                stats_top,
            );
            selected_id.set(id.clone());
            selection_touched.set(true);
            if let Some(index) = rows_refresh.get().iter().position(|result| result.id == id) {
                selected_index.set(index);
            }
            if id == "empty-recycle-bin" {
                recycle_bin_confirmation.set(true);
                return;
            }
            if id == "flux-settings" {
                settings_visible.set(true);
                if let Some(window_size) = window_size_slot.borrow().as_ref() {
                    window_size.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                }
                return;
            }
            if id == "open-recycle-bin" {
                launch::open_recycle_bin_async();
                ctx.hide_window();
                return;
            }
            record_launch(&settings, &id, &title, stats_top);
            if let Some(target) = target.as_deref() {
                launch::open_path_async(target);
                ctx.hide_window();
                return;
            }
            if let Some(action) = plugin_actions.borrow().get(&id).cloned() {
                execute_async(action);
                ctx.hide_window();
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_merge::normalize_built_in_executable_targets;
    use flux_core::{ResultKind, ResultSource};

    #[test]
    fn icon_target_preserves_explicit_paths_and_resolves_bare_names_on_windows() {
        let explicit = r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
        assert_eq!(icon_target_for_path(explicit), explicit);
    }

    #[cfg(windows)]
    #[test]
    fn icon_target_resolves_bare_powershell_to_a_real_path() {
        let resolved = icon_target_for_path("powershell.exe").to_ascii_lowercase();
        assert!(resolved.ends_with(r"\powershell.exe"));
        assert!(resolved.contains(r"\windowspowershell\"));
    }

    #[cfg(windows)]
    #[test]
    fn builtin_power_shell_target_is_resolved_before_merge_and_icon_loading() {
        let mut results = vec![SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("powershell.exe")),
        }];

        normalize_built_in_executable_targets(&mut results);

        let target = results[0].target.as_deref().unwrap().to_ascii_lowercase();
        assert!(target.ends_with(r"\powershell.exe"));
        assert!(target.contains(r"\windowspowershell\"));
    }
}
