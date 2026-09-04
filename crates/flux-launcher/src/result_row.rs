use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use crate::actions::{actions_for_result, ActionItem};
use crate::icons::{
    bundled_icon_rgba, icon_target_for_path, request_shell_icon, trace_result_icon_probe,
    ResultIconView,
};
use crate::launch;
use crate::plugins::{self, PluginAction};
use crate::settings_state::record_query_history;
use crate::{
    title_match_doc, ACTION_WINDOW_HEIGHT, LAUNCHER_FONT_FAMILY, SETTINGS_WINDOW_HEIGHT,
    SETTINGS_WINDOW_WIDTH,
};
use flux_core::{SearchResult, Settings};
use windui::app::WindowSizeHandle;
use windui::core::{ClickFn, EventCtx, Widget};
use windui::event::{Event, MouseButton, PointerKind};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

/// Invisible reactive widget that keeps the keyboard-selected row inside the
/// surrounding windui scroll viewport without painting an additional surface.
pub(crate) struct ResultRowAnchor {
    result_id: String,
    title: String,
    title_doc_signal: Signal<RichDoc>,
    trailing_signal: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    query: Signal<String>,
    scroll_pending: Signal<bool>,
    selection_color: Signal<Color>,
    action_items: Signal<Vec<ActionItem>>,
    action_index: Signal<usize>,
    action_scroll_pending: Signal<bool>,
    action_mode: Signal<bool>,
    launcher_width: Signal<u16>,
    action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    actions: Vec<ActionItem>,
    on_click: Option<ClickFn>,
    pressed: bool,
    last_pointer: Option<(i32, i32)>,
    last_selected: Option<bool>,
    last_query: String,
}

pub(crate) fn hover_position_changed(last: &mut Option<(i32, i32)>, position: (i32, i32)) -> bool {
    if *last == Some(position) {
        return false;
    }
    *last = Some(position);
    true
}

impl ResultRowAnchor {
    fn select_self(&self) {
        if self.selected_id.get() == self.result_id {
            return;
        }
        self.selection_touched.set(true);
        self.selected_id.set(self.result_id.clone());
        if let Some(index) = self
            .rows_refresh
            .get()
            .iter()
            .position(|result| result.id == self.result_id)
        {
            self.selected_index.set(index);
        }
        // The row itself is reactive, so selection painting updates without
        // rebuilding the whole list. Rebuilding here would discard the current
        // row geometry before scroll_into_view can reveal the selected result.
    }
}

impl Widget for ResultRowAnchor {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let selected = self.selected_id.get() == self.result_id;
        let query = self.query.get();
        let selection_changed = self.last_selected != Some(selected);
        let query_changed = self.last_query != query;
        let scroll_requested = self.scroll_pending.get();
        if selection_changed || query_changed {
            self.title_doc_signal
                .set(title_match_doc(&self.title, &query));
            self.trailing_signal.set(if selected {
                String::from("↵")
            } else {
                String::new()
            });
        }
        self.last_selected = Some(selected);
        self.last_query = query;
        // Scroll only after an explicit query/keyboard request. Wheel scrolling,
        // hover selection, and list repaints must never call scroll_into_view;
        // doing so feeds a layout mutation back into the ScrollWidget and pins
        // the viewport to the selected row (usually the top).
        if selected && scroll_requested {
            let row_id = ctx.id();
            let _ = ctx.tree_mut().scroll_into_view(row_id);
            self.scroll_pending.set(false);
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        let Event::Pointer(pointer) = event else {
            return false;
        };
        match pointer.kind {
            PointerKind::Enter => {
                // Do not select merely because the window appeared under a
                // stationary cursor; select on the first real Move instead.
                self.last_pointer = Some((pointer.pos.x, pointer.pos.y));
                ctx.mark_dirty();
                true
            }
            PointerKind::Move => {
                let position = (pointer.pos.x, pointer.pos.y);
                if hover_position_changed(&mut self.last_pointer, position) {
                    self.select_self();
                    ctx.mark_dirty();
                }
                true
            }
            PointerKind::Leave => {
                self.last_pointer = None;
                ctx.mark_dirty();
                true
            }
            PointerKind::Down if pointer.button == MouseButton::Left => {
                self.select_self();
                self.pressed = true;
                ctx.capture();
                ctx.mark_dirty();
                true
            }
            PointerKind::Up if pointer.button == MouseButton::Left => {
                let was_pressed = self.pressed;
                self.pressed = false;
                let inside = ctx.bounds().contains(pointer.pos);
                ctx.release_capture();
                ctx.mark_dirty();
                if was_pressed && inside {
                    if let Some(callback) = self.on_click.as_mut() {
                        callback(ctx);
                    }
                }
                true
            }
            PointerKind::Down if pointer.button == MouseButton::Right => {
                self.select_self();
                if !self.actions.is_empty() {
                    self.action_items.set(self.actions.clone());
                    self.action_index.set(0);
                    self.action_scroll_pending.set(true);
                    self.action_mode.set(true);
                    if let Some(handle) = self.action_window_slot.borrow().as_ref() {
                        handle.set(i32::from(self.launcher_width.get()), ACTION_WINDOW_HEIGHT);
                    }
                }
                ctx.mark_dirty();
                true
            }
            _ => false,
        }
    }

    fn take_click(&mut self, callback: ClickFn) {
        self.on_click = Some(callback);
    }

    fn reset_interaction(&mut self) {
        self.pressed = false;
        self.last_pointer = None;
    }

    fn cursor(&self) -> windui::event::CursorShape {
        windui::event::CursorShape::Hand
    }

    fn wants_right_click(&self) -> bool {
        true
    }

    fn paint(
        &self,
        bounds: windui::geometry::Rect,
        _content: windui::geometry::Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &windui::style::Style,
    ) {
        let selected = self.selected_id.get() == self.result_id;
        let color = if selected {
            self.selection_color.get()
        } else {
            Color::rgba(255, 255, 255, 18)
        };
        canvas.fill_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            10.0,
            &Paint::fill(color),
        );
    }
}

/// A reactive action-menu row that paints its own selection state and asks the
/// nearest scroll container to reveal the selected row after keyboard or pointer
/// navigation. It mirrors the result-list interaction contract so the submenu
/// scrolls exactly like the search results when the actions overflow the viewport.
pub(crate) struct ActionRowAnchor {
    pub(crate) item_index: usize,
    pub(crate) action_index: Signal<usize>,
    pub(crate) scroll_pending: Signal<bool>,
    pub(crate) last_pointer: Option<(i32, i32)>,
    pub(crate) pressed: bool,
    pub(crate) on_click: Option<ClickFn>,
}
impl Widget for ActionRowAnchor {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        if self.action_index.get() == self.item_index && self.scroll_pending.get() {
            let row_id = ctx.id();
            let _ = ctx.tree_mut().scroll_into_view(row_id);
            self.scroll_pending.set(false);
        }
    }
    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        let Event::Pointer(pointer) = event else {
            return false;
        };
        match pointer.kind {
            PointerKind::Enter => {
                self.last_pointer = Some((pointer.pos.x, pointer.pos.y));
                ctx.mark_dirty();
                true
            }
            PointerKind::Move => {
                let position = (pointer.pos.x, pointer.pos.y);
                if hover_position_changed(&mut self.last_pointer, position) {
                    self.action_index.set(self.item_index);
                    self.scroll_pending.set(true);
                    ctx.mark_dirty();
                }
                true
            }
            PointerKind::Leave => {
                self.last_pointer = None;
                ctx.mark_dirty();
                true
            }
            PointerKind::Down if pointer.button == MouseButton::Left => {
                self.action_index.set(self.item_index);
                self.scroll_pending.set(true);
                self.pressed = true;
                ctx.capture();
                ctx.mark_dirty();
                true
            }
            PointerKind::Up if pointer.button == MouseButton::Left => {
                let was_pressed = self.pressed;
                self.pressed = false;
                let inside = ctx.bounds().contains(pointer.pos);
                ctx.release_capture();
                ctx.mark_dirty();
                if was_pressed && inside {
                    if let Some(callback) = self.on_click.as_mut() {
                        callback(ctx);
                    }
                }
                true
            }
            _ => false,
        }
    }
    fn take_click(&mut self, callback: ClickFn) {
        self.on_click = Some(callback);
    }
    fn reset_interaction(&mut self) {
        self.pressed = false;
        self.last_pointer = None;
    }
    fn cursor(&self) -> windui::event::CursorShape {
        windui::event::CursorShape::Hand
    }
    fn paint(
        &self,
        bounds: windui::geometry::Rect,
        _content: windui::geometry::Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &windui::style::Style,
    ) {
        let selected = self.action_index.get() == self.item_index;
        let color = if selected {
            Color::rgba(76, 139, 245, 92)
        } else {
            Color::rgba(255, 255, 255, 14)
        };
        canvas.fill_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            9.0,
            &Paint::fill(color),
        );
    }
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
            record_query_history(&settings, &query_history, &query.get());
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
            if let Some(target) = target.as_deref() {
                launch::open_path_async(target);
                ctx.hide_window();
                return;
            }
            if let Some(action) = plugin_actions.borrow().get(&id).cloned() {
                plugins::execute_async(action);
                ctx.hide_window();
            }
        })
}
