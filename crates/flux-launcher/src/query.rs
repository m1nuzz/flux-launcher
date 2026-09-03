use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;

use crate::applications::{
    canonical_application_id, canonical_application_key, resolve_bare_executable_path,
};
use flux_core::{
    rank_results_with_priorities, PriorityEntry, ResultKind, ResultSource, SearchResult,
};
use windui::prelude::Signal;

const MAX_VISIBLE_RESULTS: usize = 16;

pub(crate) fn should_publish_initial_query_results(
    has_query: bool,
    built_in_results_are_empty: bool,
    displayed_results_are_empty: bool,
) -> bool {
    !has_query || !built_in_results_are_empty || displayed_results_are_empty
}

#[derive(Default)]
pub(crate) struct ProviderResults {
    pub(crate) sequence: u64,
    pub(crate) built_in: Vec<SearchResult>,
    pub(crate) applications: Vec<SearchResult>,
    pub(crate) everything: Vec<SearchResult>,
    pub(crate) plugins: Vec<SearchResult>,
    pub(crate) native_plugins: Vec<SearchResult>,
    pub(crate) applications_ready: bool,
    pub(crate) everything_ready: bool,
}

impl ProviderResults {
    pub(crate) fn reset(
        &mut self,
        sequence: u64,
        built_in: Vec<SearchResult>,
        everything_expected: bool,
    ) {
        self.sequence = sequence;
        self.built_in = built_in;
        self.applications.clear();
        self.everything.clear();
        self.plugins.clear();
        self.native_plugins.clear();
        self.applications_ready = false;
        self.everything_ready = !everything_expected;
    }

    pub(crate) fn core_ready(&self) -> bool {
        self.applications_ready && (self.everything_ready || !self.built_in.is_empty())
    }

    fn merged(&self, query: &str, priorities: &[String]) -> Vec<SearchResult> {
        let mut seen = HashSet::new();
        let collected = self
            .built_in
            .iter()
            .chain(&self.applications)
            .chain(&self.everything)
            .chain(&self.plugins)
            .chain(&self.native_plugins)
            .filter(|result| seen.insert(result.id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_application_duplicates(collected);
        rank_results_with_priorities(query, &mut merged, priorities);
        preserve_everything_file_order(&mut merged, &self.everything);
        merged.truncate(MAX_VISIBLE_RESULTS);
        trace_query_probe(query, &merged);
        merged
    }
}

fn trace_query_probe(query: &str, results: &[SearchResult]) {
    let normalized = query.trim().to_ascii_lowercase();
    if !matches!(
        normalized.as_str(),
        "1+1"
            | "2026-08"
            | "powershell"
            | "pwsh"
            | "q"
            | "中"
            | "文"
            | "中文"
            | "q中"
            | "q文"
            | "q中文"
    ) {
        return;
    }
    let Some(path) = std::env::var_os("FLUX_QUERY_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let snapshot = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    let sanitize = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let _ = writeln!(
        file,
        "snapshot={snapshot}\tquery={}\tcount={}",
        sanitize(&normalized),
        results.len()
    );
    for (index, result) in results.iter().enumerate() {
        let target = result.target.as_deref().map(sanitize).unwrap_or_default();
        let identity = canonical_application_key(result)
            .map(|value| sanitize(&value))
            .unwrap_or_default();
        let _ = writeln!(
            file,
            "snapshot={snapshot}\tquery={}\tindex={index}\tid={}\ttitle={}\tsource={:?}\tkind={:?}\ttarget={}\tidentity={}",
            sanitize(&normalized),
            sanitize(&result.id),
            sanitize(&result.title),
            result.source,
            result.kind,
            target,
            identity
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_provider_results(
    providers: &ProviderResults,
    query: &str,
    priorities: &[String],
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    inline_completion: Signal<String>,
    results: Signal<Vec<SearchResult>>,
) {
    let merged = providers.merged(query, priorities);
    if !selection_touched.get() {
        selected_index.set(0);
        selected_id.set(
            merged
                .first()
                .map(|result| result.id.clone())
                .unwrap_or_default(),
        );
    }
    inline_completion.set(crate::inline_completion_suffix(query, &merged));
    results.set(merged);
}

pub(crate) fn merge_application_duplicates(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut positions = HashMap::<String, usize>::new();
    let mut merged = Vec::with_capacity(results.len());

    for result in results {
        let Some(identity) = canonical_application_key(&result) else {
            merged.push(result);
            continue;
        };
        let Some(existing_index) = positions.get(&identity).copied() else {
            positions.insert(identity, merged.len());
            merged.push(result);
            continue;
        };

        let existing_is_exact_console = is_exact_console_result(&merged[existing_index]);
        let result_is_exact_console = is_exact_console_result(&result);
        if application_source_rank(&result) < application_source_rank(&merged[existing_index]) {
            let preserved_id = result_is_exact_console
                .then(|| result.id.clone())
                .or_else(|| existing_is_exact_console.then(|| merged[existing_index].id.clone()));
            merged[existing_index] = result;
            if let Some(id) = preserved_id {
                merged[existing_index].id = id;
            }
        } else if result_is_exact_console && !existing_is_exact_console {
            merged[existing_index].id = result.id;
        }
    }
    merged
}

fn is_exact_console_result(result: &SearchResult) -> bool {
    matches!(
        result.id.as_str(),
        "system:command-prompt" | "system:powershell"
    )
}

fn application_source_rank(result: &SearchResult) -> u8 {
    let subtitle = result.subtitle.to_ascii_lowercase();
    match result.source {
        ResultSource::ApplicationCatalog if subtitle.contains("start menu") => 0,
        ResultSource::ApplicationCatalog => 1,
        ResultSource::Everything => 2,
        ResultSource::Plugin => 3,
        ResultSource::BuiltIn => 4,
    }
}

pub(crate) fn preserve_everything_file_order(
    merged: &mut [SearchResult],
    provider_order: &[SearchResult],
) {
    let mut available = merged
        .iter()
        .filter(|result| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        })
        .map(|result| (result.id.clone(), result.clone()))
        .collect::<HashMap<_, _>>();
    let slots = merged
        .iter()
        .enumerate()
        .filter(|(_, result)| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    for (slot, provider_result) in slots
        .into_iter()
        .zip(provider_order.iter().filter(|result| {
            result.source == ResultSource::Everything && result.kind == ResultKind::File
        }))
    {
        let Some(result) = available.remove(&provider_result.id) else {
            continue;
        };
        merged[slot] = result;
    }
}

pub(crate) fn normalize_built_in_executable_targets(results: &mut [SearchResult]) {
    for result in results {
        if result.source != ResultSource::BuiltIn || !result.id.starts_with("system:") {
            continue;
        }
        let Some(target) = result.target.as_deref() else {
            continue;
        };
        if let Some(resolved) = resolve_bare_executable_path(target) {
            result.target = Some(resolved);
        }
    }
}

pub(crate) fn refresh_merged_results(
    providers: &Rc<RefCell<ProviderResults>>,
    query: Signal<String>,
    priorities: Signal<Vec<PriorityEntry>>,
    results: Signal<Vec<SearchResult>>,
) {
    let priority_ids = priorities
        .get()
        .into_iter()
        .flat_map(|entry| {
            let mut ids = vec![entry.id];
            if let Some(canonical_id) = canonical_application_id(&entry.target) {
                ids.push(canonical_id);
            }
            ids
        })
        .collect::<Vec<_>>();
    let merged = providers.borrow().merged(&query.get(), &priority_ids);
    results.set(merged);
}
