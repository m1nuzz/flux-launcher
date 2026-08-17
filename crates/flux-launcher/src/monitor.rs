use flux_core::MonitorPreference;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MonitorBounds {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl MonitorBounds {
    fn width(self) -> i32 {
        self.right - self.left
    }

    fn height(self) -> i32 {
        self.bottom - self.top
    }
}

pub(crate) fn centered_position(
    preference: MonitorPreference,
    window_width: i32,
    window_height: i32,
) -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        let bounds = selected_monitor_bounds(preference)?;
        Some(centered_position_in_bounds(
            bounds,
            window_width,
            window_height,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (preference, window_width, window_height);
        None
    }
}

pub(crate) fn centered_position_in_bounds(
    bounds: MonitorBounds,
    window_width: i32,
    window_height: i32,
) -> (i32, i32) {
    let width = window_width.max(1).min(bounds.width().max(1));
    let height = window_height.max(1).min(bounds.height().max(1));
    (
        bounds.left + (bounds.width() - width).max(0) / 2,
        bounds.top + (bounds.height() - height).max(0) / 2,
    )
}

#[cfg(windows)]
fn selected_monitor_bounds(preference: MonitorPreference) -> Option<MonitorBounds> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, MONITORINFOF_PRIMARY,
    };

    fn bounds_for_monitor(monitor: HMONITOR) -> Option<(MonitorBounds, u32)> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() } {
            return None;
        }
        let rect = info.rcWork;
        Some((
            MonitorBounds {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            },
            info.dwFlags,
        ))
    }

    match preference {
        MonitorPreference::Cursor => {
            let mut point = POINT::default();
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                return selected_monitor_bounds(MonitorPreference::Primary);
            }
            let monitor = unsafe {
                windows::Win32::Graphics::Gdi::MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST)
            };
            bounds_for_monitor(monitor).map(|(bounds, _)| bounds)
        }
        MonitorPreference::Foreground => {
            let foreground = unsafe { GetForegroundWindow() };
            if foreground.is_invalid() {
                return selected_monitor_bounds(MonitorPreference::Primary);
            }
            let monitor = unsafe {
                windows::Win32::Graphics::Gdi::MonitorFromWindow(
                    foreground,
                    MONITOR_DEFAULTTONEAREST,
                )
            };
            bounds_for_monitor(monitor).map(|(bounds, _)| bounds)
        }
        MonitorPreference::Primary => {
            struct EnumState {
                primary: Option<MonitorBounds>,
                first: Option<MonitorBounds>,
            }

            unsafe extern "system" fn callback(
                monitor: HMONITOR,
                _dc: HDC,
                _rect: *mut RECT,
                data: LPARAM,
            ) -> BOOL {
                let state = &mut *(data.0 as *mut EnumState);
                if let Some((bounds, flags)) = bounds_for_monitor(monitor) {
                    state.first.get_or_insert(bounds);
                    if flags & MONITORINFOF_PRIMARY != 0 {
                        state.primary = Some(bounds);
                    }
                }
                BOOL(1)
            }

            let mut state = EnumState {
                primary: None,
                first: None,
            };
            let _ = unsafe {
                EnumDisplayMonitors(
                    None,
                    None,
                    Some(callback),
                    LPARAM(&mut state as *mut EnumState as isize),
                )
            };
            state.primary.or(state.first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_window_inside_positive_monitor_work_area() {
        let bounds = MonitorBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        assert_eq!(centered_position_in_bounds(bounds, 420, 72), (750, 504));
    }

    #[test]
    fn preserves_negative_virtual_desktop_coordinates() {
        let bounds = MonitorBounds {
            left: -1920,
            top: 100,
            right: 0,
            bottom: 1180,
        };
        assert_eq!(centered_position_in_bounds(bounds, 420, 520), (-750, 380));
    }

    #[test]
    fn clamps_window_larger_than_monitor() {
        let bounds = MonitorBounds {
            left: 100,
            top: 50,
            right: 500,
            bottom: 350,
        };
        assert_eq!(centered_position_in_bounds(bounds, 800, 700), (100, 50));
    }
}
