use std::sync::{Arc, RwLock};

use flux_core::Settings;
use windui::prelude::*;
use windui::signal::Signal;

use super::everything;
use super::launch;
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

pub(crate) fn build_recycle_bin_dialog(
    recycle_bin_confirmation: Signal<bool>,
    status: Signal<String>,
) -> Element {
    let confirmation_for_close = recycle_bin_confirmation;
    let confirmation_for_cancel = recycle_bin_confirmation;
    let confirmation_for_empty = recycle_bin_confirmation;
    let status_for_confirmation = status;
    Element::dialog_panel(
        recycle_bin_confirmation,
        "Empty Recycle Bin",
        360,
        move |_| confirmation_for_close.set(false),
        Element::col()
            .spacing(8)
            .child(
                Element::label("This permanently deletes all items in the Recycle Bin.")
                    .font_size(13.0)
                    .fg(Color::rgba(245, 248, 255, 245)),
            )
            .child(
                Element::label("This action cannot be undone.")
                    .font_size(12.0)
                    .fg(Color::rgba(255, 190, 190, 235)),
            ),
        Element::row()
            .width_match()
            .spacing(8)
            .child(Element::flex_spacer())
            .child(
                Element::button("Cancel")
                    .neutral()
                    .outline_soft()
                    .on_click(move |_| confirmation_for_cancel.set(false)),
            )
            .child(
                Element::button("Empty Recycle Bin")
                    .danger()
                    .on_click(move |_| {
                        confirmation_for_empty.set(false);
                        if launch::empty_recycle_bin() {
                            status_for_confirmation.set(String::from("Recycle Bin emptied"));
                        } else {
                            status_for_confirmation
                                .set(String::from("Could not empty the Recycle Bin"));
                        }
                    }),
            ),
    )
}
