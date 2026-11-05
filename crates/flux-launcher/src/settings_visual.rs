use std::sync::Arc;

use flux_core::{
    DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::prelude::*;

use super::color_picker;
use super::settings_shell::{settings_scroll_gutter, settings_section_header};
use super::settings_ui::SettingsUi;
use super::theme_text::selection_palette;
use super::ui_constants::VISUAL_SLIDER_WIDTH;
use super::window_geometry::dimension_slider_fraction;

pub(crate) fn build_visual_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let smooth_caret = ui.smooth_caret;
    let custom_color_visible = ui.use_system_accent;

    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 1)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(12)
                        .child(settings_section_header("Caret"))
                        .child(Element::setting_row_desc(
                            "Smooth caret",
                            "Animate search caret movement",
                            Element::row()
                                .spacing(8)
                                .cross(Align::Center)
                                .child(Element::switch(ui.smooth_caret))
                                .child(
                                    Element::text_input(ui.caret_duration, "95")
                                        .width(64)
                                        .enabled_when(move || smooth_caret.get()),
                                )
                                .child(Element::label("ms").font_size(11.0)),
                        ))
                        .child(Element::divider())
                        .child(settings_section_header("Selection color"))
                        .child(Element::setting_row_desc(
                            "Use Windows accent",
                            "Follow the Windows 11 system accent color when available",
                            Element::switch(ui.use_system_accent),
                        ))
                        .child(
                            Element::label("The custom color below is both the safe fallback and the manual choice.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 235))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::col()
                                .width_match()
                                .spacing(8)
                                .visible_when(move || !custom_color_visible.get())
                                .child(
                                    Element::text_input(ui.custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(
                                    ui.custom_selection_color,
                                    ui.color_hsv,
                                    ui.selection_color,
                                    &ui.theme_handle,
                                ))
                                .child(color_picker::build_color_picker(
                                    ui.custom_selection_color,
                                    ui.color_hsv,
                                    ui.selection_color,
                                    &ui.theme_handle,
                                    Arc::clone(&ui.shared_settings),
                                )),
                        )
                        .child(Element::divider())
                        .child(settings_section_header("Launcher size"))
                        .child(Element::setting_row_desc(
                            "Launcher width",
                            format!("Safe range: {MIN_LAUNCHER_WIDTH}-{MAX_LAUNCHER_WIDTH} logical px (DIP)"),
                            dimension_editor(
                                ui.launcher_width_slider,
                                ui.launcher_width_input,
                                ui.launcher_width,
                                ui.launcher_height,
                                ui.visual_preview_generation,
                                ui.launcher_preview_text,
                                true,
                            ),
                        ))
                        .child(Element::setting_row_desc(
                            "Results height",
                            format!("Safe range: {MIN_LAUNCHER_HEIGHT}-{MAX_LAUNCHER_HEIGHT} logical px (DIP)"),
                            dimension_editor(
                                ui.launcher_height_slider,
                                ui.launcher_height_input,
                                ui.launcher_width,
                                ui.launcher_height,
                                ui.visual_preview_generation,
                                ui.launcher_preview_text,
                                false,
                            ),
                        ))
                        .child(Element::divider())
                        .child(settings_section_header("Live preview"))
                        .child(Element::label_signal(ui.launcher_preview_text).font_size(12.0))
                        .child(
                            Element::label("The native preview uses the exact requested logical client dimensions. Physical pixels scale with the preview monitor DPI; Apply settings saves the values.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 235))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        ),
                )
                .child(settings_scroll_gutter()),
        )
}

/// One size row: slider, numeric field, per-row reset and the DIP unit, in the
/// same order for both dimensions so the pair reads as one control.
fn dimension_editor(
    slider: Signal<f32>,
    input: Signal<String>,
    width: Signal<u16>,
    height: Signal<u16>,
    preview_generation: Signal<u64>,
    preview_text: Signal<String>,
    is_width: bool,
) -> Element {
    let (default, min, max, placeholder) = if is_width {
        (
            DEFAULT_LAUNCHER_WIDTH,
            MIN_LAUNCHER_WIDTH,
            MAX_LAUNCHER_WIDTH,
            "420",
        )
    } else {
        (
            DEFAULT_LAUNCHER_HEIGHT,
            MIN_LAUNCHER_HEIGHT,
            MAX_LAUNCHER_HEIGHT,
            "382",
        )
    };
    let label = if is_width { "width" } else { "height" };

    Element::row()
        .spacing(8)
        .cross(Align::Center)
        .child(Element::slider(slider).width(VISUAL_SLIDER_WIDTH))
        .child(Element::text_input(input, placeholder).width(64))
        .child(
            Element::button("Reset")
                .small()
                .neutral()
                .outline_soft()
                .on_click(move |_| {
                    let (next_width, next_height) = if is_width {
                        (default, height.get())
                    } else {
                        (width.get(), default)
                    };
                    eprintln!("Visual {label} reset clicked: {next_width}x{next_height}");
                    width.set(next_width);
                    height.set(next_height);
                    input.set(if is_width {
                        next_width.to_string()
                    } else {
                        next_height.to_string()
                    });
                    slider.set(dimension_slider_fraction(
                        if is_width { next_width } else { next_height },
                        min,
                        max,
                    ));
                    preview_text.set(format!(
                        "Current launcher client area: {next_width} × {next_height} logical px (DIP)"
                    ));
                    preview_generation.set(preview_generation.get().saturating_add(1));
                }),
        )
        .child(Element::label("DIP").font_size(11.0))
}
