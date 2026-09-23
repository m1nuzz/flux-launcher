use std::sync::{Arc, RwLock};

use flux_core::{Settings, MIN_LAUNCHER_WIDTH};
use windui::app::{App, CursorVisibilityHandle, ThemeHandle, WindowOpHandle, WindowSizeHandle};
use windui::platform::{Backdrop, Renderer};
use windui::prelude::*;
use windui::signal::Signal;

use super::keyboard_layout;
use super::launch;
use super::ui_constants::{
    COMPACT_WINDOW_HEIGHT, CURRENT_VERSION, LAUNCHER_FONT_FAMILY, SETTINGS_WINDOW_HEIGHT,
    SETTINGS_WINDOW_WIDTH,
};
use super::window_geometry::launcher_window_geometry_with_sizes;

pub(crate) fn build_shell_content(
    launcher_surface: Element,
    settings_panel: Element,
    settings_visible: Signal<bool>,
    clear_history_dialog: Element,
) -> Element {
    let launcher_page = Element::stack()
        .fill()
        .child(launcher_surface)
        .visible_when(move || !settings_visible.get());
    let settings_page = Element::col()
        .fill()
        .padding(18)
        .child(settings_panel)
        .visible_signal(settings_visible);
    if std::env::var_os("FLUX_SMOKE_SETTINGS_UI").is_some() {
        eprintln!(
            "Settings UI contract: UpdateActionVersionLabel=Current version: {CURRENT_VERSION}; SmoothCaretTab=Visual; SmoothCaretGeneral=false"
        );
    }

    Element::stack()
        .fill()
        .font_family(LAUNCHER_FONT_FAMILY)
        .child(launcher_page)
        .child(settings_page)
        .child(clear_history_dialog)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_launcher(
    app: App,
    tray: Tray,
    startup_launch: bool,
    single_instance_disabled: bool,
    window_op: WindowOpHandle,
    content: Element,
    settings_visible: Signal<bool>,
    query: Signal<String>,
    query_caret_position: Signal<usize>,
    results: Signal<Vec<flux_core::SearchResult>>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    selection_touched: Signal<bool>,
    show_results: Signal<bool>,
    history_mode: Signal<bool>,
    history_cursor: Signal<Option<usize>>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<super::result_actions::ActionItem>>,
    inline_completion: Signal<String>,
    scroll_request: Signal<bool>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    clear_query_on_activation: Signal<bool>,
    size_for_visibility: WindowSizeHandle,
    window_size: WindowSizeHandle,
    cursor_visibility: CursorVisibilityHandle,
    shared_settings: Arc<RwLock<Settings>>,
    selection_color: Signal<Color>,
    theme: ThemeHandle,
) {
    let mut app = if startup_launch {
        app.start_hidden()
    } else {
        app
    };
    let clear_for_second_instance = clear_query_on_activation;
    let settings_for_second_instance = Arc::clone(&shared_settings);
    let query_for_second_instance = query;
    let caret_for_second_instance = query_caret_position;
    let results_for_second_instance = results;
    let selected_id_for_second_instance = selected_id;
    let selected_index_for_second_instance = selected_index;
    let selection_touched_for_second_instance = selection_touched;
    let show_results_for_second_instance = show_results;
    let history_mode_for_second_instance = history_mode;
    let history_cursor_for_second_instance = history_cursor;
    let action_mode_for_second_instance = action_mode;
    let action_index_for_second_instance = action_index;
    let action_items_for_second_instance = action_items;
    let inline_completion_for_second_instance = inline_completion;
    let scroll_request_for_second_instance = scroll_request;
    let settings_visible_for_second_instance = settings_visible;
    let launcher_width_for_second_instance = launcher_width;
    let launcher_height_for_second_instance = launcher_height;
    let size_for_second_instance = size_for_visibility.clone();
    let second_instance_sender = app.channel::<()>(move |ctx, ()| {
        // Same clear-before-show contract as the activation hotkey: a second
        // launch must never present a stale query frame.
        let clear_query = settings_for_second_instance
            .read()
            .map(|settings| settings.clear_query_on_activation)
            .unwrap_or(clear_for_second_instance.get());
        if clear_query {
            super::activation_clear::clear_query_for_activation(
                query_for_second_instance,
                caret_for_second_instance,
                results_for_second_instance,
                selected_id_for_second_instance,
                selected_index_for_second_instance,
                selection_touched_for_second_instance,
                show_results_for_second_instance,
                history_mode_for_second_instance,
                history_cursor_for_second_instance,
                action_mode_for_second_instance,
                action_index_for_second_instance,
                action_items_for_second_instance,
                inline_completion_for_second_instance,
                scroll_request_for_second_instance,
                settings_visible_for_second_instance,
                launcher_width_for_second_instance,
                launcher_height_for_second_instance,
                size_for_second_instance.clone(),
            );
        }
        ctx.show_window();
    });
    let second_instance_sender_for_callback = second_instance_sender.clone();
    let second_instance_window_op = window_op.clone();
    let shutdown_window_op = window_op.clone();
    if !single_instance_disabled {
        app = app.single_instance(super::ui_constants::SINGLE_INSTANCE_ID, move |argv| {
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
        // Keep the HWND background transparent so Acrylic/DWM remains visible
        // through the launcher and its install prompt instead of adding a solid slab.
        .bg(Color::TRANSPARENT)
        .centered()
        .frameless()
        .resizable(false)
        .min_size(MIN_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
        .renderer(Renderer::Auto)
        .backdrop(Backdrop::Acrylic)
        .theme(super::theme_text::launcher_theme())
        .content(content)
        .on_window_show({
            let settings = Arc::clone(&shared_settings);
            let cursor_visibility_for_show = cursor_visibility.clone();
            let settings_visible_for_show = settings_visible;
            let size_for_show = window_size.clone();
            let theme_for_show = theme.clone();
            move || {
                cursor_visibility_for_show.show();
                if let Ok(settings) = settings.read() {
                    // Runs after .theme(), so the handle keeps the effective
                    // accent instead of being reset to the theme default.
                    super::theme_text::push_selection_appearance(
                        &theme_for_show,
                        selection_color,
                        super::theme_text::selection_rgb_for_settings(&settings),
                    );
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
                    scroll_request.set(false);
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
