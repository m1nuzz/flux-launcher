use std::sync::Arc;

use windui::prelude::*;

use super::hotkeys;
use super::settings_actions::{apply_settings, reset_active_tab, tab_is_resettable};
use super::settings_ui::SettingsUi;
use super::ui_constants::{COMPACT_WINDOW_HEIGHT, SETTINGS_SIDEBAR_WIDTH};
use super::window_geometry::request_monitor_position;

/// Sidebar order is the tab-index contract: the smoke runs select a tab by its
/// numeric index through `FLUX_SMOKE_SETTINGS_TAB`, and every tab body gates on
/// the same index, so renaming alone is safe but reordering is not.
const TAB_NAMES: [&str; 5] = ["General", "Visual", "Priorities", "Plugins", "Stats"];
/// Segoe Fluent Icons codepoints for the navigation rows, taken from the
/// documented glyph table: Settings, Color, Sort, Puzzle, AreaChart. The font
/// ships with Windows 11 and is already used for the Recycle Bin rows.
const TAB_GLYPHS: [&str; 5] = ["\u{e713}", "\u{e790}", "\u{e8cb}", "\u{ea86}", "\u{e9d2}"];
const ICON_FONT: &str = "Segoe Fluent Icons";
const TAB_DESCRIPTIONS: [&str; 5] = [
    "Startup, activation and update behavior.",
    "Caret, selection color and launcher size.",
    "Applications that always rank first.",
    "File, note and web search providers.",
    "What you searched and opened.",
];

/// A settings group title: semibold, slightly larger than row text, no fill so
/// the Acrylic backdrop stays untouched.
pub(crate) fn settings_section_header(title: &str) -> Element {
    Element::label(title)
        .font_size(15.0)
        .font_weight(600)
        .fg_role(Role::Text)
}

/// One settings group: a title above the panel that holds its rows.
///
/// On the solid fallback surface the panel is a raised card with a hairline
/// edge, which is what carries the hierarchy in the reference. With live
/// Acrylic the panel paints nothing: the system material has to stay the
/// background of the whole window, so only the spacing is shared.
pub(crate) fn settings_group(title: &str, rows: Vec<Element>) -> Element {
    let mut body = Element::col()
        .width_match()
        .padding_edges(16, 8, 16, 8)
        .spacing(4);
    for row in rows {
        body = body.child(row);
    }
    if super::theme_text::needs_opaque_surface() {
        body = body
            .bg(super::theme_text::FALLBACK_PANEL)
            .corner(12.0)
            .border(super::theme_text::FALLBACK_EDGE, 1);
    }
    Element::col()
        .width_match()
        .spacing(8)
        .child(settings_section_header(title))
        .child(body)
}

/// Right gutter for settings tab scroll content.
///
/// windui's overlay scrollbar reserves only TRACK_W + MARGIN + CONTENT_GAP
/// for layout while its grab zone (HIT_W) is wider, so controls glued to the
/// content edge feel overlapped by the thumb. Wrap the tab body as
/// `scroll > row[width_match] > col[weight(1.0)] + gutter`: the fixed spacer
/// keeps right-edge controls (checkboxes, buttons) clear of the thumb and
/// its hit zone, gives breathing room against the panel edge when the tab
/// does not overflow, and leaves the left edge aligned with the header.
pub(crate) fn settings_scroll_gutter() -> Element {
    Element::leaf().size(12, 1)
}

pub(crate) fn build_settings_panel(
    ui: &SettingsUi,
    tab_general: Element,
    tab_visual: Element,
    tab_priorities: Element,
    tab_plugins: Element,
    tab_stats: Element,
) -> Element {
    let settings_tab = ui.settings_tab;
    let settings_visible = ui.settings_visible;
    let show_results_for_back = ui.show_results;
    let position_for_back = ui.window_position.clone();
    let settings_for_back_position = Arc::clone(&ui.shared_settings);
    let size_for_back = ui.window_size.clone();
    let launcher_width = ui.launcher_width;
    let launcher_height = ui.launcher_height;
    // Closing Settings must not leave the global key recorder armed: while the
    // flag is set the launcher swallows every keystroke and the activation
    // hotkey stays unregistered.
    let activation_recording_for_back = ui.activation_recording;
    let activation_display_for_back = ui.activation_display;
    let activation_key_for_back = ui.activation_key;
    let activation_ctrl_for_back = ui.activation_ctrl;
    let activation_alt_for_back = ui.activation_alt;
    let activation_shift_for_back = ui.activation_shift;
    let activation_meta_for_back = ui.activation_meta;
    let activation_handle_for_back = ui.activation_handle.clone();
    let ui_for_reset = ui.clone();
    let ui_for_apply = ui.clone();

    // Only the active tab's title and summary are shown; the hidden pairs take
    // no space, so the header keeps a constant height instead of jumping when
    // descriptions differ in length.
    let mut heading = Element::col().weight(1.0).spacing(4);
    for index in 0..TAB_NAMES.len() {
        heading = heading
            .child(
                Element::label(TAB_NAMES[index])
                    .font_size(24.0)
                    .font_weight(600)
                    .fg(Color::WHITE)
                    .visible_when(move || settings_tab.get() == index),
            )
            .child(
                Element::label(TAB_DESCRIPTIONS[index])
                    .font_size(12.0)
                    .fg(Color::rgba(235, 241, 255, 235))
                    .visible_when(move || settings_tab.get() == index),
            );
    }

    let header = Element::row()
        .width_match()
        .spacing(12)
        .cross(Align::End)
        .child(heading)
        .child(
            Element::button("Back")
                .neutral()
                .outline_soft()
                .on_click(move |_| {
                    hotkeys::end_recording(
                        activation_recording_for_back,
                        activation_display_for_back,
                        activation_key_for_back,
                        activation_ctrl_for_back,
                        activation_alt_for_back,
                        activation_shift_for_back,
                        activation_meta_for_back,
                        &activation_handle_for_back,
                    );
                    settings_visible.set(false);
                    let height = if show_results_for_back.get() {
                        i32::from(launcher_height.get())
                    } else {
                        COMPACT_WINDOW_HEIGHT
                    };
                    if let Ok(settings) = settings_for_back_position.read() {
                        request_monitor_position(
                            &position_for_back,
                            settings.monitor_preference,
                            i32::from(launcher_width.get()),
                            height,
                        );
                    }
                    size_for_back.set(i32::from(launcher_width.get()), height);
                }),
        );

    let footer = Element::row()
        .width_match()
        .spacing(10)
        .cross(Align::Center)
        .child(
            Element::label("Changes are written when you press Apply settings.")
                .font_size(11.0)
                .fg(Color::rgba(235, 241, 255, 210))
                .weight(1.0),
        )
        .child(
            Element::button("\u{2713} Apply settings")
                .on_click(move |ctx| apply_settings(&ui_for_apply, ctx)),
        );

    let reset = Element::button("Reset tab defaults")
        .small()
        .neutral()
        .outline_soft()
        .width_match()
        .tooltip("Restore this tab's defaults. Nothing is saved until you press Apply settings.")
        .visible_when(move || tab_is_resettable(settings_tab.get()))
        .on_click(move |ctx| reset_active_tab(&ui_for_reset, ctx));

    let sidebar = Element::col()
        .width(SETTINGS_SIDEBAR_WIDTH)
        .height_match()
        .padding_edges(14, 12, 12, 14)
        .spacing(14)
        .child(
            Element::list_pill_glyphs(
                TAB_NAMES
                    .iter()
                    .zip(TAB_GLYPHS)
                    .map(|(name, glyph)| ((*name).to_string(), glyph.to_string()))
                    .collect(),
                settings_tab,
                ICON_FONT,
            )
            .weight(1.0),
        )
        .child(reset);

    // The reference frame puts the brand and the window controls in a strip
    // above both columns, so the frameless window stays movable and closable
    // without a system titlebar. Maximize is deliberately absent: the Settings
    // canvas has one fixed size, so a maximize control would be a dead button.
    let titlebar = Element::row()
        .width_match()
        .height(40)
        .padding_edges(16, 6, 6, 0)
        .spacing(8)
        .cross(Align::Center)
        .child(
            Element::label("\u{e945}")
                .font_size(15.0)
                .font_family(ICON_FONT)
                .fg_role(Role::Accent),
        )
        .child(
            Element::label("Flux Launcher")
                .font_size(13.0)
                .font_weight(650)
                .fg(Color::WHITE),
        )
        .child(Element::flex_spacer().window_drag())
        .child(Element::window_button(WindowButtonKind::Minimize))
        .child(Element::window_button(WindowButtonKind::Close));

    let content = Element::col()
        .weight(1.0)
        .height_match()
        .padding_edges(18, 16, 16, 8)
        .spacing(10)
        .child(header)
        .child(tab_general)
        .child(tab_visual)
        .child(tab_priorities)
        .child(tab_plugins)
        .child(tab_stats)
        .child(footer);

    // Settings shares the same continuous Acrylic surface as the launcher: no
    // dark card here, or the blur disappears behind the old opaque slab. The
    // solid fallback has no material to edge it, so it draws its own hairline
    // border and a divider between the navigation column and the rows.
    let fallback = super::theme_text::needs_opaque_surface();
    let mut body_children = vec![sidebar];
    if fallback {
        body_children.push(
            Element::leaf()
                .width(1)
                .height_match()
                .bg(super::theme_text::FALLBACK_EDGE),
        );
    }
    body_children.push(content);

    let body = Element::row()
        .width_match()
        .weight(1.0)
        .spacing(0)
        .children(body_children);

    let mut root = Element::col()
        .fill()
        .spacing(0)
        .bg(super::theme_text::window_background())
        .child(titlebar)
        .child(body);
    if fallback {
        root = root
            .corner(10.0)
            .border(super::theme_text::FALLBACK_EDGE, 1);
    }
    root
}
