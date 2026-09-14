use super::search::{ResultKind, ResultSource, SearchResult, MAX_RESULTS};

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

pub(crate) fn system_results(query: &str) -> Vec<SearchResult> {
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

pub(crate) fn built_in_results(query: &str) -> Vec<SearchResult> {
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
    use crate::search_model::SearchModel;

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
}
