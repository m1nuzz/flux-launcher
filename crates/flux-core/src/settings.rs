use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const DEFAULT_CARET_DURATION_MS: u16 = 95;
const MIN_CARET_DURATION_MS: u16 = 60;
const MAX_CARET_DURATION_MS: u16 = 160;
const MAX_QUERY_HISTORY: usize = 32;
const MAX_PRIORITY_ENTRIES: usize = 64;
const DEFAULT_UPDATE_INTERVAL_HOURS: u32 = 24;
const MIN_UPDATE_INTERVAL_HOURS: u32 = 1;
const MAX_UPDATE_INTERVAL_HOURS: u32 = 168;

pub const DEFAULT_LAUNCHER_WIDTH: u16 = 420;
pub const MIN_LAUNCHER_WIDTH: u16 = 380;
pub const MAX_LAUNCHER_WIDTH: u16 = 900;
pub const DEFAULT_LAUNCHER_HEIGHT: u16 = 382;
pub const MIN_LAUNCHER_HEIGHT: u16 = 300;
pub const MAX_LAUNCHER_HEIGHT: u16 = 720;

fn default_update_interval_hours() -> u32 {
    DEFAULT_UPDATE_INTERVAL_HOURS
}

fn enabled_by_default() -> bool {
    true
}

fn disabled_by_default() -> bool {
    false
}

fn default_caret_duration() -> u16 {
    DEFAULT_CARET_DURATION_MS
}

fn default_selection_color() -> u32 {
    0x4c8bf4
}

fn default_launcher_width() -> u16 {
    DEFAULT_LAUNCHER_WIDTH
}

fn default_launcher_height() -> u16 {
    DEFAULT_LAUNCHER_HEIGHT
}

fn default_obsidian_alias() -> String {
    String::from("ob")
}

fn default_google_alias() -> String {
    String::from("g")
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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum Language {
    #[serde(rename = "system")]
    #[default]
    System,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PriorityEntry {
    pub id: String,
    pub title: String,
    pub target: String,
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
    #[serde(default = "disabled_by_default")]
    pub switch_to_english_layout: bool,
    #[serde(default)]
    pub keyboard_layout_default_migrated: bool,
    #[serde(default = "enabled_by_default")]
    pub use_system_accent: bool,
    #[serde(default = "default_selection_color")]
    pub custom_selection_color: u32,
    #[serde(default = "default_launcher_width")]
    pub launcher_width: u16,
    #[serde(default = "default_launcher_height")]
    pub launcher_height: u16,
    #[serde(default = "enabled_by_default")]
    pub clear_query_on_activation: bool,
    #[serde(default = "enabled_by_default")]
    pub start_with_windows: bool,
    #[serde(default = "enabled_by_default")]
    pub everything_enabled: bool,
    #[serde(default)]
    pub everything_install_prompt_seen: bool,
    #[serde(default = "enabled_by_default")]
    pub update_checks_enabled: bool,
    #[serde(default = "default_update_interval_hours")]
    pub update_interval_hours: u32,
    #[serde(default = "disabled_by_default")]
    pub auto_install_updates: bool,
    #[serde(default)]
    pub last_update_check_unix: u64,
    #[serde(default = "enabled_by_default")]
    pub obsidian_enabled: bool,
    #[serde(default = "default_obsidian_alias")]
    pub obsidian_alias: String,
    #[serde(default = "enabled_by_default")]
    pub google_enabled: bool,
    #[serde(default = "default_google_alias")]
    pub google_alias: String,
    #[serde(default = "enabled_by_default")]
    pub system_commands_enabled: bool,
    #[serde(default)]
    pub monitor_preference: MonitorPreference,
    #[serde(default)]
    pub language: Language,
    #[serde(default = "default_caret_duration")]
    pub smooth_caret_duration_ms: u16,
    #[serde(default)]
    pub query_history: Vec<String>,
    #[serde(default)]
    pub priority_entries: Vec<PriorityEntry>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            activation_hotkey: HotkeyConfig::default(),
            ignore_hotkeys_in_fullscreen: true,
            game_mode: false,
            smooth_caret: true,
            switch_to_english_layout: false,
            keyboard_layout_default_migrated: false,
            use_system_accent: true,
            custom_selection_color: default_selection_color(),
            launcher_width: DEFAULT_LAUNCHER_WIDTH,
            launcher_height: DEFAULT_LAUNCHER_HEIGHT,
            clear_query_on_activation: true,
            start_with_windows: true,
            everything_enabled: true,
            everything_install_prompt_seen: false,
            update_checks_enabled: true,
            update_interval_hours: DEFAULT_UPDATE_INTERVAL_HOURS,
            auto_install_updates: false,
            last_update_check_unix: 0,
            obsidian_enabled: true,
            obsidian_alias: default_obsidian_alias(),
            google_enabled: true,
            google_alias: default_google_alias(),
            system_commands_enabled: true,
            monitor_preference: MonitorPreference::default(),
            language: Language::default(),
            smooth_caret_duration_ms: DEFAULT_CARET_DURATION_MS,
            query_history: Vec::new(),
            priority_entries: Vec::new(),
        }
    }
}

impl Settings {
    /// 基于集中定义的默认模板恢复可编辑设置，同时保留用户积累的数据。
    pub fn default_editable_template(&self) -> Self {
        Self {
            query_history: self.query_history.clone(),
            priority_entries: self.priority_entries.clone(),
            everything_install_prompt_seen: self.everything_install_prompt_seen,
            last_update_check_unix: self.last_update_check_unix,
            keyboard_layout_default_migrated: self.keyboard_layout_default_migrated,
            ..Self::default()
        }
    }

    pub fn normalize(&mut self) {
        self.smooth_caret_duration_ms = self
            .smooth_caret_duration_ms
            .clamp(MIN_CARET_DURATION_MS, MAX_CARET_DURATION_MS);
        self.update_interval_hours = self
            .update_interval_hours
            .clamp(MIN_UPDATE_INTERVAL_HOURS, MAX_UPDATE_INTERVAL_HOURS);
        self.launcher_width = self
            .launcher_width
            .clamp(MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH);
        self.launcher_height = self
            .launcher_height
            .clamp(MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT);
        if self.activation_hotkey.key.trim().is_empty() {
            self.activation_hotkey = HotkeyConfig::default();
        }
        self.obsidian_alias = self.obsidian_alias.trim().to_owned();
        if self.obsidian_alias.is_empty() {
            self.obsidian_alias = default_obsidian_alias();
        }
        self.google_alias = self.google_alias.trim().to_owned();
        if self.google_alias.is_empty() {
            self.google_alias = default_google_alias();
        }
        self.normalize_query_history();
        self.normalize_priorities();
    }

    pub fn add_priority(&mut self, entry: PriorityEntry) {
        self.priority_entries.retain(|item| item.id != entry.id);
        self.priority_entries.insert(0, entry);
        self.normalize_priorities();
    }

    pub fn remove_priority(&mut self, id: &str) -> bool {
        let before = self.priority_entries.len();
        self.priority_entries.retain(|item| item.id != id);
        before != self.priority_entries.len()
    }

    pub fn move_priority(&mut self, index: usize, direction: i32) -> bool {
        let Some(next) = index.checked_add_signed(direction as isize) else {
            return false;
        };
        if next >= self.priority_entries.len() || index >= self.priority_entries.len() {
            return false;
        }
        self.priority_entries.swap(index, next);
        true
    }

    fn normalize_priorities(&mut self) {
        let mut normalized = Vec::with_capacity(self.priority_entries.len());
        for entry in self.priority_entries.drain(..) {
            let id = entry.id.trim();
            let title = entry.title.trim();
            let target = entry.target.trim();
            if id.is_empty()
                || target.is_empty()
                || title.is_empty()
                || normalized.iter().any(|item: &PriorityEntry| item.id == id)
            {
                continue;
            }
            normalized.push(PriorityEntry {
                id: id.to_owned(),
                title: title.to_owned(),
                target: target.to_owned(),
            });
        }
        normalized.truncate(MAX_PRIORITY_ENTRIES);
        self.priority_entries = normalized;
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
        Self::load_from_or_default(&Self::config_path())
    }

    /// 加载配置并回退默认值。
    ///
    /// 文件不存在（首次运行）静默回退；JSON 损坏或 IO 错误向 stderr 输出
    /// 一行诊断（仓库规则允许 stderr 不翻译），避免损坏配置被下次保存
    /// 整体覆盖而无任何线索。
    pub fn load_from_or_default(path: &Path) -> Self {
        match Self::load_from(path) {
            Ok(settings) => settings,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("flux: failed to load settings {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub fn load_from(path: &Path) -> io::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let mut settings: Self = serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        settings.normalize();
        // 一次性迁移：v0.2.0 及之前该选项默认开启且随配置显式序列化为 true，
        // 对中文用户会在每次激活时打断输入法。首次加载时回置为默认关闭并写入
        // 迁移标记，此后用户显式修改的值不再被覆盖。
        if !settings.keyboard_layout_default_migrated {
            settings.switch_to_english_layout = false;
            settings.keyboard_layout_default_migrated = true;
        }
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
        assert!(!settings.switch_to_english_layout);
        assert!(settings.use_system_accent);
        assert_eq!(settings.custom_selection_color, 0x4c8bf4);
        assert_eq!(settings.launcher_width, DEFAULT_LAUNCHER_WIDTH);
        assert_eq!(settings.launcher_height, DEFAULT_LAUNCHER_HEIGHT);
        assert!(settings.clear_query_on_activation);
        assert!(settings.start_with_windows);
        assert!(settings.everything_enabled);
        assert!(!settings.everything_install_prompt_seen);
        assert!(settings.update_checks_enabled);
        assert_eq!(
            settings.update_interval_hours,
            DEFAULT_UPDATE_INTERVAL_HOURS
        );
        assert!(!settings.auto_install_updates);
        assert_eq!(settings.last_update_check_unix, 0);
        assert!(settings.obsidian_enabled);
        assert_eq!(settings.obsidian_alias, "ob");
        assert!(settings.google_enabled);
        assert_eq!(settings.google_alias, "g");
        assert_eq!(settings.monitor_preference, MonitorPreference::Cursor);
        assert_eq!(settings.language, Language::System);
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
            switch_to_english_layout: true,
            keyboard_layout_default_migrated: true,
            use_system_accent: false,
            custom_selection_color: 0x12ab34,
            launcher_width: 640,
            launcher_height: 520,
            clear_query_on_activation: false,
            start_with_windows: false,
            everything_enabled: false,
            everything_install_prompt_seen: true,
            update_checks_enabled: false,
            update_interval_hours: 6,
            auto_install_updates: true,
            last_update_check_unix: 1_700_000_000,
            obsidian_enabled: false,
            obsidian_alias: String::from("notes"),
            google_enabled: false,
            google_alias: String::from("search"),
            system_commands_enabled: false,
            monitor_preference: MonitorPreference::Foreground,
            language: Language::SimplifiedChinese,
            smooth_caret_duration_ms: 120,
            query_history: vec![String::from("steam"), String::from("ext:zip")],
            priority_entries: vec![PriorityEntry {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                target: String::from("C:/Steam.lnk"),
            }],
        };

        expected.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).unwrap(), expected);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn load_from_or_default_silently_defaults_for_missing_file() {
        let path = temporary_path("settings-missing");
        let _ = fs::remove_file(&path);
        assert_eq!(Settings::load_from_or_default(&path), Settings::default());
    }

    #[test]
    fn load_from_or_default_falls_back_on_corrupt_json() {
        let path = temporary_path("settings-corrupt");
        fs::write(&path, "{\"game_mode\": tru").unwrap();
        assert_eq!(Settings::load_from_or_default(&path), Settings::default());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn legacy_layout_setting_is_migrated_to_disabled_once() {
        let path = temporary_path("settings-layout-migration");
        let legacy = r#"{"game_mode":false,"switch_to_english_layout":true}"#;
        fs::write(&path, legacy).unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        assert!(!loaded.switch_to_english_layout);
        assert!(loaded.keyboard_layout_default_migrated);

        loaded.save_to(&path).unwrap();
        let reloaded = Settings::load_from(&path).unwrap();
        assert!(!reloaded.switch_to_english_layout);
        assert!(reloaded.keyboard_layout_default_migrated);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn explicit_layout_choice_survives_once_migration_marker_is_present() {
        let path = temporary_path("settings-layout-explicit");
        let explicit =
            r#"{"switch_to_english_layout":true,"keyboard_layout_default_migrated":true}"#;
        fs::write(&path, explicit).unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        assert!(loaded.switch_to_english_layout);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn priorities_insert_newest_first_and_support_manual_management() {
        let mut settings = Settings::default();
        settings.add_priority(PriorityEntry {
            id: String::from("application:one"),
            title: String::from("One"),
            target: String::from("C:/One.lnk"),
        });
        settings.add_priority(PriorityEntry {
            id: String::from("application:two"),
            title: String::from("Two"),
            target: String::from("C:/Two.lnk"),
        });
        assert_eq!(
            settings
                .priority_entries
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["application:two", "application:one"]
        );
        settings.add_priority(PriorityEntry {
            id: String::from("application:one"),
            title: String::from("One"),
            target: String::from("C:/One.lnk"),
        });
        assert_eq!(settings.priority_entries[0].id, "application:one");
        assert_eq!(settings.priority_entries.len(), 2);
        assert!(settings.move_priority(0, 1));
        assert_eq!(settings.priority_entries[1].id, "application:one");
        assert!(settings.remove_priority("application:one"));
        assert_eq!(settings.priority_entries.len(), 1);
        assert!(!settings.remove_priority("missing"));
    }

    #[test]
    fn normalize_priorities_discards_invalid_and_duplicate_entries() {
        let mut settings = Settings {
            priority_entries: vec![
                PriorityEntry {
                    id: String::from(" application:one "),
                    title: String::from(" One "),
                    target: String::from(" C:/One.lnk "),
                },
                PriorityEntry {
                    id: String::from("application:one"),
                    title: String::from("Duplicate"),
                    target: String::from("C:/Duplicate.lnk"),
                },
                PriorityEntry {
                    id: String::from("application:invalid"),
                    title: String::new(),
                    target: String::from("C:/Invalid.lnk"),
                },
            ],
            ..Settings::default()
        };
        settings.normalize();
        assert_eq!(settings.priority_entries.len(), 1);
        assert_eq!(settings.priority_entries[0].id, "application:one");
        assert_eq!(settings.priority_entries[0].target, "C:/One.lnk");
    }

    #[test]
    fn missing_obsidian_fields_use_backward_compatible_defaults() {
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
        let path = temporary_path("settings-monitor-default");
        fs::write(&path, r#"{"activation_hotkey":{"key":"Space"}}"#).unwrap();
        let settings = Settings::load_from(&path).unwrap();
        assert_eq!(settings.monitor_preference, MonitorPreference::Cursor);
        assert_eq!(settings.launcher_width, DEFAULT_LAUNCHER_WIDTH);
        assert_eq!(settings.launcher_height, DEFAULT_LAUNCHER_HEIGHT);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn dimensions_are_clamped_to_safe_window_bounds() {
        let mut settings = Settings {
            launcher_width: 1,
            launcher_height: u16::MAX,
            ..Settings::default()
        };
        settings.normalize();
        assert_eq!(settings.launcher_width, MIN_LAUNCHER_WIDTH);
        assert_eq!(settings.launcher_height, MAX_LAUNCHER_HEIGHT);

        settings.launcher_width = u16::MAX;
        settings.launcher_height = 1;
        settings.normalize();
        assert_eq!(settings.launcher_width, MAX_LAUNCHER_WIDTH);
        assert_eq!(settings.launcher_height, MIN_LAUNCHER_HEIGHT);
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
    fn default_editable_template_resets_preferences_but_preserves_user_data() {
        let settings = Settings {
            activation_hotkey: HotkeyConfig {
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
                key: String::from("K"),
            },
            query_history: vec![String::from("steam")],
            priority_entries: vec![PriorityEntry {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                target: String::from("steam.exe"),
            }],
            everything_install_prompt_seen: true,
            last_update_check_unix: 42,
            keyboard_layout_default_migrated: true,
            ..Settings::default()
        };

        let reset = settings.default_editable_template();

        assert_eq!(reset.activation_hotkey, HotkeyConfig::default());
        assert_eq!(reset.query_history, settings.query_history);
        assert_eq!(reset.priority_entries, settings.priority_entries);
        assert!(reset.everything_install_prompt_seen);
        assert_eq!(reset.last_update_check_unix, 42);
        assert!(reset.keyboard_layout_default_migrated);
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
