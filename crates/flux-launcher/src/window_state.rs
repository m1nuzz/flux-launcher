use std::sync::{Arc, RwLock};

use crate::monitor;
use crate::{
    COMPACT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_HEIGHT, EVERYTHING_PROMPT_WINDOW_WIDTH,
    MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
    SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_WIDTH,
};
use flux_core::{MonitorPreference, Settings};
#[cfg(test)]
use flux_core::{DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH};
use windui::app::{WindowPositionHandle, WindowSizeHandle};
use windui::prelude::*;

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

pub(crate) fn request_scroll(scroll_pending: Signal<bool>) {
    scroll_pending.set(true);
}

#[cfg(test)]
pub(crate) fn launcher_window_geometry(settings_visible: bool, show_results: bool) -> (i32, i32) {
    launcher_window_geometry_with_sizes(
        settings_visible,
        show_results,
        DEFAULT_LAUNCHER_WIDTH as i32,
        DEFAULT_LAUNCHER_HEIGHT as i32,
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

#[cfg(windows)]
pub(crate) fn launcher_is_foreground() -> bool {
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

pub(crate) fn should_show_launcher(is_foreground: bool) -> bool {
    !is_foreground
}
