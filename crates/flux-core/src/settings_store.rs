use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::settings::Settings;

impl Settings {
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
    use crate::settings::{HotkeyConfig, MonitorPreference, PriorityEntry};
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
            launcher_width: 640,
            launcher_height: 520,
            clear_query_on_activation: false,
            start_with_windows: false,
            auto_enable_everything: false,
            everything_install_prompt_seen: true,
            update_checks_enabled: false,
            update_interval_hours: 6,
            auto_install_updates: true,
            last_update_check_unix: 1_700_000_000,
            obsidian_enabled: false,
            obsidian_alias: String::from("notes"),
            google_enabled: false,
            google_alias: String::from("search"),
            monitor_preference: MonitorPreference::Foreground,
            smooth_caret_duration_ms: 120,
            query_history: vec![String::from("steam"), String::from("ext:zip")],
            history_selections: std::collections::HashMap::from([(
                String::from("steam"),
                String::from("application:steam"),
            )]),
            priority_entries: vec![PriorityEntry {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                target: String::from("C:/Steam.lnk"),
            }],
            total_queries_committed: 41,
            launch_counts: std::collections::HashMap::from([(
                String::from("application:steam"),
                crate::settings::LaunchCount {
                    title: String::from("Steam"),
                    count: 7,
                },
            )]),
        };

        expected.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_obsidian_fields_use_backward_compatible_defaults() {
        use crate::settings::DEFAULT_UPDATE_INTERVAL_HOURS;

        let path = temporary_path("settings-obsidian-default");
        fs::write(&path, r#"{"activation_hotkey":{"key":"Space"}}"#).unwrap();
        let settings = Settings::load_from(&path).unwrap();
        assert!(settings.start_with_windows);
        assert!(!settings.everything_install_prompt_seen);
        assert!(settings.update_checks_enabled);
        assert_eq!(
            settings.update_interval_hours,
            DEFAULT_UPDATE_INTERVAL_HOURS
        );
        assert!(!settings.auto_install_updates);
        assert!(settings.obsidian_enabled);
        assert_eq!(settings.obsidian_alias, "ob");
        assert!(settings.google_enabled);
        assert_eq!(settings.google_alias, "g");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_monitor_preference_uses_cursor_default() {
        use crate::settings::{DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH};

        let path = temporary_path("settings-monitor-default");
        fs::write(&path, r#"{"activation_hotkey":{"key":"Space"}}"#).unwrap();
        let settings = Settings::load_from(&path).unwrap();
        assert_eq!(settings.monitor_preference, MonitorPreference::Cursor);
        assert_eq!(settings.launcher_width, DEFAULT_LAUNCHER_WIDTH);
        assert_eq!(settings.launcher_height, DEFAULT_LAUNCHER_HEIGHT);
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
}
