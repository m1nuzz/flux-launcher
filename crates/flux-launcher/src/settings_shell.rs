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
const TAB_DESCRIPTIONS: [&str; 5] = [
    "Startup, activation and update behavior.",
    "Caret, selection color and launcher size.",
    "Applications that always rank first.",
    "File, note and web search providers.",
    "What you searched and opened.",
];

/// A settings group title: semibold, slightly larger than row text, no fill so
/// the Acrylic backdrop stays untouched. Pair with `Element::divider()` between
/// groups; keep controls off the header line so the eye parses sections first.
pub(crate) fn settings_section_header(title: &str) -> Element {
    Element::label(title)
        .font_size(15.0)
        .font_weight(600)
        .fg(Color::WHITE)
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
                    .font_size(21.0)
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
            Element::button("Apply settings")
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
        .padding_edges(18, 18, 12, 14)
        .spacing(14)
        .child(
            Element::label("Flux Launcher")
                .font_size(13.0)
                .font_weight(650)
                .fg(Color::WHITE),
        )
        .child(Element::divider())
        .child(Element::list_pill(TAB_NAMES.to_vec(), settings_tab).weight(1.0))
        .child(reset);

    let mut content = Element::col()
        .weight(1.0)
        .height_match()
        .padding_edges(18, 16, 14, 6)
        .spacing(10)
        .child(header)
        .child(tab_general)
        .child(tab_visual)
        .child(tab_priorities)
        .child(tab_plugins)
        .child(tab_stats)
        .child(footer);

    if super::theme_text::needs_opaque_surface() {
        // With no system material behind the window, the tab body needs its own
        // plane or the navigation column and the settings read as one flat field.
        content = content
            .bg(super::theme_text::FALLBACK_PANEL)
            .corner(12.0)
            .border(super::theme_text::FALLBACK_EDGE, 1);
    }

    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window. The sidebar is separated
    // from the tab body by its own padding and the divider, not by a fill.
    Element::row()
        .fill()
        .spacing(0)
        .bg(Color::rgba(0, 0, 0, 0))
        .child(sidebar)
        .child(content)
}
