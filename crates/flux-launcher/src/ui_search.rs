use windui::prelude::*;
use windui::signal::Signal;

use super::ui_constants::LAUNCHER_FONT_FAMILY;

pub(crate) fn build_search_box(
    query: Signal<String>,
    query_caret_position: Signal<usize>,
    inline_completion: Signal<String>,
    smooth_caret: bool,
    smooth_caret_duration_ms: u16,
) -> Element {
    Element::text_input(query, "Search")
        .cursor_position(query_caret_position)
        .leading_icon('⌕')
        .transparent_surface()
        .smooth_caret(smooth_caret, smooth_caret_duration_ms)
        .inline_completion(inline_completion)
        .show_focus_ring(false)
        .width_match()
        .font_family(LAUNCHER_FONT_FAMILY)
        .font_size(15.0)
        .font_weight(500)
        .corner(10.0)
        // The entire Search control stays transparent so the Windows Acrylic
        // material remains visible through the input, caret, and leading icon.
        .border(Color::rgba(0, 0, 0, 0), 0)
        .padding_xy(13, 0)
}
