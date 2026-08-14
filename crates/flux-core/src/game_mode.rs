use crate::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WindowBounds {
    pub const fn width(self) -> i32 {
        self.right - self.left
    }

    pub const fn height(self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowClass {
    Console,
    Desktop,
    Flip3D,
    Normal,
    Shell,
}

pub const fn should_suppress_activation(
    settings: &Settings,
    foreground_is_fullscreen: bool,
) -> bool {
    settings.game_mode || (settings.ignore_hotkeys_in_fullscreen && foreground_is_fullscreen)
}

pub const fn matches_display_bounds(window: WindowBounds, monitor: WindowBounds) -> bool {
    window.width() == monitor.width() && window.height() == monitor.height()
}

pub const fn is_flow_excluded_class(class: WindowClass) -> bool {
    matches!(
        class,
        WindowClass::Desktop | WindowClass::Flip3D | WindowClass::Shell
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: WindowBounds = WindowBounds {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };

    #[test]
    fn display_match_requires_full_display_not_work_area() {
        assert!(matches_display_bounds(DISPLAY, DISPLAY));
        assert!(!matches_display_bounds(
            WindowBounds {
                bottom: 1400,
                ..DISPLAY
            },
            DISPLAY
        ));
    }

    #[test]
    fn shell_and_flip3d_are_never_games() {
        assert!(is_flow_excluded_class(WindowClass::Desktop));
        assert!(is_flow_excluded_class(WindowClass::Shell));
        assert!(is_flow_excluded_class(WindowClass::Flip3D));
        assert!(!is_flow_excluded_class(WindowClass::Console));
        assert!(!is_flow_excluded_class(WindowClass::Normal));
    }

    #[test]
    fn manual_game_mode_has_priority_over_fullscreen_preference() {
        let disabled_protection = Settings {
            ignore_hotkeys_in_fullscreen: false,
            game_mode: true,
            ..Settings::default()
        };
        assert!(should_suppress_activation(&disabled_protection, false));

        let fullscreen_protection = Settings {
            ignore_hotkeys_in_fullscreen: true,
            game_mode: false,
            ..Settings::default()
        };
        assert!(should_suppress_activation(&fullscreen_protection, true));
        assert!(!should_suppress_activation(&fullscreen_protection, false));
    }
}
