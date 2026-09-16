use std::sync::Arc;

use flux_core::{
    DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::prelude::*;

use super::color_picker;
use super::settings_ui::SettingsUi;
use super::theme_text::{
    effective_selection_rgb, push_selection_appearance, selection_palette, stage_selection_color,
};
use super::ui_constants::{COMPACT_WINDOW_HEIGHT, VISUAL_SLIDER_WIDTH};
use super::update_tasks::save_settings;
use super::window_geometry::{
    dimension_slider_fraction, parse_dimension_input, request_monitor_position,
};

pub(crate) fn build_visual_tab(ui: &SettingsUi) -> Element {
    let visual_preview_generation_for_width_reset = ui.visual_preview_generation;
    let visual_preview_generation_for_height_reset = ui.visual_preview_generation;
    let settings_for_visual_apply = Arc::clone(&ui.shared_settings);
    let theme_for_visual_apply = ui.theme_handle.clone();
    let size_for_visual_apply = ui.window_size.clone();
    let position_for_visual_apply = ui.window_position.clone();
    let settings_visible_for_visual_apply = ui.settings_visible;
    let show_results_for_visual_apply = ui.show_results;
    let smooth_caret = ui.smooth_caret;
    let caret_duration = ui.caret_duration;
    let use_system_accent = ui.use_system_accent;
    let selection_color = ui.selection_color;
    let color_hsv = ui.color_hsv;
    let custom_selection_color = ui.custom_selection_color;
    let launcher_width = ui.launcher_width;
    let launcher_height = ui.launcher_height;
    let launcher_width_input = ui.launcher_width_input;
    let launcher_height_input = ui.launcher_height_input;
    let launcher_width_slider = ui.launcher_width_slider;
    let launcher_height_slider = ui.launcher_height_slider;
    let launcher_preview_text = ui.launcher_preview_text;
    let settings_tab = ui.settings_tab;
    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 1)
        .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::label("Visual appearance").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("The live preview is a separate native windui window. Its client area is resized directly in realtime; the Settings window stays centered and stable while dragging.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Smooth Caret",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::checkbox("Animate search caret movement", smooth_caret)
                                        .width_match(),
                                )
                                .child(Element::text_input(caret_duration, "95").width(76))
                                .child(Element::label("ms").font_size(11.0)),
                        ))
                        .child(Element::field(
                            "Selection color",
                            Element::checkbox(
                                "Use the Windows 11 system accent color when available",
                                use_system_accent,
                            )
                            .on_toggle({
                                let theme = ui.theme_handle.clone();
                                move |_| {
                                    // on_toggle replaces the default flip: apply it
                                    // manually, then refresh rows and chrome
                                    // immediately while Apply remains the persist point.
                                    let next = !use_system_accent.get();
                                    use_system_accent.set(next);
                                    let text = custom_selection_color.get();
                                    push_selection_appearance(
                                        &theme,
                                        selection_color,
                                        effective_selection_rgb(next, &text),
                                    );
                                }
                            }),
                        ))
                        .child(Element::label("Windows accent is read from the current user profile; the custom color is used as a safe fallback.").font_size(10.0).fg(Color::rgba(235, 241, 255, 150)).max_lines(2).truncate(Truncate::End))
                        .child(Element::label("The exact native preview window opens beside Settings when this Visual tab is active."
).font_size(10.0).fg(Color::rgba(235, 241, 255, 170)).max_lines(2).truncate(Truncate::End))
                        .child(
                            Element::col()
                                .spacing(8)
                                .visible_when(move || !use_system_accent.get())
                                .child(
                                    Element::text_input(custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(
                                    custom_selection_color,
                                    color_hsv,
                                    selection_color,
                                    &ui.theme_handle,
                                ))
                                .child(color_picker::build_color_picker(
                                    custom_selection_color,
                                    color_hsv,
                                    selection_color,
                                    &ui.theme_handle,
                                    Arc::clone(&ui.shared_settings),
                                )),
                        )
                        .child(Element::field(
                            "Launcher width",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_width_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_width_input, "420")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = DEFAULT_LAUNCHER_WIDTH;
                                            let height = launcher_height.get();
                                            eprintln!("Visual width reset clicked: {}x{}", width, height);
                                            launcher_width.set(width);
                                            launcher_width_input.set(width.to_string());
                                            launcher_width_slider.set(dimension_slider_fraction(
                                                width,
                                                MIN_LAUNCHER_WIDTH,
                                                MAX_LAUNCHER_WIDTH,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_width_reset.set(
                                                visual_preview_generation_for_width_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::field(
                            "Results height",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_height_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_height_input, "382")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = launcher_width.get();
                                            let height = DEFAULT_LAUNCHER_HEIGHT;
                                            eprintln!("Visual height reset clicked: {}x{}", width, height);
                                            launcher_height.set(height);
                                            launcher_height_input.set(height.to_string());
                                            launcher_height_slider.set(dimension_slider_fraction(
                                                height,
                                                MIN_LAUNCHER_HEIGHT,
                                                MAX_LAUNCHER_HEIGHT,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_height_reset.set(
                                                visual_preview_generation_for_height_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::label_signal(launcher_preview_text).font_size(12.0).fg(Color::WHITE))
                        .child(
                            Element::label("The native preview uses the exact requested logical client dimensions. Physical GetClientRect pixels scale with the preview monitor DPI; Apply saves the values.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::button("Apply dimensions").on_click(move |ctx| {
                                let mut width = parse_dimension_input(
                                    &launcher_width_input.get(),
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                                let mut height = parse_dimension_input(
                                    &launcher_height_input.get(),
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                                let duration = caret_duration
                                    .get()
                                    .trim()
                                    .parse::<u16>()
                                    .unwrap_or(95)
                                    .clamp(60, 160);
                                stage_selection_color(
                                    use_system_accent,
                                    custom_selection_color,
                                    selection_color,
                                    &theme_for_visual_apply,
                                    &settings_for_visual_apply,
                                    &mut *ctx,
                                );
                                let Ok(mut settings) = settings_for_visual_apply.write() else {
                                    ctx.toast_ok("Could not lock Flux settings");
                                    return;
                                };
                                settings.launcher_width = width;
                                settings.launcher_height = height;
                                settings.smooth_caret = smooth_caret.get();
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                width = settings.launcher_width;
                                height = settings.launcher_height;
                                let preference = settings.monitor_preference;
                                if !save_settings(&settings) {
                                    ctx.toast_ok("Could not save visual dimensions");
                                    return;
                                }
                                launcher_width.set(width);
                                launcher_height.set(height);
                                launcher_width_input.set(width.to_string());
                                launcher_height_input.set(height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(format!(
                                    "Current launcher client area: {} × {} logical px (DIP)",
                                    width, height
                                ));
                                eprintln!("Visual Apply dimensions clicked: {}x{}", width, height);
                                settings_visible_for_visual_apply.set(false);
                                let target_height = if show_results_for_visual_apply.get() {
                                    i32::from(height)
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                };
                                request_monitor_position(
                                    &position_for_visual_apply,
                                    preference,
                                    i32::from(width),
                                    target_height,
                                );
                                size_for_visual_apply.set(i32::from(width), target_height);
                                ctx.toast_ok("Visual dimensions applied");
                            }),
                        ),
                )
}
