use flux_core::HotkeyConfig;
use windui::event::{Key as EventKey, KeyEvent};
use windui::prelude::{Hotkey, Key};

pub fn activation_hotkey(config: &HotkeyConfig) -> Hotkey {
    let mut hotkey = Hotkey::new(parse_key(&config.key));
    if config.ctrl {
        hotkey = hotkey.ctrl();
    }
    if config.alt {
        hotkey = hotkey.alt();
    }
    if config.shift {
        hotkey = hotkey.shift();
    }
    if config.meta {
        hotkey = hotkey.meta();
    }
    hotkey
}

pub fn game_mode_toggle_hotkey() -> Hotkey {
    Hotkey::new(Key::Other(0x7B)).ctrl()
}

/// Converts a captured key event into the persisted hotkey configuration.
///
/// The Win32 backend supplies physical keys as `Key::Other(vk)`, so Numpad5
/// (VK 0x65) remains distinct from the top-row 5 (VK 0x35).
pub fn capture_config(event: &KeyEvent, alt: bool, meta: bool) -> Option<HotkeyConfig> {
    if !event.pressed || is_modifier_key(event.key) {
        return None;
    }
    let key = key_name(event.key)?;
    Some(HotkeyConfig {
        ctrl: event.ctrl,
        alt,
        shift: event.shift,
        meta,
        key,
    })
}

pub fn display_config(config: &HotkeyConfig) -> String {
    let mut parts = Vec::with_capacity(5);
    if config.ctrl {
        parts.push(String::from("Ctrl"));
    }
    if config.alt {
        parts.push(String::from("Alt"));
    }
    if config.shift {
        parts.push(String::from("Shift"));
    }
    if config.meta {
        parts.push(String::from("Win"));
    }
    parts.push(config.key.clone());
    parts.join(" + ")
}

pub fn meta_key_is_down() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_LWIN, VK_RWIN};
        unsafe { GetKeyState(VK_LWIN.0 as i32) < 0 || GetKeyState(VK_RWIN.0 as i32) < 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn parse_key(value: &str) -> Key {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "SPACE" => Key::Space,
        "TAB" => Key::Tab,
        "ENTER" | "RETURN" => Key::Enter,
        "ESC" | "ESCAPE" => Key::Escape,
        "BACKSPACE" => Key::Backspace,
        "DELETE" | "DEL" => Key::Delete,
        "LEFT" => Key::Left,
        "RIGHT" => Key::Right,
        "UP" => Key::Up,
        "DOWN" => Key::Down,
        "HOME" => Key::Home,
        "END" => Key::End,
        _ => named_virtual_key(&normalized)
            .or_else(|| function_key(&normalized))
            .map(Key::Other)
            .or_else(|| {
                normalized
                    .chars()
                    .next()
                    .filter(|_| normalized.chars().count() == 1)
                    .map(Key::Char)
            })
            .unwrap_or(Key::Space),
    }
}

fn key_name(key: EventKey) -> Option<String> {
    match key {
        EventKey::Space => Some(String::from("Space")),
        EventKey::Tab => Some(String::from("Tab")),
        EventKey::Enter => Some(String::from("Enter")),
        EventKey::Escape => Some(String::from("Escape")),
        EventKey::Backspace => Some(String::from("Backspace")),
        EventKey::Delete => Some(String::from("Delete")),
        EventKey::Left => Some(String::from("Left")),
        EventKey::Right => Some(String::from("Right")),
        EventKey::Up => Some(String::from("Up")),
        EventKey::Down => Some(String::from("Down")),
        EventKey::Home => Some(String::from("Home")),
        EventKey::End => Some(String::from("End")),
        EventKey::Char(value) if value.is_ascii_alphanumeric() => {
            Some(value.to_ascii_uppercase().to_string())
        }
        EventKey::Char(_) => None,
        EventKey::Other(vk) => virtual_key_name(vk),
    }
}

fn is_modifier_key(key: EventKey) -> bool {
    matches!(
        key,
        EventKey::Other(0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5)
    )
}

fn virtual_key_name(vk: u32) -> Option<String> {
    let name = match vk {
        0x30..=0x39 | 0x41..=0x5A => return Some(char::from_u32(vk)?.to_string()),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        0x6A => String::from("NumpadMultiply"),
        0x6B => String::from("NumpadAdd"),
        0x6D => String::from("NumpadSubtract"),
        0x6E => String::from("NumpadDecimal"),
        0x6F => String::from("NumpadDivide"),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x2D => String::from("Insert"),
        0x21 => String::from("PageUp"),
        0x22 => String::from("PageDown"),
        0x23 => String::from("End"),
        0x24 => String::from("Home"),
        0x25 => String::from("Left"),
        0x26 => String::from("Up"),
        0x27 => String::from("Right"),
        0x28 => String::from("Down"),
        0x20 => String::from("Space"),
        0x09 => String::from("Tab"),
        0x0D => String::from("Enter"),
        0x1B => String::from("Escape"),
        0x08 => String::from("Backspace"),
        0x2E => String::from("Delete"),
        _ => return None,
    };
    Some(name)
}

fn named_virtual_key(value: &str) -> Option<u32> {
    match value {
        "INSERT" => Some(0x2D),
        "PAGEUP" => Some(0x21),
        "PAGEDOWN" => Some(0x22),
        "NUMPADMULTIPLY" => Some(0x6A),
        "NUMPADADD" => Some(0x6B),
        "NUMPADSUBTRACT" => Some(0x6D),
        "NUMPADDECIMAL" => Some(0x6E),
        "NUMPADDIVIDE" => Some(0x6F),
        _ => value
            .strip_prefix("NUMPAD")
            .filter(|number| {
                number.len() == 1 && number.chars().all(|value| value.is_ascii_digit())
            })
            .and_then(|number| number.parse::<u32>().ok())
            .filter(|number| *number <= 9)
            .map(|number| 0x60 + number),
    }
}

fn function_key(value: &str) -> Option<u32> {
    let number = value.strip_prefix('F')?.parse::<u32>().ok()?;
    (1..=24).contains(&number).then_some(0x70 + number - 1)
}

#[cfg(test)]
mod tests {
    use super::{capture_config, display_config, function_key, key_name, parse_key};
    use windui::event::{Key, KeyEvent};
    use windui::prelude::Key as HotkeyKey;

    #[test]
    fn maps_full_function_key_range() {
        assert_eq!(function_key("F1"), Some(0x70));
        assert_eq!(function_key("F12"), Some(0x7B));
        assert_eq!(function_key("F24"), Some(0x87));
        assert_eq!(function_key("F25"), None);
    }

    #[test]
    fn captures_numpad_as_distinct_from_top_row_digit() {
        let numpad = KeyEvent {
            key: Key::Other(0x65),
            pressed: true,
            shift: false,
            ctrl: true,
        };
        let top_row = KeyEvent {
            key: Key::Other(0x35),
            pressed: true,
            shift: false,
            ctrl: true,
        };
        assert_eq!(capture_config(&numpad, true, false).unwrap().key, "Numpad5");
        assert_eq!(capture_config(&top_row, true, false).unwrap().key, "5");
    }

    #[test]
    fn round_trips_numpad_and_named_keys_into_hotkey_keys() {
        assert_eq!(parse_key("Numpad5"), HotkeyKey::Other(0x65));
        assert_eq!(parse_key("Insert"), HotkeyKey::Other(0x2D));
        assert_eq!(parse_key("PageDown"), HotkeyKey::Other(0x22));
        assert_eq!(parse_key("Space"), HotkeyKey::Space);
    }

    #[test]
    fn formats_modifiers_in_stable_order() {
        let config = flux_core::HotkeyConfig {
            ctrl: true,
            alt: true,
            shift: false,
            meta: false,
            key: String::from("Numpad5"),
        };
        assert_eq!(display_config(&config), "Ctrl + Alt + Numpad5");
    }

    #[test]
    fn ignores_modifier_only_events() {
        let event = KeyEvent {
            key: Key::Other(0xA5),
            pressed: true,
            shift: false,
            ctrl: false,
        };
        assert!(capture_config(&event, true, false).is_none());
        assert_eq!(key_name(Key::Other(0x65)).as_deref(), Some("Numpad5"));
    }
}
