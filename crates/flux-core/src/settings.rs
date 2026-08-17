use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_CARET_DURATION_MS: u16 = 95;
const MIN_CARET_DURATION_MS: u16 = 60;
const MAX_CARET_DURATION_MS: u16 = 160;
const MAX_QUERY_HISTORY: usize = 32;

fn enabled_by_default() -> bool {
    true
}

fn default_caret_duration() -> u16 {
    DEFAULT_CARET_DURATION_MS
}

fn default_selection_color() -> u32 {
    0x4c8bf4
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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum MonitorPreference {
    #[serde(rename = "primary")]
    Primary,
    #[default]
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "foreground")]
    Foreground,
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
    #[serde(default = "enabled_by_default")]
    pub use_system_accent: bool,
    #[serde(default = "default_selection_color")]
    pub custom_selection_color: u32,
    #[serde(default = "enabled_by_default")]
    pub clear_query_on_activation: bool,
    #[serde(default = "enabled_by_default")]
    pub auto_enable_everything: bool,
    #[serde(default)]
    pub monitor_preference: MonitorPreference,
    #[serde(default = "default_caret_duration")]
    pub smooth_caret_duration_ms: u16,
    #[serde(default)]
    pub query_history: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            activation_hotkey: HotkeyConfig::default(),
            ignore_hotkeys_in_fullscreen: true,
            game_mode: false,
            smooth_caret: true,
            switch_to_english_layout: true,
            use_system_accent: true,
            custom_selection_color: default_selection_color(),
            clear_query_on_activation: true,
            auto_enable_everything: true,
            monitor_preference: MonitorPreference::default(),
            smooth_caret_duration_ms: DEFAULT_CARET_DURATION_MS,
            query_history: Vec::new(),
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
        self.normalize_query_history();
    }

    pub fn record_query(&mut self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return false;
        }
        if let Some(index) = self
            .query_history
            .iter()
            .position(|item| item.eq_ignore_ascii_case(query))
        {
            self.query_history.remove(index);
        }
        self.query_history.push(query.to_owned());
        self.normalize_query_history();
        true
    }

    pub fn clear_query_history(&mut self) {
        self.query_history.clear();
    }

    fn normalize_query_history(&mut self) {
        let mut normalized = Vec::with_capacity(self.query_history.len());
        for query in self.query_history.drain(..) {
            let query = query.trim();
            if query.is_empty()
                || normalized
                    .iter()
                    .any(|item: &String| item.eq_ignore_ascii_case(query))
            {
                continue;
            }
            normalized.push(query.to_owned());
        }
        if normalized.len() > MAX_QUERY_HISTORY {
            let start = normalized.len() - MAX_QUERY_HISTORY;
            normalized.drain(..start);
        }
        self.query_history = normalized;
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
        assert!(settings.use_system_accent);
        assert_eq!(settings.custom_selection_color, 0x4c8bf4);
        assert!(settings.clear_query_on_activation);
        assert!(settings.auto_enable_everything);
        assert_eq!(settings.monitor_preference, MonitorPreference::Cursor);
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
            use_system_accent: false,
            custom_selection_color: 0x12ab34,
            clear_query_on_activation: false,
            auto_enable_everything: false,
            monitor_preference: MonitorPreference::Foreground,
            smooth_caret_duration_ms: 120,
            query_history: vec![String::from("steam"), String::from("ext:zip")],
        };

        expected.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_monitor_preference_uses_cursor_default() {
        let path = temporary_path("settings-monitor-default");
        fs::write(&path, r#"{"activation_hotkey":{"key":"Space"}}"#).unwrap();
        let settings = Settings::load_from(&path).unwrap();
        assert_eq!(settings.monitor_preference, MonitorPreference::Cursor);
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
    fn query_history_is_deduplicated_and_bounded() {
        let mut settings = Settings::default();
        assert!(settings.record_query(" Steam "));
        assert!(settings.record_query("ext:zip"));
        assert!(settings.record_query("steam"));
        assert_eq!(settings.query_history, ["ext:zip", "steam"]);
        settings
            .query_history
            .extend((0..40).map(|i| format!("q{i}")));
        settings.normalize();
        assert_eq!(settings.query_history.len(), MAX_QUERY_HISTORY);
        assert_eq!(settings.query_history.last().unwrap(), "q39");
        settings.clear_query_history();
        assert!(settings.query_history.is_empty());
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
