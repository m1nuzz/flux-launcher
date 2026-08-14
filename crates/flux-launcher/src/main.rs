#![cfg_attr(windows, windows_subsystem = "windows")]

mod fullscreen;

use std::time::Duration;

use flux_core::{SearchModel, SearchResult, Settings};
use windui::prelude::*;

const WINDOW_WIDTH: i32 = 720;
const WINDOW_HEIGHT: i32 = 520;
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);

fn result_row(result: SearchResult, selected_id: Signal<String>) -> Element {
    let id = result.id.to_owned();
    let title = result.title;
    let subtitle = result.subtitle;
    Element::row()
        .width_match()
        .height(58)
        .padding_xy(14, 8)
        .spacing(12)
        .corner(10.0)
        .bg(Color::rgba(255, 255, 255, 20))
        .child(
            Element::label(title)
                .font_size(15.0)
                .fg(Color::WHITE)
                .weight(1.0),
        )
        .child(
            Element::label(subtitle)
                .font_size(12.0)
                .fg(Color::rgba(235, 241, 255, 180))
                .align(Align::Center),
        )
        .on_click(move |_| selected_id.set(id.clone()))
}

fn main() {
    let settings = Settings::load_or_default();
    let query = signal(String::new());
    let selected_id = signal(String::new());
    let status = signal(String::from("Ready"));

    let mut model = SearchModel::new();
    let results = signal(model.results().to_vec());
    let result_source = results;
    let selected_for_rows = selected_id;

    let search_box = Element::text_input(query, "Search files, apps, and commands")
        .leading_icon('>')
        .width_match()
        .height(52)
        .font_size(17.0)
        .corner(12.0)
        .bg(Color::rgba(255, 255, 255, 26))
        .border(Color::rgba(255, 255, 255, 60), 1)
        .padding_xy(14, 0);

    let result_list = Element::list_signal(
        result_source,
        |result| result.id,
        move |result| result_row(result, selected_for_rows),
    )
    .weight(1.0)
    .corner(12.0);

    let surface = Element::col()
        .fill()
        .padding(24)
        .spacing(16)
        .corner(20.0)
        .bg(Color::rgba(18, 22, 30, 192))
        .border(Color::rgba(255, 255, 255, 48), 1)
        .shadow(Shadow::new(0.0, 18.0, 48.0, Color::rgba(0, 0, 0, 110)))
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(3)
                        .child(
                            Element::label("Flux Launcher")
                                .font_size(27.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label("Fast local search with native Windows integrations")
                                .font_size(13.0)
                                .fg(Color::rgba(235, 241, 255, 180)),
                        ),
                )
                .child(
                    Element::label("MVP")
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 205))
                        .padding_xy(10, 5)
                        .corner(8.0)
                        .bg(Color::rgba(91, 155, 255, 70)),
                ),
        )
        .child(search_box)
        .child(result_list)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::label(status)
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 170))
                        .weight(1.0),
                )
                .child(
                    Element::label("Esc hides | Enter opens | Settings in tray")
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 150)),
                ),
        );

    let content = Element::col().fill().padding(18).child(surface);
    let query_for_interval = query;
    let results_for_interval = results;
    let status_for_interval = status;
    let mut last_query = String::new();

    App::new("Flux Launcher", WINDOW_WIDTH, WINDOW_HEIGHT)
        .bg(Color::rgba(0, 0, 0, 0))
        .centered()
        .frameless()
        .resizable(false)
        .min_size(520, 360)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Mica)
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |_| {
            let next_query = query_for_interval.get();
            if next_query == last_query {
                return;
            }

            model.set_query(&next_query);
            let count = model.results().len();
            results_for_interval.set(model.results().to_vec());
            status_for_interval.set(if count == 0 {
                String::from("No built-in commands match this query")
            } else {
                format!("{count} built-in command result(s)")
            });
            last_query = next_query;
        })
        .run();

    let _ = settings;
}
