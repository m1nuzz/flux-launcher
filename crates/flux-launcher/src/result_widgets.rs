use std::cell::RefCell;
use std::rc::Rc;

use flux_core::SearchResult;
use windui::app::WindowSizeHandle;
use windui::core::{ClickFn, EventCtx, Widget};
use windui::event::{Event, MouseButton, PointerKind};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

use super::result_actions::ActionItem;
use super::ui_constants::ACTION_WINDOW_HEIGHT;

/// Invisible reactive widget that keeps the keyboard-selected row inside the
/// surrounding windui scroll viewport without painting an additional surface.
pub(crate) struct ResultRowAnchor {
    pub(crate) result_id: String,
    pub(crate) title: String,
    pub(crate) title_doc_signal: Signal<RichDoc>,
    pub(crate) trailing_signal: Signal<String>,
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) selection_touched: Signal<bool>,
    pub(crate) rows_refresh: Signal<Vec<SearchResult>>,
    pub(crate) query: Signal<String>,
    pub(crate) scroll_pending: Signal<bool>,
    pub(crate) selection_color: Signal<Color>,
    pub(crate) action_items: Signal<Vec<ActionItem>>,
    pub(crate) action_index: Signal<usize>,
    pub(crate) action_scroll_pending: Signal<bool>,
    pub(crate) action_mode: Signal<bool>,
    pub(crate) launcher_width: Signal<u16>,
    pub(crate) action_window_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
    pub(crate) actions: Vec<ActionItem>,
    pub(crate) on_click: Option<ClickFn>,
    pub(crate) pressed: bool,
    pub(crate) last_pointer: Option<(i32, i32)>,
    pub(crate) last_selected: Option<bool>,
    pub(crate) last_query: String,
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
                .set(crate::title_match_doc(&self.title, &query));
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

/// A stable result-row icon that starts with a lightweight fallback and swaps to the
/// cached Windows Shell image when the background icon worker completes. Keeping this
/// widget inside the existing row avoids rebuilding the dynamic result list, which
/// would otherwise reset row-local interaction state and can disturb scrolling.
pub(crate) struct ResultIconView {
    target: Option<String>,
    fallback: String,
    fallback_font: &'static str,
    refresh_generation: Signal<u64>,
    last_generation: u64,
    image: Option<Image>,
}

impl ResultIconView {
    pub(crate) fn new(
        target: Option<String>,
        fallback: String,
        fallback_font: &'static str,
        initial_rgba: Option<Vec<u8>>,
        refresh_generation: Signal<u64>,
    ) -> Self {
        let image = initial_rgba
            .as_deref()
            .and_then(|rgba| Image::from_rgba(32, 32, rgba).ok());
        Self {
            target,
            fallback,
            fallback_font,
            refresh_generation,
            last_generation: refresh_generation.get(),
            image,
        }
    }

    fn refresh_cached_image(&mut self) {
        let Some(target) = self.target.as_deref() else {
            return;
        };
        #[cfg(windows)]
        let rgba = crate::shell_icon_cache_lookup(target).flatten();
        #[cfg(not(windows))]
        let rgba: Option<Vec<u8>> = None;
        self.image = rgba
            .as_deref()
            .and_then(|bytes| Image::from_rgba(32, 32, bytes).ok());
    }
}

impl Widget for ResultIconView {
    fn measure(
        &self,
        _avail: Size,
        _style: &Style,
        _text: &mut dyn windui::text::TextEngine,
    ) -> Size {
        Size::new(32, 32)
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &Style,
    ) {
        if let Some(image) = self.image.as_ref() {
            canvas.draw_image(image, bounds, Fit::Contain, style.corner_radius, 1.0);
            return;
        }
        let fallback_style = Style {
            font_family: Some(self.fallback_font.to_owned()),
            font_size: 20.0,
            text_align: Align::Center,
            fg: Color::rgba(201, 218, 240, 235),
            fg_role: None,
            ..style.clone()
        };
        canvas.draw_text(
            &self.fallback,
            bounds,
            fallback_style.fg,
            Align::Center,
            &windui::text::TextStyle::of(&fallback_style),
        );
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let generation = self.refresh_generation.get();
        if generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        self.refresh_cached_image();
        ctx.mark_dirty();
    }

    fn on_event(&mut self, _ctx: &mut EventCtx, _event: &Event) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_constants::LAUNCHER_FONT_FAMILY;

    #[test]
    fn result_icon_view_accepts_valid_cached_rgba_and_keeps_placeholder_on_miss() {
        let generation = windui::prelude::signal(0_u64);
        let valid = [255_u8, 128, 64, 255].repeat(32 * 32);
        let loaded = ResultIconView::new(
            Some(String::from(r"C:\Program Files\Demo\demo.exe")),
            String::from("▣"),
            LAUNCHER_FONT_FAMILY,
            Some(valid),
            generation,
        );
        assert!(loaded.image.is_some());

        let pending = ResultIconView::new(
            Some(String::from(r"C:\Program Files\Pending\pending.exe")),
            String::from("▣"),
            LAUNCHER_FONT_FAMILY,
            None,
            generation,
        );
        assert!(pending.image.is_none());
    }

    #[test]
    fn stationary_pointer_after_enter_does_not_trigger_hover_selection() {
        let mut last = None;
        assert!(hover_position_changed(&mut last, (240, 120)));
        assert!(!hover_position_changed(&mut last, (240, 120)));
        assert!(hover_position_changed(&mut last, (241, 120)));
    }
}
