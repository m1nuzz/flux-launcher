use windui::core::{EventCtx, Widget};
use windui::event::{Event, PointerKind};
use windui::prelude::*;
use windui::render::Canvas;
use windui::signal::Signal;

use super::ui_constants::LAUNCHER_FONT_FAMILY;

/// A small clear affordance at the right edge of the search strip. It is not
/// focusable, so clicking it clears the query without stealing the caret from
/// the input.
struct SearchClear {
    query: Signal<String>,
}

impl Widget for SearchClear {
    fn paint(
        &self,
        bounds: windui::geometry::Rect,
        _content: windui::geometry::Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &windui::style::Style,
    ) {
        if self.query.get().is_empty() {
            return;
        }
        canvas.draw_text(
            "\u{e8bb}",
            bounds,
            Color::rgba(235, 243, 255, 185),
            Align::Center,
            &windui::text::TextStyle::of(style),
        );
    }

    fn on_event(&mut self, ctx: &mut EventCtx, ev: &Event) -> bool {
        if let Event::Pointer(pointer) = ev {
            if pointer.kind == PointerKind::Up && ctx.bounds().contains(pointer.pos) {
                self.query.set(String::new());
                ctx.mark_dirty();
                ctx.request_interval_tick();
                return true;
            }
        }
        false
    }

    fn cursor(&self) -> windui::event::CursorShape {
        windui::event::CursorShape::Hand
    }
}

pub(crate) fn build_search_box(
    query: Signal<String>,
    query_caret_position: Signal<usize>,
    inline_completion: Signal<String>,
    smooth_caret: bool,
    smooth_caret_duration_ms: u16,
) -> Element {
    let input = Element::text_input(query, "Search")
        .cursor_position(query_caret_position)
        .leading_icon('⌕')
        .transparent_surface()
        .smooth_caret(smooth_caret, smooth_caret_duration_ms)
        .inline_completion(inline_completion)
        .show_focus_ring(false)
        .weight(1.0)
        .font_family(LAUNCHER_FONT_FAMILY)
        .font_size(15.0)
        .font_weight(500)
        .corner(10.0)
        // The entire Search control stays transparent so the Windows Acrylic
        // material remains visible through the input, caret, and leading icon.
        .border(Color::rgba(0, 0, 0, 0), 0)
        .padding_xy(13, 0);
    Element::row()
        .width_match()
        .cross(Align::Center)
        .child(input)
        .child(
            Element::leaf()
                .widget(SearchClear { query })
                .reactive()
                .width(24)
                .height(24)
                .font_family("Segoe Fluent Icons")
                .font_size(11.0),
        )
}
