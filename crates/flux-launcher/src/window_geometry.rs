use std::sync::{Arc, RwLock};

use flux_core::{
    MonitorPreference, Settings, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT,
    MIN_LAUNCHER_WIDTH,
};
use windui::app::{WindowPositionHandle, WindowSizeHandle};

use super::monitor;
use super::ui_constants::{
    COMPACT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_WIDTH,
    LAUNCHER_CHROME_HEIGHT, RESULT_LIST_VERTICAL_INSET, RESULT_ROWS_VISIBLE, RESULT_ROW_HEIGHT,
    SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH,
};

pub(crate) fn monitor_preference_index(preference: MonitorPreference) -> usize {
    match preference {
        MonitorPreference::Primary => 0,
        MonitorPreference::Cursor => 1,
        MonitorPreference::Foreground => 2,
    }
}

pub(crate) fn monitor_preference_from_index(index: usize) -> MonitorPreference {
    match index {
        1 => MonitorPreference::Cursor,
        2 => MonitorPreference::Foreground,
        _ => MonitorPreference::Primary,
    }
}

pub(crate) fn request_monitor_position(
    position: &WindowPositionHandle,
    preference: MonitorPreference,
    width: i32,
    height: i32,
) {
    if let Some((x, y)) = monitor::centered_position(preference, width, height) {
        position.set(x, y);
    }
}

/// Edge margin kept clear around the Settings canvas when it has to shrink.
const SETTINGS_FIT_MARGIN: i32 = 24;

/// The Settings canvas size for the monitor it will open on.
///
/// The canvas is one fixed logical size. On a small screen at a high scale
/// factor that size can exceed the work area and push the footer under the
/// taskbar, so it is capped to the work area minus a margin. It is never grown:
/// on a large monitor the fixed size stands and nothing changes.
pub(crate) fn settings_window_size(preference: MonitorPreference) -> (i32, i32) {
    let Some((bounds, dpi)) = monitor::work_area_with_dpi(preference) else {
        return (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT);
    };
    let scale = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
    let available_width =
        (f64::from(bounds.width()) / scale).round() as i32 - 2 * SETTINGS_FIT_MARGIN;
    let available_height =
        (f64::from(bounds.height()) / scale).round() as i32 - 2 * SETTINGS_FIT_MARGIN;
    (
        SETTINGS_WINDOW_WIDTH.min(available_width.max(360)),
        SETTINGS_WINDOW_HEIGHT.min(available_height.max(300)),
    )
}

/// Shrink the results window to the rows it actually holds.
///
/// The configured `launcher_height` is a maximum: the result viewport is a fixed
/// six-row area, so a query that answers with one row used to leave five rows of
/// empty acrylic under it - which reads as a broken panel for as long as it takes
/// to type the next character. The search strip and the footer stay, the viewport
/// follows the row count, and anything past the six visible rows still scrolls.
///
/// The list's vertical padding is part of its scroll content exactly once. It
/// must not be folded into a per-row pitch: for one to five rows that made the
/// viewport shorter than the content and left a stale scroll offset of up to ten
/// pixels, which clipped the top row after an Alt+Up history recall.
pub(crate) fn launcher_content_height(result_count: usize, configured_height: i32) -> i32 {
    let rows = result_count.min(RESULT_ROWS_VISIBLE as usize) as i32;
    let list_inset = if rows == 0 {
        0
    } else {
        2 * RESULT_LIST_VERTICAL_INSET
    };
    let content = LAUNCHER_CHROME_HEIGHT + rows * RESULT_ROW_HEIGHT + list_inset;
    content.min(configured_height).max(COMPACT_WINDOW_HEIGHT)
}

#[cfg(test)]
pub(crate) fn launcher_window_geometry(settings_visible: bool, show_results: bool) -> (i32, i32) {
    launcher_window_geometry_with_sizes(
        settings_visible,
        show_results,
        flux_core::DEFAULT_LAUNCHER_WIDTH as i32,
        flux_core::DEFAULT_LAUNCHER_HEIGHT as i32,
    )
}

pub(crate) fn launcher_window_geometry_with_sizes(
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

pub(crate) fn launcher_window_geometry_with_prompt(
    settings_visible: bool,
    prompt_visible: bool,
    show_results: bool,
    launcher_width: i32,
    launcher_height: i32,
) -> (i32, i32) {
    if settings_visible {
        (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
    } else if prompt_visible {
        (
            EVERYTHING_PROMPT_WINDOW_WIDTH,
            EVERYTHING_PROMPT_WINDOW_HEIGHT,
        )
    } else {
        launcher_window_geometry_with_sizes(false, show_results, launcher_width, launcher_height)
    }
}

pub(crate) fn should_show_everything_install_prompt(
    everything_installed: bool,
    auto_enable_everything: bool,
    prompt_seen: bool,
    prompt_disabled: bool,
) -> bool {
    auto_enable_everything && !everything_installed && !prompt_seen && !prompt_disabled
}

/// Monitor selection for [`visual_preview_pair_in_bounds`].
pub(crate) fn visual_preview_pair(
    preference: MonitorPreference,
    preview_width: i32,
    preview_height: i32,
) -> ((i32, i32), (i32, i32), bool) {
    #[cfg(windows)]
    {
        let Some((bounds, dpi)) = monitor::work_area_with_dpi(preference) else {
            return ((0, 0), (0, 0), false);
        };
        visual_preview_pair_in_bounds(bounds, dpi, preview_width, preview_height)
    }
    #[cfg(not(windows))]
    {
        let _ = (preference, preview_width, preview_height);
        ((0, 0), (0, 0), false)
    }
}

/// Side-by-side placement for the Visual tab.
///
/// Settings starts centred for the narrowest preview and slides left in a
/// straight line, just enough that the widest preview still ends inside the
/// screen. The slide (about 138 px on a 1920 wide screen) stays below the
/// slider's travel (300 px track, 274 px usable), so the slider's client-space
/// mapping converges with the thumb under the cursor: both windows visibly move
/// together and the value never runs away.
///
/// Returns the Settings position, the preview position and whether Settings sits
/// away from a plain centred window.
pub(crate) fn visual_preview_pair_in_bounds(
    bounds: monitor::MonitorBounds,
    dpi: u32,
    preview_width: i32,
    preview_height: i32,
) -> ((i32, i32), (i32, i32), bool) {
    let scale = dpi.max(96) as f32 / 96.0;
    let to_physical = |logical: i32| (logical as f32 * scale).round() as i32;
    let settings_w = to_physical(SETTINGS_WINDOW_WIDTH);
    let settings_h = to_physical(SETTINGS_WINDOW_HEIGHT);
    let preview_w = to_physical(preview_width.max(1));
    let preview_h = to_physical(preview_height.max(1));
    let gap = to_physical(24);
    let min_preview_w = to_physical(i32::from(MIN_LAUNCHER_WIDTH));
    let max_preview_w = to_physical(i32::from(MAX_LAUNCHER_WIDTH));
    let (centered_x, settings_y) =
        monitor::centered_position_in_bounds(bounds, settings_w, settings_h);
    // The pair slot for the narrowest preview: the Settings window stands at the
    // left edge of a centred (Settings + gap + min preview) box.
    let (base_x, _) =
        monitor::centered_position_in_bounds(bounds, settings_w + gap + min_preview_w, settings_h);
    // The furthest left Settings may stand while the widest preview still ends
    // inside the screen.
    let max_allowed = (bounds.right - (settings_w + gap + max_preview_w)).max(bounds.left);
    let slide = (base_x - max_allowed).max(0);
    let span = (max_preview_w - min_preview_w).max(1);
    let fraction = ((preview_w - min_preview_w) as f32 / span as f32).clamp(0.0, 1.0);
    let settings_x = base_x - (slide as f32 * fraction).round() as i32;
    let preview_x = settings_x + settings_w + gap;
    let preview_y = settings_y + (settings_h - preview_h).max(0) / 2;
    (
        (settings_x, settings_y),
        (
            preview_x,
            preview_y.clamp(bounds.top, (bounds.bottom - preview_h).max(bounds.top)),
        ),
        settings_x != centered_x,
    )
}

/// The plain centred slot for the Settings window (no preview beside it).
pub(crate) fn settings_centered_position(preference: MonitorPreference) -> Option<(i32, i32)> {
    monitor::centered_position(preference, SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
}

pub(crate) fn dimension_slider_fraction(value: u16, min: u16, max: u16) -> f32 {
    if max <= min {
        return 0.0;
    }
    (value.clamp(min, max) - min) as f32 / (max - min) as f32
}

pub(crate) fn dimension_from_slider(value: f32, min: u16, max: u16) -> u16 {
    if max <= min {
        return min;
    }
    let span = (max - min) as f32;
    (min as f32 + value.clamp(0.0, 1.0) * span).round() as u16
}

pub(crate) fn parse_dimension_input(value: &str, min: u16, max: u16) -> Option<u16> {
    value
        .trim()
        .parse::<u16>()
        .ok()
        .map(|value| value.clamp(min, max))
}

pub(crate) fn apply_launcher_size(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_constants::RESULT_VIEWPORT_HEIGHT;
    use flux_core::{
        DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
        MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
    };

    #[test]
    fn the_results_window_grows_with_the_rows_it_holds() {
        // The strip and the footer stay, the viewport is exactly as tall as the
        // rows plus the list's one-time vertical padding.
        assert_eq!(launcher_content_height(0, 382), 94);
        assert_eq!(launcher_content_height(1, 382), 152);
        assert_eq!(launcher_content_height(2, 382), 198);
        assert_eq!(launcher_content_height(3, 382), 244);
        assert_eq!(launcher_content_height(4, 382), 290);
        assert_eq!(launcher_content_height(5, 382), 336);
        // Six rows fill the configured viewport; the configured height is a ceiling.
        assert_eq!(launcher_content_height(6, 382), 382);
        assert_eq!(launcher_content_height(16, 382), 382);
        assert_eq!(launcher_content_height(4, 200), 200);
    }

    #[test]
    fn short_lists_have_no_residual_scroll_range() {
        for rows in 1..=RESULT_ROWS_VISIBLE {
            let fitted = launcher_content_height(rows as usize, 382);
            let viewport = fitted - LAUNCHER_CHROME_HEIGHT;
            let content = rows * RESULT_ROW_HEIGHT + 2 * RESULT_LIST_VERTICAL_INSET;
            assert_eq!(
                viewport, content,
                "{rows} rows must not leave an unnecessary scroll range"
            );
        }

        // A seventh row is clipped by the six-row viewport and must still scroll.
        let content = 7 * RESULT_ROW_HEIGHT + 2 * RESULT_LIST_VERTICAL_INSET;
        assert!(content > RESULT_VIEWPORT_HEIGHT);
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
            (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
        );
        assert_eq!(
            launcher_window_geometry_with_sizes(true, false, 380, 300),
            (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
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
            (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn activation_clear_uses_compact_geometry_after_expanded_query() {
        assert_eq!(
            launcher_window_geometry(false, true),
            (
                DEFAULT_LAUNCHER_WIDTH as i32,
                DEFAULT_LAUNCHER_HEIGHT as i32,
            )
        );
        assert_eq!(
            launcher_window_geometry(false, false),
            (DEFAULT_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn query_cleanup_keeps_open_settings_at_full_geometry() {
        // hide-on-deactivate clears the query asynchronously. The following
        // query transition must not resize the already-open Settings panel.
        assert_eq!(
            launcher_window_geometry_with_sizes(true, false, 420, 56),
            (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn missing_everything_prompt_uses_visible_dialog_geometry() {
        assert_eq!(
            launcher_window_geometry_with_prompt(false, true, false, 420, 382),
            (
                EVERYTHING_PROMPT_WINDOW_WIDTH,
                EVERYTHING_PROMPT_WINDOW_HEIGHT
            )
        );
        assert_eq!(
            launcher_window_geometry_with_prompt(true, true, false, 420, 382),
            (SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn missing_everything_prompt_does_not_override_normal_launcher_geometry() {
        assert_eq!(
            launcher_window_geometry_with_prompt(false, false, true, 640, 520),
            (640, 520)
        );
        assert_eq!(
            launcher_window_geometry_with_prompt(false, false, false, 640, 520),
            (640, COMPACT_WINDOW_HEIGHT)
        );
    }

    #[test]
    fn visual_preview_sits_to_the_right_of_settings_at_100_percent() {
        let bounds = monitor::MonitorBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let ((settings_x, _), (x, y), _) = visual_preview_pair_in_bounds(bounds, 96, 420, 200);
        assert_eq!(x, settings_x + SETTINGS_WINDOW_WIDTH + 24);
        assert_eq!(y, 440, "vertically centered against Settings");
    }

    #[test]
    fn preview_slides_settings_left_instead_of_leaving_the_screen() {
        let bounds = monitor::MonitorBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let ((settings_x, _), (preview_x, _), shifted) =
            visual_preview_pair_in_bounds(bounds, 96, 900, 382);
        assert!(
            shifted,
            "a wide preview must move Settings from its centred slot"
        );
        assert_eq!(preview_x + 900, 1920, "the preview must end at the edge");
        assert_eq!(settings_x, 1920 - (SETTINGS_WINDOW_WIDTH + 24 + 900));
    }

    #[test]
    fn pair_slot_starts_centred_for_the_narrowest_width_and_slides_less_than_track_travel() {
        let bounds = monitor::MonitorBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let ((min_x, _), (min_px, _), _) = visual_preview_pair_in_bounds(bounds, 96, 380, 382);
        let ((max_x, _), (max_px, _), shifted) =
            visual_preview_pair_in_bounds(bounds, 96, 900, 382);
        assert_eq!(min_x, (1920 - (SETTINGS_WINDOW_WIDTH + 24 + 380)) / 2);
        assert_eq!(min_px, min_x + SETTINGS_WINDOW_WIDTH + 24);
        assert!(shifted);
        let slide = min_x - max_x;
        // Track travel = 300 - 2 * 13 = 274. The slide has to stay below it or the
        // client-space mapping diverges and the thumb runs away.
        assert_eq!(slide, 138);
        assert!(slide < 274);
        assert_eq!(max_px + 900, 1920);
    }

    #[test]
    fn visual_preview_clears_scaled_settings_at_150_percent_4k() {
        let bounds = monitor::MonitorBounds {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        let ((settings_x, _), (x, _), _) = visual_preview_pair_in_bounds(bounds, 144, 420, 200);
        let settings_right = settings_x + (SETTINGS_WINDOW_WIDTH as f32 * 1.5).round() as i32;
        assert!(
            x >= settings_right,
            "preview must start right of the DPI-scaled Settings edge ({settings_right}), got {x}"
        );
    }

    #[test]
    fn everything_prompt_requires_missing_auto_enabled_and_unseen_state() {
        assert!(should_show_everything_install_prompt(
            false, true, false, false
        ));
        assert!(!should_show_everything_install_prompt(
            true, true, false, false
        ));
        assert!(!should_show_everything_install_prompt(
            false, false, false, false
        ));
        assert!(!should_show_everything_install_prompt(
            false, true, true, false
        ));
        assert!(!should_show_everything_install_prompt(
            false, true, false, true
        ));
    }
}
