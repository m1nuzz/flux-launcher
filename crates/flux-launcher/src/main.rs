#![cfg_attr(windows, windows_subsystem = "windows")]

mod accent;
mod applications;
mod everything;
mod fullscreen;
mod hotkeys;
mod keyboard_layout;
mod launch;
mod monitor;
mod plugins;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use applications::{
    canonical_application_id, canonical_application_key, ApplicationResponse, ApplicationWorker,
};
use everything::{EverythingResponse, EverythingWorker, InstallationState};
use flux_core::{
    history_results, rank_results_with_priorities, should_suppress_activation, HotkeyConfig,
    MonitorPreference, PriorityEntry, ResultKind, ResultSource, SearchModel, SearchResult,
    Settings,
};
use plugins::{FlowPluginWorker, PluginInvocation, PluginQueryResponse};
use windui::app::{CursorVisibilityHandle, WindowOpHandle, WindowPositionHandle, WindowSizeHandle};
use windui::core::{ClickFn, ClipboardProvider, EventCtx, Widget};
use windui::event::{Event, Key, KeyEvent, MouseButton, PointerKind};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

const WINDOW_WIDTH: i32 = 420;
const SETTINGS_WINDOW_WIDTH: i32 = 720;
const COMPACT_WINDOW_HEIGHT: i32 = 72;
// Keep the result palette compact like the reference while exposing a six-row
// viewport; additional results remain available through the native wheel scroll.
const EXPANDED_WINDOW_HEIGHT: i32 = 382;
const ACTION_WINDOW_HEIGHT: i32 = 250;
const RESULT_VIEWPORT_HEIGHT: i32 = 270;
const SETTINGS_WINDOW_HEIGHT: i32 = 520;
const LAUNCHER_FONT_FAMILY: &str = "Segoe UI Variable";
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const EVERYTHING_MIN_QUERY_LEN: usize = 1;
const PLUGIN_MIN_QUERY_LEN: usize = 2;
const MAX_VISIBLE_RESULTS: usize = 16;

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

fn launcher_window_geometry(settings_visible: bool, show_results: bool) -> (i32, i32) {
    if settings_visible {
        (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
    } else if show_results {
        (WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT)
    } else {
        (WINDOW_WIDTH, COMPACT_WINDOW_HEIGHT)
    }
}

#[derive(Default)]
struct ProviderResults {
    sequence: u64,
    built_in: Vec<SearchResult>,
    applications: Vec<SearchResult>,
    everything: Vec<SearchResult>,
    plugins: Vec<SearchResult>,
}

impl ProviderResults {
    fn reset(&mut self, sequence: u64, built_in: Vec<SearchResult>) {
        self.sequence = sequence;
        self.built_in = built_in;
        self.applications.clear();
        self.everything.clear();
        self.plugins.clear();
    }

    fn merged(&self, query: &str, priorities: &[String]) -> Vec<SearchResult> {
        let mut seen = HashSet::new();
        let collected = self
            .built_in
            .iter()
            .chain(&self.applications)
            .chain(&self.everything)
            .chain(&self.plugins)
            .filter(|result| seen.insert(result.id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        let mut merged = merge_application_duplicates(collected);
        rank_results_with_priorities(query, &mut merged, priorities);
        preserve_everything_file_order(&mut merged, &self.everything);
        merged.truncate(MAX_VISIBLE_RESULTS);
        merged
    }
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

        if application_source_rank(&result) < application_source_rank(&merged[existing_index]) {
            merged[existing_index] = result;
        }
    }
    merged
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
    RunPlugin(PluginInvocation),
}

#[derive(Clone, Debug)]
struct ActionItem {
    id: String,
    label: String,
    kind: ActionKind,
}

fn actions_for_result(
    result: &SearchResult,
    plugin_actions: &HashMap<String, PluginInvocation>,
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
            label: String::from("Run plugin action"),
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
    const MAX_TITLE_CHARS: usize = 20;
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= MAX_TITLE_CHARS {
        return title.to_owned();
    }
    chars
        .into_iter()
        .take(MAX_TITLE_CHARS.saturating_sub(1))
        .chain(std::iter::once('…'))
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
static SHELL_ICON_CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
static SETTINGS_SAVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(windows)]
fn shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(entries) = cache.lock() {
        if let Some(icon) = entries.get(target) {
            return icon.clone();
        }
    }

    let icon = extract_shell_thumbnail_rgba(target).or_else(|| extract_shell_icon_rgba(target));
    if let Ok(mut entries) = cache.lock() {
        entries.insert(target.to_owned(), icon.clone());
    }
    icon
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
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_LARGEICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

    const ICON_SIZE: i32 = 32;
    let path: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
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
    if result == 0 || file_info.hIcon.is_invalid() {
        return None;
    }

    let hdc = unsafe { CreateCompatibleDC(None) };
    if hdc.is_invalid() {
        unsafe { DestroyIcon(file_info.hIcon).ok()? };
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
            DestroyIcon(file_info.hIcon).ok();
            let _ = DeleteDC(hdc);
        }
        return None;
    };
    let previous = unsafe { SelectObject(hdc, HGDIOBJ(bitmap.0)) };
    let drawn = unsafe {
        DrawIconEx(
            hdc,
            0,
            0,
            file_info.hIcon,
            ICON_SIZE,
            ICON_SIZE,
            0,
            None,
            DI_NORMAL,
        )
        .is_ok()
    };
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
        DestroyIcon(file_info.hIcon).ok();
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

#[allow(clippy::too_many_arguments)]
fn result_row(
    result: SearchResult,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginInvocation>>>,
    query: Signal<String>,
    scroll_pending: Signal<bool>,
    selection_color: Signal<Color>,
    settings: Arc<RwLock<Settings>>,
    query_history: Rc<RefCell<Vec<String>>>,
    history_mode: Signal<bool>,
    recycle_bin_confirmation: Signal<bool>,
    settings_visible: Signal<bool>,
    window_size: WindowSizeHandle,
) -> Element {
    let id = result.id;
    let target = result.target;
    let title = result.title;
    let subtitle = result.subtitle;
    let (glyph, glyph_font) = match id.as_str() {
        "empty-recycle-bin" => (String::from("\u{ea99}"), "Segoe Fluent Icons"),
        "open-recycle-bin" => (String::from("\u{e74d}"), "Segoe Fluent Icons"),
        _ if subtitle.contains("Application") => (String::from("◉"), LAUNCHER_FONT_FAMILY),
        _ => (String::from("▣"), LAUNCHER_FONT_FAMILY),
    };
    let icon = target.as_deref().and_then(shell_icon_rgba);
    let icon_element = if let Some(icon) = icon.as_deref() {
        Element::image_rgba(32, 32, icon)
            .width(32)
            .height(32)
            .corner(7.0)
    } else {
        Element::label(glyph)
            .font_family(glyph_font)
            .font_size(20.0)
            .fg(Color::rgba(201, 218, 240, 235))
            .width(28)
            .align(Align::Center)
    };
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
                window_size.set(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
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
    let settings = Settings::load_or_default();
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
    let current_sequence = signal(0_u64);
    let game_mode = signal(settings.game_mode);
    let game_mode_status = signal(game_mode_label(settings.game_mode));
    let settings_visible = signal(std::env::var_os("FLUX_OPEN_SETTINGS").is_some());
    let settings_tab = signal(0_usize);
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
    let clear_query_on_activation = signal(settings.clear_query_on_activation);
    let auto_enable_everything = signal(settings.auto_enable_everything);
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
    let plugin_actions = Rc::new(RefCell::new(HashMap::<String, PluginInvocation>::new()));
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
    let settings_visible_for_rows = settings_visible;
    let size_for_rows = window_size.clone();
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
    let action_bar = Element::row()
        .width_match()
        .height(28)
        .padding_xy(4, 2)
        .spacing(8)
        .child(action_hint("↵", "Open"))
        .child(action_hint("Ctrl + R", "Run as admin"))
        .child(action_hint("Alt + Enter", "Open file location"))
        .child(
            Element::label_signal(status)
                .font_size(9.0)
                .fg(Color::rgba(210, 224, 244, 175))
                .max_lines(1)
                .truncate(Truncate::End)
                .weight(1.0),
        )
        .visible_when(move || show_results.get() && !action_mode.get());

    let result_list_body = Element::host_signal(result_source, move |result| {
        result_row(
            result,
            selected_for_rows,
            selected_index_for_rows,
            selection_touched_for_rows,
            result_source,
            Rc::clone(&actions_for_rows),
            query_for_rows,
            scroll_request_for_rows,
            selection_color,
            Arc::clone(&settings_for_rows),
            Rc::clone(&history_for_rows),
            history_mode_for_rows,
            recycle_bin_confirmation,
            settings_visible_for_rows,
            size_for_rows.clone(),
        )
    })
    .width_match()
    // Keep the result body transparent so the window remains one continuous
    // Acrylic surface. Only individual result rows draw controls.
    .padding(6);
    let result_list = Element::scroll()
        .width_match()
        .height(RESULT_VIEWPORT_HEIGHT)
        .child(result_list_body)
        .visible_when(move || show_results.get() && !action_mode.get());

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
                            handle.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
                        }
                    }
                })
        },
    )
    .height(174)
    .corner(12.0)
    .visible_signal(action_mode);

    // The HWND itself owns the system Acrylic surface. Keep this root transparent so
    // the blur fills the complete 420px client area instead of becoming an inset card.
    let launcher_content = Element::col()
        .width(364)
        .padding(10)
        .spacing(4)
        .child(search_box)
        .child(result_list)
        .child(action_bar)
        .child(action_list)
        .child(recycle_bin_dialog);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Center));

    let query_for_interval = query;
    let results_for_interval = results;
    let status_for_interval = status;
    let show_results_for_interval = show_results;
    let inline_completion_for_interval = inline_completion;
    let selection_touched_for_interval = selection_touched;
    let sequence_for_interval = current_sequence;
    let providers_for_interval = Rc::clone(&provider_results);
    let scroll_request_for_interval = scroll_request_for_rows;
    let priorities_for_interval = priorities;
    let actions_for_interval = Rc::clone(&plugin_actions);
    let auto_enable_everything_for_interval = auto_enable_everything;
    let history_mode_for_interval = history_mode;
    let settings_visible_for_interval = settings_visible;
    let tray_settings_smoke_pending_for_interval = Rc::clone(&tray_settings_smoke_pending);
    let mut last_query = String::new();
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
        WINDOW_WIDTH
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
    let window_op: WindowOpHandle = app.window_op_handle();
    let cursor_visibility: CursorVisibilityHandle = app.cursor_visibility_handle();
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
        let merged = providers.merged(
            &query_for_applications.get(),
            &priorities_for_applications
                .get()
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
        );
        if !selection_touched_for_applications.get() {
            selected_index_for_applications.set(0);
            selected_id_for_applications.set(
                merged
                    .first()
                    .map(|result| result.id.clone())
                    .unwrap_or_default(),
            );
        }
        inline_completion_for_applications.set(inline_completion_suffix(
            &query_for_applications.get(),
            &merged,
        ));
        results_for_applications.set(merged);
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
            || response.query != query_for_everything.get()
        {
            return;
        }
        let mut providers = providers_for_everything.borrow_mut();
        if providers.sequence != response.sequence {
            return;
        }
        if response.available {
            everything_installed_for_response.set(true);
            everything_status_for_response.set(String::from("Everything IPC is available"));
            providers.everything = response.results;
            let merged = providers.merged(
                &query_for_everything.get(),
                &priorities_for_everything
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>(),
            );
            if !selection_touched_for_everything.get() {
                selected_index_for_everything.set(0);
                selected_id_for_everything.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
            }
            inline_completion_for_everything.set(inline_completion_suffix(
                &query_for_everything.get(),
                &merged,
            ));
            results_for_everything.set(merged);
        } else if everything_installed_for_response.get() {
            everything_status_for_response.set(String::from(
                "Everything is installed but its local IPC is unavailable",
            ));
        } else {
            everything_status_for_response.set(String::from(
                "Everything is not installed. Install it with winget to enable file search.",
            ));
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
            let merged = providers.merged(
                &query_for_plugins.get(),
                &priorities_for_plugins
                    .get()
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<Vec<_>>(),
            );
            if !selection_touched_for_plugins.get() {
                selected_index_for_plugins.set(0);
                selected_id_for_plugins.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
            }
            inline_completion_for_plugins
                .set(inline_completion_suffix(&query_for_plugins.get(), &merged));
            results_for_plugins.set(merged);
        }
        status_for_plugins.set(response.status);
    });
    let plugin_worker = FlowPluginWorker::spawn(plugin_sender);

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
                let (compact_width, compact_height) =
                    launcher_window_geometry(settings_visible_for_activation.get(), false);
                size_for_activation.set(compact_width, compact_height);
            }
            let (width, height) = launcher_window_geometry(
                settings_visible_for_activation.get(),
                show_results_for_activation.get(),
            );
            request_monitor_position(
                &position_for_activation,
                settings.monitor_preference,
                width,
                height,
            );
            cursor_visibility_for_activation.show();
            ctx.toggle_window();
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
            size_for_keys.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
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
                    size_for_keys.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
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
                    size_for_keys.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
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
                        size_for_keys.set(WINDOW_WIDTH, ACTION_WINDOW_HEIGHT);
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
                EXPANDED_WINDOW_HEIGHT
            } else {
                COMPACT_WINDOW_HEIGHT
            };
            if let Ok(settings) = settings_for_left_click.read() {
                request_monitor_position(
                    &position_for_left_click,
                    settings.monitor_preference,
                    WINDOW_WIDTH,
                    height,
                );
            }
            size_for_left_click.set(WINDOW_WIDTH, height);
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item("Show launcher", move |ctx| {
                settings_visible_for_tray.set(false);
                let height = if show_results_for_tray.get() {
                    EXPANDED_WINDOW_HEIGHT
                } else {
                    COMPACT_WINDOW_HEIGHT
                };
                if let Ok(settings) = settings_for_tray_position.read() {
                    request_monitor_position(
                        &position_for_tray,
                        settings.monitor_preference,
                        WINDOW_WIDTH,
                        height,
                    );
                }
                size_for_tray.set(WINDOW_WIDTH, height);
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
                // show_window invokes on_window_show, which clears the normal
                // launcher state. Apply the Settings size after that lifecycle
                // callback so it cannot overwrite the 520px panel height.
                ctx.show_window();
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
    let size_for_back = window_size.clone();
    let size_for_apply = window_size.clone();
    let settings_for_clear_history = Arc::clone(&shared_settings);
    let history_for_clear = Rc::clone(&query_history);
    let history_cursor_for_clear = history_cursor;
    let auto_enable_everything_for_apply = auto_enable_everything;
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
                    vec!["General", "Priorities"],
                    settings_tab,
                ))
                .child(
                    Element::button("Back")
                        .neutral()
                        .on_click(move |_| {
                            settings_visible.set(false);
                            size_for_back.set(
                                WINDOW_WIDTH,
                                if show_results_for_back.get() {
                                    EXPANDED_WINDOW_HEIGHT
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                },
                            );
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
                        "Selection color",
                        Element::checkbox(
                            "Use the Windows 11 system accent color",
                            use_system_accent,
                        ),
                    ))
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
                        "Query on activation",
                        Element::checkbox(
                            "Clear the previous query when opened with the global hotkey",
                            clear_query_on_activation,
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
                            .spacing(6)
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
                            if let Ok(mut settings) = settings_for_apply.write() {
                                settings.activation_hotkey = configuration;
                                settings.ignore_hotkeys_in_fullscreen = ignore_fullscreen.get();
                                settings.game_mode = game_mode.get();
                                settings.smooth_caret = smooth_caret.get();
                                settings.switch_to_english_layout = switch_to_english_layout.get();
                                settings.use_system_accent = use_system_accent.get();
                                settings.custom_selection_color = custom_color;
                                settings.clear_query_on_activation = clear_query_on_activation.get();
                                settings.auto_enable_everything = auto_enable_everything_for_apply.get();
                                settings.monitor_preference = monitor_preference_from_index(monitor_preference.get());
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                activation_recording_for_apply.set(false);
                                activation_display_for_apply
                                    .set(hotkeys::display_config(&settings.activation_hotkey));
                                selection_color.set(selection_color_for_settings(&settings));
                                custom_selection_color.set(selection_color_hex(settings.custom_selection_color));
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
                            }
                            settings_visible_for_apply.set(false);
                            let selected_preference = monitor_preference_from_index(monitor_preference.get());
                            request_monitor_position(
                                &position_for_apply,
                                selected_preference,
                                WINDOW_WIDTH,
                                if show_results.get() {
                                    EXPANDED_WINDOW_HEIGHT
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                },
                            );
                            size_for_apply.set(
                                WINDOW_WIDTH,
                                if show_results.get() {
                                    EXPANDED_WINDOW_HEIGHT
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                },
                            );
                            ctx.toast_ok("Settings applied");
                        }),
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

    app.tray(tray)
        .hide_on_close()
        // The Win32 backend keeps this transparent on local Acrylic-capable
        // sessions and uses this dark color only for an honest RDP fallback.
        .bg(Color::rgba(32, 33, 35, 255))
        .centered()
        .frameless()
        .resizable(false)
        .min_size(380, COMPACT_WINDOW_HEIGHT)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        .theme(launcher_theme())
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |ctx| {
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
                WINDOW_WIDTH,
                if has_query {
                    EXPANDED_WINDOW_HEIGHT
                } else {
                    COMPACT_WINDOW_HEIGHT
                },
            );
            sequence = sequence.wrapping_add(1);
            sequence_for_interval.set(sequence);
            model.set_query(&next_query);
            {
                let mut providers = providers_for_interval.borrow_mut();
                providers.reset(sequence, model.results().to_vec());
                let merged = providers.merged(
                    &next_query,
                    &priorities_for_interval
                        .get()
                        .iter()
                        .map(|entry| entry.id.clone())
                        .collect::<Vec<_>>(),
                );
                selection_touched_for_interval.set(false);
                selected_index.set(0);
                selected_id.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
                inline_completion_for_interval.set(inline_completion_suffix(&next_query, &merged));
                results_for_interval.set(merged);
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
                // query. Pass the raw query unchanged so native Everything syntax
                // such as `ext:zip`, `parent:`, `file:`, and `dm:today` works.
                if auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN
                {
                    everything_worker.request(sequence, next_query.clone());
                }
                if next_query.trim().len() >= PLUGIN_MIN_QUERY_LEN {
                    plugin_worker.request(sequence, next_query.clone());
                }
            }
            last_query = next_query;
        })
        .on_window_show({
            let settings = Arc::clone(&shared_settings);
            let cursor_visibility_for_show = cursor_visibility.clone();
            move || {
                cursor_visibility_for_show.show();
                if let Ok(settings) = settings.read() {
                    selection_color.set(selection_color_for_settings(&settings));
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
                    let (width, height) = launcher_window_geometry(settings_visible.get(), false);
                    size_for_visibility.set(width, height);
                }
            }
        })
        .run();
}

#[cfg(test)]
mod tests {
    use super::{
        actions_for_result, history_cursor_step, hover_position_changed, is_run_as_admin_key,
        launcher_window_geometry, merge_application_duplicates, preserve_everything_file_order,
        quoted_result_path, COMPACT_WINDOW_HEIGHT, EXPANDED_WINDOW_HEIGHT, WINDOW_WIDTH,
    };
    use flux_core::{ResultKind, ResultSource, SearchResult};
    use windui::event::{Key, KeyEvent};

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
    fn activation_clear_uses_compact_geometry_after_expanded_query() {
        assert_eq!(
            launcher_window_geometry(false, true),
            (WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT)
        );
        assert_eq!(
            launcher_window_geometry(false, false),
            (WINDOW_WIDTH, COMPACT_WINDOW_HEIGHT)
        );
    }
}
