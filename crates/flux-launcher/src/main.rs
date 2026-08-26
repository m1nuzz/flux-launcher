#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
mod applications;
mod builtin;
mod everything;
mod fullscreen;
mod hotkeys;
mod keyboard_layout;
mod launch;
mod monitor;
mod native_host;
mod plugin_limits;
mod plugin_transport;
mod plugins;
mod startup;
mod updater;
mod visual_preview;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender},
    Arc, Mutex, OnceLock, RwLock,
};
use std::thread;
use std::time::Duration;

use applications::{
    canonical_application_id, canonical_application_key, resolve_bare_executable_path,
    ApplicationResponse, ApplicationWorker,
};
use everything::{EverythingResponse, EverythingWorker, InstallationState};
use flux_core::{
    history_results, rank_results_with_priorities, should_suppress_activation, HotkeyConfig,
    MonitorPreference, PriorityEntry, ResultKind, ResultSource, SearchModel, SearchResult,
    Settings, DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT,
    MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use plugins::{
    native_plugin_install_path, FlowPluginWorker, NativePluginQueryResponse, NativePluginWorker,
    PluginAction, PluginQueryResponse,
};
use windui::app::{CursorVisibilityHandle, WindowOpHandle, WindowPositionHandle, WindowSizeHandle};
use windui::core::{ClickFn, ClipboardProvider, EventCtx, Widget};
use windui::event::{Event, Key, KeyEvent, MouseButton, PointerKind};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const SINGLE_INSTANCE_ID: &str = "m1nuzz.flux-launcher";
const SETTINGS_WINDOW_WIDTH: i32 = 720;
// The empty launcher is a compact search strip; the results state keeps the user-configured height.
const COMPACT_WINDOW_HEIGHT: i32 = 56;
const VISUAL_SLIDER_WIDTH: i32 = 200;
// The action group stays narrower than the minimum launcher content width so it
// can be centered between the same left/right content insets at every size.
const ACTION_BAR_WIDTH: i32 = 340;
const ACTION_BAR_HEIGHT: i32 = 22;
// Keep the result palette compact like the reference while exposing a six-row
// viewport; additional results remain available through the native wheel scroll.
const ACTION_WINDOW_HEIGHT: i32 = 250;
// Six 46-DIP result rows plus local scroll padding keep the footer close to the results.
const RESULT_VIEWPORT_HEIGHT: i32 = 288;
const SETTINGS_WINDOW_HEIGHT: i32 = 520;
const LAUNCHER_FONT_FAMILY: &str = "Segoe UI Variable";
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const EVERYTHING_MIN_QUERY_LEN: usize = 1;
const PLUGIN_MIN_QUERY_LEN: usize = 2;
const MAX_VISIBLE_RESULTS: usize = 16;

static GOOGLE_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();
static OBSIDIAN_ICON_RGBA: OnceLock<Option<Vec<u8>>> = OnceLock::new();

fn should_claim_single_instance(mode: Option<&std::ffi::OsStr>) -> bool {
    !matches!(
        mode,
        Some(mode)
            if mode == std::ffi::OsStr::new("--plugin-host")
                || mode == std::ffi::OsStr::new("--folder-launch-smoke")
                || mode == std::ffi::OsStr::new("--shortcut-icon-smoke")
    )
}
fn is_shutdown_mode(mode: Option<&std::ffi::OsStr>) -> bool {
    mode == Some(std::ffi::OsStr::new("--shutdown"))
}
fn is_run_as_admin_key(event: &KeyEvent) -> bool {
    event.ctrl
        && matches!(
            event.key,
            Key::Other(0x52) | Key::Char('r') | Key::Char('R')
        )
}

fn monitor_preference_index(preference: MonitorPreference) -> usize {
    match preference {
        MonitorPreference::Primary => 0,
        MonitorPreference::Cursor => 1,
        MonitorPreference::Foreground => 2,
    }
}

fn monitor_preference_from_index(index: usize) -> MonitorPreference {
    match index {
        1 => MonitorPreference::Cursor,
        2 => MonitorPreference::Foreground,
        _ => MonitorPreference::Primary,
    }
}

fn request_monitor_position(
    position: &WindowPositionHandle,
    preference: MonitorPreference,
    width: i32,
    height: i32,
) {
    if let Some((x, y)) = monitor::centered_position(preference, width, height) {
        position.set(x, y);
    }
}

fn request_scroll(scroll_pending: Signal<bool>) {
    scroll_pending.set(true);
}

#[cfg(test)]
fn launcher_window_geometry(settings_visible: bool, show_results: bool) -> (i32, i32) {
    launcher_window_geometry_with_sizes(
        settings_visible,
        show_results,
        DEFAULT_LAUNCHER_WIDTH as i32,
        DEFAULT_LAUNCHER_HEIGHT as i32,
    )
}

fn launcher_window_geometry_with_sizes(
    settings_visible: bool,
    show_results: bool,
    launcher_width: i32,
    launcher_height: i32,
) -> (i32, i32) {
    if settings_visible {
        (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
    } else if show_results {
        (launcher_width, launcher_height)
    } else {
        (launcher_width, COMPACT_WINDOW_HEIGHT)
    }
}

fn visual_preview_position(
    preference: MonitorPreference,
    preview_width: i32,
    preview_height: i32,
) -> (i32, i32) {
    #[cfg(windows)]
    {
        let Some(bounds) = monitor::work_area(preference) else {
            return (0, 0);
        };
        let (settings_x, settings_y) =
            monitor::centered_position(preference, SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
                .unwrap_or((bounds.left, bounds.top));
        let gap = 24;
        let right_x = settings_x + SETTINGS_WINDOW_WIDTH + gap;
        let left_x = settings_x - preview_width - gap;
        // Prefer a fully visible side-by-side preview. On a small CI desktop there
        // may be no non-overlapping rectangle for 720x520 Settings plus the selected
        // preview size; keep the preview outside Settings and let Windows clip its
        // off-screen portion rather than covering the controls being dragged.
        let x = if right_x + preview_width <= bounds.right {
            right_x
        } else if left_x >= bounds.left {
            left_x
        } else {
            right_x
        };
        let y = settings_y + (SETTINGS_WINDOW_HEIGHT - preview_height).max(0) / 2;
        (
            x,
            y.clamp(bounds.top, (bounds.bottom - preview_height).max(bounds.top)),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (preference, preview_width, preview_height);
        (0, 0)
    }
}

fn dimension_slider_fraction(value: u16, min: u16, max: u16) -> f32 {
    if max <= min {
        return 0.0;
    }
    (value.clamp(min, max) - min) as f32 / (max - min) as f32
}

fn dimension_from_slider(value: f32, min: u16, max: u16) -> u16 {
    if max <= min {
        return min;
    }
    let span = (max - min) as f32;
    (min as f32 + value.clamp(0.0, 1.0) * span).round() as u16
}

fn parse_dimension_input(value: &str, min: u16, max: u16) -> Option<u16> {
    value
        .trim()
        .parse::<u16>()
        .ok()
        .map(|value| value.clamp(min, max))
}

fn apply_launcher_size(
    size: &WindowSizeHandle,
    position: &WindowPositionHandle,
    settings: &Arc<RwLock<Settings>>,
    width: u16,
    height: u16,
    settings_visible: bool,
    show_results: bool,
) {
    let width = i32::from(width.clamp(MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH));
    let height = i32::from(height.clamp(MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT));
    let (target_width, target_height) =
        launcher_window_geometry_with_sizes(settings_visible, show_results, width, height);
    // Keep the Settings canvas fixed while visual values are edited. The real preview
    // process is resized separately; outside Settings, apply the dimensions to the launcher.
    size.set(target_width, target_height);
    if !settings_visible {
        if let Ok(settings) = settings.read() {
            request_monitor_position(
                position,
                settings.monitor_preference,
                target_width,
                target_height,
            );
        }
    }
}

/// Keep the previous result list visible while asynchronous providers compute a
/// new non-empty query. Immediate publication is safe for the home page and for
/// actionable synchronous built-in results, but publishing an empty vector for
/// every keystroke creates a visible blank frame and makes the list flicker.
fn should_publish_initial_query_results(
    has_query: bool,
    built_in_results_are_empty: bool,
    displayed_results_are_empty: bool,
) -> bool {
    !has_query || !built_in_results_are_empty || displayed_results_are_empty
}

#[derive(Default)]
struct ProviderResults {
    sequence: u64,
    built_in: Vec<SearchResult>,
    applications: Vec<SearchResult>,
    everything: Vec<SearchResult>,
    plugins: Vec<SearchResult>,
    native_plugins: Vec<SearchResult>,
    applications_ready: bool,
    everything_ready: bool,
}

impl ProviderResults {
    fn reset(&mut self, sequence: u64, built_in: Vec<SearchResult>, everything_expected: bool) {
        self.sequence = sequence;
        self.built_in = built_in;
        self.applications.clear();
        self.everything.clear();
        self.plugins.clear();
        self.native_plugins.clear();
        self.applications_ready = false;
        self.everything_ready = !everything_expected;
    }

    fn core_ready(&self) -> bool {
        // Built-in/system results must be actionable without waiting for the
        // asynchronous Everything response. When a query has no built-in result,
        // retain the atomic application+Everything snapshot behavior.
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
        "1+1" | "2026-08" | "powershell" | "pwsh"
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
fn commit_provider_results(
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
    inline_completion.set(inline_completion_suffix(query, &merged));
    results.set(merged);
}

fn merge_application_duplicates(results: Vec<SearchResult>) -> Vec<SearchResult> {
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

/// Keep Everything's native modified-date order for non-application files.
///
/// The global ranker still decides which provider tier occupies each result
/// slot, so application results remain first. Only the Everything file slots
/// are replaced in the order returned by the date-sorted IPC query.
fn preserve_everything_file_order(merged: &mut [SearchResult], provider_order: &[SearchResult]) {
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

fn normalize_built_in_executable_targets(results: &mut [SearchResult]) {
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

fn refresh_merged_results(
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

#[derive(Clone, Debug)]
enum ActionKind {
    Open,
    RunAsAdmin,
    OpenLocation,
    CopyPath,
    CopyName,
    SetPriority,
    RunPlugin(PluginAction),
}

#[derive(Clone, Debug)]
struct ActionItem {
    id: String,
    label: String,
    kind: ActionKind,
}

fn plugin_action_label(action: &PluginAction) -> &'static str {
    match action {
        PluginAction::Flow(_) => "Run plugin action",
        PluginAction::OpenUrl(_) => "Open web result",
        PluginAction::OpenPath(_) => "Open path",
        PluginAction::CopyText(_) => "Copy text",
    }
}

fn actions_for_result(
    result: &SearchResult,
    plugin_actions: &HashMap<String, PluginAction>,
) -> Vec<ActionItem> {
    let mut actions = Vec::with_capacity(4);
    if matches!(result.id.as_str(), "empty-recycle-bin" | "open-recycle-bin") {
        return actions;
    }
    if result.id.starts_with("system:") {
        actions.push(ActionItem {
            id: format!("{}:open", result.id),
            label: String::from("Open"),
            kind: ActionKind::Open,
        });
        actions.push(ActionItem {
            id: format!("{}:copy-name", result.id),
            label: String::from("Copy name"),
            kind: ActionKind::CopyName,
        });
        return actions;
    }
    if result.target.is_some() {
        if matches!(result.kind, ResultKind::Application) {
            actions.push(ActionItem {
                id: format!("{}:set-priority", result.id),
                label: String::from("Set as priority (move to top)"),
                kind: ActionKind::SetPriority,
            });
        }
        actions.push(ActionItem {
            id: format!("{}:open", result.id),
            label: String::from("Open"),
            kind: ActionKind::Open,
        });
        actions.push(ActionItem {
            id: format!("{}:run-as-admin", result.id),
            label: String::from("Run as admin"),
            kind: ActionKind::RunAsAdmin,
        });
        actions.push(ActionItem {
            id: format!("{}:open-location", result.id),
            label: String::from("Open file location"),
            kind: ActionKind::OpenLocation,
        });
        if !matches!(result.kind, ResultKind::Application) {
            actions.push(ActionItem {
                id: format!("{}:copy-path", result.id),
                label: String::from("Copy path"),
                kind: ActionKind::CopyPath,
            });
        }
    }
    if let Some(invocation) = plugin_actions.get(&result.id).cloned() {
        actions.push(ActionItem {
            id: format!("{}:plugin", result.id),
            label: String::from(plugin_action_label(&invocation)),
            kind: ActionKind::RunPlugin(invocation),
        });
    }
    if !matches!(result.kind, ResultKind::Application) {
        actions.push(ActionItem {
            id: format!("{}:copy-name", result.id),
            label: String::from("Copy name"),
            kind: ActionKind::CopyName,
        });
    }
    actions
}

fn selected_result(
    results: &[SearchResult],
    selected_id: &str,
    selected_index: usize,
) -> Option<SearchResult> {
    results
        .iter()
        .find(|result| result.id == selected_id)
        .cloned()
        .or_else(|| results.get(selected_index).cloned())
        .or_else(|| results.first().cloned())
}

/// Invisible reactive widget that keeps the keyboard-selected row inside the
/// surrounding windui scroll viewport without painting an additional surface.
struct ResultRowAnchor {
    result_id: String,
    title: String,
    title_doc_signal: Signal<RichDoc>,
    trailing_signal: Signal<String>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    query: Signal<String>,
    scroll_pending: Signal<bool>,
    selection_color: Signal<Color>,
    on_click: Option<ClickFn>,
    pressed: bool,
    last_pointer: Option<(i32, i32)>,
    last_selected: Option<bool>,
    last_query: String,
}

fn hover_position_changed(last: &mut Option<(i32, i32)>, position: (i32, i32)) -> bool {
    if *last == Some(position) {
        return false;
    }
    *last = Some(position);
    true
}

impl ResultRowAnchor {
    fn select_self(&self) {
        if self.selected_id.get() == self.result_id {
            return;
        }
        self.selection_touched.set(true);
        self.selected_id.set(self.result_id.clone());
        if let Some(index) = self
            .rows_refresh
            .get()
            .iter()
            .position(|result| result.id == self.result_id)
        {
            self.selected_index.set(index);
        }
        // The row itself is reactive, so selection painting updates without
        // rebuilding the whole list. Rebuilding here would discard the current
        // row geometry before scroll_into_view can reveal the selected result.
    }
}

impl Widget for ResultRowAnchor {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let selected = self.selected_id.get() == self.result_id;
        let query = self.query.get();
        let selection_changed = self.last_selected != Some(selected);
        let query_changed = self.last_query != query;
        let scroll_requested = self.scroll_pending.get();
        if selection_changed || query_changed {
            self.title_doc_signal
                .set(title_match_doc(&self.title, &query));
            self.trailing_signal.set(if selected {
                String::from("↵")
            } else {
                String::new()
            });
        }
        self.last_selected = Some(selected);
        self.last_query = query;
        // Scroll only after an explicit query/keyboard request. Wheel scrolling,
        // hover selection, and list repaints must never call scroll_into_view;
        // doing so feeds a layout mutation back into the ScrollWidget and pins
        // the viewport to the selected row (usually the top).
        if selected && scroll_requested {
            let row_id = ctx.id();
            let _ = ctx.tree_mut().scroll_into_view(row_id);
            self.scroll_pending.set(false);
        }
    }

    fn on_event(&mut self, ctx: &mut EventCtx, event: &Event) -> bool {
        let Event::Pointer(pointer) = event else {
            return false;
        };
        match pointer.kind {
            PointerKind::Enter => {
                // Do not select merely because the window appeared under a
                // stationary cursor; select on the first real Move instead.
                self.last_pointer = Some((pointer.pos.x, pointer.pos.y));
                ctx.mark_dirty();
                true
            }
            PointerKind::Move => {
                let position = (pointer.pos.x, pointer.pos.y);
                if hover_position_changed(&mut self.last_pointer, position) {
                    self.select_self();
                    ctx.mark_dirty();
                }
                true
            }
            PointerKind::Leave => {
                self.last_pointer = None;
                ctx.mark_dirty();
                true
            }
            PointerKind::Down if pointer.button == MouseButton::Left => {
                self.select_self();
                self.pressed = true;
                ctx.capture();
                ctx.mark_dirty();
                true
            }
            PointerKind::Up if pointer.button == MouseButton::Left => {
                let was_pressed = self.pressed;
                self.pressed = false;
                let inside = ctx.bounds().contains(pointer.pos);
                ctx.release_capture();
                ctx.mark_dirty();
                if was_pressed && inside {
                    if let Some(callback) = self.on_click.as_mut() {
                        callback(ctx);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn take_click(&mut self, callback: ClickFn) {
        self.on_click = Some(callback);
    }

    fn reset_interaction(&mut self) {
        self.pressed = false;
        self.last_pointer = None;
    }

    fn cursor(&self) -> windui::event::CursorShape {
        windui::event::CursorShape::Hand
    }

    fn paint(
        &self,
        bounds: windui::geometry::Rect,
        _content: windui::geometry::Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        _style: &windui::style::Style,
    ) {
        let selected = self.selected_id.get() == self.result_id;
        let color = if selected {
            self.selection_color.get()
        } else {
            Color::rgba(255, 255, 255, 18)
        };
        canvas.fill_round_rect(
            bounds.x as f32,
            bounds.y as f32,
            bounds.w as f32,
            bounds.h as f32,
            10.0,
            &Paint::fill(color),
        );
    }
}

/// A stable result-row icon that starts with a lightweight fallback and swaps to the
/// cached Windows Shell image when the background icon worker completes. Keeping this
/// widget inside the existing row avoids rebuilding the dynamic result list, which
/// would otherwise reset row-local interaction state and can disturb scrolling.
struct ResultIconView {
    target: Option<String>,
    fallback: String,
    fallback_font: &'static str,
    refresh_generation: Signal<u64>,
    last_generation: u64,
    image: Option<Image>,
}

impl ResultIconView {
    fn new(
        target: Option<String>,
        fallback: String,
        fallback_font: &'static str,
        initial_rgba: Option<Vec<u8>>,
        refresh_generation: Signal<u64>,
    ) -> Self {
        let image = initial_rgba
            .as_deref()
            .and_then(|rgba| Image::from_rgba(32, 32, rgba).ok());
        Self {
            target,
            fallback,
            fallback_font,
            refresh_generation,
            last_generation: refresh_generation.get(),
            image,
        }
    }

    fn refresh_cached_image(&mut self) {
        let Some(target) = self.target.as_deref() else {
            return;
        };
        #[cfg(windows)]
        let rgba = shell_icon_cache_lookup(target).flatten();
        #[cfg(not(windows))]
        let rgba: Option<Vec<u8>> = None;
        self.image = rgba
            .as_deref()
            .and_then(|bytes| Image::from_rgba(32, 32, bytes).ok());
    }
}

impl Widget for ResultIconView {
    fn measure(
        &self,
        _avail: Size,
        _style: &Style,
        _text: &mut dyn windui::text::TextEngine,
    ) -> Size {
        Size::new(32, 32)
    }

    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        canvas: &mut dyn Canvas,
        style: &Style,
    ) {
        if let Some(image) = self.image.as_ref() {
            canvas.draw_image(image, bounds, Fit::Contain, style.corner_radius, 1.0);
            return;
        }
        let fallback_style = Style {
            font_family: Some(self.fallback_font.to_owned()),
            font_size: 20.0,
            text_align: Align::Center,
            fg: Color::rgba(201, 218, 240, 235),
            fg_role: None,
            ..style.clone()
        };
        canvas.draw_text(
            &self.fallback,
            bounds,
            fallback_style.fg,
            Align::Center,
            &windui::text::TextStyle::of(&fallback_style),
        );
    }

    fn on_update(&mut self, ctx: &mut EventCtx) {
        let generation = self.refresh_generation.get();
        if generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        self.refresh_cached_image();
        ctx.mark_dirty();
    }

    fn on_event(&mut self, _ctx: &mut EventCtx, _event: &Event) -> bool {
        false
    }
}

fn quoted_result_path(result: &SearchResult) -> Option<String> {
    let target = result.target.as_deref()?.trim();
    if target.is_empty() {
        return None;
    }
    let target = target
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(target);
    Some(format!("\"{target}\""))
}

fn copy_result_path(result: &SearchResult) -> bool {
    let Some(path) = quoted_result_path(result) else {
        return false;
    };
    windui::platform::Clipboard.set_text(&path);
    true
}

fn execute_result_action(result: &SearchResult, action: &ActionKind) -> bool {
    match action {
        ActionKind::Open => {
            if let Some(target) = result.target.as_deref() {
                launch::open_path_async(target);
                true
            } else {
                false
            }
        }
        ActionKind::RunAsAdmin => result
            .target
            .as_deref()
            .map(launch::run_as_admin)
            .unwrap_or(false),
        ActionKind::OpenLocation => {
            if let Some(target) = result.target.as_deref() {
                let _ = launch::open_file_location(target);
                true
            } else {
                false
            }
        }
        ActionKind::CopyPath => copy_result_path(result),
        ActionKind::CopyName => {
            windui::platform::Clipboard.set_text(&result.title);
            true
        }
        ActionKind::SetPriority => false,
        ActionKind::RunPlugin(invocation) => {
            plugins::execute_async(invocation.clone());
            true
        }
    }
}

fn decode_bundled_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    const ICON_SIZE: usize = 32;
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let Ok(mut reader) = decoder.read_info() else {
        return None;
    };
    let mut source = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut source) else {
        return None;
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }

    let source = &source[..info.buffer_size()];
    let mut icon = vec![0_u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        let source_y = y * info.height as usize / ICON_SIZE;
        for x in 0..ICON_SIZE {
            let source_x = x * info.width as usize / ICON_SIZE;
            let source_index = (source_y * info.width as usize + source_x) * 4;
            let target_index = (y * ICON_SIZE + x) * 4;
            icon[target_index..target_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    Some(icon)
}

fn bundled_icon_rgba(result_id: &str) -> Option<Vec<u8>> {
    match result_id {
        "builtin:google-search" => google_icon_rgba(),
        _ if result_id.starts_with("builtin:obsidian:") => obsidian_icon_rgba(),
        _ => None,
    }
}

fn google_icon_rgba() -> Option<Vec<u8>> {
    GOOGLE_ICON_RGBA
        .get_or_init(|| decode_bundled_icon(include_bytes!("../assets/google.png")))
        .clone()
}

fn obsidian_icon_rgba() -> Option<Vec<u8>> {
    OBSIDIAN_ICON_RGBA
        .get_or_init(|| decode_bundled_icon(include_bytes!("../assets/obsidian.png")))
        .clone()
}

fn tray_icon() -> Vec<u8> {
    const ICON_SIZE: usize = 16;
    let fallback = || {
        let mut pixels = Vec::with_capacity(ICON_SIZE * ICON_SIZE * 4);
        for y in 0..ICON_SIZE {
            for x in 0..ICON_SIZE {
                let active = (x + y) % 5 < 3;
                let (red, green, blue) = if active { (78, 139, 255) } else { (28, 39, 62) };
                pixels.extend([red, green, blue, 255]);
            }
        }
        pixels
    };

    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../../assets/ico.png"
    )));
    let Ok(mut reader) = decoder.read_info() else {
        return fallback();
    };
    let mut source = vec![0; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut source) else {
        return fallback();
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return fallback();
    }

    let source = &source[..info.buffer_size()];
    let mut icon = vec![0_u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        let source_y = y * info.height as usize / ICON_SIZE;
        for x in 0..ICON_SIZE {
            let source_x = x * info.width as usize / ICON_SIZE;
            let source_index = (source_y * info.width as usize + source_x) * 4;
            let target_index = (y * ICON_SIZE + x) * 4;
            icon[target_index..target_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
    icon
}

fn game_mode_label(enabled: bool) -> String {
    if enabled {
        String::from("Game Mode: On")
    } else {
        String::from("Game Mode: Off")
    }
}

fn custom_selection_color_rgb(value: u32) -> (u8, u8, u8) {
    (
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

fn selection_color_for_settings(settings: &Settings) -> Color {
    let (r, g, b) = if settings.use_system_accent {
        accent::system_accent_rgb()
            .unwrap_or_else(|| custom_selection_color_rgb(settings.custom_selection_color))
    } else {
        custom_selection_color_rgb(settings.custom_selection_color)
    };
    Color::rgba(r, g, b, 84)
}

fn selection_color_hex(value: u32) -> String {
    format!("#{value:06X}")
}

fn parse_selection_color(value: &str) -> Option<u32> {
    let trimmed = value.trim().trim_start_matches('#');
    (trimmed.len() == 6)
        .then(|| u32::from_str_radix(trimmed, 16).ok())
        .flatten()
}

fn selection_palette(custom_selection_color: Signal<String>) -> Element {
    const COLORS: &[u32] = &[
        0x4c8bf4, 0x0078d4, 0x00a4ef, 0x107c10, 0x498205, 0xffb900, 0xd83b01, 0xe74856, 0x8764b8,
        0x744da9, 0x038387, 0x605e5c,
    ];
    let mut row = Element::row().spacing(6).width_match();
    for &value in COLORS {
        let label = selection_color_hex(value);
        row = row.child(
            Element::col()
                .width(24)
                .height(24)
                .bg(Color::rgb(
                    ((value >> 16) & 0xff) as u8,
                    ((value >> 8) & 0xff) as u8,
                    (value & 0xff) as u8,
                ))
                .corner(6.0)
                .clickable()
                .tooltip(label)
                .on_click(move |_| custom_selection_color.set(selection_color_hex(value))),
        );
    }
    row
}

fn display_title(title: &str) -> String {
    const MAX_TITLE_CHARS: usize = 26;
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= MAX_TITLE_CHARS {
        return title.to_owned();
    }

    let extension_start = title
        .char_indices()
        .rev()
        .find_map(|(index, character)| (character == '.' && index > 0).then_some(index));
    let (stem, extension) = extension_start
        .map(|index| title.split_at(index))
        .unwrap_or((title, ""));
    let extension_chars: Vec<char> = extension.chars().collect();
    let available_stem_chars = MAX_TITLE_CHARS
        .saturating_sub(extension_chars.len())
        .saturating_sub(1);
    if available_stem_chars < 2 {
        return chars
            .into_iter()
            .take(MAX_TITLE_CHARS.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect();
    }

    let stem_chars: Vec<char> = stem.chars().collect();
    let prefix_len = available_stem_chars.div_ceil(2);
    let suffix_len = available_stem_chars / 2;
    stem_chars
        .iter()
        .take(prefix_len)
        .chain(std::iter::once(&'…'))
        .chain(
            stem_chars
                .iter()
                .skip(stem_chars.len().saturating_sub(suffix_len)),
        )
        .copied()
        .chain(extension_chars)
        .collect()
}

fn title_match_doc(title: &str, query: &str) -> RichDoc {
    // Follow the Windows 11 type hierarchy: regular body text, with stronger
    // weight reserved for the characters matched by the current query.
    let normal = SpanStyle::new()
        .family(LAUNCHER_FONT_FAMILY)
        .weight(400)
        .fg(Color::rgba(255, 255, 255, 255));
    let matched = SpanStyle::new()
        .family(LAUNCHER_FONT_FAMILY)
        .weight(650)
        .fg(Color::rgba(255, 255, 255, 255));
    let mut para = Para::new();

    let query_chars: Vec<char> = query
        .trim()
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let display_title = display_title(title);
    let title_chars: Vec<char> = display_title.chars().collect();
    let mut matched_flags = vec![false; title_chars.len()];
    let mut query_index = 0;
    for (index, character) in title_chars.iter().enumerate() {
        if query_index < query_chars.len()
            && character
                .to_lowercase()
                .eq(query_chars[query_index].to_lowercase())
        {
            matched_flags[index] = true;
            query_index += 1;
        }
    }

    let mut start = 0;
    while start < title_chars.len() {
        let is_match = matched_flags[start];
        let mut end = start + 1;
        while end < title_chars.len() && matched_flags[end] == is_match {
            end += 1;
        }
        let text: String = title_chars[start..end].iter().collect();
        para = para.span(
            text,
            if is_match {
                matched.clone()
            } else {
                normal.clone()
            },
        );
        start = end;
    }
    RichDoc::new().para(para)
}

fn normalize_everything_query(query: &str) -> String {
    let trimmed = query.trim();
    let Some(rest) = trimmed.strip_prefix('.') else {
        return trimmed.to_owned();
    };
    let (extension, remainder) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if extension.is_empty()
        || !extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return trimmed.to_owned();
    }
    let remainder = remainder.trim();
    if remainder.is_empty() {
        format!("ext:{extension}")
    } else {
        format!("ext:{extension} {remainder}")
    }
}

fn inline_completion_suffix(query: &str, results: &[SearchResult]) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let query_lower = trimmed.to_lowercase();
    let query_len = trimmed.chars().count();
    results
        .iter()
        .filter(|result| matches!(result.kind, ResultKind::Application))
        .find_map(|result| {
            let title_lower = result.title.to_lowercase();
            if !title_lower.starts_with(&query_lower) {
                return None;
            }
            Some(result.title.chars().skip(query_len).collect())
        })
        .unwrap_or_default()
}

fn history_cursor_step(history_len: usize, cursor: Option<usize>, key: Key) -> Option<usize> {
    if history_len == 0 {
        return None;
    }
    Some(match (key, cursor) {
        (Key::Up, Some(index)) => index.saturating_sub(1),
        (Key::Down, Some(index)) => (index + 1).min(history_len - 1),
        (_, _) => history_len - 1,
    })
}

#[cfg(windows)]
fn alt_key_is_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_MENU};
    unsafe { GetKeyState(VK_MENU.0 as i32) < 0 }
}

#[cfg(not(windows))]
fn alt_key_is_down() -> bool {
    false
}

#[cfg(windows)]
fn launcher_is_foreground() -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }
        let mut process_id = 0_u32;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        process_id == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
fn launcher_is_foreground() -> bool {
    false
}

fn should_show_launcher(is_foreground: bool) -> bool {
    !is_foreground
}

fn relaunch_mode_for_auto_install() -> updater::RelaunchMode {
    // Automatic updates must remain invisible: a restart should return to the
    // tray and never reopen Search. Manual Install now uses Visible explicitly.
    updater::RelaunchMode::Hidden
}

fn icon_completion_generation_changed(previous: u64, current: u64) -> bool {
    previous != current
}

#[cfg(windows)]
const MAX_SHELL_ICON_CACHE_ENTRIES: usize = 128;

#[cfg(windows)]
struct ShellIconCache {
    entries: HashMap<String, Option<Vec<u8>>>,
    lru_order: VecDeque<String>,
}

#[cfg(windows)]
impl ShellIconCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    fn get(&mut self, target: &str) -> Option<Option<Vec<u8>>> {
        let icon = self.entries.get(target).cloned()?;
        self.touch(target);
        Some(icon)
    }

    fn insert(&mut self, target: String, icon: Option<Vec<u8>>) {
        if self.entries.contains_key(&target) {
            self.entries.insert(target.clone(), icon);
            self.touch(&target);
            return;
        }
        while self.entries.len() >= MAX_SHELL_ICON_CACHE_ENTRIES {
            let Some(oldest) = self.lru_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.lru_order.push_back(target.clone());
        self.entries.insert(target, icon);
    }

    fn touch(&mut self, target: &str) {
        if let Some(position) = self.lru_order.iter().position(|key| key == target) {
            self.lru_order.remove(position);
        }
        self.lru_order.push_back(target.to_owned());
    }
}

#[cfg(windows)]
static SHELL_ICON_CACHE: OnceLock<Mutex<ShellIconCache>> = OnceLock::new();
static SETTINGS_SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static SHELL_ICON_COMPLETION_GENERATION: AtomicU64 = AtomicU64::new(0);

struct ShellIconWorker {
    pending: Arc<Mutex<HashSet<String>>>,
    wake: SyncSender<String>,
}

#[cfg(windows)]
fn initialize_shell_icon_worker_com() -> bool {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if result.is_ok() {
        true
    } else if result == RPC_E_CHANGED_MODE {
        eprintln!("[flux] shell icon worker inherited an initialized COM apartment");
        false
    } else {
        eprintln!("[flux] shell icon worker COM initialization failed: {result:?}");
        false
    }
}

impl ShellIconWorker {
    fn spawn() -> Self {
        let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
        let pending_for_worker = Arc::clone(&pending);
        let (wake, receiver) = mpsc::sync_channel::<String>(64);
        thread::Builder::new()
            .name(String::from("flux-shell-icons"))
            .spawn(move || {
                #[cfg(windows)]
                let owns_com_apartment = initialize_shell_icon_worker_com();

                while let Ok(target) = receiver.recv() {
                    if let Ok(mut pending) = pending_for_worker.lock() {
                        pending.remove(&target);
                    }
                    #[cfg(windows)]
                    let _ = shell_icon_rgba(&target);
                    SHELL_ICON_COMPLETION_GENERATION.fetch_add(1, Ordering::Release);
                }

                #[cfg(windows)]
                if owns_com_apartment {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            })
            .expect("failed to create shell icon worker thread");
        Self { pending, wake }
    }

    fn request(&self, target: String) {
        let should_send = self
            .pending
            .lock()
            .map(|mut pending| pending.insert(target.clone()))
            .unwrap_or(false);
        if !should_send {
            return;
        }
        if self.wake.try_send(target.clone()).is_err() {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&target);
            }
        }
    }
}

static SHELL_ICON_WORKER: OnceLock<ShellIconWorker> = OnceLock::new();

fn shell_icon_worker() -> &'static ShellIconWorker {
    SHELL_ICON_WORKER.get_or_init(ShellIconWorker::spawn)
}

#[cfg(windows)]
fn shell_icon_cache_lookup(target: &str) -> Option<Option<Vec<u8>>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    cache.lock().ok().and_then(|mut cache| cache.get(target))
}

#[cfg(windows)]
fn request_shell_icon(target: &str) -> Option<Vec<u8>> {
    if let Some(icon) = shell_icon_cache_lookup(target) {
        return icon;
    }
    shell_icon_worker().request(target.to_owned());
    None
}

#[cfg(not(windows))]
fn request_shell_icon(_target: &str) -> Option<Vec<u8>> {
    None
}

fn trace_result_icon_probe(
    title: &str,
    target: Option<&str>,
    icon_target: Option<&str>,
    initial_loaded: bool,
) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let sanitize = |value: &str| value.replace(['\t', '\r', '\n'], " ");
    let title = sanitize(title);
    let target = target.map(sanitize).unwrap_or_default();
    let icon_target = icon_target.map(sanitize).unwrap_or_default();
    let _ = writeln!(
        file,
        "title={title}\ttarget={target}\ticon_target={icon_target}\tinitial_loaded={initial_loaded}"
    );
}

fn trace_shell_icon_probe(target: &str, loaded: bool) {
    let Some(path) = std::env::var_os("FLUX_ICON_PROBE_FILE") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let target = target.replace(['\t', '\r', '\n'], " ");
    let _ = writeln!(file, "target={target}\tloaded={loaded}");
}

#[cfg(windows)]
fn shortcut_icon_smoke(target: &str) -> bool {
    shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .is_some_and(|rgba| rgba.len() == 32 * 32 * 4)
}

#[cfg(windows)]
fn shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(ShellIconCache::new()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(icon) = cache.get(target) {
            return icon;
        }
    }

    // Steam's Start Menu entries are commonly .lnk/.url shortcuts whose icon is
    // stored separately from the launch target. Resolve that explicit icon first,
    // matching Flow Launcher's shortcut-aware image loader, then use Shell fallbacks.
    let icon = shortcut_icon_location(target)
        .and_then(|(path, index)| extract_icon_rgba_from_source(&path, Some(index)))
        .or_else(|| {
            is_executable_icon_target(target)
                .then(|| extract_shell_icon_rgba(target))
                .flatten()
        })
        .or_else(|| extract_shell_thumbnail_rgba(target))
        .or_else(|| extract_shell_icon_rgba(target));
    trace_shell_icon_probe(target, icon.is_some());
    if let Ok(mut cache) = cache.lock() {
        cache.insert(target.to_owned(), icon.clone());
    }
    icon
}

#[cfg(windows)]
fn shortcut_icon_location(target: &str) -> Option<(String, i32)> {
    let extension = std::path::Path::new(target)
        .extension()
        .and_then(|value| value.to_str())?;
    if extension.eq_ignore_ascii_case("lnk") {
        return shell_link_icon_location(target);
    }
    if extension.eq_ignore_ascii_case("url") {
        let contents = std::fs::read_to_string(target).ok()?;
        let (path, index) = parse_internet_shortcut_icon_location(&contents)?;
        return resolve_shortcut_icon_path(target, &path).map(|path| (path, index));
    }
    None
}

fn parse_internet_shortcut_icon_location(contents: &str) -> Option<(String, i32)> {
    let mut icon_file = None;
    let mut icon_index = 0_i32;
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"');
        if key.eq_ignore_ascii_case("IconFile") && !value.is_empty() {
            icon_file = Some(value.to_owned());
        } else if key.eq_ignore_ascii_case("IconIndex") {
            icon_index = value.parse::<i32>().unwrap_or(0);
        }
    }
    icon_file.map(|path| (path, icon_index))
}

fn resolve_shortcut_icon_path(shortcut_path: &str, icon_path: &str) -> Option<String> {
    let expanded = expand_percent_variables_for_icon(icon_path)?;
    let expanded = expanded.trim().trim_matches('"');
    if expanded.is_empty() {
        return None;
    }
    let path = std::path::Path::new(expanded);
    if path.is_absolute() {
        return Some(expanded.to_owned());
    }
    let parent = std::path::Path::new(shortcut_path).parent()?;
    Some(parent.join(path).to_string_lossy().into_owned())
}

fn expand_percent_variables_for_icon(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let variable = &rest[start + 1..];
        let end = variable.find('%')?;
        let name = &variable[..end];
        let replacement = std::env::var_os(name)?.to_string_lossy().into_owned();
        output.push_str(&replacement);
        rest = &variable[end + 1..];
    }
    output.push_str(rest);
    Some(output)
}

#[cfg(windows)]
fn shell_link_icon_location(path: &str) -> Option<(String, i32)> {
    use windows::core::{Interface, GUID, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::IShellLinkW;

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    let result = (|| unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER).ok()?;
        let persist: IPersistFile = link.cast().ok()?;
        let wide_path = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM_READ).ok()?;
        let mut icon_path = [0_u16; 32_768];
        let mut icon_index = 0_i32;
        link.GetIconLocation(&mut icon_path, &mut icon_index).ok()?;
        let end = icon_path
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(icon_path.len());
        let icon_path = String::from_utf16_lossy(&icon_path[..end]);
        resolve_shortcut_icon_path(path, &icon_path).map(|path| (path, icon_index))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(windows))]
fn shell_icon_rgba(_target: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn extract_shell_thumbnail_rgba(target: &str) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_ICONONLY,
        SIIGBF_SCALEUP,
    };

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let shell_item: IShellItem =
        unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None).ok()? };
    let image_factory: IShellItemImageFactory = shell_item.cast().ok()?;
    let bitmap = unsafe {
        image_factory
            .GetImage(
                SIZE {
                    cx: ICON_SIZE,
                    cy: ICON_SIZE,
                },
                SIIGBF(SIIGBF_ICONONLY.0 | SIIGBF_SCALEUP.0),
            )
            .ok()?
    };
    if bitmap.0.is_null() {
        return None;
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
        }
        return None;
    }
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bgra = vec![0_u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let copied = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            ICON_SIZE as u32,
            Some(bgra.as_mut_ptr().cast::<c_void>()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    if copied == 0 {
        return None;
    }

    let has_alpha = bgra.chunks_exact(4).any(|pixel| pixel[3] != 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend([
            pixel[2],
            pixel[1],
            pixel[0],
            if has_alpha { pixel[3] } else { 255 },
        ]);
    }
    Some(rgba)
}

#[cfg(windows)]
fn extract_shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    extract_icon_rgba_from_source(target, None)
}

fn is_executable_icon_target(target: &str) -> bool {
    matches!(
        std::path::Path::new(target.trim().trim_matches('"'))
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("exe") | Some("com") | Some("bat") | Some("cmd")
    )
}

#[cfg(windows)]
fn extract_icon_rgba_from_source(source: &str, icon_index: Option<i32>) -> Option<Vec<u8>> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        ExtractIconExW, SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON};

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
    let icon = if let Some(index) = icon_index {
        let mut icon = HICON::default();
        let extracted = unsafe {
            ExtractIconExW(
                PCWSTR(path.as_ptr()),
                index,
                Some(&mut icon as *mut HICON),
                None,
                1,
            )
        };
        (extracted > 0 && !icon.is_invalid()).then_some(icon)?
    } else {
        let mut file_info = SHFILEINFOW::default();
        let flags = SHGFI_FLAGS(SHGFI_ICON.0 | SHGFI_LARGEICON.0);
        let result = unsafe {
            SHGetFileInfoW(
                PCWSTR(path.as_ptr()),
                Default::default(),
                Some(&mut file_info),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        (result != 0 && !file_info.hIcon.is_invalid()).then_some(file_info.hIcon)?
    };

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe { DestroyIcon(icon).ok()? };
        return None;
    }

    let mut bits: *mut c_void = null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let bitmap =
        unsafe { CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) };
    let Ok(bitmap) = bitmap else {
        unsafe {
            DestroyIcon(icon).ok();
            let _ = DeleteDC(hdc);
        }
        return None;
    };
    let previous = unsafe { SelectObject(hdc, HGDIOBJ(bitmap.0)) };
    let drawn =
        unsafe { DrawIconEx(hdc, 0, 0, icon, ICON_SIZE, ICON_SIZE, 0, None, DI_NORMAL).is_ok() };
    let rgba = if drawn && !bits.is_null() {
        let bgra = unsafe {
            std::slice::from_raw_parts(bits.cast::<u8>(), (ICON_SIZE * ICON_SIZE * 4) as usize)
        };
        let mut rgba = Vec::with_capacity(bgra.len());
        for pixel in bgra.chunks_exact(4) {
            rgba.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        Some(rgba)
    } else {
        None
    };
    unsafe {
        SelectObject(hdc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(hdc);
        DestroyIcon(icon).ok();
    }
    rgba
}

fn launcher_theme() -> Theme {
    let mut theme = Theme::dark();
    theme.palette.bg = Color::rgba(0, 0, 0, 0);
    theme.palette.surface = Color::rgba(38, 39, 41, 180);
    theme.palette.surface_alt = Color::rgba(48, 49, 51, 205);
    theme.palette.border = Color::rgba(255, 255, 255, 22);
    // The Search control is transparent, so its foreground must stay readable
    // over both dark and light Acrylic samples. Keep ordinary text neutral and
    // opaque; reserve accent blue for selection/focus feedback only.
    theme.palette.text = Color::rgba(250, 252, 255, 255);
    theme.palette.placeholder = Color::rgba(238, 243, 255, 230);
    theme.input.bg = Some(Color::rgba(29, 30, 32, 188));
    theme.input.border = Some(Color::rgba(255, 255, 255, 24));
    theme.input.border_focus = Some(Color::rgba(133, 181, 255, 135));
    theme.input.text = Some(Color::rgba(250, 252, 255, 255));
    theme.input.placeholder = Some(Color::rgba(238, 243, 255, 230));
    theme.input.selection = Some(Color::rgba(76, 139, 245, 150));
    theme.input.cursor = Some(Color::rgba(255, 255, 255, 255));
    theme
}

fn settings_save_lock() -> &'static Mutex<()> {
    SETTINGS_SAVE_LOCK.get_or_init(|| Mutex::new(()))
}

fn save_settings(settings: &Settings) -> bool {
    let Ok(_save_guard) = settings_save_lock().lock() else {
        return false;
    };
    settings.save().is_ok()
}

fn request_update_check(
    sender: Sender<updater::UpdateCheckResponse>,
    in_flight: &Cell<bool>,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_check(sender);
    true
}

fn spawn_update_check(sender: Sender<updater::UpdateCheckResponse>) {
    let _ = std::thread::Builder::new()
        .name(String::from("flux-update-check"))
        .spawn(move || {
            let checked_at = updater::unix_now();
            let result = updater::check_stable(CURRENT_VERSION);
            let _ = sender.send(updater::UpdateCheckResponse { checked_at, result });
        });
}

#[derive(Clone, Debug)]
enum UpdateInstallResponse {
    Progress {
        version: String,
        progress: updater::DownloadProgress,
    },
    Started {
        version: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

fn request_update_install(
    update: updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    in_flight: &Cell<bool>,
    relaunch_mode: updater::RelaunchMode,
) -> bool {
    if in_flight.replace(true) {
        return false;
    }
    spawn_update_install(update, sender, relaunch_mode);
    true
}

fn spawn_update_install(
    update: updater::StableUpdate,
    sender: Sender<UpdateInstallResponse>,
    relaunch_mode: updater::RelaunchMode,
) {
    let _ = std::thread::Builder::new()
        .name(String::from("flux-update-install"))
        .spawn(move || {
            let version = update.version.to_string();
            trace_update_event(&format!("update-install-start\\t{version}"));
            let installer_path =
                std::env::temp_dir().join(format!("FluxLauncher-update-{}.exe", update.version));
            let version_for_progress = version.clone();
            let progress_sender = sender.clone();
            let download =
                updater::download_installer_to_path(&update, &installer_path, move |progress| {
                    trace_update_event(&format!(
                        "update-progress\t{}\t{}\t{:?}",
                        version_for_progress, progress.received_bytes, progress.total_bytes
                    ));
                    let _ = progress_sender.send(UpdateInstallResponse::Progress {
                        version: version_for_progress.clone(),
                        progress,
                    });
                });
            match download {
                Ok(_) => match updater::handoff_installer(&installer_path, relaunch_mode) {
                    Ok(()) => {
                        trace_update_event(&format!("update-installer-started\\t{version}"));
                        let _ = sender.send(UpdateInstallResponse::Started { version });
                    }
                    Err(error) => {
                        trace_update_event(&format!("update-failed\\t{version}\\t{error}"));
                        let _ = std::fs::remove_file(&installer_path);
                        let _ = sender.send(UpdateInstallResponse::Failed { version, error });
                    }
                },
                Err(error) => {
                    trace_update_event(&format!("update-failed\\t{version}\\t{error}"));
                    let _ = sender.send(UpdateInstallResponse::Failed { version, error });
                }
            }
        });
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn trace_update_event(event: &str) {
    let Some(path) = std::env::var_os("FLUX_UPDATE_TRACE_FILE") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write as _;
        let _ = writeln!(file, "{event}");
    }
}

fn update_check_due(settings: &Settings) -> bool {
    let forced = std::env::var("FLUX_FORCE_UPDATE_CHECK")
        .map(|value| value == "1")
        .unwrap_or(false);
    forced
        || updater::should_check(
            updater::unix_now(),
            settings.last_update_check_unix,
            settings.update_interval_hours,
        )
}

fn format_update_progress(version: &str, progress: &updater::DownloadProgress) -> String {
    match progress.total_bytes.filter(|total| *total > 0) {
        Some(total) => {
            let received = progress.received_bytes.min(total);
            let percent = received.saturating_mul(100) / total;
            let remaining = total.saturating_sub(received);
            format!(
                "Downloading stable {version}: {percent}% — {} / {} ({} remaining)",
                format_bytes(received),
                format_bytes(total),
                format_bytes(remaining)
            )
        }
        None => format!(
            "Downloading stable {version}: {} received",
            format_bytes(progress.received_bytes)
        ),
    }
}

fn save_settings_async(settings: &Arc<RwLock<Settings>>) {
    let settings = Arc::clone(settings);
    let _ = std::thread::Builder::new()
        .name(String::from("flux-settings-save"))
        .spawn(move || {
            // Read the latest settings snapshot after waiting for any mutation.
            if let Ok(settings_guard) = settings.read() {
                let _ = save_settings(&settings_guard);
            }
        });
}

fn record_query_history(
    settings: &Arc<RwLock<Settings>>,
    history: &Rc<RefCell<Vec<String>>>,
    query: &str,
) {
    let Ok(mut settings_guard) = settings.write() else {
        return;
    };
    if !settings_guard.record_query(query) {
        return;
    }
    *history.borrow_mut() = settings_guard.query_history.clone();
    drop(settings_guard);
    // Keep Enter→hide free of synchronous filesystem I/O.
    save_settings_async(settings);
}

fn set_result_priority(
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

fn remove_priority_entry(
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

fn move_priority_entry(
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

fn set_game_mode(
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

#[derive(Default)]
struct ActionBarGeometryProbe {
    last: Cell<Option<(i32, i32, i32, i32)>>,
}

impl Widget for ActionBarGeometryProbe {
    fn paint(
        &self,
        bounds: Rect,
        _content: Rect,
        _focused: bool,
        _enabled: bool,
        _canvas: &mut dyn Canvas,
        _style: &Style,
    ) {
        if std::env::var_os("FLUX_SMOKE_ACTION_BAR").is_none() {
            return;
        }
        let geometry = (bounds.x, bounds.y, bounds.w, bounds.h);
        if self.last.get() != Some(geometry) {
            eprintln!(
                "ActionBarGeometry: x={} y={} width={} height={}",
                geometry.0, geometry.1, geometry.2, geometry.3
            );
            self.last.set(Some(geometry));
        }
    }
}

fn icon_target_for_path(target: &str) -> String {
    resolve_bare_executable_path(target).unwrap_or_else(|| target.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn result_row(
    result: SearchResult,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    icon_refresh_generation: Signal<u64>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    query: Signal<String>,
    scroll_pending: Signal<bool>,
    selection_color: Signal<Color>,
    settings: Arc<RwLock<Settings>>,
    query_history: Rc<RefCell<Vec<String>>>,
    history_mode: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    settings_visible: Signal<bool>,
    window_size_slot: Rc<RefCell<Option<WindowSizeHandle>>>,
) -> Element {
    let id = result.id;
    let target = result.target;
    let title = result.title;
    let subtitle = result.subtitle;
    let icon_target = target.as_deref().map(icon_target_for_path);
    let (glyph, glyph_font) = match id.as_str() {
        "empty-recycle-bin" => (String::from("\u{ea99}"), "Segoe Fluent Icons"),
        "open-recycle-bin" => (String::from("\u{e74d}"), "Segoe Fluent Icons"),
        _ if subtitle.contains("Application") => (String::from("◉"), LAUNCHER_FONT_FAMILY),
        _ => (String::from("▣"), LAUNCHER_FONT_FAMILY),
    };
    let icon =
        bundled_icon_rgba(&id).or_else(|| icon_target.as_deref().and_then(request_shell_icon));
    trace_result_icon_probe(
        &title,
        target.as_deref(),
        icon_target.as_deref(),
        icon.is_some(),
    );
    let icon_element = Element::leaf()
        .widget(ResultIconView::new(
            icon_target,
            glyph,
            glyph_font,
            icon,
            icon_refresh_generation,
        ))
        .reactive()
        .width(32)
        .height(32)
        .corner(7.0);
    let selected = selected_id.get() == id;
    let title_doc_signal = signal(title_match_doc(&title, &query.get()));
    let trailing_signal = signal(if selected {
        String::from("↵")
    } else {
        String::new()
    });
    Element::row()
        .widget(ResultRowAnchor {
            result_id: id.clone(),
            title: title.clone(),
            title_doc_signal,
            trailing_signal,
            selected_id,
            selected_index,
            selection_touched,
            rows_refresh,
            query,
            scroll_pending,
            selection_color,
            on_click: None,
            pressed: false,
            last_pointer: None,
            last_selected: None,
            last_query: query.get(),
        })
        .reactive()
        .width_match()
        .height(46)
        .padding_xy(12, 3)
        .spacing(10)
        .corner(10.0)
        // Selection background is owned exclusively by ResultRowAnchor. Keeping
        // a static background here leaves stale highlights after selection moves.
        .child(icon_element)
        .child(
            Element::col()
                .weight(1.0)
                .spacing(1)
                .child(
                    Element::rich_signal(title_doc_signal)
                        .font_family(LAUNCHER_FONT_FAMILY)
                        .font_size(14.0)
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                )
                .child(
                    Element::label(subtitle)
                        .font_family(LAUNCHER_FONT_FAMILY)
                        .font_size(12.0)
                        .fg(Color::rgba(248, 251, 255, 255))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                ),
        )
        .child(
            Element::label_signal(trailing_signal)
                .font_size(17.0)
                .fg(Color::rgba(238, 246, 255, 230))
                .width(22)
                .align(Align::Center),
        )
        .on_click(move |ctx| {
            if history_mode.get() {
                query.set(title.clone());
                history_mode.set(false);
                return;
            }
            record_query_history(&settings, &query_history, &query.get());
            selected_id.set(id.clone());
            selection_touched.set(true);
            if let Some(index) = rows_refresh.get().iter().position(|result| result.id == id) {
                selected_index.set(index);
            }
            if id == "empty-recycle-bin" {
                recycle_bin_confirmation.set(true);
                return;
            }
            if id == "flux-settings" {
                settings_visible.set(true);
                if let Some(window_size) = window_size_slot.borrow().as_ref() {
                    window_size.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                }
                return;
            }
            if id == "open-recycle-bin" {
                launch::open_recycle_bin_async();
                ctx.hide_window();
                return;
            }
            if let Some(target) = target.as_deref() {
                launch::open_path_async(target);
                ctx.hide_window();
                return;
            }
            if let Some(action) = plugin_actions.borrow().get(&id).cloned() {
                plugins::execute_async(action);
                ctx.hide_window();
            }
        })
}

fn main() {
    let mut args = std::env::args_os();
    let _executable = args.next();
    let mode = args.next();
    if mode.as_deref() == Some(std::ffi::OsStr::new("--visual-preview")) {
        let mut values = [0_i32; 4];
        for value in &mut values {
            let Some(raw) = args.next() else {
                eprintln!("visual preview requires width height x y");
                std::process::exit(2);
            };
            let Ok(parsed) = raw.to_string_lossy().parse::<i32>() else {
                eprintln!("visual preview dimensions and position must be integers");
                std::process::exit(2);
            };
            *value = parsed;
        }
        visual_preview::run(values[0], values[1], values[2], values[3]);
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--plugin-host")) {
        let root = args
            .next()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("FLUX_NATIVE_PLUGIN_DIR").map(std::path::PathBuf::from))
            .unwrap_or_else(|| std::path::PathBuf::from("NativePlugins"));
        let pipe_name = args
            .next()
            .map(|value| value.to_string_lossy().into_owned());
        native_host::run(root, pipe_name);
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--folder-launch-smoke")) {
        if let Some(target) = args.next() {
            launch::open_path_async(&target.to_string_lossy());
            std::thread::sleep(Duration::from_millis(900));
        }
        return;
    }
    if mode.as_deref() == Some(std::ffi::OsStr::new("--shortcut-icon-smoke")) {
        #[cfg(windows)]
        {
            let Some(target) = args.next() else {
                eprintln!("shortcut icon smoke requires a shortcut path");
                std::process::exit(2);
            };
            if !shortcut_icon_smoke(&target.to_string_lossy()) {
                eprintln!(
                    "shortcut icon extraction failed for {}",
                    target.to_string_lossy()
                );
                std::process::exit(1);
            }
        }
        return;
    }
    let single_instance_disabled = std::env::var_os("FLUX_DISABLE_SINGLE_INSTANCE").is_some();
    if !single_instance_disabled
        && should_claim_single_instance(mode.as_deref())
        && matches!(
            windui::claim_instance(SINGLE_INSTANCE_ID),
            windui::InstanceRole::Handoff
        )
    {
        return;
    }
    // The uninstaller uses this one-shot mode only to reach the already-running
    // instance through the single-instance listener. Never create a new UI if
    // there is no instance left to shut down.
    if is_shutdown_mode(mode.as_deref()) {
        return;
    }
    let startup_launch = mode.as_deref() == Some(std::ffi::OsStr::new("--startup"));

    let settings = Settings::load_or_default();
    if let Err(error) = startup::set_enabled(settings.start_with_windows) {
        eprintln!("Could not synchronize Windows startup setting: {error}");
    }
    let activation_hotkey = hotkeys::activation_hotkey(&settings.activation_hotkey);
    let shared_settings = Arc::new(RwLock::new(settings.clone()));
    let query_history = Rc::new(RefCell::new(settings.query_history.clone()));
    let priorities = signal(settings.priority_entries.clone());
    let history_cursor = signal(None::<usize>);
    let history_mode = signal(false);

    let query = signal(String::new());
    let selected_id = signal(String::new());
    let selected_index = signal(0_usize);
    let selection_touched = signal(false);
    let action_mode = signal(false);
    let action_index = signal(0_usize);
    let recycle_bin_confirmation = signal(false);
    let action_items = signal(Vec::<ActionItem>::new());
    let action_window_slot = Rc::new(RefCell::new(None::<WindowSizeHandle>));
    let status = signal(String::from("Ready"));
    let update_status = signal(String::from("Stable updates are checked automatically"));
    let update_available = signal(None::<updater::StableUpdate>);
    let update_install_progress = signal(None::<(String, updater::DownloadProgress)>);
    let update_installing = signal(false);
    let current_sequence = signal(0_u64);
    let game_mode = signal(settings.game_mode);
    let game_mode_status = signal(game_mode_label(settings.game_mode));
    let settings_visible = signal(std::env::var_os("FLUX_OPEN_SETTINGS").is_some());
    let settings_tab = signal(
        std::env::var("FLUX_SMOKE_SETTINGS_TAB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|tab| *tab < 4)
            .unwrap_or(0),
    );
    let tray_settings_smoke_pending = Rc::new(Cell::new(
        std::env::var_os("FLUX_SMOKE_TRAY_SETTINGS").is_some(),
    ));
    let show_results = signal(false);
    let activation_key = signal(settings.activation_hotkey.key.clone());
    let activation_display = signal(hotkeys::display_config(&settings.activation_hotkey));
    let activation_recording = signal(false);
    let activation_ctrl = signal(settings.activation_hotkey.ctrl);
    let activation_alt = signal(settings.activation_hotkey.alt);
    let activation_shift = signal(settings.activation_hotkey.shift);
    let activation_meta = signal(settings.activation_hotkey.meta);
    let ignore_fullscreen = signal(settings.ignore_hotkeys_in_fullscreen);
    let smooth_caret = signal(settings.smooth_caret);
    let switch_to_english_layout = signal(settings.switch_to_english_layout);
    let use_system_accent = signal(settings.use_system_accent);
    let custom_selection_color = signal(selection_color_hex(settings.custom_selection_color));
    let launcher_width = signal(settings.launcher_width);
    let launcher_height = signal(settings.launcher_height);
    let launcher_width_input = signal(settings.launcher_width.to_string());
    let launcher_height_input = signal(settings.launcher_height.to_string());
    let launcher_width_slider = signal(dimension_slider_fraction(
        settings.launcher_width,
        MIN_LAUNCHER_WIDTH,
        MAX_LAUNCHER_WIDTH,
    ));
    let launcher_height_slider = signal(dimension_slider_fraction(
        settings.launcher_height,
        MIN_LAUNCHER_HEIGHT,
        MAX_LAUNCHER_HEIGHT,
    ));
    let launcher_preview_text = signal(format!(
        "Current launcher client area: {} × {} logical px (DIP)",
        settings.launcher_width, settings.launcher_height
    ));
    let visual_preview_generation = signal(0_u64);
    let clear_query_on_activation = signal(settings.clear_query_on_activation);
    let start_with_windows = signal(settings.start_with_windows);
    let auto_enable_everything = signal(settings.auto_enable_everything);
    let update_checks_enabled = signal(settings.update_checks_enabled);
    let update_interval_hours = signal(settings.update_interval_hours.to_string());
    let auto_install_updates = signal(settings.auto_install_updates);
    let obsidian_enabled = signal(settings.obsidian_enabled);
    let obsidian_alias = signal(settings.obsidian_alias.clone());
    let google_enabled = signal(settings.google_enabled);
    let google_alias = signal(settings.google_alias.clone());
    let initial_monitor_preference = std::env::var("FLUX_SMOKE_MONITOR_PREFERENCE")
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "primary" => Some(MonitorPreference::Primary),
            "cursor" => Some(MonitorPreference::Cursor),
            "foreground" => Some(MonitorPreference::Foreground),
            _ => None,
        })
        .unwrap_or(settings.monitor_preference);
    let monitor_preference = signal(monitor_preference_index(initial_monitor_preference));
    let initial_everything_state = everything::installation_state();
    let everything_installed = signal(initial_everything_state.is_installed());
    let everything_prompt_visible = signal(false);
    let everything_status = signal(if everything_installed.get() {
        String::from("Everything detected; Flux will enable IPC automatically")
    } else {
        String::from("Everything is not installed. Install it with winget to enable file search.")
    });
    let selection_color = signal(selection_color_for_settings(&settings));
    let caret_duration = signal(settings.smooth_caret_duration_ms.to_string());

    let mut model = SearchModel::new();
    let results = signal(model.results().to_vec());
    let provider_results = Rc::new(RefCell::new(ProviderResults::default()));
    let plugin_actions = Rc::new(RefCell::new(HashMap::<String, PluginAction>::new()));
    let result_source = results;
    let selected_for_rows = selected_id;
    let selected_index_for_rows = selected_index;
    let selection_touched_for_rows = selection_touched;
    let actions_for_rows = Rc::clone(&plugin_actions);
    let settings_for_rows = Arc::clone(&shared_settings);
    let history_for_rows = Rc::clone(&query_history);
    let history_mode_for_rows = history_mode;
    let action_items_for_rows = action_items;
    let action_index_for_rows = action_index;
    let action_mode_for_rows = action_mode;
    let action_window_slot_for_rows = Rc::clone(&action_window_slot);
    let query_for_rows = query;
    let scroll_request_for_rows = signal(false);
    let icon_refresh_generation = signal(SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire));
    let settings_visible_for_rows = settings_visible;
    let window_size_slot_for_rows = Rc::clone(&action_window_slot);
    let inline_completion = signal(String::new());

    let search_box = Element::text_input(query, "Search")
        .leading_icon('⌕')
        .transparent_surface()
        .smooth_caret(settings.smooth_caret, settings.smooth_caret_duration_ms)
        .inline_completion(inline_completion)
        .show_focus_ring(false)
        .width_match()
        .font_family(LAUNCHER_FONT_FAMILY)
        .font_size(15.0)
        .font_weight(500)
        .corner(10.0)
        // The entire Search control stays transparent so the Windows Acrylic
        // material remains visible through the input, caret, and leading icon.
        .border(Color::rgba(0, 0, 0, 0), 0)
        .padding_xy(13, 0);

    let action_hint = |key: &'static str, label: &'static str| {
        Element::row()
            .height(22)
            .cross(Align::Center)
            .spacing(4)
            .child(
                Element::label(key)
                    .font_size(9.0)
                    .fg(Color::rgba(235, 243, 255, 235))
                    .bg(Color::rgba(255, 255, 255, 24))
                    .corner(5.0)
                    .padding_xy(4, 2),
            )
            .child(
                Element::label(label)
                    .font_size(10.0)
                    .fg(Color::rgba(222, 233, 248, 220)),
            )
    };
    // Use a bounded frame plus an explicitly centered content row instead of
    // full-width spacer children. This keeps the three hints visually centered
    // between the launcher content insets while the window width changes.
    let action_bar_content = Element::row()
        .height(22)
        .spacing(8)
        .child(action_hint("↵", "Open"))
        .child(action_hint("Ctrl + R", "Run as admin"))
        .child(action_hint("Alt + Enter", "Open file location"));
    let action_bar = Element::stack()
        .width(ACTION_BAR_WIDTH)
        .height(ACTION_BAR_HEIGHT)
        .child(action_bar_content.align(Align::Center))
        // Keep the probe inside the same real frame so its telemetry describes
        // the exact slot that is centered between the launcher insets.
        .child(
            Element::leaf()
                .widget(ActionBarGeometryProbe::default())
                .fill(),
        )
        .align(Align::Center)
        .visible_when(move || show_results.get() && !action_mode.get());

    let result_list_body = Element::host_signal(result_source, move |result| {
        result_row(
            result,
            selected_for_rows,
            selected_index_for_rows,
            selection_touched_for_rows,
            result_source,
            icon_refresh_generation,
            Rc::clone(&actions_for_rows),
            query_for_rows,
            scroll_request_for_rows,
            selection_color,
            Arc::clone(&settings_for_rows),
            Rc::clone(&history_for_rows),
            history_mode_for_rows,
            recycle_bin_confirmation,
            settings_visible_for_rows,
            Rc::clone(&window_size_slot_for_rows),
        )
    })
    .width_match()
    // Keep the result body transparent so the window remains one continuous
    // Acrylic surface. Only individual result rows draw controls. The extra
    // right inset is local to the scroll content: it keeps the thumb clear of
    // row cards without changing the launcher window width.
    .padding_edges(6, 6, 18, 6);
    let result_list = Element::scroll()
        .width_match()
        .height(RESULT_VIEWPORT_HEIGHT)
        .child(result_list_body)
        .visible_when(move || show_results.get() && !action_mode.get());

    let everything_prompt_for_close = everything_prompt_visible;
    let everything_prompt_for_decline = everything_prompt_visible;
    let everything_prompt_for_install = everything_prompt_visible;
    let everything_status_for_prompt = everything_status;
    let settings_for_everything_prompt_close = Arc::clone(&shared_settings);
    let settings_for_everything_prompt_decline = Arc::clone(&shared_settings);
    let settings_for_everything_prompt_install = Arc::clone(&shared_settings);
    let everything_install_prompt = Element::dialog_panel(
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
            ),
    );

    let confirmation_for_close = recycle_bin_confirmation;
    let confirmation_for_cancel = recycle_bin_confirmation;
    let confirmation_for_empty = recycle_bin_confirmation;
    let status_for_confirmation = status;
    let recycle_bin_dialog = Element::dialog_panel(
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
    );

    let settings_for_action_list = Arc::clone(&shared_settings);
    let priorities_for_action_list = priorities;
    let providers_for_action_list = Rc::clone(&provider_results);
    let query_for_action_list = query;
    let action_list = Element::list_signal(
        action_items_for_rows,
        |item| item.id.clone(),
        move |item| {
            let item_id = item.id.clone();
            let item_label = item.label.clone();
            let item_kind = item.kind.clone();
            let settings_for_item_action = Arc::clone(&settings_for_action_list);
            let priorities_for_item_action = priorities_for_action_list;
            let providers_for_item_action = Rc::clone(&providers_for_action_list);
            let query_for_item_action = query_for_action_list;
            let is_selected = action_items_for_rows
                .get()
                .iter()
                .position(|candidate| candidate.id == item_id)
                .map(|index| index == action_index_for_rows.get())
                .unwrap_or(false);
            Element::row()
                .width_match()
                .height(36)
                .padding_xy(10, 4)
                .corner(9.0)
                .bg(if is_selected {
                    Color::rgba(76, 139, 245, 92)
                } else {
                    Color::rgba(255, 255, 255, 14)
                })
                .child(
                    Element::label(item_label)
                        .font_size(13.0)
                        .fg(Color::rgba(250, 252, 255, 255))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                )
                .on_click({
                    let action_window_slot = action_window_slot_for_rows.clone();
                    move |ctx| {
                        let executed = selected_result(
                            &result_source.get(),
                            &selected_for_rows.get(),
                            selected_index_for_rows.get(),
                        )
                        .is_some_and(|result| {
                            if matches!(item_kind, ActionKind::SetPriority) {
                                let saved = set_result_priority(
                                    &settings_for_item_action,
                                    priorities_for_item_action,
                                    &result,
                                );
                                if saved {
                                    refresh_merged_results(
                                        &providers_for_item_action,
                                        query_for_item_action,
                                        priorities_for_item_action,
                                        result_source,
                                    );
                                }
                                saved
                            } else {
                                execute_result_action(&result, &item_kind)
                            }
                        });
                        if executed {
                            ctx.hide_window();
                        }
                        action_mode_for_rows.set(false);
                        if let Some(handle) = action_window_slot.borrow().as_ref() {
                            handle.set(
                                i32::from(launcher_width.get()),
                                i32::from(launcher_height.get()),
                            );
                        }
                    }
                })
        },
    )
    .height(174)
    .corner(12.0)
    .visible_signal(action_mode);

    // The HWND itself owns the system Acrylic surface. Keep this root transparent so
    // the blur fills the complete client area instead of becoming an inset card. The
    // content must match the live window width so result rows expand with resizing.
    // Keep the empty search strip and the results palette intrinsically sized. A
    // full-height column plus a weighted spacer made the compact state look too
    // tall and left an oversized gap between the last result and the footer.
    // Keep the compact Search baseline genuinely centered: equal vertical
    // insets avoid moving the empty-state control toward either edge.
    let launcher_content = Element::col()
        .width_match()
        .padding_edges(10, 7, 10, 7)
        .spacing(4)
        .child(search_box)
        .child(result_list)
        // The result viewport now ends immediately before the fixed footer. Do not
        // add a weighted spacer: it creates a visible blank band for short queries.
        .child(action_bar)
        .child(action_list)
        .child(recycle_bin_dialog)
        .child(everything_install_prompt);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Center));

    let query_for_interval = query;
    let results_for_interval = results;
    let width_for_interval = launcher_width;
    let height_for_interval = launcher_height;
    let width_input_for_interval = launcher_width_input;
    let height_input_for_interval = launcher_height_input;
    let width_slider_for_interval = launcher_width_slider;
    let height_slider_for_interval = launcher_height_slider;
    let preview_text_for_interval = launcher_preview_text;
    let icon_refresh_generation_for_interval = icon_refresh_generation;
    let status_for_interval = status;
    let show_results_for_interval = show_results;
    let inline_completion_for_interval = inline_completion;
    let selection_touched_for_interval = selection_touched;
    let sequence_for_interval = current_sequence;
    let providers_for_interval = Rc::clone(&provider_results);
    let scroll_request_for_interval = scroll_request_for_rows;
    let actions_for_interval = Rc::clone(&plugin_actions);
    let auto_enable_everything_for_interval = auto_enable_everything;
    let obsidian_enabled_for_interval = obsidian_enabled;
    let obsidian_alias_for_interval = obsidian_alias;
    let google_enabled_for_interval = google_enabled;
    let google_alias_for_interval = google_alias;
    let history_mode_for_interval = history_mode;
    let settings_visible_for_interval = settings_visible;
    let settings_tab_for_interval = settings_tab;
    let visual_preview_generation_for_interval = visual_preview_generation;
    let visual_preview_smoke_for_interval =
        std::env::var_os("FLUX_SMOKE_VISUAL_SETTINGS").is_some();
    let tray_settings_smoke_pending_for_interval = Rc::clone(&tray_settings_smoke_pending);
    let mut last_icon_generation = icon_refresh_generation.get();
    let mut last_launcher_width = launcher_width.get();
    let mut last_launcher_height = launcher_height.get();
    let mut last_settings_visible = settings_visible.get();
    let mut last_query = String::new();
    let mut visual_preview_process: Option<visual_preview::PreviewProcess> = None;
    let mut last_visual_preview_request: Option<(u16, u16)> = None;
    let mut last_visual_preview_generation = visual_preview_generation.get();
    let mut last_visual_control_state: Option<(u16, u16, u32, u32)> = None;
    let mut sequence = 0_u64;

    let settings_at_start = settings_visible.get();
    let initial_height = if settings_at_start {
        SETTINGS_WINDOW_HEIGHT
    } else {
        COMPACT_WINDOW_HEIGHT
    };
    let initial_width = if settings_at_start {
        SETTINGS_WINDOW_WIDTH
    } else {
        launcher_width.get() as i32
    };
    let window_icon = tray_icon();
    let mut app =
        App::new("Flux Launcher", initial_width, initial_height).icon_rgba(16, 16, &window_icon);
    if let Some((x, y)) =
        monitor::centered_position(initial_monitor_preference, initial_width, initial_height)
    {
        app = app.position(x, y);
    }
    let window_size = app.window_size_handle();
    let window_position = app.window_position_handle();
    let position_for_interval = window_position.clone();
    let settings_for_interval_geometry = Arc::clone(&shared_settings);
    let window_op: WindowOpHandle = app.window_op_handle();
    let cursor_visibility: CursorVisibilityHandle = app.cursor_visibility_handle();
    let update_status_for_channel = update_status;
    let update_available_for_channel = update_available;
    let update_install_progress_for_channel = update_install_progress;
    let update_installing_for_channel = update_installing;
    let update_install_in_flight = Rc::new(Cell::new(false));
    let update_install_in_flight_for_channel = Rc::clone(&update_install_in_flight);
    let update_install_sender =
        app.channel::<UpdateInstallResponse>(move |ctx, response| match response {
            UpdateInstallResponse::Progress { version, progress } => {
                update_install_progress_for_channel.set(Some((version.clone(), progress.clone())));
                update_status_for_channel.set(format_update_progress(&version, &progress));
            }
            UpdateInstallResponse::Started { version } => {
                update_install_in_flight_for_channel.set(false);
                update_installing_for_channel.set(false);
                update_install_progress_for_channel.set(None);
                update_status_for_channel.set(format!(
                    "Installing stable {version}; Flux Launcher is restarting"
                ));
                ctx.toast_ok(format!("Installing stable {version}"));
                ctx.quit();
            }
            UpdateInstallResponse::Failed { version, error } => {
                update_install_in_flight_for_channel.set(false);
                update_installing_for_channel.set(false);
                update_install_progress_for_channel.set(None);
                update_status_for_channel.set(format!("Stable {version} update failed: {error}"));
                ctx.toast_ok(format!("Update install failed: {error}"));
            }
        });
    let update_install_sender_for_channel = update_install_sender.clone();
    let settings_for_update_channel = Arc::clone(&shared_settings);
    let update_check_in_flight = Rc::new(Cell::new(false));
    let update_check_in_flight_for_channel = Rc::clone(&update_check_in_flight);
    let update_install_in_flight_for_check_channel = Rc::clone(&update_install_in_flight);
    let update_sender = app.channel::<updater::UpdateCheckResponse>(move |ctx, response| {
        update_check_in_flight_for_channel.set(false);
        if let Ok(mut settings) = settings_for_update_channel.write() {
            settings.last_update_check_unix = response.checked_at;
            let _ = save_settings(&settings);
        }
        match response.result {
            Ok(Some(update)) => {
                let message = format!("Stable {} is available", update.version);
                update_status_for_channel.set(message.clone());
                update_available_for_channel.set(Some(update.clone()));
                let auto_install = settings_for_update_channel
                    .read()
                    .map(|settings| settings.auto_install_updates)
                    .unwrap_or(false);
                if auto_install {
                    let relaunch_mode = relaunch_mode_for_auto_install();
                    update_installing_for_channel.set(true);
                    update_status_for_channel.set(format!(
                        "Preparing stable {} for installation...",
                        update.version
                    ));
                    if !request_update_install(
                        update,
                        update_install_sender_for_channel.clone(),
                        &update_install_in_flight_for_check_channel,
                        relaunch_mode,
                    ) {
                        update_installing_for_channel.set(false);
                        update_status_for_channel
                            .set(String::from("An update is already being installed"));
                    }
                } else {
                    ctx.toast_ok(message);
                }
            }
            Ok(None) => {
                update_available_for_channel.set(None);
                update_status_for_channel.set(format!("Flux {CURRENT_VERSION} is up to date"));
            }
            Err(error) => {
                update_status_for_channel.set(format!("Stable update check failed: {error}"));
            }
        }
    });
    let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
        .map(|value| value != "1")
        .unwrap_or(true);
    if update_checks_allowed && settings.update_checks_enabled && update_check_due(&settings) {
        request_update_check(update_sender.clone(), &update_check_in_flight);
    }
    let settings_for_update_interval = Arc::clone(&shared_settings);
    let update_sender_for_interval = update_sender.clone();
    let update_check_in_flight_for_interval = Rc::clone(&update_check_in_flight);
    *action_window_slot.borrow_mut() = Some(window_size.clone());
    let size_for_interval = window_size.clone();
    let size_for_visibility = window_size.clone();
    let query_for_applications = query;
    let results_for_applications = results;
    let inline_completion_for_applications = inline_completion;
    let status_for_applications = status;
    let selected_id_for_applications = selected_id;
    let selected_index_for_applications = selected_index;
    let selection_touched_for_applications = selection_touched;
    let sequence_for_applications = current_sequence;
    let providers_for_applications = Rc::clone(&provider_results);
    let priorities_for_applications = priorities;
    let application_sender = app.channel::<ApplicationResponse>(move |_, response| {
        if response.sequence != sequence_for_applications.get()
            || response.query != query_for_applications.get()
        {
            return;
        }
        let mut providers = providers_for_applications.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.applications = response.results;
        providers.applications_ready = true;
        if providers.core_ready() {
            let priorities = priorities_for_applications
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_applications.get(),
                &priorities,
                selected_id_for_applications,
                selected_index_for_applications,
                selection_touched_for_applications,
                inline_completion_for_applications,
                results_for_applications,
            );
        }
        status_for_applications.set(response.status);
    });
    let application_worker = ApplicationWorker::spawn(application_sender);

    let query_for_everything = query;
    let results_for_everything = results;
    let inline_completion_for_everything = inline_completion;
    let status_for_everything = status;
    let selected_id_for_everything = selected_id;
    let selected_index_for_everything = selected_index;
    let selection_touched_for_everything = selection_touched;
    let sequence_for_everything = current_sequence;
    let providers_for_everything = Rc::clone(&provider_results);
    let priorities_for_everything = priorities;
    let auto_enable_everything_for_response = auto_enable_everything;
    let everything_installed_for_response = everything_installed;
    let everything_status_for_response = everything_status;
    let everything_sender = app.channel::<EverythingResponse>(move |_, response| {
        if !auto_enable_everything_for_response.get() {
            everything_status_for_response.set(String::from(
                "Everything auto-enable is disabled in Flux settings",
            ));
            return;
        }
        if response.sequence != sequence_for_everything.get()
            || response.query != normalize_everything_query(&query_for_everything.get())
        {
            return;
        }
        let mut providers = providers_for_everything.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        providers.everything_ready = true;
        if response.available {
            everything_installed_for_response.set(true);
            everything_status_for_response.set(String::from("Everything IPC is available"));
            providers.everything = response.results;
        } else if everything_installed_for_response.get() {
            everything_status_for_response.set(String::from(
                "Everything is installed but its local IPC is unavailable",
            ));
        } else {
            everything_status_for_response.set(String::from(
                "Everything is not installed. Install it with winget to enable file search.",
            ));
        }
        if providers.core_ready() {
            let priorities = priorities_for_everything
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_everything.get(),
                &priorities,
                selected_id_for_everything,
                selected_index_for_everything,
                selection_touched_for_everything,
                inline_completion_for_everything,
                results_for_everything,
            );
        }
        status_for_everything.set(response.status);
    });
    let everything_worker = EverythingWorker::spawn(everything_sender);
    if settings.auto_enable_everything {
        match everything::start_background_if_installed() {
            Ok(InstallationState::Installed(_)) => {
                everything_installed.set(true);
                everything_status.set(String::from(
                    "Everything detected; Flux is enabling local IPC automatically",
                ));
            }
            Ok(InstallationState::Missing) => {
                everything_installed.set(false);
                everything_status.set(String::from(
                    "Everything is not installed. Install it with winget to enable file search.",
                ));
            }
            Err(error) => {
                everything_status.set(error);
            }
        }
    } else {
        everything_status.set(String::from(
            "Everything auto-enable is disabled in Flux settings",
        ));
    }
    if settings.auto_enable_everything
        && !everything_installed.get()
        && !settings.everything_install_prompt_seen
        && std::env::var("FLUX_DISABLE_EVERYTHING_PROMPT")
            .ok()
            .as_deref()
            != Some("1")
    {
        everything_prompt_visible.set(true);
    }

    let query_for_plugins = query;
    let results_for_plugins = results;
    let inline_completion_for_plugins = inline_completion;
    let status_for_plugins = status;
    let selected_id_for_plugins = selected_id;
    let selected_index_for_plugins = selected_index;
    let selection_touched_for_plugins = selection_touched;
    let sequence_for_plugins = current_sequence;
    let providers_for_plugins = Rc::clone(&provider_results);
    let priorities_for_plugins = priorities;
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
            if providers.core_ready() {
                let priorities = priorities_for_plugins
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>();
                commit_provider_results(
                    &providers,
                    &query_for_plugins.get(),
                    &priorities,
                    selected_id_for_plugins,
                    selected_index_for_plugins,
                    selection_touched_for_plugins,
                    inline_completion_for_plugins,
                    results_for_plugins,
                );
            }
        }
        status_for_plugins.set(response.status);
    });
    let plugin_worker = FlowPluginWorker::spawn(plugin_sender);

    let query_for_native_plugins = query;
    let results_for_native_plugins = results;
    let inline_completion_for_native_plugins = inline_completion;
    let status_for_native_plugins = status;
    let selected_id_for_native_plugins = selected_id;
    let selected_index_for_native_plugins = selected_index;
    let selection_touched_for_native_plugins = selection_touched;
    let sequence_for_native_plugins = current_sequence;
    let providers_for_native_plugins = Rc::clone(&provider_results);
    let priorities_for_native_plugins = priorities;
    let actions_for_native_plugins = Rc::clone(&plugin_actions);
    let native_sender = app.channel::<NativePluginQueryResponse>(move |_, response| {
        if response.sequence != sequence_for_native_plugins.get()
            || response.query != query_for_native_plugins.get()
        {
            return;
        }
        let mut providers = providers_for_native_plugins.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        let has_native_results = !response.results.is_empty();
        providers.native_plugins = response.results;
        if response.available {
            actions_for_native_plugins
                .borrow_mut()
                .extend(response.actions);
            if has_native_results {
                status_for_native_plugins.set(response.status.clone());
            }
        }
        if providers.core_ready() {
            let priorities = priorities_for_native_plugins
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>();
            commit_provider_results(
                &providers,
                &query_for_native_plugins.get(),
                &priorities,
                selected_id_for_native_plugins,
                selected_index_for_native_plugins,
                selection_touched_for_native_plugins,
                inline_completion_for_native_plugins,
                results_for_native_plugins,
            );
        }
    });
    let native_plugin_worker = NativePluginWorker::spawn(native_sender);

    let settings_for_activation = Arc::clone(&shared_settings);
    let position_for_activation = window_position.clone();
    let cursor_visibility_for_activation = cursor_visibility.clone();
    let size_for_activation = window_size.clone();
    let query_for_activation = query;
    let results_for_activation = results;
    let selected_id_for_activation = selected_id;
    let selected_index_for_activation = selected_index;
    let selection_touched_for_activation = selection_touched;
    let show_results_for_activation = show_results;
    let history_mode_for_activation = history_mode;
    let history_cursor_for_activation = history_cursor;
    let action_mode_for_activation = action_mode;
    let action_index_for_activation = action_index;
    let action_items_for_activation = action_items;
    let inline_completion_for_activation = inline_completion;
    let scroll_request_for_activation = scroll_request_for_rows;
    let settings_visible_for_activation = settings_visible;
    let activation_handle = app.hotkey_handle(activation_hotkey, move |ctx| {
        let settings = settings_for_activation
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
            // Clear before toggling visibility. The previous implementation did
            // this only from on_window_hide, which allowed the old query frame to
            // survive in the compositor until the next repaint after re-show.
            if settings.clear_query_on_activation {
                query_for_activation.set(String::new());
                results_for_activation.set(Vec::new());
                selected_id_for_activation.set(String::new());
                selected_index_for_activation.set(0);
                selection_touched_for_activation.set(false);
                show_results_for_activation.set(false);
                history_mode_for_activation.set(false);
                history_cursor_for_activation.set(None);
                action_mode_for_activation.set(false);
                action_index_for_activation.set(0);
                action_items_for_activation.set(Vec::new());
                inline_completion_for_activation.set(String::new());
                scroll_request_for_activation.set(false);
                let (compact_width, compact_height) = launcher_window_geometry_with_sizes(
                    settings_visible_for_activation.get(),
                    false,
                    i32::from(launcher_width.get()),
                    i32::from(launcher_height.get()),
                );
                size_for_activation.set(compact_width, compact_height);
            }
            let (width, height) = launcher_window_geometry_with_sizes(
                settings_visible_for_activation.get(),
                show_results_for_activation.get(),
                i32::from(launcher_width.get()),
                i32::from(launcher_height.get()),
            );
            request_monitor_position(
                &position_for_activation,
                settings.monitor_preference,
                width,
                height,
            );
            cursor_visibility_for_activation.show();
            if should_show_launcher(launcher_is_foreground()) {
                ctx.show_window();
            } else {
                ctx.hide_window();
            }
        }
    });

    let activation_handle_for_recorder = activation_handle.clone();
    let activation_recording_for_keys = activation_recording;
    let activation_display_for_keys = activation_display;
    let activation_key_for_keys = activation_key;
    let activation_ctrl_for_keys = activation_ctrl;
    let activation_alt_for_keys = activation_alt;
    let activation_shift_for_keys = activation_shift;
    let activation_meta_for_keys = activation_meta;
    let query_for_keys = query;
    let results_for_keys = results;
    let selected_id_for_keys = selected_id;
    let selected_index_for_keys = selected_index;
    let scroll_request_for_keys = scroll_request_for_rows;
    let selection_touched_for_keys = selection_touched;
    let action_mode_for_keys = action_mode;
    let action_index_for_keys = action_index;
    let action_items_for_keys = action_items;
    let recycle_bin_confirmation_for_keys = recycle_bin_confirmation;
    let plugin_actions_for_keys = Rc::clone(&plugin_actions);
    let inline_completion_for_keys = inline_completion;
    let settings_visible_for_keys = settings_visible;
    let query_history_for_keys = Rc::clone(&query_history);
    let history_mode_for_keys = history_mode;
    let history_cursor_for_keys = history_cursor;
    let settings_for_history_for_keys = Arc::clone(&shared_settings);
    let settings_for_priority_for_keys = Arc::clone(&shared_settings);
    let priorities_for_keys = priorities;
    let providers_for_keys = Rc::clone(&provider_results);
    let query_for_priority_keys = query;
    let window_op_for_keys = window_op.clone();
    let cursor_visibility_for_keys = cursor_visibility.clone();
    let size_for_keys = window_size.clone();
    let show_results_for_keys = show_results;
    let settings_for_game_hotkey = Arc::clone(&shared_settings);
    let game_mode_for_hotkey = game_mode;
    let game_mode_status_for_hotkey = game_mode_status;
    app = app.hotkey(hotkeys::game_mode_toggle_hotkey(), move |_| {
        let enabled = !game_mode_for_hotkey.get();
        set_game_mode(
            &settings_for_game_hotkey,
            game_mode_for_hotkey,
            game_mode_status_for_hotkey,
            enabled,
        );
    });

    app = app.on_key(move |event: KeyEvent| {
        if activation_recording_for_keys.get() {
            if event.pressed {
                if let Some(configuration) =
                    hotkeys::capture_config(&event, alt_key_is_down(), hotkeys::meta_key_is_down())
                {
                    activation_key_for_keys.set(configuration.key.clone());
                    activation_ctrl_for_keys.set(configuration.ctrl);
                    activation_alt_for_keys.set(configuration.alt);
                    activation_shift_for_keys.set(configuration.shift);
                    activation_meta_for_keys.set(configuration.meta);
                    activation_display_for_keys.set(hotkeys::display_config(&configuration));
                    activation_recording_for_keys.set(false);
                    activation_handle_for_recorder.set_enabled(true);
                }
            }
            return true;
        }
        if !event.pressed || settings_visible_for_keys.get() {
            return false;
        }
        let alt_down = alt_key_is_down();
        if !event.ctrl
            && !alt_down
            && matches!(event.key, Key::Char(_) | Key::Backspace | Key::Delete)
        {
            history_cursor_for_keys.set(None);
            cursor_visibility_for_keys.hide();
        }
        if event.ctrl
            && matches!(
                event.key,
                Key::Other(0x43) | Key::Char('c') | Key::Char('C')
            )
        {
            if let Some(result) = selected_result(
                &results_for_keys.get(),
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if copy_result_path(&result) {
                    return true;
                }
            }
            return false;
        }
        if event.ctrl && matches!(event.key, Key::Char('h') | Key::Char('H')) {
            let history = query_history_for_keys.borrow();
            if history.is_empty() {
                return false;
            }
            let filtered = history_results(&history, &query_for_keys.get());
            history_mode_for_keys.set(true);
            history_cursor_for_keys.set(None);
            action_mode_for_keys.set(false);
            action_items_for_keys.set(Vec::new());
            inline_completion_for_keys.set(String::new());
            selected_index_for_keys.set(0);
            selected_id_for_keys.set(
                filtered
                    .first()
                    .map(|result| result.id.clone())
                    .unwrap_or_default(),
            );
            results_for_keys.set(filtered);
            show_results_for_keys.set(true);
            size_for_keys.set(
                i32::from(launcher_width.get()),
                i32::from(launcher_height.get()),
            );
            return true;
        }
        let query = query_for_keys.get();
        let history = query_history_for_keys.borrow();
        if alt_down && !event.ctrl && !event.shift && matches!(event.key, Key::Up | Key::Down) {
            if history.is_empty() {
                return false;
            }
            let Some(next) =
                history_cursor_step(history.len(), history_cursor_for_keys.get(), event.key)
            else {
                return false;
            };
            history_cursor_for_keys.set(Some(next));
            history_mode_for_keys.set(false);
            query_for_keys.set(history[next].clone());
            return true;
        }
        if !history_mode_for_keys.get()
            && event.key == Key::Up
            && !alt_down
            && !event.ctrl
            && !event.shift
            && query.trim().is_empty()
        {
            if let Some(latest) = history.last() {
                history_cursor_for_keys.set(Some(history.len() - 1));
                query_for_keys.set(latest.clone());
                return true;
            }
        }
        drop(history);
        if query.trim().is_empty() {
            return false;
        }
        let current_results = results_for_keys.get();
        if current_results.is_empty() {
            return false;
        }

        if event.ctrl && event.key == Key::Tab {
            let suffix = inline_completion_for_keys.get();
            if !suffix.is_empty() {
                query_for_keys.set(format!("{query}{suffix}"));
                return true;
            }
        }

        // Match Flow Launcher: plain Tab selects the next result, while
        // Shift+Tab selects the previous result. Ctrl+Tab remains reserved
        // for inline completion above.
        if !event.ctrl && !alt_down && event.key == Key::Tab {
            let count = current_results.len();
            let next = if event.shift {
                selected_index_for_keys
                    .get()
                    .checked_sub(1)
                    .unwrap_or(count - 1)
            } else {
                (selected_index_for_keys.get() + 1) % count
            };
            selection_touched_for_keys.set(true);
            selected_index_for_keys.set(next);
            if let Some(result) = current_results.get(next) {
                selected_id_for_keys.set(result.id.clone());
            }
            // Keep the existing row tree intact while changing only selection.
            // Rebuilding the DynList here resets row geometry and prevents the
            // pending scroll request from bringing the next result into view.
            request_scroll(scroll_request_for_keys);
            return true;
        }

        if event.key == Key::Enter && alt_key_is_down() {
            if history_mode_for_keys.get() {
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    query_for_keys.set(result.title.clone());
                    history_mode_for_keys.set(false);
                }
                return true;
            }
            record_query_history(
                &settings_for_history_for_keys,
                &query_history_for_keys,
                &query,
            );
            if let Some(result) = selected_result(
                &current_results,
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if let Some(target) = result.target.as_deref() {
                    let _ = launch::open_file_location(target);
                }
            }
            return true;
        }

        if action_mode_for_keys.get() {
            let count = action_items_for_keys.get().len();
            if count == 0 {
                action_mode_for_keys.set(false);
                return true;
            }
            match event.key {
                Key::Up => {
                    action_index_for_keys.set(
                        action_index_for_keys
                            .get()
                            .checked_sub(1)
                            .unwrap_or(count - 1),
                    );
                    action_items_for_keys.set(action_items_for_keys.get());
                    return true;
                }
                Key::Down => {
                    action_index_for_keys.set((action_index_for_keys.get() + 1) % count);
                    action_items_for_keys.set(action_items_for_keys.get());
                    return true;
                }
                Key::Left | Key::Escape => {
                    action_mode_for_keys.set(false);
                    action_index_for_keys.set(0);
                    size_for_keys.set(
                        i32::from(launcher_width.get()),
                        i32::from(launcher_height.get()),
                    );
                    return true;
                }
                Key::Enter | Key::Space => {
                    if history_mode_for_keys.get() {
                        if let Some(result) = selected_result(
                            &current_results,
                            &selected_id_for_keys.get(),
                            selected_index_for_keys.get(),
                        ) {
                            query_for_keys.set(result.title.clone());
                            history_mode_for_keys.set(false);
                        }
                        return true;
                    }
                    record_query_history(
                        &settings_for_history_for_keys,
                        &query_history_for_keys,
                        &query,
                    );
                    if let Some(result) = selected_result(
                        &current_results,
                        &selected_id_for_keys.get(),
                        selected_index_for_keys.get(),
                    ) {
                        if let Some(action) = action_items_for_keys
                            .get()
                            .get(action_index_for_keys.get())
                            .cloned()
                        {
                            let executed = if matches!(action.kind, ActionKind::SetPriority) {
                                let saved = set_result_priority(
                                    &settings_for_priority_for_keys,
                                    priorities_for_keys,
                                    &result,
                                );
                                if saved {
                                    refresh_merged_results(
                                        &providers_for_keys,
                                        query_for_priority_keys,
                                        priorities_for_keys,
                                        results_for_keys,
                                    );
                                }
                                saved
                            } else {
                                execute_result_action(&result, &action.kind)
                            };
                            if executed {
                                window_op_for_keys.hide_window();
                            }
                        }
                    }
                    action_mode_for_keys.set(false);
                    size_for_keys.set(
                        i32::from(launcher_width.get()),
                        i32::from(launcher_height.get()),
                    );
                    return true;
                }
                _ => return true,
            }
        }

        if is_run_as_admin_key(&event) {
            record_query_history(
                &settings_for_history_for_keys,
                &query_history_for_keys,
                &query,
            );
            if let Some(result) = selected_result(
                &current_results,
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if let Some(target) = result.target.as_deref() {
                    if launch::run_as_admin(target) {
                        window_op_for_keys.hide_window();
                    }
                }
            }
            return true;
        }
        match event.key {
            Key::Up | Key::Down | Key::Home | Key::End => {
                let count = current_results.len();
                let next = match event.key {
                    Key::Up => selected_index_for_keys
                        .get()
                        .checked_sub(1)
                        .unwrap_or(count - 1),
                    Key::Down => (selected_index_for_keys.get() + 1) % count,
                    Key::Home => 0,
                    Key::End => count - 1,
                    _ => 0,
                };
                selection_touched_for_keys.set(true);
                selected_index_for_keys.set(next);
                if let Some(result) = current_results.get(next) {
                    selected_id_for_keys.set(result.id.clone());
                }
                // Preserve the current row geometry so scroll_into_view can
                // move the viewport after the selected result changes.
                request_scroll(scroll_request_for_keys);
                true
            }
            Key::Right => {
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    let actions = actions_for_result(&result, &plugin_actions_for_keys.borrow());
                    if !actions.is_empty() {
                        action_items_for_keys.set(actions);
                        action_index_for_keys.set(0);
                        action_mode_for_keys.set(true);
                        show_results_for_keys.set(true);
                        size_for_keys.set(i32::from(launcher_width.get()), ACTION_WINDOW_HEIGHT);
                    }
                }
                true
            }
            Key::Enter => {
                if history_mode_for_keys.get() {
                    if let Some(result) = selected_result(
                        &current_results,
                        &selected_id_for_keys.get(),
                        selected_index_for_keys.get(),
                    ) {
                        query_for_keys.set(result.title.clone());
                        history_mode_for_keys.set(false);
                    }
                    return true;
                }
                record_query_history(
                    &settings_for_history_for_keys,
                    &query_history_for_keys,
                    &query,
                );
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    if result.id == "empty-recycle-bin" {
                        recycle_bin_confirmation_for_keys.set(true);
                    } else if result.id == "flux-settings" {
                        settings_visible_for_keys.set(true);
                        size_for_keys.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                    } else if result.id == "open-recycle-bin" {
                        launch::open_recycle_bin_async();
                        window_op_for_keys.hide_window();
                    } else if let Some(target) = result.target.as_deref() {
                        launch::open_path_async(target);
                        window_op_for_keys.hide_window();
                    } else if let Some(action) =
                        plugin_actions_for_keys.borrow().get(&result.id).cloned()
                    {
                        plugins::execute_async(action);
                        window_op_for_keys.hide_window();
                    }
                }
                true
            }
            _ => false,
        }
    });

    let settings_for_tray_toggle = Arc::clone(&shared_settings);
    let game_mode_for_tray = game_mode;
    let game_status_for_tray = game_mode_status;
    let settings_visible_for_tray = settings_visible;
    let settings_visible_for_left_click = settings_visible;
    let show_results_for_left_click = show_results;
    let size_for_left_click = window_size.clone();
    let position_for_left_click = window_position.clone();
    let settings_for_left_click = Arc::clone(&shared_settings);
    let show_results_for_tray = show_results;
    let size_for_tray = window_size.clone();
    let position_for_tray = window_position.clone();
    let settings_for_tray_position = Arc::clone(&shared_settings);
    let size_for_settings = window_size.clone();
    let position_for_settings = window_position.clone();
    let settings_for_settings_position = Arc::clone(&shared_settings);
    let tray = Tray::new()
        .tooltip("Flux Launcher")
        .icon_rgba(16, 16, &tray_icon())
        .on_left_click(move |ctx| {
            settings_visible_for_left_click.set(false);
            let height = if show_results_for_left_click.get() {
                launcher_height.get() as i32
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            if let Ok(settings) = settings_for_left_click.read() {
                request_monitor_position(
                    &position_for_left_click,
                    settings.monitor_preference,
                    launcher_width.get() as i32,
                    height,
                );
            }
            size_for_left_click.set(launcher_width.get() as i32, height);
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item("Show launcher", move |ctx| {
                settings_visible_for_tray.set(false);
                let height = if show_results_for_tray.get() {
                    launcher_height.get() as i32
                } else {
                    COMPACT_WINDOW_HEIGHT
                };
                if let Ok(settings) = settings_for_tray_position.read() {
                    request_monitor_position(
                        &position_for_tray,
                        settings.monitor_preference,
                        launcher_width.get() as i32,
                        height,
                    );
                }
                size_for_tray.set(launcher_width.get() as i32, height);
                ctx.show_window();
            }),
            TrayMenuItem::item("Settings", move |ctx| {
                settings_visible.set(true);
                if let Ok(settings) = settings_for_settings_position.read() {
                    request_monitor_position(
                        &position_for_settings,
                        settings.monitor_preference,
                        SETTINGS_WINDOW_WIDTH,
                        SETTINGS_WINDOW_HEIGHT,
                    );
                }
                // Queue the Settings size before showing the hidden tray window. The
                // first frame must not use the compact 72-DIP launcher height.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                ctx.show_window();
                // Keep the request after show as well because the native show lifecycle
                // may consume a stale compact-size request from the previous hide.
                size_for_settings.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::check("Game Mode", game_mode, move |_| {
                let enabled = !game_mode_for_tray.get();
                set_game_mode(
                    &settings_for_tray_toggle,
                    game_mode_for_tray,
                    game_status_for_tray,
                    enabled,
                );
            }),
            TrayMenuItem::separator(),
            TrayMenuItem::item("Exit", |ctx| ctx.quit()),
        ]);

    let settings_for_apply = Arc::clone(&shared_settings);
    let position_for_apply = window_position.clone();
    let activation_handle_for_apply = activation_handle.clone();
    let activation_handle_for_record_button = activation_handle.clone();
    let activation_recording_for_record_button = activation_recording;
    let activation_recording_for_apply = activation_recording;
    let activation_display_for_ui = activation_display;
    let activation_display_for_apply = activation_display;
    let game_mode_status_for_apply = game_mode_status;
    let settings_visible_for_apply = settings_visible;
    let show_results_for_back = show_results;
    let position_for_back = window_position.clone();
    let settings_for_back_position = Arc::clone(&shared_settings);
    let size_for_back = window_size.clone();
    let size_for_apply = window_size.clone();
    let settings_for_clear_history = Arc::clone(&shared_settings);
    let history_for_clear = Rc::clone(&query_history);
    let history_cursor_for_clear = history_cursor;
    let start_with_windows_for_apply = start_with_windows;
    let update_checks_enabled_for_apply = update_checks_enabled;
    let update_interval_hours_for_apply = update_interval_hours;
    let auto_install_updates_for_apply = auto_install_updates;
    let update_status_for_apply = update_status;
    let update_available_for_install = update_available;
    let update_status_for_install = update_status;
    let update_installing_for_ui = update_installing;
    let update_install_progress_for_ui = update_install_progress;
    let update_install_sender_for_ui = update_install_sender.clone();
    let update_install_in_flight_for_ui = Rc::clone(&update_install_in_flight);
    let update_sender_for_apply = update_sender.clone();
    let update_sender_for_check_now = update_sender_for_apply.clone();
    let update_check_in_flight_for_apply = Rc::clone(&update_check_in_flight);
    let update_check_in_flight_for_check_now = Rc::clone(&update_check_in_flight);
    let auto_enable_everything_for_apply = auto_enable_everything;
    let obsidian_enabled_for_apply = obsidian_enabled;
    let obsidian_alias_for_apply = obsidian_alias;
    let google_enabled_for_apply = google_enabled;
    let google_alias_for_apply = google_alias;
    let everything_status_for_apply = everything_status;
    let everything_installed_for_ui = everything_installed;
    let settings_for_priority_ui = Arc::clone(&shared_settings);
    let providers_for_priority_ui = Rc::clone(&provider_results);
    let query_for_priority_ui = query;
    let priority_list = Element::list_signal(
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
                                .font_size(10.0)
                                .fg(Color::rgba(235, 241, 255, 170))
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
    );
    let priorities_empty = Element::label(
        "No explicit priorities yet. Select an application, press Right, then choose Set as priority.",
    )
    .font_size(12.0)
    .fg(Color::rgba(235, 241, 255, 185))
    .max_lines(2)
    .truncate(Truncate::End)
    .visible_when(move || priorities.get().is_empty());

    let visual_preview_generation_for_width_reset = visual_preview_generation;
    let visual_preview_generation_for_height_reset = visual_preview_generation;
    let settings_for_visual_apply = Arc::clone(&shared_settings);
    let size_for_visual_apply = window_size.clone();
    let position_for_visual_apply = window_position.clone();
    let settings_visible_for_visual_apply = settings_visible;
    let show_results_for_visual_apply = show_results;

    // Settings shares the same continuous Acrylic surface as the launcher.
    // Do not add a dark card here: it hides the blur and creates the old opaque
    // search-style slab inside the transparent window.
    let settings_panel = Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(0, 0, 0, 0))
        .border(Color::rgba(0, 0, 0, 0), 0)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(3)
                        .child(Element::label("Settings").font_size(25.0).fg(Color::WHITE))
                        .child(
                            Element::label("Changes apply immediately and are saved atomically")
                                .font_size(12.0)
                                .fg(Color::rgba(235, 241, 255, 180)),
                        ),
                )
                .child(Element::segmented(
                    vec!["General", "Visual", "Priorities", "Plugins"],
                    settings_tab,
                ))
                .child(
                    Element::button("Back")
                        .neutral()
                        .on_click(move |_| {
                            settings_visible.set(false);
                            let height = if show_results_for_back.get() {
                                launcher_height.get() as i32
                            } else {
                                COMPACT_WINDOW_HEIGHT
                            };
                            if let Ok(settings) = settings_for_back_position.read() {
                                request_monitor_position(
                                    &position_for_back,
                                    settings.monitor_preference,
                                    launcher_width.get() as i32,
                                    height,
                                );
                            }
                            size_for_back.set(launcher_width.get() as i32, height);
                        }),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 0)
                .child(
                Element::col()
                    .width_match()
                    .spacing(12)
                    .child(Element::field(
                        "Activation key",
                        Element::col()
                            .width_match()
                            .spacing(6)
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::label_signal(activation_display_for_ui)
                                            .width_match()
                                            .padding_xy(10, 8)
                                            .bg(Color::rgba(255, 255, 255, 24))
                                            .corner(8.0),
                                    )
                                    .child(
                                        Element::button("Record key")
                                            .neutral()
                                            .on_click(move |ctx| {
                                                activation_recording_for_record_button.set(true);
                                                activation_handle_for_record_button.set_enabled(false);
                                                ctx.toast_ok("Press the desired activation key");
                                            }),
                                    ),
                            )
                            .child(
                                Element::label("Click Record key, then press one key or a key combination")
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 170))
                                    .visible_when(move || activation_recording_for_record_button.get()),
                            ),
                    ))
                    .child(
                        Element::row()
                            .width_match()
                            .spacing(10)
                            .child(Element::checkbox("Ctrl", activation_ctrl))
                            .child(Element::checkbox("Alt", activation_alt))
                            .child(Element::checkbox("Shift", activation_shift))
                            .child(Element::checkbox("Windows", activation_meta)),
                    )
                    .child(Element::field(
                        "Fullscreen protection",
                        Element::checkbox("Ignore activation while another app is fullscreen", ignore_fullscreen),
                    ))
                    .child(Element::field(
                        "Game Mode",
                        Element::checkbox("Suppress the launcher until manually disabled", game_mode),
                    ))
                    .child(Element::field(
                        "Keyboard layout",
                        Element::checkbox(
                            "Start typing in English and restore the previous layout on hide",
                            switch_to_english_layout,
                        ),
                    ))
                    .child(Element::field(
                        "Query on activation",
                        Element::checkbox(
                            "Clear the previous query when opened with the global hotkey",
                            clear_query_on_activation,
                        ),
                    ))
                    .child(Element::field(
                        "Windows startup",
                        Element::checkbox(
                            "Start Flux automatically with Windows",
                            start_with_windows,
                        ),
                    ))
                    .child(Element::field(
                        "Open launcher on",
                        Element::col()
                            .spacing(6)
                            .child(Element::radio(
                                "Primary display",
                                monitor_preference,
                                0,
                            ))
                            .child(Element::radio(
                                "Display with the mouse cursor",
                                monitor_preference,
                                1,
                            ))
                            .child(Element::radio(
                                "Display with the focused window",
                                monitor_preference,
                                2,
                            )),
                    ))
                    .child(
                        Element::col()
                            .width_match()
                            .spacing(8)
                            .child(Element::field(
                                "Updates",
                                Element::checkbox(
                                    "Check stable GitHub releases automatically",
                                    update_checks_enabled,
                                ),
                            ))
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::text_input(update_interval_hours, "24")
                                            .width_match(),
                                    )
                                    .child(Element::label("hours between checks").font_size(11.0)),
                            )
                            .child(Element::field(
                                "Update action",
                                Element::checkbox(
                                    "Install stable updates automatically",
                                    auto_install_updates,
                                ),
                            ))
                            .child(
                                Element::row()
                                    .width_match()
                                    .spacing(8)
                                    .child(
                                        Element::label_signal(update_status)
                                            .font_size(11.0)
                                            .fg(Color::rgba(235, 241, 255, 190))
                                            .max_lines(2)
                                            .truncate(Truncate::End)
                                            .width_match(),
                                    )
                                    .child(Element::button("Check for updates").on_click(move |ctx| {
                                        update_status_for_apply.set(String::from(
                                            "Checking stable GitHub releases...",
                                        ));
                                        request_update_check(
                                            update_sender_for_check_now.clone(),
                                            &update_check_in_flight_for_check_now,
                                        );
                                        ctx.toast_ok("Checking stable updates");
                                    }))
                                    .child(
                                        Element::button("Install now")
                                            .visible_when(move || {
                                                update_available_for_install.get().is_some()
                                                    && !update_installing_for_ui.get()
                                            })
                                            .on_click(move |ctx| {
                                                if update_installing_for_ui.get() {
                                                    return;
                                                }
                                                if let Some(update) = update_available_for_install.get() {
                                                    update_installing_for_ui.set(true);
                                                    update_install_progress_for_ui.set(None);
                                                    update_status_for_install.set(format!(
                                                        "Preparing stable {} for download...",
                                                        update.version
                                                    ));
                                                    if !request_update_install(
                                                        update,
                                                        update_install_sender_for_ui.clone(),
                                                        &update_install_in_flight_for_ui,
                                                        updater::RelaunchMode::Visible,
                                                    ) {
                                                        update_installing_for_ui.set(false);
                                                        update_status_for_install.set(String::from(
                                                            "An update is already being installed",
                                                        ));
                                                        ctx.toast_ok("An update is already being installed");
                                                    }
                                                }
                                            }),
                                    ),
                            )
                            .child(Element::field(
                                "Everything",
                                Element::checkbox(
                                    "Auto-enable Everything when installed",
                                    auto_enable_everything,
                                ),
                            ))
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
                            ),
                    )
                    .child(
                        Element::row()
                            .width_match()
                            .spacing(10)
                            .child(
                                Element::label("Query history: Ctrl+H recalls committed searches")
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 175))
                                    .width_match(),
                            )
                            .child(Element::button("Clear history").on_click(move |ctx| {
                                if let Ok(mut settings) = settings_for_clear_history.write() {
                                    settings.clear_query_history();
                                    let _ = save_settings(&settings);
                                }
                                history_for_clear.borrow_mut().clear();
                                history_cursor_for_clear.set(None);
                                ctx.toast_ok("Query history cleared");
                            })),
                    )
                    .child(Element::field(
                        "Smooth Caret",
                        Element::checkbox("Animate search caret movement", smooth_caret),
                    ))
                    .child(Element::field(
                        "Caret duration (ms)",
                        Element::text_input(caret_duration, "95").width_match(),
                    ))
                    .child(
                        Element::label("Native Flow plugins: %APPDATA%\\FluxLauncher\\Plugins or FLUX_PLUGIN_DIR")
                            .font_size(12.0)
                            .fg(Color::rgba(235, 241, 255, 160)),
                    )
                    .child(
                        Element::button("Apply settings").on_click(move |ctx| {
                            let duration = caret_duration
                                .get()
                                .trim()
                                .parse::<u16>()
                                .unwrap_or(95)
                                .clamp(60, 160);
                            let configuration = HotkeyConfig {
                                ctrl: activation_ctrl.get(),
                                alt: activation_alt.get(),
                                shift: activation_shift.get(),
                                meta: activation_meta.get(),
                                key: activation_key.get(),
                            };
                            let custom_color = parse_selection_color(&custom_selection_color.get())
                                .unwrap_or(0x4c8bf4);
                            let configured_width = parse_dimension_input(
                                &launcher_width_input.get(),
                                MIN_LAUNCHER_WIDTH,
                                MAX_LAUNCHER_WIDTH,
                            )
                            .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                            let configured_height = parse_dimension_input(
                                &launcher_height_input.get(),
                                MIN_LAUNCHER_HEIGHT,
                                MAX_LAUNCHER_HEIGHT,
                            )
                            .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                            if let Ok(mut settings) = settings_for_apply.write() {
                                settings.activation_hotkey = configuration;
                                settings.ignore_hotkeys_in_fullscreen = ignore_fullscreen.get();
                                settings.game_mode = game_mode.get();
                                settings.smooth_caret = smooth_caret.get();
                                settings.switch_to_english_layout = switch_to_english_layout.get();
                                settings.use_system_accent = use_system_accent.get();
                                settings.custom_selection_color = custom_color;
                                settings.launcher_width = configured_width;
                                settings.launcher_height = configured_height;
                                settings.clear_query_on_activation = clear_query_on_activation.get();
                                settings.start_with_windows = start_with_windows_for_apply.get();
                                settings.update_checks_enabled = update_checks_enabled_for_apply.get();
                                settings.update_interval_hours = update_interval_hours_for_apply
                                    .get()
                                    .trim()
                                    .parse::<u32>()
                                    .unwrap_or(24)
                                    .clamp(1, 168);
                                settings.auto_install_updates = auto_install_updates_for_apply.get();
                                update_interval_hours_for_apply
                                    .set(settings.update_interval_hours.to_string());
                                settings.auto_enable_everything = auto_enable_everything_for_apply.get();
                                settings.obsidian_enabled = obsidian_enabled_for_apply.get();
                                settings.obsidian_alias = obsidian_alias_for_apply.get();
                                settings.google_enabled = google_enabled_for_apply.get();
                                settings.google_alias = google_alias_for_apply.get();
                                settings.monitor_preference = monitor_preference_from_index(monitor_preference.get());
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                activation_recording_for_apply.set(false);
                                activation_display_for_apply
                                    .set(hotkeys::display_config(&settings.activation_hotkey));
                                selection_color.set(selection_color_for_settings(&settings));
                                custom_selection_color.set(selection_color_hex(settings.custom_selection_color));
                                launcher_width.set(settings.launcher_width);
                                launcher_height.set(settings.launcher_height);
                                launcher_width_input.set(settings.launcher_width.to_string());
                                launcher_height_input.set(settings.launcher_height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    settings.launcher_width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    settings.launcher_height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(format!(
                                    "Current launcher client area: {} × {} logical px (DIP)",
                                    settings.launcher_width, settings.launcher_height
                                ));
                                activation_handle_for_apply
                                    .set(hotkeys::activation_hotkey(&settings.activation_hotkey));
                                activation_handle_for_apply.set_enabled(true);
                                game_mode_status_for_apply.set(game_mode_label(settings.game_mode));
                                if settings.auto_enable_everything {
                                    match everything::start_background_if_installed() {
                                        Ok(InstallationState::Installed(_)) => {
                                            everything_installed.set(true);
                                            everything_status_for_apply.set(String::from(
                                                "Everything detected; Flux will enable IPC automatically",
                                            ));
                                        }
                                        Ok(InstallationState::Missing) => {
                                            everything_installed.set(false);
                                            everything_status_for_apply.set(String::from(
                                                "Everything is not installed. Install it with winget to enable file search.",
                                            ));
                                        }
                                        Err(error) => everything_status_for_apply.set(error),
                                    }
                                } else {
                                    everything_status_for_apply.set(String::from(
                                        "Everything auto-enable is disabled in Flux settings",
                                    ));
                                }
                                let _ = save_settings(&settings);
                                if let Err(error) = startup::set_enabled(settings.start_with_windows) {
                                    ctx.toast_ok(format!("Startup setting failed: {error}"));
                                }
                                if settings.update_checks_enabled && update_check_due(&settings)
                                {
                                    update_status_for_apply
                                        .set(String::from("Checking stable GitHub releases..."));
                                    request_update_check(
                                        update_sender_for_apply.clone(),
                                        &update_check_in_flight_for_apply,
                                    );
                                }
                            }
                            settings_visible_for_apply.set(false);
                            let selected_preference = monitor_preference_from_index(monitor_preference.get());
                            let applied_width = launcher_width.get() as i32;
                            let applied_height = launcher_height.get() as i32;
                            let target_height = if show_results.get() {
                                applied_height
                            } else {
                                COMPACT_WINDOW_HEIGHT
                            };
                            request_monitor_position(
                                &position_for_apply,
                                selected_preference,
                                applied_width,
                                target_height,
                            );
                            size_for_apply.set(applied_width, target_height);
                            ctx.toast_ok("Settings applied");
                        }),
                    ),
            ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 3)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
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
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 1)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::label("Visual appearance").font_size(17.0).fg(Color::WHITE))
                        .child(
                            Element::label("The live preview is a separate native windui window. Its client area is resized directly in realtime; the Settings window stays centered and stable while dragging.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(3)
                                .truncate(Truncate::End),
                        )
                        .child(Element::field(
                            "Selection color",
                            Element::checkbox(
                                "Use the Windows 11 system accent color when available",
                                use_system_accent,
                            ),
                        ))
                        .child(Element::label("Windows accent is read from the current user profile; the custom color is used as a safe fallback.").font_size(10.0).fg(Color::rgba(235, 241, 255, 150)).max_lines(2).truncate(Truncate::End))
                        .child(Element::label("The exact native preview window opens beside Settings when this Visual tab is active."
).font_size(10.0).fg(Color::rgba(235, 241, 255, 170)).max_lines(2).truncate(Truncate::End))
                        .child(
                            Element::col()
                                .spacing(8)
                                .visible_when(move || !use_system_accent.get())
                                .child(
                                    Element::text_input(custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(custom_selection_color)),
                        )
                        .child(Element::field(
                            "Launcher width",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_width_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_width_input, "420")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = DEFAULT_LAUNCHER_WIDTH;
                                            let height = launcher_height.get();
                                            eprintln!("Visual width reset clicked: {}x{}", width, height);
                                            launcher_width.set(width);
                                            launcher_width_input.set(width.to_string());
                                            launcher_width_slider.set(dimension_slider_fraction(
                                                width,
                                                MIN_LAUNCHER_WIDTH,
                                                MAX_LAUNCHER_WIDTH,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_width_reset.set(
                                                visual_preview_generation_for_width_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::field(
                            "Results height",
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(Element::slider(launcher_height_slider).width(VISUAL_SLIDER_WIDTH))
                                .child(
                                    Element::text_input(launcher_height_input, "382")
                                        .width(76),
                                )
                                .child(
                                    Element::button("Reset")
                                        .neutral()
                                        .on_click(move |_| {
                                            let width = launcher_width.get();
                                            let height = DEFAULT_LAUNCHER_HEIGHT;
                                            eprintln!("Visual height reset clicked: {}x{}", width, height);
                                            launcher_height.set(height);
                                            launcher_height_input.set(height.to_string());
                                            launcher_height_slider.set(dimension_slider_fraction(
                                                height,
                                                MIN_LAUNCHER_HEIGHT,
                                                MAX_LAUNCHER_HEIGHT,
                                            ));
                                            launcher_preview_text.set(format!(
                                                "Current launcher client area: {} × {} logical px (DIP)",
                                                width, height
                                            ));
                                            visual_preview_generation_for_height_reset.set(
                                                visual_preview_generation_for_height_reset
                                                    .get()
                                                    .saturating_add(1),
                                            );
                                        }),
                                )
                                .child(Element::label("DIP").font_size(11.0)),
                        ))
                        .child(
                            Element::label(format!(
                                "Safe range: {}–{} logical px (DIP)",
                                MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::label_signal(launcher_preview_text).font_size(12.0).fg(Color::WHITE))
                        .child(
                            Element::label("The native preview uses the exact requested logical client dimensions. Physical GetClientRect pixels scale with the preview monitor DPI; Apply saves the values.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 175))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(
                            Element::button("Apply dimensions").on_click(move |ctx| {
                                let mut width = parse_dimension_input(
                                    &launcher_width_input.get(),
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                                let mut height = parse_dimension_input(
                                    &launcher_height_input.get(),
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                                let Ok(mut settings) = settings_for_visual_apply.write() else {
                                    ctx.toast_ok("Could not lock Flux settings");
                                    return;
                                };
                                settings.launcher_width = width;
                                settings.launcher_height = height;
                                settings.normalize();
                                width = settings.launcher_width;
                                height = settings.launcher_height;
                                let preference = settings.monitor_preference;
                                if !save_settings(&settings) {
                                    ctx.toast_ok("Could not save visual dimensions");
                                    return;
                                }
                                launcher_width.set(width);
                                launcher_height.set(height);
                                launcher_width_input.set(width.to_string());
                                launcher_height_input.set(height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(format!(
                                    "Current launcher client area: {} × {} logical px (DIP)",
                                    width, height
                                ));
                                eprintln!("Visual Apply dimensions clicked: {}x{}", width, height);
                                settings_visible_for_visual_apply.set(false);
                                let target_height = if show_results_for_visual_apply.get() {
                                    i32::from(height)
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                };
                                request_monitor_position(
                                    &position_for_visual_apply,
                                    preference,
                                    i32::from(width),
                                    target_height,
                                );
                                size_for_visual_apply.set(i32::from(width), target_height);
                                ctx.toast_ok("Visual dimensions applied");
                            }),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 2)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(10)
                        .child(
                            Element::label("Explicit application priorities")
                                .font_size(17.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label("Only applications explicitly added with Set as priority appear here. Rank 1 is searched first.")
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 180))
                                .max_lines(2)
                                .truncate(Truncate::End),
                        )
                        .child(priorities_empty)
                        .child(priority_list),
                ),
        );

    let launcher_page = Element::stack()
        .fill()
        .child(launcher_surface)
        .visible_when(move || !settings_visible.get());
    let settings_page = Element::col()
        .fill()
        .padding(18)
        .child(settings_panel)
        .visible_signal(settings_visible);
    let content = Element::stack()
        .fill()
        .font_family(LAUNCHER_FONT_FAMILY)
        .child(launcher_page)
        .child(settings_page);

    let mut app = if startup_launch {
        app.start_hidden()
    } else {
        app
    };
    let second_instance_sender = app.channel::<()>(|ctx, ()| {
        ctx.show_window();
    });
    let second_instance_sender_for_callback = second_instance_sender.clone();
    let second_instance_window_op = window_op.clone();
    let shutdown_window_op = window_op.clone();
    if !single_instance_disabled {
        app = app.single_instance(SINGLE_INSTANCE_ID, move |argv| {
            if argv.iter().any(|arg| arg == "--shutdown") {
                // Uninstall is an application-controlled handoff: destroy the native
                // window and exit the event loop instead of applying hide_on_close.
                shutdown_window_op.quit();
                return;
            }
            // The native windui listener activates the window; queue Show as well so a
            // tray-hidden startup is made visible before the channel callback is drained.
            second_instance_window_op.show_window();
            let _ = second_instance_sender_for_callback.send(());
        });
    }
    app.tray(tray)
        .hide_on_close()
        .hide_on_deactivate()
        .focus_first_control_on_show()
        // The Win32 backend keeps this transparent on local Acrylic-capable
        // sessions and uses this dark color only for an honest RDP fallback.
        .bg(Color::rgba(32, 33, 35, 255))
        .centered()
        .frameless()
        .resizable(false)
        .min_size(MIN_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        .theme(launcher_theme())
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |ctx| {
            let current_width = width_for_interval.get();
            let current_height = height_for_interval.get();
            let settings_is_visible = settings_visible_for_interval.get();
            let visual_tab_is_visible = settings_tab_for_interval.get() == 1;
            let visual_preview_is_visible = settings_is_visible && visual_tab_is_visible;
            if settings_is_visible && !last_settings_visible {
                if let Ok(settings) = settings_for_interval_geometry.read() {
                    request_monitor_position(
                        &position_for_interval,
                        settings.monitor_preference,
                        SETTINGS_WINDOW_WIDTH,
                        SETTINGS_WINDOW_HEIGHT,
                    );
                }
                size_for_interval.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            }
            if visual_preview_is_visible {
                let preference = settings_for_interval_geometry
                    .read()
                    .map(|settings| settings.monitor_preference)
                    .unwrap_or(MonitorPreference::Cursor);
                let child_exited = visual_preview_process
                    .as_mut()
                    .is_some_and(|preview| !preview.is_alive());
                if child_exited {
                    visual_preview_process.take();
                    last_visual_preview_request = None;
                }
                if let Some(preview) = visual_preview_process.as_mut() {
                    match preview.poll_ready() {
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("Could not ready visual preview: {error}");
                            visual_preview_process.take();
                            last_visual_preview_request = None;
                        }
                    }
                }
                if visual_preview_process.is_none() {
                    let preview_width = i32::from(current_width);
                    let preview_height = i32::from(current_height);
                    let (preview_x, preview_y) =
                        visual_preview_position(preference, preview_width, preview_height);
                    match visual_preview::PreviewProcess::start(
                        preview_width,
                        preview_height,
                        preview_x,
                        preview_y,
                    ) {
                        Ok(preview) => {
                            visual_preview_process = Some(preview);
                            last_visual_preview_request = None;
                        }
                        Err(error) => eprintln!("Could not start visual preview: {error}"),
                    }
                }
            } else if let Some(preview) = visual_preview_process.as_mut() {
                eprintln!(
                    "Visual preview closing because Settings/Visual is hidden: pid={}",
                    preview.pid()
                );
                visual_preview_process.take();
                last_visual_preview_request = None;
            }
            last_settings_visible = settings_is_visible;
            let slider_width = dimension_from_slider(
                width_slider_for_interval.get(),
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH,
            );
            let slider_height = dimension_from_slider(
                height_slider_for_interval.get(),
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT,
            );
            if visual_preview_smoke_for_interval {
                let control_state = (
                    width_for_interval.get(),
                    height_for_interval.get(),
                    (width_slider_for_interval.get() * 10_000.0).round() as u32,
                    (height_slider_for_interval.get() * 10_000.0).round() as u32,
                );
                if last_visual_control_state != Some(control_state) {
                    eprintln!(
                        "Visual control state: width={} height={} width_slider={} height_slider={}",
                        control_state.0, control_state.1, control_state.2, control_state.3
                    );
                    last_visual_control_state = Some(control_state);
                }
            }
            let typed_width = parse_dimension_input(
                &width_input_for_interval.get(),
                MIN_LAUNCHER_WIDTH,
                MAX_LAUNCHER_WIDTH,
            );
            let typed_height = parse_dimension_input(
                &height_input_for_interval.get(),
                MIN_LAUNCHER_HEIGHT,
                MAX_LAUNCHER_HEIGHT,
            );
            let next_width = typed_width
                .filter(|value| *value != current_width)
                .unwrap_or(if slider_width != current_width {
                    slider_width
                } else {
                    current_width
                });
            let next_height = typed_height
                .filter(|value| *value != current_height)
                .unwrap_or(if slider_height != current_height {
                    slider_height
                } else {
                    current_height
                });
            if next_width != current_width || next_height != current_height {
                width_for_interval.set(next_width);
                height_for_interval.set(next_height);
                width_input_for_interval.set(next_width.to_string());
                height_input_for_interval.set(next_height.to_string());
                width_slider_for_interval.set(dimension_slider_fraction(
                    next_width,
                    MIN_LAUNCHER_WIDTH,
                    MAX_LAUNCHER_WIDTH,
                ));
                height_slider_for_interval.set(dimension_slider_fraction(
                    next_height,
                    MIN_LAUNCHER_HEIGHT,
                    MAX_LAUNCHER_HEIGHT,
                ));
                preview_text_for_interval.set(format!(
                    "Current launcher client area: {} × {} logical px (DIP)",
                    next_width, next_height
                ));
                if !(settings_visible_for_interval.get() && settings_tab_for_interval.get() == 1) {
                    apply_launcher_size(
                        &size_for_interval,
                        &position_for_interval,
                        &settings_for_interval_geometry,
                        next_width,
                        next_height,
                        false,
                        show_results_for_interval.get(),
                    );
                }
                if visual_preview_smoke_for_interval {
                    eprintln!(
                        "Visual preview dimension state: {}x{} logical px",
                        next_width, next_height
                    );
                }
                last_launcher_width = next_width;
                last_launcher_height = next_height;
            } else if current_width != last_launcher_width || current_height != last_launcher_height
            {
                last_launcher_width = current_width;
                last_launcher_height = current_height;
            }

            let preview_generation = visual_preview_generation_for_interval.get();
            if visual_preview_is_visible {
                let requested = (width_for_interval.get(), height_for_interval.get());
                let must_dispatch = last_visual_preview_request != Some(requested)
                    || last_visual_preview_generation != preview_generation;
                if must_dispatch {
                    let preference = settings_for_interval_geometry
                        .read()
                        .map(|settings| settings.monitor_preference)
                        .unwrap_or(MonitorPreference::Cursor);
                    let (preview_x, preview_y) = visual_preview_position(
                        preference,
                        i32::from(requested.0),
                        i32::from(requested.1),
                    );
                    let dispatch_result = if let Some(preview) = visual_preview_process.as_mut() {
                        match preview.poll_ready() {
                            Ok(true) => Some(preview.resize(
                                i32::from(requested.0),
                                i32::from(requested.1),
                                preview_x,
                                preview_y,
                            )),
                            Ok(false) => None,
                            Err(error) => Some(Err(error)),
                        }
                    } else {
                        None
                    };
                    match dispatch_result {
                        Some(Ok(())) => {
                            last_visual_preview_request = Some(requested);
                            last_visual_preview_generation = preview_generation;
                            eprintln!(
                                "Visual preview IPC resize dispatched: {}x{}",
                                requested.0, requested.1
                            );
                        }
                        Some(Err(error)) => {
                            eprintln!("Could not update visual preview: {error}");
                            visual_preview_process.take();
                            last_visual_preview_request = None;
                        }
                        None => {}
                    }
                }
            } else {
                last_visual_preview_request = None;
                last_visual_preview_generation = preview_generation;
            }

            if let Ok(settings) = settings_for_update_interval.read() {
                let update_checks_allowed = std::env::var("FLUX_DISABLE_UPDATE_CHECKS")
                    .map(|value| value != "1")
                    .unwrap_or(true);
                if update_checks_allowed
                    && settings.update_checks_enabled
                    && update_check_due(&settings)
                {
                    request_update_check(
                        update_sender_for_interval.clone(),
                        &update_check_in_flight_for_interval,
                    );
                }
            }
            let completed_icon_generation =
                SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire);
            if icon_completion_generation_changed(last_icon_generation, completed_icon_generation) {
                last_icon_generation = completed_icon_generation;
                icon_refresh_generation_for_interval.set(completed_icon_generation);
            }
            if tray_settings_smoke_pending_for_interval.replace(false) {
                // Exercise the same lifecycle order as the tray Settings item,
                // without relying on brittle screen-coordinate tray automation.
                settings_visible_for_interval.set(true);
                ctx.show_window();
                size_for_interval.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                return;
            }
            let next_query = query_for_interval.get();
            if next_query == last_query {
                return;
            }

            let has_query = !next_query.trim().is_empty();
            history_mode_for_interval.set(false);
            show_results_for_interval.set(has_query);
            size_for_interval.set(
                launcher_width.get() as i32,
                if has_query {
                    launcher_height.get() as i32
                } else {
                    COMPACT_WINDOW_HEIGHT
                },
            );
            sequence = sequence.wrapping_add(1);
            sequence_for_interval.set(sequence);
            model.set_query(&next_query);
            {
                let mut built_in_results = model.results().to_vec();
                normalize_built_in_executable_targets(&mut built_in_results);
                let everything_expected = auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN;
                let mut providers = providers_for_interval.borrow_mut();
                providers.reset(sequence, built_in_results.clone(), everything_expected);
                let publish_initial_results = should_publish_initial_query_results(
                    has_query,
                    built_in_results.is_empty(),
                    results_for_interval.get().is_empty(),
                );
                if publish_initial_results {
                    selection_touched_for_interval.set(false);
                    selected_index.set(0);
                    selected_id.set(
                        built_in_results
                            .first()
                            .map(|result| result.id.clone())
                            .unwrap_or_default(),
                    );
                    // Built-in/system commands are synchronous and must be actionable
                    // immediately. External providers still replace this snapshot once
                    // their responses arrive for the same query sequence.
                    results_for_interval.set(built_in_results);
                }
                // Do not derive or display completion from the previous query while
                // the current provider generation is still pending.
                inline_completion_for_interval.set(String::new());
            }
            request_scroll(scroll_request_for_interval);
            action_mode.set(false);
            action_index.set(0);
            action_items.set(Vec::new());
            actions_for_interval.borrow_mut().clear();
            if !has_query {
                inline_completion_for_interval.set(String::new());
                status_for_interval.set(String::from("Ready"));
            } else {
                status_for_interval.set(String::from(
                    "Searching applications, Everything and native Flow plugins...",
                ));
                application_worker.request(sequence, next_query.clone());
                // Everything is the always-on file provider for every non-empty
                // query. Native Everything syntax such as `ext:zip`, `parent:`,
                // `file:`, and `dm:today` stays unchanged; a leading `.ext`
                // shorthand is normalized only for this provider.
                if auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN
                {
                    everything_worker.request(sequence, normalize_everything_query(&next_query));
                }
                if next_query.trim().len() >= PLUGIN_MIN_QUERY_LEN {
                    plugin_worker.request(
                        sequence,
                        next_query.clone(),
                        obsidian_enabled_for_interval.get(),
                        obsidian_alias_for_interval.get(),
                        google_enabled_for_interval.get(),
                        google_alias_for_interval.get(),
                    );
                    native_plugin_worker.request(sequence, next_query.clone());
                }
            }
            last_query = next_query;
        })
        .on_window_show({
            let settings = Arc::clone(&shared_settings);
            let cursor_visibility_for_show = cursor_visibility.clone();
            let settings_visible_for_show = settings_visible;
            let size_for_show = window_size.clone();
            move || {
                cursor_visibility_for_show.show();
                if let Ok(settings) = settings.read() {
                    selection_color.set(selection_color_for_settings(&settings));
                }
                // Tray activation can show the HWND before the first interval pass.
                // Apply the Settings client size in this lifecycle callback too, so
                // the initial frame is the full panel rather than a 72-DIP strip.
                if settings_visible_for_show.get() {
                    size_for_show.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                }
            }
        })
        .on_window_activated({
            let settings = Arc::clone(&shared_settings);
            move || {
                let layout_enabled = settings
                    .read()
                    .map(|settings| settings.switch_to_english_layout)
                    .unwrap_or(true);
                if layout_enabled {
                    keyboard_layout::switch_to_english();
                }
            }
        })
        .on_window_deactivated(|| {
            launch::trace_launch_event("window-deactivated");
        })
        .on_window_hide({
            let settings = Arc::clone(&shared_settings);
            move || {
                launch::trace_launch_event("window-hide");
                let (enabled, clear_query) = settings
                    .read()
                    .map(|settings| {
                        (
                            settings.switch_to_english_layout,
                            settings.clear_query_on_activation,
                        )
                    })
                    .unwrap_or((true, clear_query_on_activation.get()));
                if enabled {
                    keyboard_layout::restore_previous();
                }
                if clear_query {
                    query.set(String::new());
                    results.set(Vec::new());
                    selected_id.set(String::new());
                    selected_index.set(0);
                    selection_touched.set(false);
                    show_results.set(false);
                    history_mode.set(false);
                    history_cursor.set(None);
                    action_mode.set(false);
                    action_index.set(0);
                    action_items.set(Vec::new());
                    inline_completion.set(String::new());
                    scroll_request_for_rows.set(false);
                    let (width, height) = launcher_window_geometry_with_sizes(
                        settings_visible.get(),
                        false,
                        launcher_width.get() as i32,
                        launcher_height.get() as i32,
                    );
                    size_for_visibility.set(width, height);
                }
            }
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::{
        actions_for_result, bundled_icon_rgba, canonical_application_id, dimension_from_slider,
        dimension_slider_fraction, display_title, format_bytes, format_update_progress,
        google_icon_rgba, history_cursor_step, hover_position_changed,
        icon_completion_generation_changed, icon_target_for_path, is_executable_icon_target,
        is_run_as_admin_key, is_shutdown_mode, launcher_window_geometry,
        launcher_window_geometry_with_sizes, merge_application_duplicates,
        normalize_built_in_executable_targets, normalize_everything_query, obsidian_icon_rgba,
        parse_dimension_input, parse_internet_shortcut_icon_location,
        preserve_everything_file_order, quoted_result_path, rank_results_with_priorities,
        relaunch_mode_for_auto_install, resolve_bare_executable_path, resolve_shortcut_icon_path,
        should_claim_single_instance, should_publish_initial_query_results, should_show_launcher,
        ProviderResults, ResultIconView, ShellIconCache, COMPACT_WINDOW_HEIGHT,
        LAUNCHER_FONT_FAMILY, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
        MAX_SHELL_ICON_CACHE_ENTRIES, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
    };
    use flux_core::{ResultKind, ResultSource, SearchResult};
    use windui::event::{Key, KeyEvent};

    #[test]
    fn plugin_host_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--plugin-host"
        ))));
    }

    #[test]
    fn folder_launch_smoke_mode_bypasses_main_single_instance_guard() {
        assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--folder-launch-smoke"
        ))));
    }

    #[test]
    fn normal_and_startup_modes_use_main_single_instance_guard() {
        assert!(should_claim_single_instance(None));
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--startup"
        ))));
    }

    #[test]
    fn shutdown_mode_is_a_single_instance_command() {
        assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
            "--shutdown"
        ))));
        assert!(is_shutdown_mode(Some(std::ffi::OsStr::new("--shutdown"))));
        assert!(!is_shutdown_mode(Some(std::ffi::OsStr::new("--startup"))));
        assert!(!is_shutdown_mode(None));
    }

    #[test]
    fn update_progress_text_exposes_percent_bytes_and_remaining_work() {
        let progress = super::updater::DownloadProgress {
            received_bytes: 512,
            total_bytes: Some(1024),
        };
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(
            format_update_progress("0.1.64", &progress),
            "Downloading stable 0.1.64: 50% — 512 B / 1 KiB (512 B remaining)"
        );
    }

    #[test]
    fn bundled_google_icon_decodes_to_32_pixel_rgba_bitmap() {
        let icon = google_icon_rgba().expect("bundled Google icon should decode");
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert!(icon.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn bundled_obsidian_icon_decodes_to_32_pixel_rgba_bitmap() {
        let icon = obsidian_icon_rgba().expect("bundled Obsidian icon should decode");
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert!(icon.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn obsidian_result_uses_the_bundled_icon() {
        let icon = bundled_icon_rgba("builtin:obsidian:notes/readme.md")
            .expect("Obsidian result should use the bundled icon");
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert!(bundled_icon_rgba("everything:file:readme.md").is_none());
    }

    #[test]
    fn bounded_shell_icon_cache_evicts_oldest_entries() {
        let mut cache = ShellIconCache::new();
        for index in 0..=MAX_SHELL_ICON_CACHE_ENTRIES {
            cache.insert(format!("target-{index}"), Some(vec![index as u8; 32]));
        }
        assert_eq!(cache.entries.len(), MAX_SHELL_ICON_CACHE_ENTRIES);
        assert!(cache.get("target-0").is_none());
        assert!(cache
            .get(&format!("target-{MAX_SHELL_ICON_CACHE_ENTRIES}"))
            .is_some());
    }

    #[test]
    fn shell_icon_cache_touch_preserves_recent_entry_during_eviction() {
        let mut cache = ShellIconCache::new();
        for index in 0..MAX_SHELL_ICON_CACHE_ENTRIES {
            cache.insert(format!("target-{index}"), Some(vec![index as u8; 4]));
        }
        assert!(cache.get("target-0").is_some());
        cache.insert(String::from("new-target"), None);
        assert!(cache.get("target-0").is_some());
        assert!(cache.get("target-1").is_none());
        assert!(cache.get("new-target").is_some_and(|icon| icon.is_none()));
    }

    #[test]
    fn shell_icon_cache_retains_negative_results_without_unbounded_growth() {
        let mut cache = ShellIconCache::new();
        cache.insert(String::from("missing-target"), None);
        assert!(cache
            .get("missing-target")
            .is_some_and(|icon| icon.is_none()));
    }

    #[test]
    fn icon_generation_changes_only_after_completion() {
        assert!(!icon_completion_generation_changed(4, 4));
        assert!(icon_completion_generation_changed(4, 5));
        assert!(icon_completion_generation_changed(u64::MAX, 0));
    }

    #[test]
    fn parses_steam_internet_shortcut_icon_file_and_index() {
        let shortcut = "[InternetShortcut]\nURL=steam://rungameid/730\nIconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\730.ico\nIconIndex=0\n";
        assert_eq!(
            parse_internet_shortcut_icon_location(shortcut),
            Some((
                String::from(r"C:\Program Files (x86)\Steam\steam\games\730.ico"),
                0
            ))
        );
    }

    #[test]
    fn parses_shortcut_icon_keys_case_insensitively_and_defaults_index() {
        let shortcut = "[InternetShortcut]\nurl=steam://rungameid/10\niconfile=game.ico\n";
        assert_eq!(
            parse_internet_shortcut_icon_location(shortcut),
            Some((String::from("game.ico"), 0))
        );
    }

    #[test]
    fn resolves_relative_shortcut_icon_file_against_shortcut_directory() {
        let expected = std::path::Path::new("/tmp/Steam")
            .join("icons/game.ico")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            resolve_shortcut_icon_path("/tmp/Steam/Game.url", "icons/game.ico"),
            Some(expected)
        );
    }

    #[test]
    fn result_icon_view_accepts_valid_cached_rgba_and_keeps_placeholder_on_miss() {
        let generation = windui::prelude::signal(0_u64);
        let valid = [255_u8, 128, 64, 255].repeat(32 * 32);
        let loaded = ResultIconView::new(
            Some(String::from(r"C:\Program Files\Demo\demo.exe")),
            String::from("▣"),
            LAUNCHER_FONT_FAMILY,
            Some(valid),
            generation,
        );
        assert!(loaded.image.is_some());

        let pending = ResultIconView::new(
            Some(String::from(r"C:\Program Files\Pending\pending.exe")),
            String::from("▣"),
            LAUNCHER_FONT_FAMILY,
            None,
            generation,
        );
        assert!(pending.image.is_none());
    }

    #[test]
    fn everything_file_order_is_preserved_after_app_first_ranking() {
        let application = SearchResult {
            id: String::from("application:report-viewer"),
            title: String::from("Report Viewer"),
            subtitle: String::from("Application"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\ReportViewer.lnk")),
        };
        let newest = SearchResult::file(
            String::from(r"C:\\workspace\\report-z.txt"),
            String::from("report-z.txt"),
            String::from(r"C:\\workspace"),
        );
        let older = SearchResult::file(
            String::from(r"C:\\workspace\\report-a.txt"),
            String::from("report-a.txt"),
            String::from(r"C:\\workspace"),
        );
        let newest_id = newest.id.clone();
        let older_id = older.id.clone();
        let mut merged = vec![application.clone(), older, newest];
        let provider_order = vec![merged[2].clone(), merged[1].clone()];

        preserve_everything_file_order(&mut merged, &provider_order);

        assert_eq!(merged[0].id, application.id);
        assert_eq!(merged[1].id, newest_id);
        assert_eq!(merged[2].id, older_id);
    }

    #[test]
    fn application_duplicates_merge_by_canonical_target_and_prefer_start_menu() {
        let app_paths = SearchResult {
            id: String::from("application:app-paths:chrome"),
            title: String::from("chrome"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            )),
        };
        let start_menu = SearchResult {
            id: String::from("application:start-menu:google-chrome"),
            title: String::from("Google Chrome"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(
                r"C:/Program Files/Google/Chrome/Application/chrome.exe",
            )),
        };
        let everything = SearchResult::file(
            String::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            String::from("chrome.exe"),
            String::from(r"C:\Program Files\Google\Chrome\Application"),
        );
        let merged = merge_application_duplicates(vec![app_paths, everything, start_menu]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "Google Chrome");
        assert!(merged[0].subtitle.contains("Start Menu"));
    }

    #[cfg(windows)]
    #[test]
    fn system_power_shell_merges_only_with_the_same_real_executable_path() {
        let powershell_path =
            resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
        let system = SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("powershell.exe")),
        };
        let app_path = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("PowerShell"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path),
        };
        let powershell_7 = SearchResult {
            id: String::from(r"application:target:c:\\program files\\powershell\\7\\pwsh.exe"),
            title: String::from("PowerShell 7"),
            subtitle: String::from("Application • App Paths"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(String::from(r"C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
        };

        let merged = merge_application_duplicates(vec![system, app_path, powershell_7]);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|result| result.title == "PowerShell"));
        assert!(merged.iter().any(|result| result.title == "PowerShell 7"));
    }

    #[cfg(windows)]
    #[test]
    fn post_merge_exact_console_identity_survives_catalog_collision_and_ranks_first() {
        let powershell_path =
            resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
        let system = SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(powershell_path.clone()),
        };
        let catalog = SearchResult {
            id: canonical_application_id(&powershell_path).unwrap(),
            title: String::from("Windows PowerShell"),
            subtitle: String::from("Application • Start Menu"),
            kind: ResultKind::Application,
            source: ResultSource::ApplicationCatalog,
            target: Some(powershell_path),
        };

        let mut merged = merge_application_duplicates(vec![system, catalog]);
        rank_results_with_priorities("powershell", &mut merged, &[]);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "system:powershell");
    }

    #[test]
    fn executable_icon_target_detection_accepts_shell_executables_only() {
        assert!(is_executable_icon_target(
            r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
        ));
        assert!(is_executable_icon_target(
            r"C:\\Program Files\\PowerShell\\7\\pwsh.exe"
        ));
        assert!(!is_executable_icon_target(
            r"C:\\Users\\m1nus\\PowerShell.lnk"
        ));
        assert!(!is_executable_icon_target("ms-settings:network-wifi"));
    }

    #[cfg(windows)]
    #[test]
    fn builtin_power_shell_target_is_resolved_before_merge_and_icon_loading() {
        let mut results = vec![SearchResult {
            id: String::from("system:powershell"),
            title: String::from("PowerShell"),
            subtitle: String::from("Windows PowerShell"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("powershell.exe")),
        }];

        normalize_built_in_executable_targets(&mut results);

        let target = results[0].target.as_deref().unwrap().to_ascii_lowercase();
        assert!(target.ends_with(r"\powershell.exe"));
        assert!(target.contains(r"\windowspowershell\"));
    }

    #[test]
    fn icon_target_preserves_explicit_paths_and_resolves_bare_names_on_windows() {
        let explicit = r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
        assert_eq!(icon_target_for_path(explicit), explicit);
    }

    #[cfg(windows)]
    #[test]
    fn icon_target_resolves_bare_powershell_to_a_real_path() {
        let resolved = icon_target_for_path("powershell.exe").to_ascii_lowercase();
        assert!(resolved.ends_with(r"\powershell.exe"));
        assert!(resolved.contains(r"\windowspowershell\"));
    }

    #[test]
    fn system_results_only_offer_open_and_copy_name_actions() {
        let result = SearchResult {
            id: String::from("system:settings"),
            title: String::from("Settings"),
            subtitle: String::from("Windows Settings"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("ms-settings:")),
        };
        let actions = actions_for_result(&result, &std::collections::HashMap::new());
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0].kind, super::ActionKind::Open));
        assert!(matches!(actions[1].kind, super::ActionKind::CopyName));
    }

    #[test]
    fn copy_path_always_uses_one_pair_of_quotes() {
        let result = SearchResult {
            id: String::from("file:test"),
            title: String::from("Roaming"),
            subtitle: String::new(),
            kind: ResultKind::File,
            source: ResultSource::Everything,
            target: Some(String::from(r#"C:\Users\m1nus\AppData\Roaming"#)),
        };
        assert_eq!(
            quoted_result_path(&result).as_deref(),
            Some(r#""C:\Users\m1nus\AppData\Roaming""#)
        );

        let mut already_quoted = result.clone();
        already_quoted.target = Some(String::from(r#""C:\Users\m1nus\AppData\Roaming""#));
        assert_eq!(
            quoted_result_path(&already_quoted).as_deref(),
            Some(r#""C:\Users\m1nus\AppData\Roaming""#)
        );
    }

    #[test]
    fn ctrl_r_matches_win32_other_key_event() {
        assert!(is_run_as_admin_key(&KeyEvent {
            key: Key::Other(0x52),
            pressed: true,
            shift: false,
            ctrl: true,
        }));
        assert!(!is_run_as_admin_key(&KeyEvent {
            key: Key::Other(0x52),
            pressed: true,
            shift: false,
            ctrl: false,
        }));
    }

    #[test]
    fn history_cursor_walks_older_and_newer_queries() {
        let mut cursor = None;
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(3));
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(2));
        cursor = history_cursor_step(4, cursor, Key::Up);
        assert_eq!(cursor, Some(1));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(2));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(3));
        cursor = history_cursor_step(4, cursor, Key::Down);
        assert_eq!(cursor, Some(3));
        assert_eq!(history_cursor_step(0, cursor, Key::Up), None);
    }

    #[test]
    fn stationary_pointer_after_enter_does_not_trigger_hover_selection() {
        let mut last = None;
        assert!(hover_position_changed(&mut last, (240, 120)));
        assert!(!hover_position_changed(&mut last, (240, 120)));
        assert!(hover_position_changed(&mut last, (241, 120)));
    }

    #[test]
    fn extension_aliases_normalize_only_the_everything_query_prefix() {
        assert_eq!(normalize_everything_query(".zip"), "ext:zip");
        assert_eq!(
            normalize_everything_query(".mp4 something"),
            "ext:mp4 something"
        );
        assert_eq!(
            normalize_everything_query("  .pdf  report  "),
            "ext:pdf report"
        );
        assert_eq!(normalize_everything_query("ext:zip"), "ext:zip");
        assert_eq!(normalize_everything_query("settings"), "settings");
        assert_eq!(normalize_everything_query("."), ".");
    }

    #[test]
    fn display_title_keeps_extension_and_filename_ending_visible() {
        let first = display_title("finishлицензии_0019.veg");
        let second = display_title("finishлицензии_0019_Untitled Timeline.veg");
        assert_eq!(first, "finishлицензии_0019.veg");
        assert!(first.ends_with(".veg"));
        assert!(second.ends_with(".veg"));
        assert!(second.contains("Timeline"));
        assert_ne!(first, second);
    }

    #[test]
    fn display_title_uses_middle_ellipsis_for_long_names() {
        let displayed = display_title("finishлицензии_0019_Untitled Timeline.veg");
        assert!(displayed.contains('…'));
        assert!(displayed.ends_with(".veg"));
        assert!(displayed.starts_with("finish"));
        assert!(displayed.contains("Timeline"));
        assert!(displayed.chars().count() <= 26);
    }

    #[test]
    fn activation_shows_when_flux_is_not_foreground() {
        assert!(should_show_launcher(false));
        assert!(!should_show_launcher(true));
    }

    #[test]
    fn automatic_update_always_restarts_hidden() {
        assert_eq!(
            relaunch_mode_for_auto_install(),
            super::updater::RelaunchMode::Hidden
        );
    }

    #[test]
    fn pending_non_empty_query_keeps_previous_result_list_visible() {
        assert!(!should_publish_initial_query_results(true, true, false));
        assert!(should_publish_initial_query_results(true, true, true));
    }

    #[test]
    fn synchronous_built_in_results_can_replace_list_immediately() {
        assert!(should_publish_initial_query_results(true, false, false));
        assert!(should_publish_initial_query_results(false, true, false));
    }

    #[test]
    fn core_provider_snapshot_waits_for_both_search_providers() {
        let mut providers = ProviderResults::default();
        providers.reset(7, Vec::new(), true);
        assert!(!providers.core_ready());

        providers.applications_ready = true;
        assert!(!providers.core_ready());

        providers.everything_ready = true;
        assert!(providers.core_ready());
    }

    #[test]
    fn disabled_everything_does_not_delay_application_snapshot() {
        let mut providers = ProviderResults::default();
        providers.reset(8, Vec::new(), false);
        providers.applications_ready = true;
        assert!(providers.core_ready());
    }

    #[test]
    fn builtin_snapshot_does_not_wait_for_everything() {
        let mut providers = ProviderResults::default();
        providers.reset(
            9,
            vec![SearchResult {
                id: String::from("system:wifi"),
                title: String::from("Wi-Fi"),
                subtitle: String::from("Windows Settings"),
                kind: ResultKind::Command,
                source: ResultSource::BuiltIn,
                target: Some(String::from("ms-settings:network-wifi")),
            }],
            true,
        );
        providers.applications_ready = true;
        assert!(providers.core_ready());
        assert!(!providers.everything_ready);
    }

    #[test]
    fn dimension_sliders_round_trip_at_safe_bounds() {
        assert_eq!(
            dimension_slider_fraction(MIN_LAUNCHER_WIDTH, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            0.0
        );
        assert_eq!(
            dimension_slider_fraction(MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            1.0
        );
        assert_eq!(
            dimension_from_slider(0.0, MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT),
            MIN_LAUNCHER_HEIGHT
        );
        assert_eq!(
            dimension_from_slider(1.0, MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT),
            MAX_LAUNCHER_HEIGHT
        );
        assert_eq!(
            dimension_from_slider(0.5, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            640
        );
    }

    #[test]
    fn dimension_input_clamps_out_of_range_values_and_rejects_partial_input() {
        assert_eq!(
            parse_dimension_input("100", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            Some(MIN_LAUNCHER_WIDTH)
        );
        assert_eq!(
            parse_dimension_input("1200", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            Some(MAX_LAUNCHER_WIDTH)
        );
        assert_eq!(
            parse_dimension_input("", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            None
        );
        assert_eq!(
            parse_dimension_input("abc", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
            None
        );
    }

    #[test]
    fn settings_canvas_stays_fixed_while_visual_values_change() {
        // This is the geometry contract used by the Windows slider smoke: changing
        // either visual value must not resize or drift the Settings HWND itself.
        assert_eq!(
            launcher_window_geometry_with_sizes(true, true, 640, 520),
            (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(true, false, 380, 300),
            (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(false, true, 640, 520),
            (640, 520)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(false, false, 640, 520),
            (640, COMPACT_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn custom_geometry_uses_visual_dimensions_and_keeps_compact_height() {
        assert_eq!(
            launcher_window_geometry_with_sizes(false, true, 640, 520),
            (640, 520)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(false, false, 640, 520),
            (640, COMPACT_WINDOW_HEIGHT)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(true, true, 640, 520),
            (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn activation_clear_uses_compact_geometry_after_expanded_query() {
        assert_eq!(
            launcher_window_geometry(false, true),
            (
                super::DEFAULT_LAUNCHER_WIDTH as i32,
                super::DEFAULT_LAUNCHER_HEIGHT as i32,
            )
        );
        assert_eq!(
            launcher_window_geometry(false, false),
            (super::DEFAULT_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
        );
    }
}
