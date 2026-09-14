use std::sync::{Arc, RwLock};

use flux_core::{
    MonitorPreference, Settings, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT,
    MIN_LAUNCHER_WIDTH,
};
use windui::app::{WindowPositionHandle, WindowSizeHandle};

use super::monitor;
use super::ui_constants::{
    COMPACT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_WIDTH,
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
