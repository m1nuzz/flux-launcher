use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::settings::Settings;

/// What happened to the settings file on disk while loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsLoadOutcome {
    /// The stored settings were parsed, or there is no settings file yet.
    Loaded,
    /// The file could not be parsed and was renamed to this path, so its bytes
    /// survive while this run uses defaults.
    MovedAside(PathBuf),
    /// The file could not be parsed and could not be renamed either.
    Unreadable,
}

impl Settings {
    pub fn config_path() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("FluxLauncher").join("settings.json")
    }

    /// Load settings from `path`, falling back to defaults, and report what
    /// became of the file.
    ///
    /// A file that exists but cannot be parsed is moved aside first: the launcher
    /// saves settings after every launch, so leaving unreadable bytes in place
    /// would replace the only copy of the user's history and priorities with
    /// defaults. Replacing the file takes the same directory rename permission as
    /// moving it aside, so a save cannot destroy anything the move could not.
    pub fn load_or_default_from(path: &Path) -> (Self, SettingsLoadOutcome) {
        match Self::load_from(path) {
            Ok(settings) => (settings, SettingsLoadOutcome::Loaded),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => match quarantine(path) {
                Ok(backup) => (Self::default(), SettingsLoadOutcome::MovedAside(backup)),
                Err(_) => (Self::default(), SettingsLoadOutcome::Unreadable),
            },
            Err(_) => (Self::default(), SettingsLoadOutcome::Loaded),
        }
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

/// Rename an unreadable settings file to a free `*.json.corrupt[-N]` name so its
/// bytes survive, and report where they went.
fn quarantine(path: &Path) -> io::Result<PathBuf> {
    let mut backup = path.with_extension("json.corrupt");
    let mut index = 1_u32;
    while backup.exists() {
        backup = path.with_extension(format!("json.corrupt-{index}"));
        index += 1;
    }
    fs::rename(path, &backup)?;
    Ok(backup)
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

    #[test]
    fn malformed_settings_are_moved_aside_before_defaults_can_replace_them() {
        let path = temporary_path("settings-quarantine");
        let backup = path.with_extension("json.corrupt");
        fs::write(&path, r#"{"query_history": [unclosed"#).unwrap();

        let (settings, outcome) = Settings::load_or_default_from(&path);
        assert_eq!(settings, Settings::default());
        assert_eq!(outcome, SettingsLoadOutcome::MovedAside(backup.clone()));
        assert!(!path.exists(), "unreadable bytes must not stay");
        assert_eq!(
            fs::read_to_string(&backup).unwrap(),
            r#"{"query_history": [unclosed"#
        );

        // The next save writes fresh defaults; the preserved file is untouched.
        Settings::default().save_to(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&backup).unwrap(),
            r#"{"query_history": [unclosed"#
        );
        fs::remove_file(path).unwrap();
        fs::remove_file(backup).unwrap();
    }

    #[test]
    fn each_malformed_settings_file_keeps_its_own_backup() {
        let path = temporary_path("settings-quarantine-twice");
        let first = path.with_extension("json.corrupt");
        let second = path.with_extension("json.corrupt-1");
        fs::write(&path, "one").unwrap();
        let _ = Settings::load_or_default_from(&path);
        fs::write(&path, "two").unwrap();
        let _ = Settings::load_or_default_from(&path);

        assert_eq!(fs::read_to_string(&first).unwrap(), "one");
        assert_eq!(fs::read_to_string(&second).unwrap(), "two");
        fs::remove_file(first).unwrap();
        fs::remove_file(second).unwrap();
    }

    #[test]
    fn an_absent_settings_file_falls_back_without_renaming_anything() {
        let path = temporary_path("settings-absent");
        let (settings, outcome) = Settings::load_or_default_from(&path);
        assert_eq!(settings, Settings::default());
        assert_eq!(outcome, SettingsLoadOutcome::Loaded);
        assert!(!path.with_extension("json.corrupt").exists());
    }
}
