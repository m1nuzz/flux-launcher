use std::sync::{Arc, RwLock};

use flux_core::{
    MonitorPreference, Settings, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT,
    MIN_LAUNCHER_WIDTH,
};
use windui::app::{WindowPositionHandle, WindowSizeHandle};

use super::monitor;
use super::ui_constants::{
    COMPACT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_WIDTH,
    LAUNCHER_CHROME_HEIGHT, RESULT_ROWS_VISIBLE, RESULT_ROW_PITCH, SETTINGS_WINDOW_HEIGHT,
    SETTINGS_WINDOW_WIDTH,
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

/// Shrink the results window to the rows it actually holds.
///
/// The configured `launcher_height` is a maximum: the result viewport is a fixed
/// six-row area, so a query that answers with one row used to leave five rows of
/// empty acrylic under it - which reads as a broken panel for as long as it takes
/// to type the next character. The search strip and the footer stay, the viewport
/// follows the row count, and anything past the six visible rows still scrolls.
pub(crate) fn launcher_content_height(result_count: usize, configured_height: i32) -> i32 {
    let rows = result_count.min(RESULT_ROWS_VISIBLE as usize) as i32;
    let content = LAUNCHER_CHROME_HEIGHT + rows * RESULT_ROW_PITCH;
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

pub(crate) fn visual_preview_position(
    preference: MonitorPreference,
    preview_width: i32,
    preview_height: i32,
) -> (i32, i32) {
    #[cfg(windows)]
    {
        let Some((bounds, dpi)) = monitor::work_area_with_dpi(preference) else {
            return (0, 0);
        };
        visual_preview_position_in_bounds(bounds, dpi, preview_width, preview_height)
    }
    #[cfg(not(windows))]
    {
        let _ = (preference, preview_width, preview_height);
        (0, 0)
    }
}

/// Pure side-by-side placement of the launcher preview next to the centered Settings
/// window, returned in **physical** screen pixels (the preview `App::position` and the
/// monitor work area are both physical). `preview_width`/`preview_height` are logical
/// (DIP) client dimensions, exactly as the Settings constants are logical, so every size
/// is scaled by the monitor DPI here. Before this scaling existed the horizontal offset
/// added a logical Settings width to a physical Settings origin, so at >100% scaling
/// (4K) the preview slid left and covered the Settings menu; at 100% it only looked
/// correct by coincidence.
pub(crate) fn visual_preview_position_in_bounds(
    bounds: monitor::MonitorBounds,
    dpi: u32,
    preview_width: i32,
    preview_height: i32,
) -> (i32, i32) {
    let scale = dpi.max(96) as f32 / 96.0;
    let to_physical = |logical: i32| (logical as f32 * scale).round() as i32;
    let settings_phys_w = to_physical(SETTINGS_WINDOW_WIDTH);
    let settings_phys_h = to_physical(SETTINGS_WINDOW_HEIGHT);
    let preview_phys_w = to_physical(preview_width.max(1));
    let preview_phys_h = to_physical(preview_height.max(1));
    let gap = to_physical(24);
    let (settings_x, settings_y) =
        monitor::centered_position_in_bounds(bounds, settings_phys_w, settings_phys_h);
    let right_x = settings_x + settings_phys_w + gap;
    let left_x = settings_x - preview_phys_w - gap;
    // Prefer a fully visible side-by-side preview. When neither side fits (a small or
    // CI desktop), keep it on the right and let Windows clip the off-screen portion
    // rather than cover the controls being dragged.
    let x = if right_x + preview_phys_w <= bounds.right {
        right_x
    } else if left_x >= bounds.left {
        left_x
    } else {
        right_x
    };
    let y = settings_y + (settings_phys_h - preview_phys_h).max(0) / 2;
    (
        x,
        y.clamp(bounds.top, (bounds.bottom - preview_phys_h).max(bounds.top)),
    )
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
    use flux_core::{
        DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
        MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
    };

    #[test]
    fn the_results_window_grows_with_the_rows_it_holds() {
        // The strip and the footer stay, the viewport is as tall as the rows.
        assert_eq!(launcher_content_height(0, 382), 94);
        assert_eq!(launcher_content_height(1, 382), 142);
        assert_eq!(launcher_content_height(3, 382), 238);
        // Six rows fill the configured viewport; the configured height is a ceiling.
        assert_eq!(launcher_content_height(6, 382), 382);
        assert_eq!(launcher_content_height(16, 382), 382);
        assert_eq!(launcher_content_height(4, 200), 200);
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
        // Settings 720 wide centered → left 600, right 1320; +24 gap = 1344.
        let (x, y) = visual_preview_position_in_bounds(bounds, 96, 420, 200);
        assert_eq!(x, 1344);
        assert_eq!(y, 440, "vertically centered against Settings");
    }

    #[test]
    fn visual_preview_clears_scaled_settings_at_150_percent_4k() {
        let bounds = monitor::MonitorBounds {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2160,
        };
        let (x, _y) = visual_preview_position_in_bounds(bounds, 144, 420, 200);
        // Settings physical width 720*1.5=1080 centered → left 1380, right 2460.
        let settings_right = 1380 + (SETTINGS_WINDOW_WIDTH as f32 * 1.5).round() as i32;
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
