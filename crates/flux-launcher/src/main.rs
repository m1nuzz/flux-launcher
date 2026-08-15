#![cfg_attr(windows, windows_subsystem = "windows")]

mod applications;
mod everything;
mod fullscreen;
mod hotkeys;
mod launch;
mod plugins;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use applications::{ApplicationResponse, ApplicationWorker};
use everything::{EverythingResponse, EverythingWorker};
use flux_core::{
    rank_results, should_suppress_activation, HotkeyConfig, SearchModel, SearchResult, Settings,
};
use plugins::{FlowPluginWorker, PluginInvocation, PluginQueryResponse};
use windui::app::WindowSizeHandle;
use windui::core::{ClipboardProvider, EventCtx, Widget};
use windui::event::{Key, KeyEvent};
use windui::prelude::*;
use windui::render::{Canvas, Paint};

const WINDOW_WIDTH: i32 = 420;
const COMPACT_WINDOW_HEIGHT: i32 = 72;
// Keep the result palette compact like the reference: search header, three visible
// rows, and the command bar fit inside a short floating surface.
const EXPANDED_WINDOW_HEIGHT: i32 = 286;
const ACTION_WINDOW_HEIGHT: i32 = 250;
const SETTINGS_WINDOW_HEIGHT: i32 = 520;
const SEARCH_INTERVAL: Duration = Duration::from_millis(40);
const PROVIDER_MIN_QUERY_LEN: usize = 2;
const MAX_VISIBLE_RESULTS: usize = 8;

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

    fn merged(&self, query: &str) -> Vec<SearchResult> {
        let mut seen = HashSet::new();
        let mut merged = self
            .built_in
            .iter()
            .chain(&self.applications)
            .chain(&self.everything)
            .chain(&self.plugins)
            .filter(|result| seen.insert(result.id.clone()))
            .cloned()
            .collect::<Vec<_>>();
        rank_results(query, &mut merged);
        merged.truncate(MAX_VISIBLE_RESULTS);
        merged
    }
}

#[derive(Clone, Debug)]
enum ActionKind {
    Open,
    RunAsAdmin,
    OpenLocation,
    CopyPath,
    CopyName,
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
    if result.target.is_some() {
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
        actions.push(ActionItem {
            id: format!("{}:copy-path", result.id),
            label: String::from("Copy path"),
            kind: ActionKind::CopyPath,
        });
    }
    if let Some(invocation) = plugin_actions.get(&result.id).cloned() {
        actions.push(ActionItem {
            id: format!("{}:plugin", result.id),
            label: String::from("Run plugin action"),
            kind: ActionKind::RunPlugin(invocation),
        });
    }
    actions.push(ActionItem {
        id: format!("{}:copy-name", result.id),
        label: String::from("Copy name"),
        kind: ActionKind::CopyName,
    });
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
    title_signal: Signal<String>,
    trailing_signal: Signal<String>,
    selected_id: Signal<String>,
    last_selected: Option<bool>,
}

impl Widget for ResultRowAnchor {
    fn on_update(&mut self, ctx: &mut EventCtx) {
        let selected = self.selected_id.get() == self.result_id;
        if self.last_selected != Some(selected) {
            self.title_signal.set(if selected {
                format!("> {}", self.title)
            } else {
                self.title.clone()
            });
            self.trailing_signal.set(if selected {
                String::from("↵")
            } else {
                String::new()
            });
            self.last_selected = Some(selected);
        }
        if selected {
            let row_id = ctx.id();
            let _ = ctx.tree_mut().scroll_into_view(row_id);
        }
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
            Color::rgba(76, 139, 245, 72)
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

fn execute_result_action(result: &SearchResult, action: &ActionKind) {
    match action {
        ActionKind::Open => {
            if let Some(target) = result.target.as_deref() {
                let _ = launch::open_path(target);
            }
        }
        ActionKind::RunAsAdmin => {
            if let Some(target) = result.target.as_deref() {
                let _ = launch::run_as_admin(target);
            }
        }
        ActionKind::OpenLocation => {
            if let Some(target) = result.target.as_deref() {
                let _ = launch::open_file_location(target);
            }
        }
        ActionKind::CopyPath => {
            if let Some(target) = result.target.as_deref() {
                windui::platform::Clipboard.set_text(target);
            }
        }
        ActionKind::CopyName => windui::platform::Clipboard.set_text(&result.title),
        ActionKind::RunPlugin(invocation) => plugins::execute_async(invocation.clone()),
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

#[cfg(windows)]
fn shell_icon_rgba(target: &str) -> Option<Vec<u8>> {
    let cache = SHELL_ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(entries) = cache.lock() {
        if let Some(icon) = entries.get(target) {
            return icon.clone();
        }
    }

    let icon = extract_shell_icon_rgba(target);
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
        SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_ICON, SHGFI_SMALLICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL};

    const ICON_SIZE: i32 = 24;
    let path: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let mut file_info = SHFILEINFOW::default();
    let flags = SHGFI_FLAGS(SHGFI_ICON.0 | SHGFI_SMALLICON.0);
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
    theme.palette.surface = Color::rgba(24, 31, 44, 180);
    theme.palette.surface_alt = Color::rgba(31, 40, 56, 205);
    theme.palette.border = Color::rgba(255, 255, 255, 22);
    // The Search control is transparent, so its foreground must stay readable
    // over both dark and light Acrylic samples. Keep ordinary text neutral and
    // opaque; reserve accent blue for selection/focus feedback only.
    theme.palette.text = Color::rgba(250, 252, 255, 255);
    theme.palette.placeholder = Color::rgba(238, 243, 255, 230);
    theme.input.bg = Some(Color::rgba(21, 27, 39, 188));
    theme.input.border = Some(Color::rgba(255, 255, 255, 24));
    theme.input.border_focus = Some(Color::rgba(133, 181, 255, 135));
    theme.input.text = Some(Color::rgba(250, 252, 255, 255));
    theme.input.placeholder = Some(Color::rgba(238, 243, 255, 230));
    theme.input.selection = Some(Color::rgba(76, 139, 245, 150));
    theme.input.cursor = Some(Color::rgba(255, 255, 255, 255));
    theme
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
        let _ = settings.save();
    }
}

fn result_row(
    result: SearchResult,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    rows_refresh: Signal<Vec<SearchResult>>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginInvocation>>>,
) -> Element {
    let id = result.id;
    let target = result.target;
    let title = result.title;
    let subtitle = result.subtitle;
    let glyph = if subtitle.contains("Application") {
        String::from("◉")
    } else {
        String::from("▣")
    };
    let icon = target.as_deref().and_then(shell_icon_rgba);
    let icon_element = if let Some(icon) = icon.as_deref() {
        Element::image_rgba(24, 24, icon)
            .width(28)
            .height(28)
            .corner(6.0)
    } else {
        Element::label(glyph)
            .font_size(20.0)
            .fg(Color::rgba(201, 218, 240, 235))
            .text_shadow(Color::rgba(8, 12, 20, 180))
            .width(28)
            .align(Align::Center)
    };
    let selected = selected_id.get() == id;
    let title_signal = signal(if selected {
        format!("> {title}")
    } else {
        title.clone()
    });
    let trailing_signal = signal(if selected {
        String::from("↵")
    } else {
        String::new()
    });
    Element::row()
        .widget(ResultRowAnchor {
            result_id: id.clone(),
            title: title.clone(),
            title_signal,
            trailing_signal,
            selected_id,
            last_selected: None,
        })
        .reactive()
        .width_match()
        .height(44)
        .padding_xy(12, 4)
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
                    Element::label_signal(title_signal)
                        .font_size(14.0)
                        .fg(Color::rgba(250, 252, 255, 255))
                        .text_shadow(Color::rgba(8, 12, 20, 210))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                )
                .child(
                    Element::label(subtitle)
                        .font_size(11.0)
                        .fg(Color::rgba(240, 246, 255, 238))
                        .text_shadow(Color::rgba(8, 12, 20, 185))
                        .max_lines(1)
                        .truncate(Truncate::End)
                        .width_match(),
                ),
        )
        .child(
            Element::label_signal(trailing_signal)
                .font_size(17.0)
                .fg(Color::rgba(238, 246, 255, 230))
                .text_shadow(Color::rgba(8, 12, 20, 190))
                .width(22)
                .align(Align::Center),
        )
        .on_click(move |_| {
            selected_id.set(id.clone());
            selection_touched.set(true);
            if let Some(index) = rows_refresh.get().iter().position(|result| result.id == id) {
                selected_index.set(index);
            }
            rows_refresh.set(rows_refresh.get());
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
    let selected_index = signal(0_usize);
    let selection_touched = signal(false);
    let action_mode = signal(false);
    let action_index = signal(0_usize);
    let action_items = signal(Vec::<ActionItem>::new());
    let action_window_slot = Rc::new(RefCell::new(None::<WindowSizeHandle>));
    let status = signal(String::from("Ready"));
    let current_sequence = signal(0_u64);
    let game_mode = signal(settings.game_mode);
    let game_mode_status = signal(game_mode_label(settings.game_mode));
    let settings_visible = signal(std::env::var_os("FLUX_OPEN_SETTINGS").is_some());
    let show_results = signal(false);
    let activation_key = signal(settings.activation_hotkey.key.clone());
    let activation_ctrl = signal(settings.activation_hotkey.ctrl);
    let activation_alt = signal(settings.activation_hotkey.alt);
    let activation_shift = signal(settings.activation_hotkey.shift);
    let activation_meta = signal(settings.activation_hotkey.meta);
    let ignore_fullscreen = signal(settings.ignore_hotkeys_in_fullscreen);
    let smooth_caret = signal(settings.smooth_caret);
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
    let action_items_for_rows = action_items;
    let action_index_for_rows = action_index;
    let action_mode_for_rows = action_mode;
    let action_window_slot_for_rows = Rc::clone(&action_window_slot);

    let search_box = Element::text_input(query, "Search")
        .leading_icon('⌕')
        .transparent_surface()
        .smooth_caret(settings.smooth_caret, settings.smooth_caret_duration_ms)
        .width_match()
        .height(44)
        .font_size(15.0)
        .font_weight(500)
        .text_shadow(Color::rgba(8, 12, 20, 220))
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
                    .fg(Color::rgba(222, 233, 248, 220))
                    .text_shadow(Color::rgba(8, 12, 20, 140)),
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
        .visible_when(move || show_results.get() && !action_mode.get());

    let result_list = Element::list_signal(
        result_source,
        |result| result.id.clone(),
        move |result| {
            result_row(
                result,
                selected_for_rows,
                selected_index_for_rows,
                selection_touched_for_rows,
                result_source,
                Rc::clone(&actions_for_rows),
            )
        },
    )
    // Keep the expanded result area transparent so the window remains one
    // continuous Acrylic surface. Only individual result rows draw controls.
    .height(156)
    .padding(6)
    .visible_when(move || show_results.get() && !action_mode.get());

    let action_list = Element::list_signal(
        action_items_for_rows,
        |item| item.id.clone(),
        move |item| {
            let item_id = item.id.clone();
            let item_label = item.label.clone();
            let item_kind = item.kind.clone();
            let is_selected = action_items_for_rows
                .get()
                .iter()
                .position(|candidate| candidate.id == item_id)
                .map(|index| index == action_index_for_rows.get())
                .unwrap_or(false);
            Element::row()
                .width_match()
                .height(42)
                .padding_xy(12, 5)
                .corner(9.0)
                .bg(if is_selected {
                    Color::rgba(76, 139, 245, 92)
                } else {
                    Color::rgba(255, 255, 255, 14)
                })
                .child(
                    Element::label(if is_selected {
                        format!("> {item_label}")
                    } else {
                        item_label
                    })
                    .font_size(13.0)
                    .fg(Color::WHITE)
                    .weight(1.0),
                )
                .on_click({
                    let action_window_slot = action_window_slot_for_rows.clone();
                    move |_| {
                        if let Some(result) = selected_result(
                            &result_source.get(),
                            &selected_for_rows.get(),
                            selected_index_for_rows.get(),
                        ) {
                            execute_result_action(&result, &item_kind);
                        }
                        action_mode_for_rows.set(false);
                        if let Some(handle) = action_window_slot.borrow().as_ref() {
                            handle.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
                        }
                    }
                })
        },
    )
    .height(138)
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
        .child(action_list);
    let launcher_surface = Element::stack()
        .fill()
        .bg(Color::rgba(0, 0, 0, 0))
        .child(launcher_content.align(Align::Center));

    let query_for_interval = query;
    let results_for_interval = results;
    let status_for_interval = status;
    let show_results_for_interval = show_results;
    let selection_touched_for_interval = selection_touched;
    let sequence_for_interval = current_sequence;
    let providers_for_interval = Rc::clone(&provider_results);
    let actions_for_interval = Rc::clone(&plugin_actions);
    let mut last_query = String::new();
    let mut sequence = 0_u64;

    let initial_height = if settings_visible.get() {
        SETTINGS_WINDOW_HEIGHT
    } else {
        COMPACT_WINDOW_HEIGHT
    };
    let window_icon = tray_icon();
    let mut app =
        App::new("Flux Launcher", WINDOW_WIDTH, initial_height).icon_rgba(16, 16, &window_icon);
    let window_size = app.window_size_handle();
    *action_window_slot.borrow_mut() = Some(window_size.clone());
    let size_for_interval = window_size.clone();
    let query_for_applications = query;
    let results_for_applications = results;
    let status_for_applications = status;
    let selected_id_for_applications = selected_id;
    let selected_index_for_applications = selected_index;
    let selection_touched_for_applications = selection_touched;
    let sequence_for_applications = current_sequence;
    let providers_for_applications = Rc::clone(&provider_results);
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
        let merged = providers.merged(&query_for_applications.get());
        if !selection_touched_for_applications.get() {
            selected_index_for_applications.set(0);
            selected_id_for_applications.set(
                merged
                    .first()
                    .map(|result| result.id.clone())
                    .unwrap_or_default(),
            );
        }
        results_for_applications.set(merged);
        status_for_applications.set(response.status);
    });
    let application_worker = ApplicationWorker::spawn(application_sender);

    let query_for_everything = query;
    let results_for_everything = results;
    let status_for_everything = status;
    let selected_id_for_everything = selected_id;
    let selected_index_for_everything = selected_index;
    let selection_touched_for_everything = selection_touched;
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
            let merged = providers.merged(&query_for_everything.get());
            if !selection_touched_for_everything.get() {
                selected_index_for_everything.set(0);
                selected_id_for_everything.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
            }
            results_for_everything.set(merged);
        }
        status_for_everything.set(response.status);
    });
    let everything_worker = EverythingWorker::spawn(everything_sender);

    let query_for_plugins = query;
    let results_for_plugins = results;
    let status_for_plugins = status;
    let selected_id_for_plugins = selected_id;
    let selected_index_for_plugins = selected_index;
    let selection_touched_for_plugins = selection_touched;
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
            let merged = providers.merged(&query_for_plugins.get());
            if !selection_touched_for_plugins.get() {
                selected_index_for_plugins.set(0);
                selected_id_for_plugins.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
            }
            results_for_plugins.set(merged);
        }
        status_for_plugins.set(response.status);
    });
    let plugin_worker = FlowPluginWorker::spawn(plugin_sender);

    let settings_for_activation = Arc::clone(&shared_settings);
    let activation_handle = app.hotkey_handle(activation_hotkey, move |ctx| {
        let settings = settings_for_activation
            .read()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        if !should_suppress_activation(&settings, fullscreen::foreground_is_fullscreen()) {
            ctx.show_window();
        }
    });

    let query_for_keys = query;
    let results_for_keys = results;
    let selected_id_for_keys = selected_id;
    let selected_index_for_keys = selected_index;
    let selection_touched_for_keys = selection_touched;
    let action_mode_for_keys = action_mode;
    let action_index_for_keys = action_index;
    let action_items_for_keys = action_items;
    let plugin_actions_for_keys = Rc::clone(&plugin_actions);
    let settings_visible_for_keys = settings_visible;
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
        if !event.pressed || settings_visible_for_keys.get() {
            return false;
        }
        let query = query_for_keys.get();
        if query.trim().is_empty() {
            return false;
        }
        let current_results = results_for_keys.get();
        if current_results.is_empty() {
            return false;
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
                            execute_result_action(&result, &action.kind);
                        }
                    }
                    action_mode_for_keys.set(false);
                    size_for_keys.set(WINDOW_WIDTH, EXPANDED_WINDOW_HEIGHT);
                    return true;
                }
                _ => return true,
            }
        }

        if event.ctrl && matches!(event.key, Key::Char('r') | Key::Char('R')) {
            if let Some(result) = selected_result(
                &current_results,
                &selected_id_for_keys.get(),
                selected_index_for_keys.get(),
            ) {
                if let Some(target) = result.target.as_deref() {
                    let _ = launch::run_as_admin(target);
                }
            }
            return true;
        }
        if event.key == Key::Enter && alt_key_is_down() {
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
                results_for_keys.set(current_results);
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
                if let Some(result) = selected_result(
                    &current_results,
                    &selected_id_for_keys.get(),
                    selected_index_for_keys.get(),
                ) {
                    if let Some(target) = result.target.as_deref() {
                        let _ = launch::open_path(target);
                    } else if let Some(action) =
                        plugin_actions_for_keys.borrow().get(&result.id).cloned()
                    {
                        plugins::execute_async(action);
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
    let show_results_for_tray = show_results;
    let size_for_tray = window_size.clone();
    let size_for_settings = window_size.clone();
    let tray = Tray::new()
        .tooltip("Flux Launcher")
        .icon_rgba(16, 16, &tray_icon())
        .on_left_click(move |ctx| {
            settings_visible_for_left_click.set(false);
            size_for_left_click.set(
                WINDOW_WIDTH,
                if show_results_for_left_click.get() {
                    EXPANDED_WINDOW_HEIGHT
                } else {
                    COMPACT_WINDOW_HEIGHT
                },
            );
            ctx.show_window();
        })
        .menu(vec![
            TrayMenuItem::item("Show launcher", move |ctx| {
                settings_visible_for_tray.set(false);
                size_for_tray.set(
                    WINDOW_WIDTH,
                    if show_results_for_tray.get() {
                        EXPANDED_WINDOW_HEIGHT
                    } else {
                        COMPACT_WINDOW_HEIGHT
                    },
                );
                ctx.show_window();
            }),
            TrayMenuItem::item("Settings", move |ctx| {
                settings_visible.set(true);
                size_for_settings.set(WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
                ctx.show_window();
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
    let activation_handle_for_apply = activation_handle.clone();
    let game_mode_status_for_apply = game_mode_status;
    let settings_visible_for_apply = settings_visible;
    let show_results_for_back = show_results;
    let size_for_back = window_size.clone();
    let size_for_apply = window_size.clone();
    let settings_panel = Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(18, 22, 30, 212))
        .border(Color::rgba(255, 255, 255, 48), 1)
        .shadow(Shadow::new(0.0, 18.0, 48.0, Color::rgba(0, 0, 0, 110)))
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
            Element::scroll().weight(1.0).child(
                Element::col()
                    .width_match()
                    .spacing(12)
                    .child(Element::field(
                        "Activation key",
                        Element::text_input(activation_key, "Space").width_match(),
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
                            if let Ok(mut settings) = settings_for_apply.write() {
                                settings.activation_hotkey = configuration;
                                settings.ignore_hotkeys_in_fullscreen = ignore_fullscreen.get();
                                settings.game_mode = game_mode.get();
                                settings.smooth_caret = smooth_caret.get();
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                activation_handle_for_apply
                                    .set(hotkeys::activation_hotkey(&settings.activation_hotkey));
                                game_mode_status_for_apply.set(game_mode_label(settings.game_mode));
                                let _ = settings.save();
                            }
                            settings_visible_for_apply.set(false);
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
        .child(launcher_page)
        .child(settings_page);

    app.tray(tray)
        .hide_on_close()
        // The Win32 backend keeps this transparent on local Acrylic-capable
        // sessions and uses this dark color only for an honest RDP fallback.
        .bg(Color::rgba(24, 31, 44, 255))
        .centered()
        .frameless()
        .resizable(false)
        .min_size(380, COMPACT_WINDOW_HEIGHT)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        .theme(launcher_theme())
        .content(content)
        .on_interval(SEARCH_INTERVAL, move |_| {
            let next_query = query_for_interval.get();
            if next_query == last_query {
                return;
            }

            let has_query = !next_query.trim().is_empty();
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
                let merged = providers.merged(&next_query);
                selection_touched_for_interval.set(false);
                selected_index.set(0);
                selected_id.set(
                    merged
                        .first()
                        .map(|result| result.id.clone())
                        .unwrap_or_default(),
                );
                results_for_interval.set(merged);
            }
            action_mode.set(false);
            action_index.set(0);
            action_items.set(Vec::new());
            actions_for_interval.borrow_mut().clear();
            if !has_query {
                status_for_interval.set(String::from("Ready"));
            } else {
                status_for_interval.set(String::from(
                    "Searching applications, Everything and native Flow plugins...",
                ));
                application_worker.request(sequence, next_query.clone());
                if next_query.trim().len() >= PROVIDER_MIN_QUERY_LEN {
                    everything_worker.request(sequence, next_query.clone());
                    plugin_worker.request(sequence, next_query.clone());
                }
            }
            last_query = next_query;
        })
        .run();
}
