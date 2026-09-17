use std::sync::Arc;

use windui::prelude::*;

use super::everything::{self, InstallationState};
use super::plugins::native_plugin_install_path;
use super::settings_shell::settings_scroll_gutter;
use super::settings_ui::SettingsUi;

pub(crate) fn build_plugins_tab(ui: &SettingsUi) -> Element {
    let settings_for_everything_toggle = Arc::clone(&ui.shared_settings);
    let auto_enable_everything_for_toggle = ui.auto_enable_everything;
    let everything_installed_for_toggle = ui.everything_installed;
    let everything_status_for_toggle = ui.everything_status;
    let everything_installed_for_ui = ui.everything_installed;
    let auto_enable_everything = ui.auto_enable_everything;
    let everything_status = ui.everything_status;
    let obsidian_enabled = ui.obsidian_enabled;
    let obsidian_alias = ui.obsidian_alias;
    let google_enabled = ui.google_enabled;
    let google_alias = ui.google_alias;
    let settings_tab = ui.settings_tab;
    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 3)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(12)
                        .child(Element::label("Everything").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("Everything provides fast indexed file and folder search. Configure its automatic use here, alongside the other plugins.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Everything",
                            Element::checkbox(
                                "Auto-enable Everything when installed",
                                auto_enable_everything,
                            )
                            .on_toggle(move |_| {
                                let enabled = auto_enable_everything_for_toggle.get();
                                if let Ok(mut settings) = settings_for_everything_toggle.write() {
                                    settings.auto_enable_everything = enabled;
                                    settings.normalize();
                                    let _ = super::update_tasks::save_settings(&settings);
                                }
                                if !enabled {
                                    everything_status_for_toggle.set(String::from(
                                        "Everything auto-enable is disabled in Flux settings",
                                    ));
                                    return;
                                }
                                match everything::start_background_if_installed() {
                                    Ok(InstallationState::Installed(_)) => {
                                        everything_installed_for_toggle.set(true);
                                        everything_status_for_toggle.set(String::from(
                                            "Everything is already installed; Flux will enable IPC automatically",
                                        ));
                                    }
                                    Ok(InstallationState::Missing) => {
                                        everything_installed_for_toggle.set(false);
                                        everything_status_for_toggle.set(String::from(
                                            "Everything is not installed. Install it with winget to enable file search.",
                                        ));
                                    }
                                    Err(error) => everything_status_for_toggle.set(error),
                                }
                            }),
                        ))
                        .child(
                            Element::label("Everything is already installed")
                                .font_size(12.0)
                                .fg(Color::rgba(180, 255, 205, 235))
                                .visible_when(move || everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label("Everything is not installed")
                                .font_size(12.0)
                                .fg(Color::rgba(255, 225, 175, 235))
                                .visible_when(move || !everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label_signal(everything_status)
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 190))
                                .max_lines(2)
                                .truncate(Truncate::End)
                                .width_match(),
                        )
                        .child(
                            Element::label("Command: winget install -e --id voidtools.Everything")
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 155))
                                .visible_when(move || !everything_installed_for_ui.get())
                                .width_match(),
                        )
                        .child(
                            Element::button("Install Everything")
                                .visible_when(move || !everything_installed_for_ui.get())
                                .on_click(move |ctx| {
                                    match everything::launch_winget_install() {
                                        Ok(()) => {
                                            everything_status.set(String::from(
                                                "winget install started. Restart Flux after Everything is installed.",
                                            ));
                                            ctx.toast_ok("winget install started");
                                        }
                                        Err(error) => {
                                            everything_status.set(error.clone());
                                            ctx.toast_ok(error);
                                        }
                                    }
                                }),
                        )
                        .child(Element::label("Native plugins").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("Built-in providers run inside Flux. Community Rust DLL plugins run in one isolated shared worker spawned from this same flux-launcher.exe.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(format!("Community plugin folder: {}", native_plugin_install_path()))
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 150))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label("Configure built-in native Rust plugins without Python or C# runtimes.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Obsidian",
                            Element::checkbox("Enable Obsidian vault search", obsidian_enabled),
                        ))
                        .child(Element::field(
                            "Action keyword",
                            Element::text_input(obsidian_alias, "ob").width_match(),
                        ))
                        .child(
                            Element::label("Search notes and vault files with the configured keyword, for example: ob meeting. Use `ob create project` to create a new note.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Google Search",
                            Element::checkbox("Enable Google web search", google_enabled),
                        ))
                        .child(Element::field(
                            "Action keyword",
                            Element::text_input(google_alias, "g").width_match(),
                        ))
                        .child(
                            Element::label("Search the web with the configured keyword, for example: g space exploration. The result opens in your default browser.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                    )
                    .child(settings_scroll_gutter())
                )
}
