use windui::prelude::*;

use super::settings_actions::{check_for_updates_now, install_update};
use super::settings_shell::{settings_group, settings_scroll_gutter};
use super::settings_ui::SettingsUi;
use super::ui_constants::CURRENT_VERSION;

pub(crate) fn build_general_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let activation_recording = ui.activation_recording;
    let activation_handle = ui.activation_handle.clone();
    let game_mode_enabled = ui.game_mode_hotkey_enabled;
    let game_mode_recording = ui.game_mode_recording;
    let game_mode_handle = ui.game_mode_handle.clone();
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
                        .spacing(16)
                        .child(settings_group(
                            crate::i18n::tr("Interface"),
                            vec![Element::setting_row(
                                crate::i18n::tr("Language"),
                                Element::dropdown(
                                    std::iter::once(crate::i18n::tr("System default")).chain(flux_core::Locale::ALL.iter().map(|language| crate::i18n::native_name(*language))).collect(),
                                    ui.language,
                                ).width(200),
                            )],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Activation"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Activation key"),
                                    crate::i18n::tr("Press the key combination that opens the launcher"),
                                    Element::row()
                                        .spacing(8)
                                        .cross(Align::Center)
                                        .child(
                                            Element::label_signal(ui.activation_display)
                                                .padding_xy(10, 6)
                                                .bg(Color::rgba(255, 255, 255, 24))
                                                .corner(8.0),
                                        )
                                        .child(
                                            Element::button(crate::i18n::tr("Record key"))
                                                .small()
                                                .on_click(move |ctx| {
                                                    activation_recording.set(true);
                                                    activation_handle.set_enabled(false);
                                                    ctx.toast_ok(crate::i18n::tr("Press the desired activation key"));
                                                }),
                                        ),
                                ),
                                Element::row()
                                    .width_match()
                                    .spacing(10)
                                    .cross(Align::Center)
                                    .child(Element::flex_spacer())
                                    .child(
                                        Element::label(crate::i18n::tr("Modifiers:"))
                                            .font_size(11.0)
                                            .fg_role(Role::TextSubtle),
                                    )
                                    .child(Element::checkbox("Ctrl", ui.activation_ctrl))
                                    .child(Element::checkbox("Alt", ui.activation_alt))
                                    .child(Element::checkbox("Shift", ui.activation_shift))
                                    .child(Element::checkbox("Windows", ui.activation_meta)),
                                Element::label(crate::i18n::tr("Click Record key, then press one key or a key combination"))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .visible_when(move || activation_recording.get()),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Fullscreen protection"),
                                    crate::i18n::tr("Ignore activation while another app is fullscreen"),
                                    Element::switch(ui.ignore_fullscreen),
                                ),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Behavior"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Game Mode"),
                                    crate::i18n::tr("Suppress the launcher until manually disabled"),
                                    Element::switch(ui.game_mode),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Keyboard layout"),
                                    crate::i18n::tr("Start typing in English and restore the previous layout on hide"),
                                    Element::switch(ui.switch_to_english_layout),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Query on activation"),
                                    crate::i18n::tr("Clear the previous query when opened with the global hotkey"),
                                    Element::switch(ui.clear_query_on_activation),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Windows startup"),
                                    crate::i18n::tr("Start Flux automatically with Windows"),
                                    Element::switch(ui.start_with_windows),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Open launcher on"),
                                    crate::i18n::tr("The launcher window appears on the chosen display"),
                                    Element::col()
                                        .spacing(6)
                                        .child(Element::radio(
                                            crate::i18n::tr("Primary display"),
                                            ui.monitor_preference,
                                            0,
                                        ))
                                        .child(Element::radio(
                                            crate::i18n::tr("Display with the mouse cursor"),
                                            ui.monitor_preference,
                                            1,
                                        ))
                                        .child(Element::radio(
                                            crate::i18n::tr("Display with the focused window"),
                                            ui.monitor_preference,
                                            2,
                                        )),
                                ),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Game Mode shortcut"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Shortcut enabled"),
                                    crate::i18n::tr("Toggle Game Mode with a global shortcut"),
                                    Element::switch(ui.game_mode_hotkey_enabled),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Game Mode key"),
                                    crate::i18n::tr("Press the combination that toggles Game Mode"),
                                    Element::row()
                                        .spacing(8)
                                        .cross(Align::Center)
                                        .child(
                                            Element::label_signal(ui.game_mode_display)
                                                .padding_xy(10, 6)
                                                .bg(Color::rgba(255, 255, 255, 24))
                                                .corner(8.0),
                                        )
                                        .child(
                                            Element::button(crate::i18n::tr("Record key"))
                                                .small()
                                                .enabled_when(move || game_mode_enabled.get())
                                                .on_click(move |ctx| {
                                                    game_mode_recording.set(true);
                                                    game_mode_handle.set_enabled(false);
                                                    ctx.toast_ok(crate::i18n::tr("Press the desired Game Mode key"));
                                                }),
                                        ),
                                ),
                                Element::row()
                                    .width_match()
                                    .spacing(10)
                                    .cross(Align::Center)
                                    .child(Element::flex_spacer())
                                    .child(
                                        Element::label(crate::i18n::tr("Modifiers:"))
                                            .font_size(11.0)
                                            .fg_role(Role::TextSubtle),
                                    )
                                    .child(Element::checkbox("Ctrl", ui.game_mode_ctrl))
                                    .child(Element::checkbox("Alt", ui.game_mode_alt))
                                    .child(Element::checkbox("Shift", ui.game_mode_shift))
                                    .child(Element::checkbox("Windows", ui.game_mode_meta)),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Updates"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Check for updates"),
                                    crate::i18n::tr("Check stable GitHub releases automatically"),
                                    Element::switch(ui.update_checks_enabled),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Check interval"),
                                    crate::i18n::tr("How often Flux looks for a stable release"),
                                    Element::row()
                                        .spacing(8)
                                        .cross(Align::Center)
                                        .child(
                                            Element::text_input(ui.update_interval_hours, "24")
                                                .width(76),
                                        )
                                        .child(Element::label(crate::i18n::tr("hours")).font_size(11.0)),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Automatic install"),
                                    crate::i18n::tr("Install stable updates automatically"),
                                    Element::switch(ui.auto_install_updates),
                                ),
                                Element::row()
                                    .width_match()
                                    .spacing(10)
                                    .cross(Align::Center)
                                    .padding_xy(0, 4)
                                    .child(
                                        Element::col()
                                            .weight(1.0)
                                            .spacing(4)
                                            .child(
                                                Element::label_signal(ui.update_status)
                                                    .font_size(11.0)
                                                    .fg_role(Role::TextSubtle)
                                                    .max_lines(2)
                                                    .truncate(Truncate::End)
                                                    .width_match(),
                                            )
                                            .child(Element::badge_intent(
                                                crate::i18n::format("Current version: {version}", &[("version", CURRENT_VERSION.to_string())]),
                                                Intent::Neutral,
                                            )),
                                    )
                                    .child(
                                        Element::button(crate::i18n::tr("Check now"))
                                            .small()
                                            .on_click(move |ctx| {
                                                check_for_updates_now(&ui_for_check, ctx)
                                            }),
                                    )
                                    .child(
                                        Element::button(crate::i18n::tr("Install now"))
                                            .small()
                                            .visible_when(move || {
                                                update_available.get().is_some()
                                                    && !update_installing.get()
                                            })
                                            .on_click(move |ctx| {
                                                install_update(&ui_for_install, ctx)
                                            }),
                                    ),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("History"),
                            vec![Element::setting_row_desc(
                                crate::i18n::tr("Query history"),
                                crate::i18n::tr("Ctrl+H recalls committed searches"),
                                Element::button(crate::i18n::tr("Clear history"))
                                    .small()
                                    .on_click(move |_| {
                                        clear_history_confirm.set(true);
                                    }),
                            )],
                        )),
                )
                .child(settings_scroll_gutter()),
        )
}
