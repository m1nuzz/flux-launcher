use flux_core::HotkeyConfig;
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

fn parse_key(value: &str) -> Key {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "SPACE" => Key::Space,
        "TAB" => Key::Tab,
        "ENTER" => Key::Enter,
        "ESC" | "ESCAPE" => Key::Escape,
        "BACKSPACE" => Key::Backspace,
        "DELETE" => Key::Delete,
        "LEFT" => Key::Left,
        "RIGHT" => Key::Right,
        "UP" => Key::Up,
        "DOWN" => Key::Down,
        "HOME" => Key::Home,
        "END" => Key::End,
        _ => function_key(&normalized)
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

fn function_key(value: &str) -> Option<u32> {
    let number = value.strip_prefix('F')?.parse::<u32>().ok()?;
    (1..=24).contains(&number).then_some(0x70 + number - 1)
}

#[cfg(test)]
mod tests {
    use super::function_key;

    #[test]
    fn maps_full_function_key_range() {
        assert_eq!(function_key("F1"), Some(0x70));
        assert_eq!(function_key("F12"), Some(0x7B));
        assert_eq!(function_key("F24"), Some(0x87));
        assert_eq!(function_key("F25"), None);
    }
}
