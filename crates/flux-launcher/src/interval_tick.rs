use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{atomic::Ordering, Arc, RwLock};

use flux_core::{
    path_query_body, MonitorPreference, PriorityEntry, SearchModel, SearchResult, Settings,
    MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use windui::app::{App, WindowOpHandle, WindowPositionHandle, WindowSizeHandle};
use windui::signal::Signal;

use super::applications::ApplicationWorker;
use super::everything::EverythingWorker;
use super::path_probe::{is_remote_path_query, PathProbeWorker};
use super::plugins::{FlowPluginWorker, NativePluginWorker, PluginAction};
use super::provider_merge::normalize_built_in_executable_targets;
use super::provider_snapshot::{
    commit_provider_results, should_publish_initial_query_results, ProviderResults, Publish,
};
use super::request_scroll;
use super::shell_icon_cache::{
    icon_completion_generation_changed, shell_icons_in_flight, SHELL_ICON_COMPLETION_GENERATION,
};
use super::theme_text::normalize_everything_query;
use super::ui_constants::{
    EVERYTHING_MIN_QUERY_LEN, ICON_SETTLE_TICKS, PLUGIN_MIN_QUERY_LEN, SEARCH_INTERVAL,
    SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH, SLOW_PROVIDER_DEBOUNCE_MS, TYPING_QUIET_MS,
};
use super::update_tasks::{request_update_check, update_check_due};
use super::visual_preview;

use super::window_geometry::{
    apply_launcher_size, dimension_from_slider, dimension_slider_fraction, launcher_content_height,
    launcher_window_geometry_with_prompt, parse_dimension_input, request_monitor_position,
};

/// Ease-in-out glide between the centred slot and the Visual pair slot. 400 ms
/// and a symmetric curve, matching the motion used in the design mock.
const SETTINGS_SLIDE_MS: u64 = 400;

/// Duration of an eased size reset (Reset tab defaults / the row Reset).
const SETTINGS_EASE_MS: f32 = 400.0;

/// Last applied pair frame: `(settings_x, settings_y, preview_x, preview_y,
/// width, height)`.
type PairFrame = (i32, i32, i32, i32, u16, u16);

/// A running glide of the Settings window, optionally carrying the preview
/// window along with it.
struct SettingsSlide {
    from_x: i32,
    to_x: i32,
    y: i32,
    ride: Option<PreviewRide>,
    started_ms: u64,
}

/// The preview window rides the Settings glide: it appears beside the panel's
/// current right edge and travels with it, settling the last few pixels into the
/// final gap. This keeps the two windows on one motion instead of two clocks.
struct PreviewRide {
    to_x: i32,
    y: i32,
    width: i32,
    height: i32,
    joined_ms: Option<u64>,
}

/// How far right of the final slot the preview settles from, and over what time.
const PREVIEW_SETTLE_PX: i32 = 32;
const PREVIEW_SETTLE_MS: f32 = 160.0;
/// The preview process is up after roughly this long; the window is created at
/// the ride position for this instant so it does not jump when it appears.
const PREVIEW_READY_ESTIMATE_MS: f32 = 171.0;

impl SettingsSlide {
    fn x_at(&self, now_ms: u64) -> i32 {
        let elapsed = now_ms.saturating_sub(self.started_ms);
        let t = (elapsed as f32 / SETTINGS_SLIDE_MS as f32).clamp(0.0, 1.0);
        let eased = ease_in_out_cubic(t);
        self.from_x + ((self.to_x - self.from_x) as f32 * eased).round() as i32
    }
}

/// Symmetric ease-in-out cubic (accelerate, then decelerate).
fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f = -2.0 * t + 2.0;
        1.0 - (f * f * f) / 2.0
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn register_interval(
    app: App,
    query: Signal<String>,
    results: Signal<Vec<SearchResult>>,
    launcher_width: Signal<u16>,
    launcher_height: Signal<u16>,
    launcher_width_input: Signal<String>,
    launcher_height_input: Signal<String>,
    launcher_width_slider: Signal<f32>,
    launcher_height_slider: Signal<f32>,
    launcher_preview_text: Signal<String>,
    icon_refresh_generation: Signal<u64>,
    status: Signal<String>,
    show_results: Signal<bool>,
    inline_completion: Signal<String>,
    selection_touched: Signal<bool>,
    current_sequence: Signal<u64>,
    providers: Rc<RefCell<ProviderResults>>,
    priorities: Signal<Vec<PriorityEntry>>,
    scroll_request: Signal<bool>,
    plugin_actions: Rc<RefCell<HashMap<String, PluginAction>>>,
    auto_enable_everything: Signal<bool>,
    obsidian_enabled: Signal<bool>,
    obsidian_alias: Signal<String>,
    google_enabled: Signal<bool>,
    google_alias: Signal<String>,
    history_mode: Signal<bool>,
    settings_visible: Signal<bool>,
    settings_hide_request: Signal<bool>,
    settings_tab: Signal<usize>,
    settings_show_generation: Signal<u64>,
    width_ease: Rc<Cell<Option<(f32, f32, std::time::Instant)>>>,
    height_ease: Rc<Cell<Option<(f32, f32, std::time::Instant)>>>,
    everything_prompt_visible: Signal<bool>,
    everything_installed: Signal<bool>,
    everything_status: Signal<String>,
    visual_preview_generation: Signal<u64>,
    selected_id: Signal<String>,
    selected_index: Signal<usize>,
    action_mode: Signal<bool>,
    action_index: Signal<usize>,
    action_items: Signal<Vec<super::result_actions::ActionItem>>,
    shared_settings: Arc<RwLock<Settings>>,
    update_sender: windui::prelude::Sender<super::updater::UpdateCheckResponse>,
    update_check_in_flight: Rc<Cell<bool>>,
    window_size: WindowSizeHandle,
    window_position: WindowPositionHandle,
    application_worker: ApplicationWorker,
    everything_worker: EverythingWorker,
    path_probe_worker: PathProbeWorker,
    plugin_worker: FlowPluginWorker,
    native_plugin_worker: NativePluginWorker,
    recycle_bin_confirmation: Signal<bool>,
    window_op: WindowOpHandle,
    visual_preview_slot: Rc<RefCell<Option<visual_preview::PreviewProcess>>>,
) -> App {
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
    let providers_for_interval = Rc::clone(&providers);
    let priorities_for_interval = priorities;
    let scroll_request_for_interval = scroll_request;
    let actions_for_interval = Rc::clone(&plugin_actions);
    let auto_enable_everything_for_interval = auto_enable_everything;
    let obsidian_enabled_for_interval = obsidian_enabled;
    let obsidian_alias_for_interval = obsidian_alias;
    let google_enabled_for_interval = google_enabled;
    let google_alias_for_interval = google_alias;
    let history_mode_for_interval = history_mode;
    let settings_visible_for_interval = settings_visible;
    let settings_tab_for_interval = settings_tab;
    let everything_prompt_visible_for_interval = everything_prompt_visible;
    let everything_installed_for_interval = everything_installed;
    let everything_status_for_interval = everything_status;
    let visual_preview_generation_for_interval = visual_preview_generation;
    let visual_preview_smoke_for_interval =
        std::env::var_os("FLUX_SMOKE_VISUAL_SETTINGS").is_some();
    let everything_plugins_smoke_for_interval =
        std::env::var_os("FLUX_SMOKE_EVERYTHING_PLUGINS").is_some();
    let tray_settings_smoke_pending = Rc::new(Cell::new(
        std::env::var_os("FLUX_SMOKE_TRAY_SETTINGS").is_some(),
    ));
    let tray_settings_smoke_pending_for_interval = Rc::clone(&tray_settings_smoke_pending);
    let mut last_icon_generation = icon_refresh_generation.get();
    let mut icon_wait_ticks: u32 = 0;
    let mut last_launcher_width = launcher_width.get();
    let mut last_launcher_height = launcher_height.get();
    let mut last_settings_visible = settings_visible.get();
    let mut last_settings_tab = settings_tab.get();
    let mut last_show_generation = settings_show_generation.get();
    let mut settings_positioned = false;
    // Glide of the Settings window between the centred slot (ordinary tabs) and
    // the left pair slot (Visual): (from_x, to_x, y, started_ms). The vertical
    // position is captured at the start and replayed every frame: the window
    // origin is only published for positions the app itself requested, so reading
    // it mid-glide can be stale and would drop the window to y = 0.
    let settings_slide: Rc<RefCell<Option<SettingsSlide>>> = Rc::new(RefCell::new(None));
    let slide_clock = std::time::Instant::now();
    let settings_slide_for_slide = Rc::clone(&settings_slide);
    let position_for_slide = window_position.clone();
    // Display-rate pair follow: the monitor geometry is cached in the tick so the
    // frame callback can recompute the pair from the slider without touching the
    // monitor every frame.
    let pair_monitor: Rc<Cell<Option<(super::monitor::MonitorBounds, u32)>>> =
        Rc::new(Cell::new(None));
    let pair_monitor_for_frame = Rc::clone(&pair_monitor);
    let last_pair_frame: Rc<Cell<Option<PairFrame>>> = Rc::new(Cell::new(None));
    let last_pair_frame_for_frame = Rc::clone(&last_pair_frame);
    let mut last_pair_monitor_pref: Option<MonitorPreference> = None;
    let mut last_everything_prompt_visible = everything_prompt_visible.get();
    // The search clock, and the reason it is not `EventCtx::now_ms`: that reads the
    // shared animation clock, which the host refreshes before a frame, a pointer
    // event and a key, and which is frozen in between - windui's own doc says so and
    // warns against reading a frozen period as how long the user has been away.
    // Inside an interval callback there is no guarantee of a refresh at all, so the
    // tick stamped a query change with a time that could be hundreds of milliseconds
    // old, and the next tick read a fresh clock, called a live query quiet, and let
    // every typing hold stand down. Measured on a failing flicker run: the keystroke
    // and its tick were 5 ms apart, yet the tick stamped `now_ms=5401` and the tick
    // after it read 5603 and declared a 202 ms pause. This clock is monotonic and
    // read live, so `query_is_quiet` and the probe's publish budget both mean what
    // they say.
    let search_clock = std::time::Instant::now();
    let mut last_query = String::new();
    let mut last_query_change_ms: u64 = 0;
    let mut slow_sent_sequence: u64 = 0;
    let mut everything_sent_sequence: u64 = 0;
    let mut path_probe_sent_sequence: u64 = 0;
    let mut last_fitted_height: i32 = 0;
    let visual_preview_process = visual_preview_slot;
    let visual_preview_for_frame = Rc::clone(&visual_preview_process);
    let mut last_visual_preview_request: Option<(u16, u16)> = None;
    let mut last_visual_preview_generation = visual_preview_generation.get();
    let mut last_visual_control_state: Option<(u16, u16, u32, u32)> = None;
    let mut everything_plugins_smoke_reported = false;
    let mut sequence = 0_u64;
    let mut model = SearchModel::new();
    let position_for_interval = window_position.clone();
    let settings_for_interval_geometry = Arc::clone(&shared_settings);
    let size_for_interval = window_size.clone();
    let settings_for_update_interval = Arc::clone(&shared_settings);
    let update_sender_for_interval = update_sender.clone();
    let update_check_in_flight_for_interval = Rc::clone(&update_check_in_flight);
    let recycle_bin_confirmation_for_interval = recycle_bin_confirmation;
    let window_op_for_interval = window_op;
    // Smoke hooks. None of these fire in normal use: they are gated on env vars
    // that the recycle-confirm smoke sets, and they let the automated suite drive
    // the real spawn/reap state machine without synthesising keystrokes or a
    // re-show gesture. `no_hide` keeps the interval timers alive after the child
    // spawns (normally the hide kills them until the next activation), so the
    // parent reaps the child on a later tick and the whole flow runs headless.
    let recycle_smoke_no_hide = std::env::var_os("FLUX_SMOKE_RECYCLE_NO_HIDE").is_some();
    let recycle_smoke_arm = std::env::var_os("FLUX_SMOKE_RECYCLE_ARM").is_some();
    let recycle_smoke_rearm_left = std::cell::Cell::new(
        std::env::var("FLUX_SMOKE_RECYCLE_REARM")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
    );
    if recycle_smoke_arm {
        recycle_bin_confirmation_for_interval.set(true);
        super::paint_trace::note("recycle-confirm-armed", "smoke");
    }
    let mut recycle_confirm_child: Option<std::process::Child> = None;
    app.on_interval(SEARCH_INTERVAL, move |ctx| {
        // Emptying the Recycle Bin uses the native Windows shell confirmation. The
        // launcher hides itself and spawns a short-lived child process that owns the
        // blocking shell prompt, so the launcher's UI thread never enters a nested
        // modal loop (exit 0 = emptied, non-zero = declined or failed). The parent must
        // never empty the bin itself: the child already raised the one prompt the
        // gesture asked for, and a second call here is what used to re-pop the dialog
        // on every activation that followed.
        //
        // Hiding stops the interval timers, so this tick cannot run again until the
        // launcher is shown. The child is therefore reaped on the first tick after the
        // next activation, and that tick is what publishes the outcome.
        if recycle_bin_confirmation_for_interval.get() {
            if recycle_confirm_child.is_none() {
                match std::env::current_exe() {
                    Ok(exe) => {
                        if !recycle_smoke_no_hide {
                            window_op_for_interval.hide_window();
                        }
                        super::paint_trace::note("recycle-confirm-spawn", "");
                        match std::process::Command::new(exe)
                            .arg("--empty-recycle-confirm")
                            .spawn()
                        {
                            Ok(child) => recycle_confirm_child = Some(child),
                            Err(error) => {
                                eprintln!("Could not start Recycle Bin confirmation: {error}");
                                if !recycle_smoke_no_hide {
                                    window_op_for_interval.show_window();
                                }
                                recycle_bin_confirmation_for_interval.set(false);
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("Could not resolve the executable for confirmation: {error}");
                        recycle_bin_confirmation_for_interval.set(false);
                    }
                }
            } else if let Some(child) = recycle_confirm_child.as_mut() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        recycle_confirm_child = None;
                        if status.success() {
                            super::paint_trace::note("recycle-confirm-emptied", "");
                            status_for_interval.set(String::from(crate::i18n::tr("Recycle Bin emptied")));
                        } else {
                            super::paint_trace::note("recycle-confirm-declined", "");
                            status_for_interval.set(String::from(crate::i18n::tr("Ready")));
                        }
                        // Smoke re-arm: a re-show after the prompt is what arms the
                        // confirmation again in real use. Re-arming here, up to the
                        // requested count, proves each gesture spawns exactly one child
                        // and never cascades into several.
                        if recycle_smoke_rearm_left.get() > 0 {
                            recycle_smoke_rearm_left.set(recycle_smoke_rearm_left.get() - 1);
                            recycle_bin_confirmation_for_interval.set(true);
                            super::paint_trace::note("recycle-confirm-armed", "smoke-rearm");
                        } else {
                            recycle_bin_confirmation_for_interval.set(false);
                        }
                        if !recycle_smoke_no_hide {
                            window_op_for_interval.show_window();
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("Recycle Bin confirmation process error: {error}");
                        recycle_confirm_child = None;
                        recycle_bin_confirmation_for_interval.set(false);
                        window_op_for_interval.show_window();
                    }
                }
            }
            return;
        }
        let current_width = width_for_interval.get();
        let current_height = height_for_interval.get();
        let settings_is_visible = settings_visible_for_interval.get();
        // The intervals stop while the window is hidden, so a hide never reaches
        // this tick. The show generation is how a fresh show is detected.
        let show_changed = settings_show_generation.get() != last_show_generation;
        last_show_generation = settings_show_generation.get();
        let prompt_is_visible = everything_prompt_visible_for_interval.get();
        let visual_tab_is_visible = settings_tab_for_interval.get() == 1;
        let plugins_tab_is_visible = settings_tab_for_interval.get() == 3;
        let visual_preview_is_visible = settings_is_visible && visual_tab_is_visible;
        // Refresh the cached monitor geometry only when the preference changes;
        // the frame callback then recomputes the pair at display rate for free.
        if let Ok(settings) = settings_for_interval_geometry.read() {
            let preference = settings.monitor_preference;
            if last_pair_monitor_pref != Some(preference) {
                if let Some((bounds, dpi)) = super::monitor::work_area_with_dpi(preference) {
                    pair_monitor.set(Some((bounds, dpi)));
                }
                last_pair_monitor_pref = Some(preference);
            }
        }
        if everything_plugins_smoke_for_interval
            && settings_is_visible
            && plugins_tab_is_visible
            && !everything_plugins_smoke_reported
        {
            let installed = everything_installed_for_interval.get();
            let auto_enable = auto_enable_everything_for_interval.get();
            let status = everything_status_for_interval.get();
            eprintln!(
                "Everything Plugins UI: tab_visible=true everything_section=true auto_enable_checkbox=true status_label=true install_button_label=Install_Everything already_installed_label=Everything_is_already_installed auto_enable={} installed={} install_button_visible={} already_installed_visible={} status={}",
                auto_enable,
                installed,
                !installed,
                installed,
                status.replace(' ', "_")
            );
            everything_plugins_smoke_reported = true;
        }
        if settings_is_visible && (!last_settings_visible || show_changed) {
            let mut canvas = (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
            if let Ok(settings) = settings_for_interval_geometry.read() {
                let (width, height) =
                    super::window_geometry::settings_window_size(settings.monitor_preference);
                canvas = (width, height);
                request_monitor_position(
                    &position_for_interval,
                    settings.monitor_preference,
                    width,
                    height,
                );
            }
            size_for_interval.set(canvas.0, canvas.1);
        }
        // Ordinary tabs keep the window centred; the Visual tab lives in the
        // left pair slot beside the preview. The glide starts immediately; the
        // preview joins it as a rider once its process is up, so the two windows
        // travel on one motion instead of two independent clocks.
        if settings_is_visible {
            let current_tab = settings_tab_for_interval.get();
            let just_shown = !last_settings_visible || !settings_positioned || show_changed;
            if just_shown || current_tab != last_settings_tab {
                if let Ok(settings) = settings_for_interval_geometry.read() {
                    let preference = settings.monitor_preference;
                    let (centered_x, centered_y) =
                        super::window_geometry::settings_centered_position(preference)
                            .unwrap_or((ctx.window_origin().x, ctx.window_origin().y));
                    let from_x = if just_shown {
                        centered_x
                    } else {
                        ctx.window_origin().x
                    };
                    let (to_x, to_y, ride) = if current_tab == 1 {
                        let ((sx, sy), (px, py), _) =
                            super::window_geometry::visual_preview_pair(
                                preference,
                                i32::from(current_width),
                                i32::from(current_height),
                            );
                        (
                            sx,
                            sy,
                            Some(PreviewRide {
                                to_x: px,
                                y: py,
                                width: i32::from(current_width),
                                height: i32::from(current_height),
                                joined_ms: None,
                            }),
                        )
                    } else {
                        (centered_x, centered_y, None)
                    };
                    *settings_slide.borrow_mut() = Some(SettingsSlide {
                        from_x,
                        to_x,
                        y: to_y,
                        ride,
                        started_ms: slide_clock.elapsed().as_millis() as u64,
                    });
                    // Kick the display-rate frame loop; the frame callback
                    // keeps it running until the slide lands.
                    windui::anim::request_repaint();
                }
                last_settings_tab = current_tab;
            }
            settings_positioned = true;
        } else {
            last_settings_tab = settings_tab_for_interval.get();
            settings_positioned = false;
            *settings_slide.borrow_mut() = None;
        }
        if prompt_is_visible != last_everything_prompt_visible {
            last_everything_prompt_visible = prompt_is_visible;
            let (prompt_width, prompt_height) = launcher_window_geometry_with_prompt(
                settings_is_visible,
                prompt_is_visible,
                show_results_for_interval.get(),
                width_for_interval.get() as i32,
                height_for_interval.get() as i32,
            );
            if let Ok(settings) = settings_for_interval_geometry.read() {
                request_monitor_position(
                    &position_for_interval,
                    settings.monitor_preference,
                    prompt_width,
                    prompt_height,
                );
            }
            size_for_interval.set(prompt_width, prompt_height);
        }
        if visual_preview_is_visible {
            let preference = settings_for_interval_geometry
                .read()
                .map(|settings| settings.monitor_preference)
                .unwrap_or(MonitorPreference::Cursor);
            // One borrow per statement: closing the preview from inside an active
            // borrow panics the RefCell, and a panic inside a window callback
            // aborts the whole process.
            let child_exited = {
                let mut slot = visual_preview_process.borrow_mut();
                slot.as_mut().is_some_and(|preview| !preview.is_alive())
            };
            if child_exited {
                visual_preview_process.borrow_mut().take();
                last_visual_preview_request = None;
            }
            let ready_error = {
                let mut slot = visual_preview_process.borrow_mut();
                slot.as_mut().and_then(|preview| preview.poll_ready().err())
            };
            if let Some(error) = ready_error {
                eprintln!("Could not ready visual preview: {error}");
                visual_preview_process.borrow_mut().take();
                last_visual_preview_request = None;
            }
            // Spawn the preview as soon as the Visual tab opens. When the panel is
            // already gliding, open the window where the ride will have it by the
            // time the process is up, so it does not jump when it appears.
            if visual_preview_process.borrow().is_none() {
                let preview_width = i32::from(current_width);
                let preview_height = i32::from(current_height);
                let (_, (preview_x, preview_y), _) = super::window_geometry::visual_preview_pair(
                    preference,
                    preview_width,
                    preview_height,
                );
                let (start_x, start_y) = {
                    let slide = settings_slide.borrow();
                    match slide.as_ref().and_then(|slide| {
                        slide.ride.as_ref().map(|ride| (slide, ride))
                    }) {
                        Some((slide, _ride)) => {
                            let estimate = slide.started_ms + PREVIEW_READY_ESTIMATE_MS as u64;
                            let sx = slide.x_at(estimate);
                            (sx + (preview_x - slide.to_x) + PREVIEW_SETTLE_PX, preview_y)
                        }
                        None => (preview_x, preview_y),
                    }
                };
                match visual_preview::PreviewProcess::start(
                    preview_width,
                    preview_height,
                    start_x,
                    start_y,
                ) {
                    Ok(preview) => {
                        *visual_preview_process.borrow_mut() = Some(preview);
                        last_visual_preview_request = None;
                    }
                    Err(error) => eprintln!("Could not start visual preview: {error}"),
                }
            }
        } else if let Some(preview) = visual_preview_process.borrow_mut().take() {
            // The preview belongs to the Visual tab only: leaving it takes the
            // window away instead of leaving it floating over other pages.
            eprintln!(
                "Visual preview closing because Settings/Visual is hidden: pid={}",
                preview.pid()
            );
            last_visual_preview_request = None;
        }
        if settings_hide_request.get() {
            // Hide only here: dropping the preview above is what keeps the
            // child from outliving the window, and the timers die with the
            // hide, so no later tick would ever clean it up.
            settings_hide_request.set(false);
            window_op_for_interval.hide_window();
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
            preview_text_for_interval.set(crate::i18n::format(
                "Current launcher client area: {width} × {height} logical px (DIP)",
                &[("width", next_width.to_string()), ("height", next_height.to_string())],
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
            // While the slot glide runs, the frame callback owns BOTH windows:
            // the panel and the riding preview must not be pushed by a second
            // clock. The dispatch resumes when the glide lands.
            if must_dispatch && settings_slide.borrow().is_none() {
                let preference = settings_for_interval_geometry
                    .read()
                    .map(|settings| settings.monitor_preference)
                    .unwrap_or(MonitorPreference::Cursor);
                // The window slides live with the preview; the pair slide is
                // shorter than the slider's travel, so the client-space value
                // converges with the thumb under the cursor instead of running.
                let ((settings_x, settings_y), (preview_x, preview_y), _shifted) =
                    super::window_geometry::visual_preview_pair(
                        preference,
                        i32::from(requested.0),
                        i32::from(requested.1),
                    );
                position_for_interval.set_move_only(settings_x, settings_y);
                let dispatch_result = if let Some(preview) = visual_preview_process.borrow_mut().as_mut()
                {
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
                        visual_preview_process.borrow_mut().take();
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
        if tray_settings_smoke_pending_for_interval.replace(false) {
            // Exercise the same lifecycle order as the tray Settings item,
            // without relying on brittle screen-coordinate tray automation.
            settings_visible_for_interval.set(true);
            ctx.show_window();
            let preference = settings_for_interval_geometry
                .read()
                .map(|settings| settings.monitor_preference)
                .unwrap_or_default();
            let (width, height) = super::window_geometry::settings_window_size(preference);
            size_for_interval.set(width, height);
            return;
        }
        let next_query = query_for_interval.get();
        let now_ms = search_clock.elapsed().as_millis() as u64;
        let query_changed = next_query != last_query;
        let has_query = !next_query.trim().is_empty();
        if query_changed {
            last_query = next_query.clone();
            last_query_change_ms = now_ms;
            super::paint_trace::note(
                "query",
                &format!("value={next_query} len={}", next_query.chars().count()),
            );
        } else {
            // Settled tick: fire debounced slow providers at most once per
            // query generation; everything else already ran on change.
            if has_query
                && slow_providers_due(
                    now_ms,
                    last_query_change_ms,
                    slow_sent_sequence,
                    sequence,
                    SLOW_PROVIDER_DEBOUNCE_MS,
                )
            {
                slow_sent_sequence = sequence;
                // Normally the change tick already asked; this covers a query that
                // became eligible without changing (Everything auto-enabled while
                // the text stood still).
                if everything_sent_sequence != sequence
                    && auto_enable_everything_for_interval.get()
                    && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN
                {
                    everything_sent_sequence = sequence;
                    super::paint_trace::note(
                        "request-everything",
                        &format!("query={next_query} late=true"),
                    );
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
            // Late answers were kept back while the user typed: a second publish
            // for the same query would rebuild every row again and read as a
            // full-list flash. Once the query has been quiet, it lands.
            let query_is_quiet = now_ms.saturating_sub(last_query_change_ms) >= TYPING_QUIET_MS;
            // A share is only asked about once the query stands still: one `stat`
            // there can hold the probe's whole thread for the redirector's timeout,
            // and while the user types that would be a generation that never
            // closes. Tracked per generation so a settled query asks once.
            if query_is_quiet
                && has_query
                && path_probe_sent_sequence != sequence
                && is_remote_path_query(&next_query)
            {
                path_probe_sent_sequence = sequence;
                super::paint_trace::note(
                    "request-path",
                    &format!("query={next_query} late=true"),
                );
                path_probe_worker.request(sequence, next_query.clone());
            }
            let mut providers = providers_for_interval.borrow_mut();
            providers.typing_active = !query_is_quiet;
            // The budget decides how long the probe may hold the list, not how often
            // the list is published - that is decided inside `commit_provider_results`.
            // So opening the gate is not a licence to publish a half snapshot: the
            // commit below still holds while another provider owes rows.
            let probe_gate_opened =
                providers.path_probe_budget_spent(now_ms) && providers.core_ready();
            if (query_is_quiet && providers.pending_publish) || probe_gate_opened {
                let priority_ids = priorities_for_interval
                    .get()
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>();
                commit_provider_results(
                    &mut providers,
                    &next_query,
                    &priority_ids,
                    selected_id,
                    selected_index,
                    selection_touched_for_interval,
                    results_for_interval,
                    history_mode_for_interval, Publish::DeferWhileTyping);
            }
            drop(providers);
            // Shell icons are not held back that way - a new query would show
            // placeholder squares until the typing paused. They are held back
            // *per page*: the generation only reaches the tree once the icon
            // thread has nothing left to load, so a page of rows gains its icons
            // in one repaint instead of one repaint per icon. A queue that never
            // drains (a shell item that hangs) is cut off after the tick budget.
            let completed_icon_generation =
                SHELL_ICON_COMPLETION_GENERATION.load(Ordering::Acquire);
            if !icon_completion_generation_changed(last_icon_generation, completed_icon_generation) {
                icon_wait_ticks = 0;
            } else if !shell_icons_in_flight() || {
                icon_wait_ticks = icon_wait_ticks.saturating_add(1);
                icon_wait_ticks >= ICON_SETTLE_TICKS
            } {
                icon_wait_ticks = 0;
                last_icon_generation = completed_icon_generation;
                super::paint_trace::note(
                    "icon-refresh",
                    &format!("generation={completed_icon_generation}"),
                );
                icon_refresh_generation_for_interval.set(completed_icon_generation);
            }
            // A provider can also answer after the keystroke that opened the
            // generation, and the window has to follow the row count then too. The
            // action bar owns its own height while it is open: refitting the window
            // to the row count would cut off the actions the user just opened.
            if has_query
                && !settings_visible_for_interval.get()
                && !everything_prompt_visible_for_interval.get()
                && !action_mode.get()
            {
                // `with` reads the length in place. `get` would deep-clone the whole
                // result vector - every title, subtitle and path of every row - on
                // every 16 ms tick, only to be dropped after taking a len.
                let row_count = results_for_interval.with(|rows| rows.len());
                let fitted = launcher_content_height(
                    row_count,
                    launcher_height.get() as i32,
                );
                if fitted != last_fitted_height {
                    last_fitted_height = fitted;
                    let width = launcher_width.get() as i32;
                    size_for_interval.set(width, fitted);
                    super::paint_trace::note(
                        "resize",
                        &format!("size={width}x{fitted} has_query=true fitted=true"),
                    );
                }
            }
            return;
        }
        history_mode_for_interval.set(false);
        show_results_for_interval.set(has_query);
        sequence = sequence.wrapping_add(1);
        sequence_for_interval.set(sequence);
        // Ask the file provider about the text the user just typed, in this same
        // tick. Waiting for a settled tick first put 16-30 ms in front of
        // Everything's own round trip, which is the whole of the window during
        // which the panel can only show the previous prefix's rows.
        if auto_enable_everything_for_interval.get()
            && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN
        {
            everything_sent_sequence = sequence;
            super::paint_trace::note("request-everything", &format!("query={next_query}"));
            everything_worker.request(sequence, normalize_everything_query(&next_query));
        }
        // A typed path is answered by the filesystem itself, not only by the
        // indexer: a volume Everything does not cover still holds real folders. A
        // share is not asked about here - one `stat` on a dead one cannot be
        // cancelled and would hold the worker across the keystrokes that follow.
        let path_probe_requested =
            path_query_body(&next_query).is_some() && !is_remote_path_query(&next_query);
        if path_probe_requested {
            path_probe_sent_sequence = sequence;
            super::paint_trace::note("request-path", &format!("query={next_query}"));
            path_probe_worker.request(sequence, next_query.clone());
        }
        model.set_query(&next_query);
        // Ghost completion is a pure function of (query, applications,
        // selection): derive it synchronously from the previous generation
        // instead of clearing it, so typing never flashes the hint off and
        // on. The prefix check inside drops stale suggestions on its own,
        // and applications are the single writer per generation, so partial
        // provider commits cannot flip the hint.
        {
            let providers = providers_for_interval.borrow();
            super::refresh_inline_completion(
                inline_completion_for_interval,
                &next_query,
                &providers.applications,
                &selected_id.get(),
            );
        }
        {
            let mut built_in_results = model.results().to_vec();
            normalize_built_in_executable_targets(&mut built_in_results);
            let everything_expected = auto_enable_everything_for_interval.get()
                && next_query.trim().len() >= EVERYTHING_MIN_QUERY_LEN;
            let mut providers = providers_for_interval.borrow_mut();
            providers.reset(sequence, built_in_results.clone(), everything_expected);
            if path_probe_requested {
                providers.expect_path_probe(now_ms);
            }
            let publish_initial_results = should_publish_initial_query_results(
                has_query,
                results_for_interval.with(|rows| rows.is_empty()),
                providers.published_query.is_empty(),
            );
            if publish_initial_results {
                super::paint_trace::note(
                    "publish-initial",
                    &format!("query={next_query} rows={}", built_in_results.len()),
                );
                // A recall pins the row that query last launched (`selection_touched`
                // plus its id). Publishing the built-in rows must not unpin it: the
                // provider commit that follows is what restores the row, and clearing
                // the pin here drops it before it is ever shown.
                if !selection_touched_for_interval.get() {
                    selected_index.set(0);
                    selected_id.set(
                        built_in_results
                            .first()
                            .map(|result| result.id.clone())
                            .unwrap_or_default(),
                    );
                }
                // Built-in/system commands are synchronous and must be actionable
                // immediately. This list holds no provider rows, so it must not
                // count as this query being published: otherwise the applications
                // and Everything commits for the same keystroke would defer
                // themselves behind the quiet window.
                providers.published_query = next_query.clone();
                providers.published_providers = false;
                results_for_interval.set(built_in_results);
            }
        }
        // A keystroke closes the action bar, and the height that bar was given is
        // not the height of the next page: reset it before the panel is measured.
        action_mode.set(false);
        action_index.set(0);
        action_items.set(Vec::new());
        actions_for_interval.borrow_mut().clear();
        // Size the panel from the rows this keystroke actually published, which is
        // why this runs after them: fitting first would leave the previous
        // generation's height for a frame on every keystroke that shortens the list.
        //
        // Query cleanup also happens when hide-on-deactivate hides the
        // launcher. Do not let that asynchronous query transition resize
        // an already-open Settings panel back to the compact search strip.
        let (target_width, target_height) = launcher_window_geometry_with_prompt(
            settings_visible_for_interval.get(),
            everything_prompt_visible_for_interval.get(),
            has_query,
            launcher_width.get() as i32,
            launcher_height.get() as i32,
        );
        // The configured height is a maximum: one result must not leave five empty
        // rows of acrylic under it, which is the frame the owner reads as broken.
        let target_height = if has_query
            && !settings_visible_for_interval.get()
            && !everything_prompt_visible_for_interval.get()
        {
            launcher_content_height(results_for_interval.with(|rows| rows.len()), target_height)
        } else {
            target_height
        };
        last_fitted_height = target_height;
        size_for_interval.set(target_width, target_height);
        super::paint_trace::note(
            "resize",
            &format!("size={target_width}x{target_height} has_query={has_query}"),
        );
        request_scroll(scroll_request_for_interval);
        if !has_query {
            inline_completion_for_interval.set(String::new());
            status_for_interval.set(String::from(crate::i18n::tr("Ready")));
        } else {
            status_for_interval.set(String::from(
                crate::i18n::tr("Searching applications, Everything and native Flow plugins..."),
            ));
            // Applications stay immediate so the top row answers within a
            // single tick; slow providers follow through the debounced path.
            application_worker.request(sequence, next_query.clone());
        }
    })
    .on_frame(move |_ctx| {
        // A size reset eases the slider signal; the follow below then moves the
        // pair on the same curve, so the thumb and the windows travel together.
        let mut easing = false;
        for (ease, slider) in [
            (width_ease.as_ref(), width_slider_for_interval),
            (height_ease.as_ref(), height_slider_for_interval),
        ] {
            if let Some((from, to, started)) = ease.get() {
                let elapsed = started.elapsed().as_secs_f32() * 1000.0;
                let t = (elapsed / SETTINGS_EASE_MS).clamp(0.0, 1.0);
                let fraction = from + (to - from) * ease_in_out_cubic(t);
                slider.set(fraction);
                if t >= 1.0 {
                    ease.set(None);
                } else {
                    easing = true;
                }
            }
        }
        let mut guard = settings_slide_for_slide.borrow_mut();
        if let Some(slide) = guard.as_mut() {
            let now = slide_clock.elapsed().as_millis() as u64;
            let elapsed = now.saturating_sub(slide.started_ms);
            let t = (elapsed as f32 / SETTINGS_SLIDE_MS as f32).clamp(0.0, 1.0);
            let eased = ease_in_out_cubic(t);
            let x = slide.from_x + ((slide.to_x - slide.from_x) as f32 * eased).round() as i32;
            position_for_slide.set_move_only(x, slide.y);
            // The preview rides the panel: it appears beside the panel's CURRENT
            // right edge and travels with it, settling the last few pixels once it
            // has joined. One motion, not two clocks.
            if let Some(ride) = slide.ride.as_mut() {
                let ready = {
                    let mut slot = visual_preview_for_frame.borrow_mut();
                    slot.as_mut()
                        .is_some_and(|preview| preview.poll_ready().unwrap_or(false))
                };
                if ready {
                    let joined = *ride.joined_ms.get_or_insert(now);
                    let settle_t = ((now.saturating_sub(joined)) as f32 / PREVIEW_SETTLE_MS)
                        .clamp(0.0, 1.0);
                    let offset = (PREVIEW_SETTLE_PX as f32 * (1.0 - ease_in_out_cubic(settle_t)))
                        .round() as i32;
                    let px = x + (ride.to_x - slide.to_x) + offset;
                    if let Some(preview) = visual_preview_for_frame.borrow_mut().as_mut() {
                        let _ = preview.resize(ride.width, ride.height, px, ride.y);
                    }
                }
            }
            if t >= 1.0 {
                *guard = None;
                easing
            } else {
                true
            }
        } else {
            // No glide: follow the width slider at the paint rate, so dragging on
            // a high-refresh display is not limited to the 16 ms search tick.
            if settings_visible_for_interval.get() && settings_tab_for_interval.get() == 1 {
                if let Some((bounds, dpi)) = pair_monitor_for_frame.get() {
                    let width = dimension_from_slider(
                        width_slider_for_interval.get(),
                        MIN_LAUNCHER_WIDTH,
                        MAX_LAUNCHER_WIDTH,
                    );
                    let height = dimension_from_slider(
                        height_slider_for_interval.get(),
                        MIN_LAUNCHER_HEIGHT,
                        MAX_LAUNCHER_HEIGHT,
                    );
                    let ((sx, sy), (px, py), _) =
                        super::window_geometry::visual_preview_pair_in_bounds(
                            bounds,
                            dpi,
                            i32::from(width),
                            i32::from(height),
                        );
                    let frame = (sx, sy, px, py, width, height);
                    if last_pair_frame_for_frame.get() != Some(frame) {
                        last_pair_frame_for_frame.set(Some(frame));
                        position_for_slide.set_move_only(sx, sy);
                        if let Some(preview) = visual_preview_for_frame.borrow_mut().as_mut() {
                            let _ = preview.resize(i32::from(width), i32::from(height), px, py);
                        }
                    }
                }
            }
            easing
        }
    })
}

/// Debounce gate for slow providers (Flow-style switchMap): an expensive
/// search fires only for a query generation that survived settled without
/// further keystrokes, at most once per generation.
pub(crate) fn slow_providers_due(
    now_ms: u64,
    last_change_ms: u64,
    sent_sequence: u64,
    sequence: u64,
    debounce_ms: u64,
) -> bool {
    sent_sequence != sequence && now_ms.saturating_sub(last_change_ms) >= debounce_ms
}

#[cfg(test)]
mod tests {
    use super::slow_providers_due;

    #[test]
    fn slow_providers_wait_for_settled_generation_once() {
        // Fresh change: not due no matter the sequence.
        assert!(!slow_providers_due(1000, 1000, 0, 1, 150));
        // Settled and unsent: due (boundary inclusive).
        assert!(slow_providers_due(1150, 1000, 0, 1, 150));
        assert!(slow_providers_due(5000, 1000, 0, 1, 150));
        // Already sent for this generation: never again.
        assert!(!slow_providers_due(5000, 1000, 1, 1, 150));
        // New generation resets the gate but still waits out the debounce.
        assert!(!slow_providers_due(1200, 1200, 1, 2, 150));
        assert!(slow_providers_due(1350, 1200, 1, 2, 150));
        // Clock warp saturates instead of underflowing.
        assert!(!slow_providers_due(100, 5000, 0, 1, 150));
    }
}
