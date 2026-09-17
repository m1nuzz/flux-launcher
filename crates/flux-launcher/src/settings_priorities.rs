use std::rc::Rc;
use std::sync::Arc;

use windui::prelude::*;

use super::history_priorities::{move_priority_entry, remove_priority_entry};
use super::provider_snapshot::refresh_merged_results;
use super::settings_shell::{settings_scroll_gutter, settings_section_header};
use super::settings_ui::SettingsUi;

pub(crate) fn build_priority_list(ui: &SettingsUi) -> Element {
    let settings_for_priority_ui = Arc::clone(&ui.shared_settings);
    let providers_for_priority_ui = Rc::clone(&ui.providers);
    let query_for_priority_ui = ui.query;
    let priorities = ui.priorities;
    let results = ui.results;
    Element::list_signal(
        priorities,
        |entry| entry.id.clone(),
        move |entry| {
            let entry_id = entry.id.clone();
            let rank = priorities
                .get()
                .iter()
                .position(|candidate| candidate.id == entry_id)
                .map(|index| index + 1)
                .unwrap_or_default();
            let title = entry.title.clone();
            let target = entry.target.clone();
            let settings_for_up = Arc::clone(&settings_for_priority_ui);
            let settings_for_down = Arc::clone(&settings_for_priority_ui);
            let settings_for_remove = Arc::clone(&settings_for_priority_ui);
            let providers_for_up = Rc::clone(&providers_for_priority_ui);
            let providers_for_down = Rc::clone(&providers_for_priority_ui);
            let providers_for_remove = Rc::clone(&providers_for_priority_ui);
            let query_for_up = query_for_priority_ui;
            let query_for_down = query_for_priority_ui;
            let query_for_remove = query_for_priority_ui;
            let id_for_up = entry_id.clone();
            let id_for_down = entry_id.clone();
            let id_for_remove = entry_id.clone();
            Element::row()
                .width_match()
                .height(58)
                .padding_xy(10, 5)
                .spacing(8)
                .corner(9.0)
                .bg(Color::rgba(255, 255, 255, 12))
                .child(
                    Element::label(format!("{rank}"))
                        .font_size(16.0)
                        .fg(Color::rgba(170, 204, 255, 245))
                        .width(24)
                        .align(Align::Center),
                )
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(1)
                        .child(
                            Element::label(title)
                                .font_size(13.0)
                                .fg(Color::WHITE)
                                .max_lines(1)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(target)
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 235))
                                .max_lines(1)
                                .truncate(Truncate::End),
                        ),
                )
                .child(
                    Element::button("↑")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if move_priority_entry(&settings_for_up, priorities, &id_for_up, -1) {
                                refresh_merged_results(
                                    &providers_for_up,
                                    query_for_up,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority moved up");
                            }
                        }),
                )
                .child(
                    Element::button("↓")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if move_priority_entry(&settings_for_down, priorities, &id_for_down, 1)
                            {
                                refresh_merged_results(
                                    &providers_for_down,
                                    query_for_down,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority moved down");
                            }
                        }),
                )
                .child(
                    Element::button("Remove")
                        .neutral()
                        .outline_soft()
                        .on_click(move |ctx| {
                            if remove_priority_entry(
                                &settings_for_remove,
                                priorities,
                                &id_for_remove,
                            ) {
                                refresh_merged_results(
                                    &providers_for_remove,
                                    query_for_remove,
                                    priorities,
                                    results,
                                );
                                ctx.toast_ok("Priority removed");
                            }
                        }),
                )
        },
    )
}

pub(crate) fn build_priorities_empty(ui: &SettingsUi) -> Element {
    let priorities = ui.priorities;
    Element::label(
        "No explicit priorities yet. Select an application, press Right, then choose Set as priority.",
    )
    .font_size(12.0)
    .fg(Color::rgba(235, 241, 255, 235))
    .max_lines(2)
    .truncate(Truncate::End)
    .visible_when(move || priorities.get().is_empty())
}

pub(crate) fn build_priorities_tab(
    ui: &SettingsUi,
    priorities_empty: Element,
    priority_list: Element,
) -> Element {
    let settings_tab = ui.settings_tab;
    Element::scroll()
        .weight(1.0)
        .visible_when(move || settings_tab.get() == 2)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(10)
                        .child(settings_section_header("Priorities"))
                        .child(
                            Element::label("Only applications explicitly added with Set as priority appear here. Rank 1 is searched first.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 235))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(priorities_empty)
                        .child(priority_list)
                    )
                    .child(settings_scroll_gutter())
        )
}
