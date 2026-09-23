use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::prelude::*;
use windui::signal::Signal;

use super::everything;
use super::settings_stats::clear_history_state;
use super::settings_ui::SettingsUi;
use super::update_tasks::save_settings;

pub(crate) fn build_everything_install_prompt(
    everything_prompt_visible: Signal<bool>,
    everything_status: Signal<String>,
    shared_settings: Arc<RwLock<Settings>>,
) -> Element {
    let everything_prompt_for_close = everything_prompt_visible;
    let everything_prompt_for_decline = everything_prompt_visible;
    let everything_prompt_for_install = everything_prompt_visible;
    let everything_status_for_prompt = everything_status;
    let settings_for_everything_prompt_close = Arc::clone(&shared_settings);
    let settings_for_everything_prompt_decline = Arc::clone(&shared_settings);
    let settings_for_everything_prompt_install = Arc::clone(&shared_settings);
    Element::dialog_glass_panel(
        everything_prompt_visible,
        "Install Everything",
        400,
        move |_| {
            everything_prompt_for_close.set(false);
            if let Ok(mut settings) = settings_for_everything_prompt_close.write() {
                settings.everything_install_prompt_seen = true;
                let _ = save_settings(&settings);
            }
        },
        Element::col()
            .spacing(10)
            .child(
                Element::label(
                    "Everything is not installed. Install it now for fast indexed file and folder search?",
                )
                .font_size(13.0)
                .fg(Color::rgba(245, 248, 255, 245)),
            )
            .child(
                Element::label("Flux will run the official winget command: winget install -e --id voidtools.Everything")
                    .font_size(11.0)
                    .fg(Color::rgba(235, 241, 255, 180))
                    .max_lines(2)
                    .truncate(Truncate::End),
            ),
        Element::row()
            .width_match()
            .spacing(8)
            .child(Element::flex_spacer())
            .child(
                Element::button("Not now")
                    .neutral()
                    .outline_soft()
                    .on_click(move |_| {
                        everything_prompt_for_decline.set(false);
                        if let Ok(mut settings) = settings_for_everything_prompt_decline.write() {
                            settings.everything_install_prompt_seen = true;
                            let _ = save_settings(&settings);
                        }
                    }),
            )
            .child(
                Element::button("Install Everything").on_click(move |ctx| {
                    everything_prompt_for_install.set(false);
                    if let Ok(mut settings) = settings_for_everything_prompt_install.write() {
                        settings.everything_install_prompt_seen = true;
                        let _ = save_settings(&settings);
                    }
                    match everything::launch_winget_install() {
                        Ok(()) => {
                            everything_status_for_prompt.set(String::from(
                                "Everything installation started with winget.",
                            ));
                            ctx.toast_ok("Everything installation started");
                        }
                        Err(error) => {
                            everything_status_for_prompt.set(error.clone());
                            ctx.toast_ok(error);
                        }
                    }
                }),
            )
            .padding_edges(0, 0, 0, 12),
    )
}

/// Clear-history confirmation dialog, shared by the Stats and General tabs.
/// Confirm forgets remembered queries and top-opened counts, saves, and
/// refreshes the Stats display; Cancel and the × button only hide.
pub(crate) fn build_clear_history_confirm(visible: Signal<bool>, ui: &SettingsUi) -> Element {
    let visible_for_close = visible;
    let visible_for_cancel = visible;
    let visible_for_clear = visible;
    let settings_for_clear = Arc::clone(&ui.shared_settings);
    let history_for_clear = Rc::clone(&ui.query_history);
    let history_cursor_for_clear = ui.history_cursor;
    let usage_for_clear = ui.stats_usage;
    let top_for_clear = ui.stats_top;
    Element::dialog_glass_panel(
        visible,
        "Clear history",
        400,
        move |_| {
            visible_for_close.set(false);
        },
        Element::col().spacing(10).child(
            Element::label(
                "Forget all remembered queries and top-opened counts? This cannot be undone.",
            )
            .font_size(13.0)
            .fg(Color::rgba(245, 248, 255, 245)),
        ),
        Element::row()
            .width_match()
            .spacing(8)
            .child(Element::flex_spacer())
            .child(
                Element::button("Cancel")
                    .neutral()
                    .outline_soft()
                    .on_click(move |_| {
                        visible_for_cancel.set(false);
                    }),
            )
            .child(Element::button("Clear history").on_click(move |ctx| {
                visible_for_clear.set(false);
                let cleared = clear_history_state(
                    &settings_for_clear,
                    &history_for_clear,
                    history_cursor_for_clear,
                    usage_for_clear,
                    top_for_clear,
                );
                if !cleared {
                    ctx.toast_ok("Could not clear history");
                    return;
                }
                if let Ok(settings) = settings_for_clear.read() {
                    let _ = save_settings(&settings);
                }
                ctx.toast_ok("Query history cleared");
            }))
            .padding_edges(0, 0, 0, 12),
    )
}
