use windui::prelude::*;

use super::everything;
use super::plugins::native_plugin_install_path;
use super::settings_shell::{settings_group, settings_scroll_gutter};
use super::settings_ui::SettingsUi;

pub(crate) fn build_plugins_tab(ui: &SettingsUi) -> Element {
    let settings_tab = ui.settings_tab;
    let everything_installed = ui.everything_installed;
    let everything_status = ui.everything_status;

    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 3)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(16)
                        .child(settings_group(
                            "Everything",
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Auto-enable Everything"),
                                    crate::i18n::tr("Use Everything for file results while it is installed"),
                                    Element::switch(ui.auto_enable_everything),
                                ),
                                Element::label(crate::i18n::tr("Everything is already installed"))
                                    .font_size(12.0)
                                    .fg(Color::rgba(180, 255, 205, 235))
                                    .visible_when(move || everything_installed.get()),
                                Element::label(crate::i18n::tr("Everything is not installed"))
                                    .font_size(12.0)
                                    .fg(Color::rgba(255, 225, 175, 235))
                                    .visible_when(move || !everything_installed.get()),
                                Element::label_signal(ui.everything_status)
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
                                Element::button(crate::i18n::tr("Install Everything"))
                                    .small()
                                    .visible_when(move || !everything_installed.get())
                                    .on_click(move |ctx| {
                                        match everything::launch_winget_install() {
                                            Ok(()) => {
                                                everything_status.set(String::from(
                                                crate::i18n::tr("winget install started. Restart Flux after Everything is installed."),
                                            ));
                                                ctx.toast_ok(crate::i18n::tr("winget install started"));
                                            }
                                            Err(error) => {
                                                let message = error.clone();
                                                everything_status.set(message.clone());
                                                ctx.toast_ok(message);
                                            }
                                        }
                                    }),
                            ],
                        ))
                        .child(settings_group(
                            "Obsidian",
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Obsidian vault search"),
                                    crate::i18n::tr("Search notes and vault files by keyword"),
                                    Element::switch(ui.obsidian_enabled),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Action keyword"),
                                    crate::i18n::tr("Prefix typed before the vault query"),
                                    Element::text_input(ui.obsidian_alias, "ob").width(140),
                                ),
                                Element::label(crate::i18n::tr("For example: ob meeting. Use `ob create project` to create a new note."))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Google Search"),
                            vec![
                                Element::setting_row_desc(
                                    crate::i18n::tr("Google web search"),
                                    crate::i18n::tr("Open a browser search from the launcher"),
                                    Element::switch(ui.google_enabled),
                                ),
                                Element::setting_row_desc(
                                    crate::i18n::tr("Action keyword"),
                                    crate::i18n::tr("Prefix typed before the web query"),
                                    Element::text_input(ui.google_alias, "g").width(140),
                                ),
                                Element::label(crate::i18n::tr("For example: g space exploration. The result opens in your default browser."))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
                            ],
                        ))
                        .child(settings_group(
                            crate::i18n::tr("Native plugins"),
                            vec![
                                Element::label(crate::i18n::tr("Built-in providers run inside Flux. Community Rust DLL plugins run in one isolated shared worker spawned from this same flux-launcher.exe."))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(3)
                                    .truncate(Truncate::End)
                                    .width_match(),
                                Element::label(crate::i18n::tr("Native Flow plugins: %APPDATA%\\FluxLauncher\\Plugins or FLUX_PLUGIN_DIR"))
                                    .font_size(11.0)
                                    .fg_role(Role::TextSubtle)
                                    .max_lines(2)
                                    .truncate(Truncate::End)
                                    .width_match(),
                                Element::label(crate::i18n::format("Community plugin folder: {path}", &[("path", native_plugin_install_path())]))
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
