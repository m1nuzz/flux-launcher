use windui::prelude::*;

use super::settings_actions::{check_for_updates_now, install_update};
use super::settings_shell::{settings_scroll_gutter, settings_section_header};
use super::settings_ui::SettingsUi;
use super::ui_constants::CURRENT_VERSION;

pub(crate) fn build_general_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let activation_recording = ui.activation_recording;
    let activation_handle = ui.activation_handle.clone();
    let clear_history_confirm = ui.clear_history_confirm;
    let update_available = ui.update_available;
    let update_installing = ui.update_installing;
    let ui_for_check = ui.clone();
    let ui_for_install = ui.clone();

    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 0)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(12)
                        .child(settings_section_header("Activation"))
                        .child(Element::setting_row_desc(
                            "Activation key",
                            "Press the key combination that opens the launcher",
                            Element::row()
                                .spacing(8)
                                .cross(Align::Center)
                                .child(
                                    Element::label_signal(ui.activation_display)
                                        .padding_xy(10, 6)
                                        .bg(Color::rgba(255, 255, 255, 24))
                                        .corner(8.0),
                                )
                                .child(Element::button("Record key").small().neutral().on_click(
                                    move |ctx| {
                                        activation_recording.set(true);
                                        activation_handle.set_enabled(false);
                                        ctx.toast_ok("Press the desired activation key");
                                    },
                                )),
                        ))
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .cross(Align::Center)
                                .child(
                                    Element::label("Modifiers")
                                        .font_size(11.0)
                                        .fg(Color::rgba(235, 241, 255, 235)),
                                )
                                .child(Element::checkbox("Ctrl", ui.activation_ctrl))
                                .child(Element::checkbox("Alt", ui.activation_alt))
                                .child(Element::checkbox("Shift", ui.activation_shift))
                                .child(Element::checkbox("Windows", ui.activation_meta)),
                        )
                        .child(
                            Element::label(
                                "Click Record key, then press one key or a key combination",
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 235))
                            .visible_when(move || activation_recording.get()),
                        )
                        .child(Element::setting_row_desc(
                            "Fullscreen protection",
                            "Ignore activation while another app is fullscreen",
                            Element::switch(ui.ignore_fullscreen),
                        ))
                        .child(Element::divider())
                        .child(settings_section_header("Behavior"))
                        .child(Element::setting_row_desc(
                            "Game Mode",
                            "Suppress the launcher until manually disabled",
                            Element::switch(ui.game_mode),
                        ))
                        .child(Element::setting_row_desc(
                            "Keyboard layout",
                            "Start typing in English and restore the previous layout on hide",
                            Element::switch(ui.switch_to_english_layout),
                        ))
                        .child(Element::setting_row_desc(
                            "Query on activation",
                            "Clear the previous query when opened with the global hotkey",
                            Element::switch(ui.clear_query_on_activation),
                        ))
                        .child(Element::setting_row_desc(
                            "Windows startup",
                            "Start Flux automatically with Windows",
                            Element::switch(ui.start_with_windows),
                        ))
                        .child(Element::setting_row_desc(
                            "Open launcher on",
                            "The launcher window appears on the chosen display",
                            Element::col()
                                .spacing(6)
                                .child(Element::radio("Primary display", ui.monitor_preference, 0))
                                .child(Element::radio(
                                    "Display with the mouse cursor",
                                    ui.monitor_preference,
                                    1,
                                ))
                                .child(Element::radio(
                                    "Display with the focused window",
                                    ui.monitor_preference,
                                    2,
                                )),
                        ))
                        .child(Element::divider())
                        .child(settings_section_header("Updates"))
                        .child(Element::setting_row_desc(
                            "Check for updates",
                            "Check stable GitHub releases automatically",
                            Element::switch(ui.update_checks_enabled),
                        ))
                        .child(Element::setting_row_desc(
                            "Check interval",
                            "How often Flux looks for a stable release",
                            Element::row()
                                .spacing(8)
                                .cross(Align::Center)
                                .child(
                                    Element::text_input(ui.update_interval_hours, "24").width(76),
                                )
                                .child(Element::label("hours").font_size(11.0)),
                        ))
                        .child(Element::setting_row_desc(
                            "Automatic install",
                            "Install stable updates automatically",
                            Element::switch(ui.auto_install_updates),
                        ))
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .cross(Align::Center)
                                .child(
                                    Element::col()
                                        .weight(1.0)
                                        .spacing(4)
                                        .child(
                                            Element::label_signal(ui.update_status)
                                                .font_size(11.0)
                                                .fg(Color::rgba(235, 241, 255, 235))
                                                .max_lines(2)
                                                .truncate(Truncate::End)
                                                .width_match(),
                                        )
                                        .child(Element::badge_intent(
                                            format!("Current version: {CURRENT_VERSION}"),
                                            Intent::Neutral,
                                        )),
                                )
                                .child(
                                    Element::button("Check now")
                                        .small()
                                        .neutral()
                                        .outline_soft()
                                        .on_click(move |ctx| {
                                            check_for_updates_now(&ui_for_check, ctx)
                                        }),
                                )
                                .child(
                                    Element::button("Install now")
                                        .small()
                                        .visible_when(move || {
                                            update_available.get().is_some()
                                                && !update_installing.get()
                                        })
                                        .on_click(move |ctx| install_update(&ui_for_install, ctx)),
                                ),
                        )
                        .child(Element::divider())
                        .child(settings_section_header("History"))
                        .child(Element::setting_row_desc(
                            "Query history",
                            "Ctrl+H recalls committed searches",
                            Element::button("Clear history")
                                .small()
                                .neutral()
                                .outline_soft()
                                .on_click(move |_| {
                                    clear_history_confirm.set(true);
                                }),
                        )),
                )
                .child(settings_scroll_gutter()),
        )
}
