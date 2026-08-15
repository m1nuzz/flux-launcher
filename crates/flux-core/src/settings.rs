use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_CARET_DURATION_MS: u16 = 95;
const MIN_CARET_DURATION_MS: u16 = 60;
const MAX_CARET_DURATION_MS: u16 = 160;

fn enabled_by_default() -> bool {
    true
}

fn default_caret_duration() -> u16 {
    DEFAULT_CARET_DURATION_MS
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            ctrl: false,
            alt: true,
            shift: false,
            meta: false,
            key: String::from("Space"),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default)]
    pub activation_hotkey: HotkeyConfig,
    #[serde(default = "enabled_by_default")]
    pub ignore_hotkeys_in_fullscreen: bool,
    pub game_mode: bool,
    #[serde(default = "enabled_by_default")]
    pub smooth_caret: bool,
    #[serde(default = "enabled_by_default")]
    pub switch_to_english_layout: bool,
    #[serde(default = "default_caret_duration")]
    pub smooth_caret_duration_ms: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            activation_hotkey: HotkeyConfig::default(),
            ignore_hotkeys_in_fullscreen: true,
            game_mode: false,
            smooth_caret: true,
            switch_to_english_layout: true,
            smooth_caret_duration_ms: DEFAULT_CARET_DURATION_MS,
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        self.smooth_caret_duration_ms = self
            .smooth_caret_duration_ms
            .clamp(MIN_CARET_DURATION_MS, MAX_CARET_DURATION_MS);
        if self.activation_hotkey.key.trim().is_empty() {
            self.activation_hotkey = HotkeyConfig::default();
        }
    }

    pub fn config_path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("FluxLauncher").join("settings.json")
    }

    pub fn load_or_default() -> Self {
        Self::load_from(&Self::config_path()).unwrap_or_default()
    }

    pub fn load_from(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let mut settings: Self = serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        settings.normalize();
        Ok(settings)
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::config_path())
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "settings path must have a parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;

        let mut normalized = self.clone();
        normalized.normalize();
        let payload = serde_json::to_vec_pretty(&normalized)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, payload)?;
        fs::rename(temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn temporary_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "flux-launcher-{name}-{}-{sequence}.json",
            std::process::id()
        ))
    }

    #[test]
    fn defaults_protect_fullscreen_enable_smooth_caret_and_use_alt_space() {
        let settings = Settings::default();
        assert_eq!(settings.activation_hotkey, HotkeyConfig::default());
        assert!(settings.ignore_hotkeys_in_fullscreen);
        assert!(settings.smooth_caret);
        assert!(settings.switch_to_english_layout);
        assert_eq!(settings.smooth_caret_duration_ms, DEFAULT_CARET_DURATION_MS);
    }

    #[test]
    fn round_trip_preserves_preferences() {
        let path = temporary_path("settings-round-trip");
        let expected = Settings {
            activation_hotkey: HotkeyConfig {
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
                key: String::from("F12"),
            },
            ignore_hotkeys_in_fullscreen: false,
            game_mode: true,
            smooth_caret: false,
            switch_to_english_layout: false,
            smooth_caret_duration_ms: 120,
        };

        expected.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn load_rejects_malformed_json() {
        let path = temporary_path("settings-malformed");
        fs::write(&path, "not json").unwrap();
        assert_eq!(
            Settings::load_from(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn duration_is_clamped_to_motion_budget() {
        let mut settings = Settings {
            smooth_caret_duration_ms: 1,
            ..Settings::default()
        };
        settings.normalize();
        assert_eq!(settings.smooth_caret_duration_ms, MIN_CARET_DURATION_MS);

        settings.smooth_caret_duration_ms = u16::MAX;
        settings.normalize();
        assert_eq!(settings.smooth_caret_duration_ms, MAX_CARET_DURATION_MS);
    }
}
