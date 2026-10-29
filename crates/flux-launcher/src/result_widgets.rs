use std::cell::RefCell;
use std::rc::Rc;

use flux_core::SearchResult;
use windui::app::WindowSizeHandle;
use windui::core::{ClickFn, EventCtx, Widget};
use windui::event::{Event, MouseButton, PointerKind};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

use super::result_actions::ActionItem;
use super::ui_constants::{ACTION_WINDOW_HEIGHT, RESULT_ROWS_VISIBLE};

/// A row the first page cannot show has to be revealed as soon as it exists.
///
/// A history recall restores the row the query last launched, and that row can sit
/// far below the viewport the recalled list starts at: no key was pressed to move
/// there, so nothing asks for it, and the highlight ends up invisible. Rows on the
/// first page are left alone - the viewport already starts there, and revealing
/// them on every repaint is what pins the list to the highlight.
pub(crate) fn needs_reveal_on_build(selected: bool, index: Option<usize>) -> bool {
    selected && index.is_some_and(|index| index >= RESULT_ROWS_VISIBLE as usize)
}

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
    /// Armed at build time for a selected row the first page cannot show; cleared
    /// once it has been brought into view. See [`needs_reveal_on_build`].
    pub(crate) reveal_on_build: bool,
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
        // A restored selection is not navigation: no key asked for it, so nothing
        // else scrolls to it. Reveal it once, on the first pass that has geometry.
        if self.reveal_on_build {
            if ctx.bounds().is_empty() {
                return;
            }
            self.reveal_on_build = false;
            let row_id = ctx.id();
            let _ = ctx.tree_mut().scroll_into_view(row_id);
            super::paint_trace::note("scroll-reveal", &format!("id={}", self.result_id));
        }
        // Scroll only after an explicit query/keyboard request. Wheel scrolling,
        // hover selection, and list repaints must never call scroll_into_view;
        // doing so feeds a layout mutation back into the ScrollWidget and pins
        // the viewport to the selected row (usually the top).
        if selected && scroll_requested {
            let row_id = ctx.id();
            // The reactive pass runs before measure/arrange, so on the frame a row
            // is rebuilt its bounds are still empty and a scroll computed from them
            // would clamp against a zero rect and the previous frame's content
            // height. Leave the request pending and let the next pass, which sees
            // real geometry, do the scroll.
            if ctx.bounds().is_empty() {
                return;
            }
            // A list without results collapses the window to a strip, and the
            // viewport is then a few pixels tall. A scroll computed against that
            // geometry is a number, not a place: the window grows back a frame
            // later and the offset stays, clipping the first row. Wait for a
            // viewport that can actually show the row.
            let tree = ctx.tree_mut();
            let viewport = tree
                .scroll_ancestor(row_id)
                .and_then(|scroll_id| tree.scroll_viewport_height(scroll_id))
                .unwrap_or(0);
            if viewport < ctx.bounds().h {
                return;
            }
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

/// Puts the result viewport back on the first row of every new list.
///
/// The scroll offset lives on the windui scroll node and replacing the rows does
/// not touch it. Recalling a query with Alt+Up therefore handed the previous
/// list's offset to the recalled rows: the row the user had just recalled - the
/// top one - was clipped by up to a row height, and the only way to read it was to
/// drag the list up by hand.
///
/// A query is a new list, and a new list starts at its first row. This anchor is
/// the first child of the scroll container, so its scroll node is the result
/// viewport, and it is built once and survives every row rebuild - which is what
/// lets it act on the query change itself, before the recalled rows exist.
pub(crate) struct ScrollTopAnchor {
    pub(crate) query: Signal<String>,
    pub(crate) history_mode: Signal<bool>,
    /// Bumped by a history recall.
    ///
    /// A recall has to put the list back on its first row even when it recalls
    /// the query that is already in the field: then the query string does not
    /// change, and nothing else would reset the viewport. The old offset then
    /// survives, and the minimal alignment that reveals the selected row puts it
    /// on the list's own top inset - which is exactly the "the top row is
    /// clipped" report.
    pub(crate) reset_request: Signal<u64>,
    pub(crate) last_query: String,
    pub(crate) last_history_mode: bool,
    pub(crate) last_reset_request: u64,
    /// The offset the trace last reported. A driver script proves the viewport is
    /// really on the first row from these numbers, so they are written whenever
    /// the offset changes and never otherwise.
    pub(crate) traced_offset: i32,
}

impl Widget for ScrollTopAnchor {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let query = self.query.get();
        let history_mode = self.history_mode.get();
        let reset_request = self.reset_request.get();
        if query != self.last_query
            || history_mode != self.last_history_mode
            || reset_request != self.last_reset_request
        {
            self.last_query = query.clone();
            self.last_history_mode = history_mode;
            self.last_reset_request = reset_request;
            // The Ctrl+H list is a new list too, and it does not change the query.
            //
            // Write the offset directly instead of revealing this anchor: a
            // zero-height anchor that has not been laid out yet has no rect, so
            // `scroll_into_view` finds nothing to align and quietly leaves the old
            // offset in place. That is the one frame that matters - the recalled
            // rows are measured right after - and it made the reset depend on
            // whether the anchor happened to have geometry already.
            let anchor_id = ctx.id();
            let tree = ctx.tree_mut();
            let scroll_id = tree.scroll_ancestor(anchor_id);
            let changed = scroll_id.is_some_and(|scroll_id| tree.set_scroll_y(scroll_id, 0));
            super::paint_trace::note(
                "scroll-top",
                &format!(
                    "query={query} history={} changed={}",
                    u8::from(history_mode),
                    u8::from(changed)
                ),
            );
        }
        // The offset itself, not just the fact that it was reset: a screenshot of a
        // clipped top row is only diagnosable if the trace says what the viewport
        // was actually scrolled to, and when.
        if super::paint_trace::enabled() {
            let anchor_id = ctx.id();
            let tree = ctx.tree_mut();
            if let Some((offset, max)) = tree
                .scroll_ancestor(anchor_id)
                .and_then(|scroll_id| tree.scroll_range(scroll_id))
            {
                if offset != self.traced_offset {
                    self.traced_offset = offset;
                    super::paint_trace::note("scroll-offset", &format!("y={offset} max={max}"));
                }
            }
        }
    }
}

/// Publishes the live selection to the paint trace.
///
/// A UI smoke has to know which row is selected before it presses Enter, and the
/// only honest way to know is to be told. A driver that assumes "Down then Up
/// lands on the first row" is wrong the moment an async provider republishes a
/// longer list between the two keys - the selection then sits on a row that
/// launches nothing, and the test has no way to tell. This is the completion
/// signal for that: one trace line per selection change, and nothing at all when
/// the trace is off.
pub(crate) struct SelectionTrace {
    pub(crate) selected_id: Signal<String>,
    pub(crate) selected_index: Signal<usize>,
    pub(crate) last: Option<(String, usize)>,
}

impl Widget for SelectionTrace {
    fn on_update(&mut self, _ctx: &mut EventCtx) {
        if !super::paint_trace::enabled() {
            return;
        }
        let id = self.selected_id.get();
        let index = self.selected_index.get();
        let unchanged = self
            .last
            .as_ref()
            .is_some_and(|(last_id, last_index)| *last_id == id && *last_index == index);
        if unchanged {
            return;
        }
        super::paint_trace::note("selection", &format!("index={index} id={id}"));
        self.last = Some((id, index));
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
            // Same reason as the result row: the reactive pass precedes arrange, so
            // a row built this frame has no geometry to scroll by yet.
            if ctx.bounds().is_empty() {
                return;
            }
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

    /// Only a selection the first page cannot show asks to be revealed. A row on
    /// the first page is where the viewport already is, and revealing it on every
    /// rebuild is what pins the list to the highlight.
    #[test]
    fn only_a_selection_below_the_first_page_reveals_itself() {
        let first_page = RESULT_ROWS_VISIBLE as usize;
        assert!(
            !needs_reveal_on_build(false, Some(first_page + 2)),
            "an unselected row never scrolls"
        );
        assert!(
            !needs_reveal_on_build(true, Some(0)),
            "the first row is where a new list starts"
        );
        assert!(
            !needs_reveal_on_build(true, Some(first_page - 1)),
            "the last row of the first page is already visible"
        );
        assert!(
            needs_reveal_on_build(true, Some(first_page)),
            "a row past the first page has to be brought into view"
        );
        assert!(
            !needs_reveal_on_build(true, None),
            "a row that is not in the list cannot be revealed"
        );
    }

    /// Recalling a query hands the previous list's scroll offset to the new rows:
    /// the top row - the one the user just recalled - was clipped by up to a row
    /// height. A query is a new list, and a new list starts at its first row.
    #[test]
    fn a_new_query_puts_the_list_back_on_its_first_row() {
        use windui::prelude::*;
        use windui::signal::signal;

        let query = signal(String::from("developer"));
        let history_mode = signal(false);
        let element = Element::scroll()
            .width(300)
            .height(100)
            .child(
                Element::leaf()
                    .widget(ScrollTopAnchor {
                        query,
                        history_mode,
                        reset_request: signal(0_u64),
                        last_query: query.get(),
                        last_history_mode: false,
                        last_reset_request: 0,
                        traced_offset: 0,
                    })
                    .reactive()
                    .height(0),
            )
            .child(Element::col().height(400));
        let mut tree = windui::core::Tree::new();
        let root = element.build(&mut tree);
        tree.root = Some(root);
        let mut engine = windui::text::NullTextEngine;
        tree.layout_root(Size::new(300, 100), &mut engine);
        tree.layout_root(Size::new(300, 100), &mut engine);
        let offset = |tree: &windui::core::Tree| tree.scroll_range(root).map(|(y, _)| y);

        // The list of the query the user is leaving was scrolled down.
        assert!(tree.set_scroll_y(root, 120));
        tree.layout_root(Size::new(300, 100), &mut engine);
        assert_eq!(offset(&tree), Some(120));

        // A repaint for the same query must not move the viewport: wheel
        // scrolling would otherwise be undone by every provider commit.
        tree.layout_root(Size::new(300, 100), &mut engine);
        assert_eq!(offset(&tree), Some(120), "a repaint must not scroll");

        query.set(String::from("perp"));
        tree.layout_root(Size::new(300, 100), &mut engine);
        assert_eq!(offset(&tree), Some(0), "a new list starts at its first row");
    }

    /// A recall that names the query that is already in the field changes nothing
    /// about the query, so nothing but the recall itself can put the list back on
    /// its first row. Without that the old offset survives and the top row comes
    /// back clipped.
    #[test]
    fn a_recall_resets_the_viewport_even_when_the_query_does_not_change() {
        use windui::prelude::*;
        use windui::signal::signal;

        let query = signal(String::from("perp"));
        let history_mode = signal(false);
        let reset_request = signal(0_u64);
        let element = Element::scroll()
            .width(300)
            .height(100)
            .child(
                Element::leaf()
                    .widget(ScrollTopAnchor {
                        query,
                        history_mode,
                        reset_request,
                        last_query: query.get(),
                        last_history_mode: false,
                        last_reset_request: 0,
                        traced_offset: 0,
                    })
                    .reactive()
                    .height(0),
            )
            .child(Element::col().height(400));
        let mut tree = windui::core::Tree::new();
        let root = element.build(&mut tree);
        tree.root = Some(root);
        let mut engine = windui::text::NullTextEngine;
        tree.layout_root(Size::new(300, 100), &mut engine);
        tree.layout_root(Size::new(300, 100), &mut engine);
        let offset = |tree: &windui::core::Tree| tree.scroll_range(root).map(|(y, _)| y);

        assert!(tree.set_scroll_y(root, 120));
        tree.layout_root(Size::new(300, 100), &mut engine);
        assert_eq!(offset(&tree), Some(120));

        // The recall asks for the reset, and the query text stays the same.
        reset_request.set(1);
        tree.layout_root(Size::new(300, 100), &mut engine);
        assert_eq!(
            offset(&tree),
            Some(0),
            "a recall puts the list back on its first row"
        );
    }

    /// A row that asks to be scrolled into view is handed to the reactive pass, which
    /// runs before measure/arrange. Without the guard the row's bounds are still empty
    /// and the scroll is clamped against a zero rect, so the viewport lands somewhere
    /// the user never asked for. With the guard the request survives the frame and is
    /// honoured once real geometry exists.
    #[test]
    fn a_row_only_scrolls_once_it_has_geometry() {
        use std::cell::Cell;
        use windui::prelude::*;

        /// Scrolls itself into view the first time it is updated, exactly like the
        /// result row does when the selection moves.
        struct ScrollOnUpdate {
            done: Rc<Cell<bool>>,
            scrolled: Rc<Cell<bool>>,
        }

        impl Widget for ScrollOnUpdate {
            fn measure(
                &self,
                _avail: Size,
                _style: &Style,
                _text: &mut dyn windui::text::TextEngine,
            ) -> Size {
                Size::new(200, 40)
            }

            fn on_update(&mut self, ctx: &mut EventCtx) {
                if self.done.get() {
                    return;
                }
                if ctx.bounds().is_empty() {
                    // No geometry yet: the request has to stay pending.
                    return;
                }
                self.scrolled.set(true);
                self.done.set(true);
            }
        }

        let done = Rc::new(Cell::new(false));
        let scrolled = Rc::new(Cell::new(false));
        let element = Element::leaf()
            .widget(ScrollOnUpdate {
                done: Rc::clone(&done),
                scrolled: Rc::clone(&scrolled),
            })
            .reactive()
            .width(200)
            .height(40);
        let mut tree = windui::core::Tree::new();
        let root = element.build(&mut tree);
        tree.root = Some(root);
        let mut engine = windui::text::NullTextEngine;

        // The first layout is the one that dispatches reactive updates before arrange,
        // so it must leave the request pending and the row unscrolled.
        tree.layout_root(Size::new(400, 300), &mut engine);
        assert!(
            !scrolled.get(),
            "a row with no geometry must not be treated as scrolled"
        );
        assert!(
            !done.get(),
            "the pending request must survive the frame that had no geometry"
        );

        // The next layout has the previous frame's bounds, so the scroll may run.
        tree.layout_root(Size::new(400, 300), &mut engine);
        assert!(
            scrolled.get(),
            "once the row has geometry the pending scroll must happen"
        );
    }
}
