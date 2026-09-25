#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
use windui::event::{Key, KeyEvent};

pub(crate) fn is_run_as_admin_key(event: &KeyEvent) -> bool {
    event.ctrl
        && matches!(
            event.key,
            Key::Other(0x52) | Key::Char('r') | Key::Char('R')
        )
}

/// Ctrl+H opens the selectable query history. `Key::Other(0x48)` is the only
/// form Win32 sends for a letter held with Ctrl (WM_KEYDOWN carries a virtual
/// key, and the WM_CHAR path reports ctrl=false), so matching the `Char` arms
/// alone made this shortcut unreachable. VK codes are layout-independent.
pub(crate) fn is_history_key(event: &KeyEvent) -> bool {
    event.ctrl
        && matches!(
            event.key,
            Key::Other(0x48) | Key::Char('h') | Key::Char('H')
        )
}

/// The keys whose meaning is "do something with the highlighted row".
///
/// A letter reaches this handler on the Win32 key-down path as `Key::Other(vk)`,
/// *before* the following `WM_CHAR` puts it in the field, so counting text keys as
/// an action would settle the previous keystroke's half-filled snapshot onto the
/// screen every time the user types: the panel collapses to the few rows that
/// prefix has and refills a moment later, which is exactly the flash. Only the keys
/// that consume a row may do that.
pub(crate) fn acts_on_the_selected_row(event: &KeyEvent) -> bool {
    if event.ctrl {
        // Ctrl+C copies the highlighted row and Ctrl+R runs it as admin; every
        // other Ctrl chord edits the field or opens something unrelated.
        return matches!(
            event.key,
            Key::Other(0x43)
                | Key::Char('c')
                | Key::Char('C')
                | Key::Other(0x52)
                | Key::Char('r')
                | Key::Char('R')
        );
    }
    matches!(
        event.key,
        Key::Enter | Key::Tab | Key::Up | Key::Down | Key::Home | Key::End | Key::Right
    )
}

#[cfg(windows)]
pub(crate) fn shift_key_is_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}

#[cfg(not(windows))]
pub(crate) fn shift_key_is_down() -> bool {
    false
}

#[cfg(windows)]
pub(crate) fn alt_key_is_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_MENU};
    unsafe { GetKeyState(VK_MENU.0 as i32) < 0 }
}

#[cfg(not(windows))]
pub(crate) fn alt_key_is_down() -> bool {
    false
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
pub(crate) fn launcher_is_foreground() -> bool {
    false
}

pub(crate) fn should_show_launcher(is_foreground: bool) -> bool {
    !is_foreground
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_r_matches_win32_other_key_event() {
        assert!(is_run_as_admin_key(&KeyEvent {
            key: Key::Other(0x52),
            pressed: true,
            shift: false,
            ctrl: true,
        }));
        assert!(!is_run_as_admin_key(&KeyEvent {
            key: Key::Other(0x52),
            pressed: true,
            shift: false,
            ctrl: false,
        }));
    }

    #[test]
    fn ctrl_h_matches_win32_other_key_event() {
        assert!(is_history_key(&KeyEvent {
            key: Key::Other(0x48),
            pressed: true,
            shift: false,
            ctrl: true,
        }));
        assert!(!is_history_key(&KeyEvent {
            key: Key::Other(0x48),
            pressed: true,
            shift: false,
            ctrl: false,
        }));
        assert!(is_history_key(&KeyEvent {
            key: Key::Char('h'),
            pressed: true,
            shift: false,
            ctrl: true,
        }));
    }

    #[test]
    fn typing_is_not_an_action_on_the_row() {
        // Win32 reports a letter as its virtual key on the key-down path, and the
        // field only learns it from the WM_CHAR that follows.
        for key in [
            Key::Other(0x50),
            Key::Char('p'),
            Key::Backspace,
            Key::Delete,
            Key::Space,
            Key::Left,
        ] {
            assert!(
                !acts_on_the_selected_row(&KeyEvent {
                    key,
                    pressed: true,
                    shift: false,
                    ctrl: false,
                }),
                "a key that edits the field must not settle a half-filled snapshot"
            );
        }
    }

    #[test]
    fn row_actions_are_the_keys_that_consume_a_result() {
        for key in [
            Key::Enter,
            Key::Tab,
            Key::Up,
            Key::Down,
            Key::Home,
            Key::End,
            Key::Right,
        ] {
            assert!(acts_on_the_selected_row(&KeyEvent {
                key,
                pressed: true,
                shift: false,
                ctrl: false,
            }));
        }
        assert!(acts_on_the_selected_row(&KeyEvent {
            key: Key::Other(0x43),
            pressed: true,
            shift: true,
            ctrl: true,
        }));
        assert!(!acts_on_the_selected_row(&KeyEvent {
            key: Key::Other(0x56),
            pressed: true,
            shift: false,
            ctrl: true,
        }));
        // Alt+Enter and Shift+Enter open the highlighted row too.
        assert!(acts_on_the_selected_row(&KeyEvent {
            key: Key::Enter,
            pressed: true,
            shift: true,
            ctrl: false,
        }));
    }

    #[test]
    fn activation_shows_when_flux_is_not_foreground() {
        assert!(should_show_launcher(false));
        assert!(!should_show_launcher(true));
    }
}
