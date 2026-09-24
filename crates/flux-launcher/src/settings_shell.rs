use std::sync::Arc;

use windui::prelude::*;

use super::hotkeys;
use super::settings_ui::SettingsUi;
use super::ui_constants::COMPACT_WINDOW_HEIGHT;
use super::window_geometry::request_monitor_position;

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
    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window.
    Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(0, 0, 0, 0))
        .border(Color::rgba(0, 0, 0, 0), 0)
        .child(
            Element::row()
                .width_match()
                .spacing(12)
                .cross(Align::Center)
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(3)
                        .child(Element::label("Settings").font_size(25.0).fg(Color::WHITE))
                        .child(
                            Element::label("Review changes, then press Apply to save")
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 235)),
                        ),
                )
                .child(Element::segmented(
                    vec!["General", "Visual", "Priorities", "Plugins", "Stats"],
                    settings_tab,
                ))
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
                                launcher_height.get() as i32
                            } else {
                                COMPACT_WINDOW_HEIGHT
                            };
                            if let Ok(settings) = settings_for_back_position.read() {
                                request_monitor_position(
                                    &position_for_back,
                                    settings.monitor_preference,
                                    launcher_width.get() as i32,
                                    height,
                                );
                            }
                            size_for_back.set(launcher_width.get() as i32, height);
                        }),
                ),
        )
        .child(tab_general)
        .child(tab_plugins)
        .child(tab_visual)
        .child(tab_priorities)
        .child(tab_stats)
}
