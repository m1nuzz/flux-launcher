use std::rc::Rc;
use std::sync::Arc;

use flux_core::{
    DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::prelude::*;

use super::color_picker;
use super::settings_shell::{settings_group, settings_scroll_gutter};
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
                        .spacing(16)
                        .child(settings_group(
                            crate::i18n::tr("Caret"),
                            vec![Element::setting_row_desc(
                                crate::i18n::tr("Smooth caret"),
                                crate::i18n::tr("Animate search caret movement"),
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
                            )],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Selection color"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Use Windows accent"),
                                    crate::i18n::tr("Follow the Windows 11 system accent color when available"),
                                    Element::switch(ui.use_system_accent),
                                ),
                                Element::label(crate::i18n::tr("The custom color below is both the safe fallback and the manual choice."))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
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
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Launcher size"),
                            vec![
                                dimension_row(
                                    crate::i18n::tr("Launcher width"),
                                    format!("{MIN_LAUNCHER_WIDTH}-{MAX_LAUNCHER_WIDTH} logical px (DIP)"),
                                    dimension_editor(
                                        ui.launcher_width_slider,
                                        ui.launcher_width_input,
                                        Rc::clone(&ui.width_ease),
                                        true,
                                    ),
                                ),
                                dimension_row(
                                    crate::i18n::tr("Results height"),
                                    format!("{MIN_LAUNCHER_HEIGHT}-{MAX_LAUNCHER_HEIGHT} logical px (DIP)"),
                                    dimension_editor(
                                        ui.launcher_height_slider,
                                        ui.launcher_height_input,
                                        Rc::clone(&ui.height_ease),
                                        false,
                                    ),
                                ),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Live preview"),
                            vec![
                                Element::label_signal(ui.launcher_preview_text).font_size(12.0),
                                Element::label(crate::i18n::tr("The native preview uses the exact requested logical client dimensions. Physical pixels scale with the preview monitor DPI; Apply settings saves the values."))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
                            ],
                        )),
                )
                .child(settings_scroll_gutter()),
        )
}

/// One size row with its label above the controls, so the wide track fits
/// without squeezing the title into a narrow column.
fn dimension_row(label: &str, description: String, editor: Element) -> Element {
    Element::col()
        .width_match()
        .spacing(6)
        .child(
            Element::label(label.to_string())
                .font_size(14.0)
                .font_weight(600),
        )
        .child(
            Element::label(description)
                .font_size(12.0)
                .fg_role(Role::TextSubtle)
                .width_match(),
        )
        .child(editor)
}

/// One size row: slider, numeric field, per-row reset and the DIP unit, in the
/// same order for both dimensions so the pair reads as one control.
fn dimension_editor(
    slider: Signal<f32>,
    input: Signal<String>,
    ease: Rc<std::cell::Cell<Option<(f32, f32, std::time::Instant)>>>,
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
            Element::button(crate::i18n::tr("Reset"))
                .small()
                .on_click(move |_| {
                    // Ease the slider to the default instead of snapping: the thumb and
                    // the Settings/preview pair travel on one curve.
                    eprintln!("Visual {label} reset clicked: {default}");
                    ease.set(Some((
                        slider.get(),
                        dimension_slider_fraction(default, min, max),
                        std::time::Instant::now(),
                    )));
                }),
        )
        .child(Element::label("DIP").font_size(11.0))
}
