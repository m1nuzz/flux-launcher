#![cfg_attr(windows, windows_subsystem = "windows")]

mod everything;
mod fullscreen;
mod hotkeys;
mod launch;
mod plugins;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use everything::{EverythingResponse, EverythingWorker};
use flux_core::{should_suppress_activation, SearchModel, SearchResult, Settings};
use plugins::{FlowPluginWorker, PluginInvocation, PluginQueryResponse};
use windui::prelude::*;

const WINDOW_WIDTH: i32 = 720;
const WINDOW_HEIGHT: i32 = 520;
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const PROVIDER_MIN_QUERY_LEN: usize = 2;
const MAX_VISIBLE_RESULTS: usize = 8;

#[derive(Default)]
struct ProviderResults {
    sequence: u64,
    built_in: Vec<SearchResult>,
    everything: Vec<SearchResult>,
    plugins: Vec<SearchResult>,
}

impl ProviderResults {
    fn reset(&mut self, sequence: u64, built_in: Vec<SearchResult>) {
        self.sequence = sequence;
        self.built_in = built_in;
        self.everything.clear();
        self.plugins.clear();
    }

    fn merged(&self) -> Vec<SearchResult> {
        self.built_in
            .iter()
            .chain(&self.everything)
            .chain(&self.plugins)
            .take(MAX_VISIBLE_RESULTS)
            .cloned()
            .collect()
    }
}

fn result_row(
    result: SearchResult,
    selected_id: Signal<String>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginInvocation>>>,
) -> Element {
    let id = result.id;
    let target = result.target;
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
        .on_click(move |_| {
            selected_id.set(id.clone());
            if let Some(target) = target.as_deref() {
                let _ = launch::open_path(target);
                return;
            }
            if let Some(action) = plugin_actions.borrow().get(&id).cloned() {
                plugins::execute_async(action);
            }
        })
}

fn main() {
    let settings = Settings::load_or_default();
    let activation_hotkey = hotkeys::activation_hotkey(&settings.activation_hotkey);
    let shared_settings = Arc::new(RwLock::new(settings.clone()));

    let query = signal(String::new());
    let selected_id = signal(String::new());
    let status = signal(String::from("Ready"));
    let current_sequence = signal(0_u64);
    let game_mode_status = signal(if settings.game_mode {
        String::from("Game Mode: On")
    } else {
        String::from("Game Mode: Off")
    });

    let mut model = SearchModel::new();
    let results = signal(model.results().to_vec());
    let provider_results = Rc::new(RefCell::new(ProviderResults::default()));
    let plugin_actions = Rc::new(RefCell::new(HashMap::<String, PluginInvocation>::new()));
    let result_source = results;
    let selected_for_rows = selected_id;
    let actions_for_rows = Rc::clone(&plugin_actions);

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
        |result| result.id.clone(),
        move |result| result_row(result, selected_for_rows, Rc::clone(&actions_for_rows)),
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
                    Element::label(game_mode_status)
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
                    Element::label("Ctrl+F12 toggles Game Mode | Settings in tray")
                        .font_size(12.0)
                        .fg(Color::rgba(235, 241, 255, 150)),
                ),
        );

    let content = Element::col().fill().padding(18).child(surface);
    let query_for_interval = query;
    let results_for_interval = results;
    let status_for_interval = status;
    let sequence_for_interval = current_sequence;
    let providers_for_interval = Rc::clone(&provider_results);
    let actions_for_interval = Rc::clone(&plugin_actions);
    let mut last_query = String::new();
    let mut sequence = 0_u64;

    let mut app = App::new("Flux Launcher", WINDOW_WIDTH, WINDOW_HEIGHT);
    let query_for_everything = query;
    let results_for_everything = results;
    let status_for_everything = status;
    let sequence_for_everything = current_sequence;
    let providers_for_everything = Rc::clone(&provider_results);
    let everything_sender = app.channel::<EverythingResponse>(move |_, response| {
        if response.sequence != sequence_for_everything.get()
            || response.query != query_for_everything.get()
        {
            return;
        }
        let mut providers = providers_for_everything.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        if response.available {
            providers.everything = response.results;
            results_for_everything.set(providers.merged());
        }
        status_for_everything.set(response.status);
    });
    let everything_worker = EverythingWorker::spawn(everything_sender);

    let query_for_plugins = query;
    let results_for_plugins = results;
    let status_for_plugins = status;
    let sequence_for_plugins = current_sequence;
    let providers_for_plugins = Rc::clone(&provider_results);
    let actions_for_plugins = Rc::clone(&plugin_actions);
    let plugin_sender = app.channel::<PluginQueryResponse>(move |_, response| {
        if response.sequence != sequence_for_plugins.get()
            || response.query != query_for_plugins.get()
        {
            return;
        }
        let mut providers = providers_for_plugins.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        if response.available {
            providers.plugins = response.results;
            *actions_for_plugins.borrow_mut() = response.actions;
            results_for_plugins.set(providers.merged());
        }
        status_for_plugins.set(response.status);
    });
    let plugin_worker = FlowPluginWorker::spawn(plugin_sender);

    let settings_for_activation = Arc::clone(&shared_settings);
    let settings_for_game_mode = Arc::clone(&shared_settings);
    let game_mode_status_for_toggle = game_mode_status;
    app = app
        .hotkey(activation_hotkey, move |ctx| {
            let settings = settings_for_activation
                .read()
                .map(|settings| settings.clone())
                .unwrap_or_default();
            if !should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
                ctx.show_window();
            }
        })
        .hotkey(hotkeys::game_mode_toggle_hotkey(), move |_| {
            if let Ok(mut settings) = settings_for_game_mode.write() {
                settings.game_mode = !settings.game_mode;
                game_mode_status_for_toggle.set(if settings.game_mode {
                    String::from("Game Mode: On")
                } else {
                    String::from("Game Mode: Off")
                });
                let _ = settings.save();
            }
        });

    app.bg(Color::rgba(0, 0, 0, 0))
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

            sequence = sequence.wrapping_add(1);
            sequence_for_interval.set(sequence);
            model.set_query(&next_query);
            {
                let mut providers = providers_for_interval.borrow_mut();
                providers.reset(sequence, model.results().to_vec());
                results_for_interval.set(providers.merged());
            }
            actions_for_interval.borrow_mut().clear();
            if next_query.trim().len() < PROVIDER_MIN_QUERY_LEN {
                status_for_interval.set(String::from("Ready"));
            } else {
                status_for_interval.set(String::from(
                    "Searching Everything and native Flow plugins...",
                ));
                everything_worker.request(sequence, next_query.clone());
                plugin_worker.request(sequence, next_query.clone());
            }
            last_query = next_query;
        })
        .run();
}
