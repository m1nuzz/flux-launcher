#[cfg(windows)]
#[allow(dead_code)]
pub fn foreground_is_fullscreen() -> bool {
    use flux_core::{matches_display_bounds, WindowBounds};
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
        IsIconic, IsWindowVisible,
    };

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground == HWND::default()
            || foreground == GetDesktopWindow()
            || foreground == GetShellWindow()
            || !IsWindowVisible(foreground).as_bool()
            || IsIconic(foreground).as_bool()
        {
            return false;
        }

        let mut class_name = [0_u16; 256];
        let class_length = GetClassNameW(foreground, &mut class_name);
        let class = String::from_utf16_lossy(&class_name[..class_length.max(0) as usize]);
        if matches!(class.as_str(), "Flip3D" | "Progman" | "WorkerW") {
            return false;
        }

        let mut window_rect = RECT::default();
        if GetWindowRect(foreground, &mut window_rect).is_err() {
            return false;
        }
        let initial_bounds = bounds_from_rect(window_rect);
        if class == "ConsoleWindowClass" {
            return initial_bounds.top < 0 && initial_bounds.bottom < 0;
        }

        let monitor = MonitorFromWindow(foreground, MONITOR_DEFAULTTONEAREST);
        if monitor.0.is_null() {
            return false;
        }
        let mut monitor_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut monitor_info).as_bool() {
            return false;
        }

        let visible_bounds = dwm_extended_frame_bounds(foreground).unwrap_or(initial_bounds);
        return matches_display_bounds(visible_bounds, bounds_from_rect(monitor_info.rcMonitor));
    }

    unsafe fn dwm_extended_frame_bounds(
        hwnd: windows::Win32::Foundation::HWND,
    ) -> Option<WindowBounds> {
        let mut rect = windows::Win32::Foundation::RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut c_void,
            size_of::<windows::Win32::Foundation::RECT>() as u32,
        )
        .ok()?;
        Some(bounds_from_rect(rect))
    }

    const fn bounds_from_rect(rect: RECT) -> WindowBounds {
        WindowBounds {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn foreground_is_fullscreen() -> bool {
    false
}
