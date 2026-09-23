use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use flux_core::{PriorityEntry, ResultKind, SearchResult, Settings};
use windui::signal::Signal;

use super::update_tasks::{save_settings, save_settings_async};

pub(crate) fn game_mode_label(enabled: bool) -> String {
    if enabled {
        String::from("Game Mode: On")
    } else {
        String::from("Game Mode: Off")
    }
}

pub(crate) fn record_query_history(
    settings: &Arc<RwLock<Settings>>,
    history: &Rc<RefCell<Vec<String>>>,
    query: &str,
    stats_usage: Signal<String>,
    stats_top: Signal<String>,
) {
    let Ok(mut settings_guard) = settings.write() else {
        return;
    };
    if !settings_guard.record_query(query) {
        return;
    }
    *history.borrow_mut() = settings_guard.query_history.clone();
    let total = settings_guard.total_queries_committed;
    let counts = settings_guard.launch_counts.clone();
    drop(settings_guard);
    super::settings_stats::refresh_stats_texts(&counts, total, stats_usage, stats_top);
    // Keep Enter→hide free of synchronous filesystem I/O.
    save_settings_async(settings);
}

/// Record one launch of an opened result for the top-opened list. Call it
/// wherever the launcher actually opens something (Enter on a result, row
/// click, run-as-admin, open-location); pure action executions without an
/// open and confirmation rows stay out.
pub(crate) fn record_launch(
    settings: &Arc<RwLock<Settings>>,
    id: &str,
    title: &str,
    stats_top: Signal<String>,
) {
    let Ok(mut settings_guard) = settings.write() else {
        return;
    };
    settings_guard.record_launch(id, title);
    let counts = settings_guard.launch_counts.clone();
    drop(settings_guard);
    stats_top.set(super::settings_stats::top_launches_block(&counts));
    // Keep Enter→hide free of synchronous filesystem I/O.
    save_settings_async(settings);
}

pub(crate) fn set_result_priority(
    settings: &Arc<RwLock<Settings>>,
    priorities: Signal<Vec<PriorityEntry>>,
    result: &SearchResult,
) -> bool {
    let Some(target) = result.target.as_deref() else {
        return false;
    };
    if !matches!(result.kind, ResultKind::Application) {
        return false;
    }
    let Ok(mut settings_guard) = settings.write() else {
        return false;
    };
    settings_guard.add_priority(PriorityEntry {
        id: result.id.clone(),
        title: result.title.clone(),
        target: target.to_owned(),
    });
    let entries = settings_guard.priority_entries.clone();
    let saved = save_settings(&settings_guard);
    if saved {
        priorities.set(entries);
    }
    saved
}

pub(crate) fn remove_priority_entry(
    settings: &Arc<RwLock<Settings>>,
    priorities: Signal<Vec<PriorityEntry>>,
    id: &str,
) -> bool {
    let Ok(mut settings_guard) = settings.write() else {
        return false;
    };
    if !settings_guard.remove_priority(id) {
        return false;
    }
    let entries = settings_guard.priority_entries.clone();
    let saved = save_settings(&settings_guard);
    if saved {
        priorities.set(entries);
    }
    saved
}

pub(crate) fn move_priority_entry(
    settings: &Arc<RwLock<Settings>>,
    priorities: Signal<Vec<PriorityEntry>>,
    id: &str,
    direction: i32,
) -> bool {
    let Ok(mut settings_guard) = settings.write() else {
        return false;
    };
    let Some(index) = settings_guard
        .priority_entries
        .iter()
        .position(|entry| entry.id == id)
    else {
        return false;
    };
    if !settings_guard.move_priority(index, direction) {
        return false;
    }
    let entries = settings_guard.priority_entries.clone();
    let saved = save_settings(&settings_guard);
    if saved {
        priorities.set(entries);
    }
    saved
}

pub(crate) fn set_game_mode(
    settings: &Arc<RwLock<Settings>>,
    game_mode: Signal<bool>,
    status: Signal<String>,
    enabled: bool,
) {
    if let Ok(mut settings) = settings.write() {
        settings.game_mode = enabled;
        game_mode.set(enabled);
        status.set(game_mode_label(enabled));
        let _ = save_settings(&settings);
    }
}
