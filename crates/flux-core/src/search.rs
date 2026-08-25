const MAX_RESULTS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Command,
    Application,
    File,
    Placeholder,
}

/// Identifies the provider that produced a result. Flow keeps program search
/// separate from file search; the source tier lets Flux preserve that boundary
/// even when both providers return executable-looking paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultSource {
    BuiltIn,
    ApplicationCatalog,
    Everything,
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: ResultKind,
    pub source: ResultSource,
    pub target: Option<String>,
}

impl SearchResult {
    pub fn file(path: String, title: String, subtitle: String) -> Self {
        let is_application = is_application_path(&path);
        let display_title = if is_application {
            title
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_owned())
                .unwrap_or(title)
        } else {
            title
        };
        Self {
            id: format!("file:{path}"),
            title: display_title,
            subtitle,
            kind: if is_application {
                ResultKind::Application
            } else {
                ResultKind::File
            },
            source: ResultSource::Everything,
            target: Some(path),
        }
    }

    pub fn display_text(&self) -> String {
        format!("{}  -  {}", self.title, self.subtitle)
    }

    /// Lower sort key means a result is more useful for the current query.
    /// Applications deliberately outrank indexed files and folders.
    pub fn relevance(&self, query: &str) -> (u8, u8, String) {
        let (priority, _, provider_tier, title_tier, title) = self.priority_relevance(query, &[]);
        debug_assert_eq!(priority, 1);
        (provider_tier, title_tier, title)
    }

    fn priority_relevance(
        &self,
        query: &str,
        priorities: &[String],
    ) -> (u8, usize, u8, u8, String) {
        let query = normalize(query);
        let title = normalize(&self.title);
        let subtitle = normalize(&self.subtitle);
        let (provider_tier, title_tier) = match self.source {
            ResultSource::ApplicationCatalog => (0, match_app_title(&title, &query)),
            ResultSource::BuiltIn => (1, match_app_title(&title, &query)),
            ResultSource::Plugin => (2, match_app_title(&title, &query)),
            ResultSource::Everything => match self.kind {
                ResultKind::Application => (3, match_app_title(&title, &query)),
                ResultKind::Command | ResultKind::Placeholder | ResultKind::File => {
                    (4, match_file_title(&title, &query))
                }
            },
        };
        let subtitle_match = if !query.is_empty() && subtitle.contains(&query) {
            0
        } else {
            1
        };
        let priority = if matches!(self.kind, ResultKind::Application) {
            priorities
                .iter()
                .position(|id| id == &self.id)
                .map(|index| index.saturating_add(1))
        } else {
            None
        };
        let exact_shell_command = self.id == "system:command-prompt"
            && matches!(query.as_str(), "cmd" | "command prompt" | "command line")
            || self.id == "system:powershell"
                && matches!(query.as_str(), "powershell" | "pwsh" | "power shell");
        (
            if exact_shell_command {
                0
            } else {
                priority.map_or(1, |_| 0)
            },
            if exact_shell_command {
                0
            } else {
                priority.unwrap_or_default()
            },
            provider_tier,
            title_tier.saturating_add(subtitle_match),
            title,
        )
    }
}

fn normalize(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn match_app_title(title: &str, query: &str) -> u8 {
    if query.is_empty() || title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else {
        3
    }
}

fn match_file_title(title: &str, query: &str) -> u8 {
    if query.is_empty() {
        3
    } else if title == query {
        0
    } else if title.starts_with(query) {
        1
    } else if title.contains(query) {
        2
    } else {
        3
    }
}

fn is_application_path(path: &str) -> bool {
    let lower = normalize(path);
    [".exe", ".lnk", ".com", ".bat", ".cmd", ".url"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub fn rank_results(query: &str, results: &mut [SearchResult]) {
    results.sort_by_key(|result| result.relevance(query));
}

pub fn rank_results_with_priorities(
    query: &str,
    results: &mut [SearchResult],
    priorities: &[String],
) {
    results.sort_by_key(|result| result.priority_relevance(query, priorities));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchModel {
    query: String,
    results: Vec<SearchResult>,
    selected: usize,
}

impl Default for SearchModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchModel {
    pub fn new() -> Self {
        let mut model = Self {
            query: String::new(),
            results: Vec::with_capacity(MAX_RESULTS),
            selected: 0,
        };
        model.set_query("");
        model
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[SearchResult] {
        &self.results
    }

    pub fn selected(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        let mut built_ins = built_in_results(&self.query);
        built_ins.extend(system_results(&self.query));
        rank_results(&self.query, &mut built_ins);
        built_ins.truncate(MAX_RESULTS);
        self.results = built_ins;
        self.selected = 0;
    }

    pub fn replace_results(&mut self, mut results: Vec<SearchResult>) {
        rank_results(&self.query, &mut results);
        results.truncate(MAX_RESULTS);
        self.results = results;
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    pub fn select_next(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1) % self.results.len();
        }
    }

    pub fn select_previous(&mut self) {
        if !self.results.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.results.len().saturating_sub(1));
        }
    }
}

pub fn history_results(history: &[String], query: &str) -> Vec<SearchResult> {
    let normalized = query.trim().to_ascii_lowercase();
    history
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, item)| {
            normalized.is_empty() || item.to_ascii_lowercase().contains(&normalized)
        })
        .take(MAX_RESULTS)
        .map(|(index, item)| SearchResult {
            id: format!("history:{index}"),
            title: item.clone(),
            subtitle: String::from("Previous search"),
            kind: ResultKind::Placeholder,
            source: ResultSource::BuiltIn,
            target: None,
        })
        .collect()
}

struct SystemResultSpec {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    target: &'static str,
    aliases: &'static [&'static str],
}

const SYSTEM_RESULT_SPECS: &[SystemResultSpec] = &[
    SystemResultSpec {
        id: "system:command-prompt",
        title: "Command Prompt",
        subtitle: "Windows command line",
        target: "cmd.exe",
        aliases: &["cmd", "command prompt", "command line", "terminal"],
    },
    SystemResultSpec {
        id: "system:powershell",
        title: "PowerShell",
        subtitle: "Windows PowerShell",
        target: "powershell.exe",
        aliases: &["powershell", "pwsh", "power shell", "terminal"],
    },
    SystemResultSpec {
        id: "system:settings",
        title: "Settings",
        subtitle: "Windows Settings",
        target: "ms-settings:",
        aliases: &["windows settings", "system settings"],
    },
    SystemResultSpec {
        id: "system:display",
        title: "Display",
        subtitle: "Windows Settings • System",
        target: "ms-settings:display",
        aliases: &["monitor", "screen", "resolution", "brightness"],
    },
    SystemResultSpec {
        id: "system:sound",
        title: "Sound",
        subtitle: "Windows Settings • System",
        target: "ms-settings:sound",
        aliases: &["audio", "volume", "microphone", "speakers"],
    },
    SystemResultSpec {
        id: "system:bluetooth",
        title: "Bluetooth",
        subtitle: "Windows Settings • Bluetooth & devices",
        target: "ms-settings:bluetooth",
        aliases: &["devices", "wireless"],
    },
    SystemResultSpec {
        id: "system:wifi",
        title: "Wi-Fi",
        subtitle: "Windows Settings • Network & internet",
        target: "ms-settings:network-wifi",
        aliases: &["wifi", "wi-fi", "wireless network", "network"],
    },
    SystemResultSpec {
        id: "system:vpn",
        title: "VPN",
        subtitle: "Windows Settings • Network & internet",
        target: "ms-settings:network-vpn",
        aliases: &["network", "virtual private network"],
    },
    SystemResultSpec {
        id: "system:proxy",
        title: "Proxy",
        subtitle: "Windows Settings • Network & internet",
        target: "ms-settings:network-proxy",
        aliases: &["network"],
    },
    SystemResultSpec {
        id: "system:installed-apps",
        title: "Installed apps",
        subtitle: "Windows Settings • Apps",
        target: "ms-settings:appsfeatures",
        aliases: &["apps", "applications", "uninstall", "programs"],
    },
    SystemResultSpec {
        id: "system:default-apps",
        title: "Default apps",
        subtitle: "Windows Settings • Apps",
        target: "ms-settings:defaultapps",
        aliases: &["apps", "file associations"],
    },
    SystemResultSpec {
        id: "system:startup-apps",
        title: "Startup apps",
        subtitle: "Windows Settings • Apps",
        target: "ms-settings:startupapps",
        aliases: &["startup", "boot apps", "apps"],
    },
    SystemResultSpec {
        id: "system:personalization",
        title: "Personalization",
        subtitle: "Windows Settings • Personalization",
        target: "ms-settings:personalization",
        aliases: &["appearance", "customize", "settings"],
    },
    SystemResultSpec {
        id: "system:background",
        title: "Background",
        subtitle: "Windows Settings • Personalization",
        target: "ms-settings:personalization-background",
        aliases: &["wallpaper", "desktop", "personalization", "settings"],
    },
    SystemResultSpec {
        id: "system:themes",
        title: "Themes",
        subtitle: "Windows Settings • Personalization",
        target: "ms-settings:themes",
        aliases: &["appearance", "personalization", "settings"],
    },
    SystemResultSpec {
        id: "system:colors",
        title: "Colors",
        subtitle: "Windows Settings • Personalization",
        target: "ms-settings:colors",
        aliases: &["accent color", "personalization", "settings"],
    },
    SystemResultSpec {
        id: "system:windows-update",
        title: "Windows Update",
        subtitle: "Windows Settings • Windows Update",
        target: "ms-settings:windowsupdate",
        aliases: &["update", "updates", "upgrade", "settings"],
    },
    SystemResultSpec {
        id: "system:privacy",
        title: "Privacy",
        subtitle: "Windows Settings • Privacy & security",
        target: "ms-settings:privacy",
        aliases: &["security", "permissions", "settings"],
    },
    SystemResultSpec {
        id: "system:storage",
        title: "Storage",
        subtitle: "Windows Settings • System",
        target: "ms-settings:storagesense",
        aliases: &["disk", "drive", "space", "cleanup", "settings"],
    },
    SystemResultSpec {
        id: "system:accounts",
        title: "Accounts",
        subtitle: "Windows Settings • Accounts",
        target: "ms-settings:accounts",
        aliases: &["user", "users", "login", "settings"],
    },
    SystemResultSpec {
        id: "system:date-time",
        title: "Date & time",
        subtitle: "Windows Settings • Time & language",
        target: "ms-settings:dateandtime",
        aliases: &["clock", "time", "timezone", "settings"],
    },
    SystemResultSpec {
        id: "system:language",
        title: "Language & region",
        subtitle: "Windows Settings • Time & language",
        target: "ms-settings:regionlanguage",
        aliases: &["language", "keyboard", "region", "settings"],
    },
    SystemResultSpec {
        id: "system:notifications",
        title: "Notifications",
        subtitle: "Windows Settings • System",
        target: "ms-settings:notifications",
        aliases: &["alerts", "focus", "settings"],
    },
    SystemResultSpec {
        id: "system:accessibility",
        title: "Accessibility",
        subtitle: "Windows Settings • Accessibility",
        target: "ms-settings:easeofaccess",
        aliases: &["ease of access", "assistive technology", "settings"],
    },
    SystemResultSpec {
        id: "system:recovery",
        title: "Recovery",
        subtitle: "Windows Settings • System",
        target: "ms-settings:recovery",
        aliases: &["reset", "restore", "settings"],
    },
    SystemResultSpec {
        id: "system:clipboard",
        title: "Clipboard",
        subtitle: "Windows Settings • System",
        target: "ms-settings:clipboard",
        aliases: &["copy", "paste", "history", "settings"],
    },
    SystemResultSpec {
        id: "system:control-panel",
        title: "Control Panel",
        subtitle: "Windows Control Panel",
        target: "control.exe",
        aliases: &["legacy settings", "settings"],
    },
    SystemResultSpec {
        id: "system:device-manager",
        title: "Device Manager",
        subtitle: "Windows administrative tools",
        target: "devmgmt.msc",
        aliases: &["devices", "drivers", "hardware"],
    },
    SystemResultSpec {
        id: "system:task-manager",
        title: "Task Manager",
        subtitle: "Windows administrative tools",
        target: "taskmgr.exe",
        aliases: &["processes", "performance", "startup"],
    },
    SystemResultSpec {
        id: "system:services",
        title: "Services",
        subtitle: "Windows administrative tools",
        target: "services.msc",
        aliases: &["background services", "service manager"],
    },
    SystemResultSpec {
        id: "system:event-viewer",
        title: "Event Viewer",
        subtitle: "Windows administrative tools",
        target: "eventvwr.msc",
        aliases: &["logs", "system logs", "events"],
    },
    SystemResultSpec {
        id: "system:disk-management",
        title: "Disk Management",
        subtitle: "Windows administrative tools",
        target: "diskmgmt.msc",
        aliases: &["partitions", "volumes", "drives", "disk"],
    },
    SystemResultSpec {
        id: "system:system-information",
        title: "System Information",
        subtitle: "Windows administrative tools",
        target: "msinfo32.exe",
        aliases: &["hardware information", "computer information", "about pc"],
    },
    SystemResultSpec {
        id: "system:resource-monitor",
        title: "Resource Monitor",
        subtitle: "Windows administrative tools",
        target: "resmon.exe",
        aliases: &["cpu", "memory", "disk", "network", "performance"],
    },
    SystemResultSpec {
        id: "system:downloads",
        title: "Downloads",
        subtitle: "Open special folder",
        target: "shell:Downloads",
        aliases: &["download folder", "files"],
    },
    SystemResultSpec {
        id: "system:documents",
        title: "Documents",
        subtitle: "Open special folder",
        target: "shell:Personal",
        aliases: &["document folder", "files"],
    },
    SystemResultSpec {
        id: "system:desktop",
        title: "Desktop",
        subtitle: "Open special folder",
        target: "shell:Desktop",
        aliases: &["desktop folder", "files"],
    },
    SystemResultSpec {
        id: "system:file-explorer",
        title: "File Explorer",
        subtitle: "Open Windows Explorer",
        target: "explorer.exe",
        aliases: &["explorer", "files", "folders"],
    },
];

fn system_results(query: &str) -> Vec<SearchResult> {
    let normalized = query.trim().to_ascii_lowercase();
    SYSTEM_RESULT_SPECS
        .iter()
        .filter(|spec| {
            normalized.is_empty()
                || spec.id.contains(&normalized)
                || spec.title.to_ascii_lowercase().contains(&normalized)
                || spec.subtitle.to_ascii_lowercase().contains(&normalized)
                || spec.aliases.iter().any(|alias| alias.contains(&normalized))
        })
        .take(MAX_RESULTS)
        .map(|spec| SearchResult {
            id: spec.id.to_owned(),
            title: spec.title.to_owned(),
            subtitle: spec.subtitle.to_owned(),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(spec.target.to_owned()),
        })
        .collect()
}

fn built_in_results(query: &str) -> Vec<SearchResult> {
    let normalized = query.trim().to_ascii_lowercase();
    let commands = [
        ("flux-settings", "Flux Settings", "Configure Flux Launcher"),
        (
            "toggle-game-mode",
            "Toggle Game Mode",
            "Suppress launcher activation",
        ),
        (
            "empty-recycle-bin",
            "Empty Recycle Bin",
            "Empty recycle bin",
        ),
        ("open-recycle-bin", "Open Recycle Bin", "Open recycle bin"),
        ("plugins", "Plugin directory", "Manage native Flow plugins"),
        ("about", "About Flux Launcher", "Version and diagnostics"),
    ];

    commands
        .into_iter()
        .filter(|(id, title, subtitle)| {
            normalized.is_empty()
                || id.contains(&normalized)
                || title.to_ascii_lowercase().contains(&normalized)
                || subtitle.to_ascii_lowercase().contains(&normalized)
                || ((*id == "empty-recycle-bin" || *id == "open-recycle-bin")
                    && matches!(
                        normalized.as_str(),
                        "recyclebin" | "recycle bin" | "trash" | "корзина"
                    ))
        })
        .take(MAX_RESULTS)
        .map(|(id, title, subtitle)| SearchResult {
            id: id.to_owned(),
            title: title.to_owned(),
            subtitle: subtitle.to_owned(),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_query_shows_bounded_command_palette() {
        let mut model = SearchModel::new();
        model.set_query("");
        assert!(!model.results().is_empty());
        assert!(model.results().len() <= MAX_RESULTS);
        assert_eq!(model.selected_index(), 0);
    }

    #[test]
    fn exact_shell_queries_put_the_matching_console_first() {
        let mut model = SearchModel::new();
        model.set_query("cmd");
        assert_eq!(model.results()[0].id, "system:command-prompt");
        assert_eq!(model.results()[0].target.as_deref(), Some("cmd.exe"));

        model.set_query("powershell");
        assert_eq!(model.results()[0].id, "system:powershell");
        assert_eq!(model.results()[0].target.as_deref(), Some("powershell.exe"));

        model.set_query("pwsh");
        assert_eq!(model.results()[0].id, "system:powershell");
    }

    #[test]
    fn windows_settings_results_match_common_aliases_and_keep_uri_targets() {
        let mut model = SearchModel::new();
        model.set_query("settings");
        let result = model
            .results()
            .iter()
            .find(|result| result.id == "system:settings")
            .expect("Windows Settings result");
        assert_eq!(result.title, "Settings");
        assert_eq!(result.target.as_deref(), Some("ms-settings:"));

        model.set_query("wifi");
        assert_eq!(
            model.results()[0].target.as_deref(),
            Some("ms-settings:network-wifi")
        );
    }

    #[test]
    fn recycle_bin_commands_appear_in_flow_order_on_recycle_query() {
        let mut model = SearchModel::new();
        model.set_query("recycle");
        assert_eq!(
            model
                .results()
                .iter()
                .map(|result| result.id.as_str())
                .collect::<Vec<_>>(),
            ["empty-recycle-bin", "open-recycle-bin"]
        );
    }

    #[test]
    fn recycle_bin_commands_appear_on_trash_query() {
        let mut model = SearchModel::new();
        model.set_query("trash");
        assert_eq!(model.results().len(), 2);
        assert_eq!(model.results()[0].id, "empty-recycle-bin");
        assert_eq!(model.results()[1].id, "open-recycle-bin");
    }

    #[test]
    fn recycle_bin_commands_appear_on_russian_alias_query() {
        let mut model = SearchModel::new();
        model.set_query("корзина");
        assert_eq!(model.results().len(), 2);
        assert_eq!(model.results()[0].id, "empty-recycle-bin");
        assert_eq!(model.results()[1].id, "open-recycle-bin");
    }

    #[test]
    fn recyclebin_alias_returns_both_commands() {
        let mut model = SearchModel::new();
        model.set_query("recyclebin");
        assert_eq!(model.results().len(), 2);
        assert_eq!(model.results()[0].title, "Empty Recycle Bin");
        assert_eq!(model.results()[1].title, "Open Recycle Bin");
    }

    #[test]
    fn everything_is_not_a_built_in_command() {
        let mut model = SearchModel::new();
        model.set_query("EVERYTHING");
        assert!(model.results().is_empty());
    }

    #[test]
    fn selection_wraps_in_both_directions() {
        let mut model = SearchModel::new();
        let last = model.results().len() - 1;
        model.select_previous();
        assert_eq!(model.selected_index(), last);
        model.select_next();
        assert_eq!(model.selected_index(), 0);
    }

    #[test]
    fn history_results_are_newest_first_and_filterable() {
        let history = vec![
            String::from("steam"),
            String::from("ext:zip"),
            String::from("chrome"),
        ];
        let all = history_results(&history, "");
        assert_eq!(all[0].title, "chrome");
        assert_eq!(all[1].title, "ext:zip");
        assert_eq!(history_results(&history, "zip")[0].title, "ext:zip");
    }

    #[test]
    fn executable_results_are_ranked_before_indexed_files() {
        let mut results = vec![
            SearchResult::file(
                String::from("C:/Windows/Steam/cache.txt"),
                String::from("cache.txt"),
                String::from("C:/Windows/Steam"),
            ),
            SearchResult::file(
                String::from("C:/Program Files/Steam/Steam.exe"),
                String::from("Steam.exe"),
                String::from("C:/Program Files/Steam"),
            ),
        ];
        rank_results("steam", &mut results);
        assert_eq!(results[0].kind, ResultKind::Application);
        assert_eq!(results[0].title, "Steam");
    }

    #[test]
    fn application_catalog_outranks_everything_executable_for_exact_name() {
        let mut results = vec![
            SearchResult::file(
                String::from("C:/Users/Test/workspace/Steam.exe"),
                String::from("Steam.exe"),
                String::from("C:/Users/Test/workspace"),
            ),
            SearchResult {
                id: String::from("application:start-menu:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from(
                    r"C:\Users\Test\AppData\Roaming\Microsoft\Windows\Start Menu\Steam.lnk",
                )),
            },
        ];
        rank_results("Steam", &mut results);
        assert_eq!(results[0].source, ResultSource::ApplicationCatalog);
        assert_eq!(results[0].title, "Steam");
    }

    #[test]
    fn explicit_priority_outranks_all_other_application_results() {
        let mut results = vec![
            SearchResult {
                id: String::from("application:steam"),
                title: String::from("Steam"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from("C:/Steam.lnk")),
            },
            SearchResult {
                id: String::from("application:chrome"),
                title: String::from("Chrome"),
                subtitle: String::from("Application • Start Menu"),
                kind: ResultKind::Application,
                source: ResultSource::ApplicationCatalog,
                target: Some(String::from("C:/Chrome.lnk")),
            },
        ];
        rank_results_with_priorities(
            "app",
            &mut results,
            &[
                String::from("application:chrome"),
                String::from("application:steam"),
            ],
        );
        assert_eq!(results[0].id, "application:chrome");
        assert_eq!(results[1].id, "application:steam");
    }

    #[test]
    fn shortcut_and_executable_paths_are_applications() {
        assert_eq!(
            SearchResult::file(
                String::from("C:/Users/Test/Google Chrome.lnk"),
                String::from("Google Chrome.lnk"),
                String::new(),
            )
            .kind,
            ResultKind::Application
        );
    }

    #[test]
    fn external_results_are_truncated_and_selection_stays_valid() {
        let mut model = SearchModel::new();
        model.select_next();
        model.replace_results(
            (0..16)
                .map(|index| SearchResult {
                    id: "fixture".to_owned(),
                    title: format!("Result {index}"),
                    subtitle: String::new(),
                    kind: ResultKind::Placeholder,
                    source: ResultSource::BuiltIn,
                    target: None,
                })
                .collect(),
        );
        assert_eq!(model.results().len(), MAX_RESULTS);
        assert!(model.selected().is_some());
    }
}
