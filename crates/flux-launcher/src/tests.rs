use super::{
    COMPACT_WINDOW_HEIGHT, LAUNCHER_FONT_FAMILY, MAX_LAUNCHER_HEIGHT, MAX_LAUNCHER_WIDTH,
    MIN_LAUNCHER_HEIGHT, MIN_LAUNCHER_WIDTH,
};
use crate::actions::{actions_for_result, quoted_result_path, ActionKind};
use crate::applications::{canonical_application_id, resolve_bare_executable_path};
use crate::entry::{is_shutdown_mode, should_claim_single_instance};
use crate::everything::normalize_everything_query;
use crate::icons::{
    bundled_icon_rgba, google_icon_rgba, icon_completion_generation_changed, icon_extraction_route,
    icon_row_within_eager_window, icon_target_for_path, is_executable_icon_target,
    obsidian_icon_rgba, parse_internet_shortcut_icon_location, resolve_shortcut_icon_path,
    IconExtractionRoute, ResultIconView, ShellIconCache, MAX_SHELL_EXTENSION_ICON_CACHE_ENTRIES,
    MAX_SHELL_ICON_CACHE_ENTRIES,
};
use crate::keyboard::{history_cursor_step, is_run_as_admin_key};
use crate::query::{
    merge_application_duplicates, normalize_built_in_executable_targets,
    preserve_everything_file_order, should_publish_initial_query_results, synthesize_path_result,
    ProviderResults,
};
use crate::result_row::hover_position_changed;
use crate::update_state::{format_bytes, format_update_progress};
use crate::updater::relaunch_mode_for_auto_install;
use crate::window_state::{
    dimension_from_slider, dimension_slider_fraction, launcher_window_geometry_with_prompt,
    parse_dimension_input,
};
use crate::window_state::{launcher_window_geometry, should_show_everything_install_prompt};
use crate::window_state::{launcher_window_geometry_with_sizes, should_show_launcher};
use flux_core::{rank_results_with_priorities, ResultKind, ResultSource, SearchResult};
use flux_core::{DEFAULT_LAUNCHER_HEIGHT, DEFAULT_LAUNCHER_WIDTH};
use windui::event::{Key, KeyEvent};

#[test]
fn plugin_host_mode_bypasses_main_single_instance_guard() {
    assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
        "--plugin-host"
    ))));
}

#[test]
fn folder_launch_smoke_mode_bypasses_main_single_instance_guard() {
    assert!(!should_claim_single_instance(Some(std::ffi::OsStr::new(
        "--folder-launch-smoke"
    ))));
}

#[test]
fn normal_and_startup_modes_use_main_single_instance_guard() {
    assert!(should_claim_single_instance(None));
    assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
        "--startup"
    ))));
}

#[test]
fn shutdown_mode_is_a_single_instance_command() {
    assert!(should_claim_single_instance(Some(std::ffi::OsStr::new(
        "--shutdown"
    ))));
    assert!(is_shutdown_mode(Some(std::ffi::OsStr::new("--shutdown"))));
    assert!(!is_shutdown_mode(Some(std::ffi::OsStr::new("--startup"))));
    assert!(!is_shutdown_mode(None));
}

#[test]
fn update_progress_text_exposes_percent_bytes_and_remaining_work() {
    let progress = super::updater::DownloadProgress {
        received_bytes: 512,
        total_bytes: Some(1024),
    };
    assert_eq!(format_bytes(1024), "1 KiB");
    assert_eq!(
        format_update_progress("0.1.64", &progress),
        "Downloading stable 0.1.64: 50% — 512 B / 1 KiB (512 B remaining)"
    );
}

#[test]
fn bundled_google_icon_decodes_to_32_pixel_rgba_bitmap() {
    let icon = google_icon_rgba().expect("bundled Google icon should decode");
    assert_eq!(icon.len(), 32 * 32 * 4);
    assert!(icon.chunks_exact(4).any(|pixel| pixel[3] > 0));
}

#[test]
fn bundled_obsidian_icon_decodes_to_32_pixel_rgba_bitmap() {
    let icon = obsidian_icon_rgba().expect("bundled Obsidian icon should decode");
    assert_eq!(icon.len(), 32 * 32 * 4);
    assert!(icon.chunks_exact(4).any(|pixel| pixel[3] > 0));
}

#[test]
fn obsidian_result_uses_the_bundled_icon() {
    let icon = bundled_icon_rgba("builtin:obsidian:notes/readme.md")
        .expect("Obsidian result should use the bundled icon");
    assert_eq!(icon.len(), 32 * 32 * 4);
    assert!(bundled_icon_rgba("everything:file:readme.md").is_none());
}

#[test]
fn bounded_shell_icon_cache_evicts_oldest_entries() {
    let mut cache = ShellIconCache::new();
    for index in 0..=MAX_SHELL_ICON_CACHE_ENTRIES {
        cache.insert(format!("target-{index}"), Some(vec![index as u8; 32]));
    }
    assert_eq!(cache.entries.len(), MAX_SHELL_ICON_CACHE_ENTRIES);
    assert!(cache.get("target-0").is_none());
    assert!(cache
        .get(&format!("target-{MAX_SHELL_ICON_CACHE_ENTRIES}"))
        .is_some());
}

#[test]
fn deleted_absolute_file_routes_to_shared_extension_icon() {
    // Everything 会继续索引已被删除的文件；这类幽灵行按扩展名共享一次提取，
    // 不再为每条路径各自支付注定失败的 Shell 往返。
    let missing = std::env::temp_dir().join(format!("flux-icon-ghost-{}.TMP", std::process::id()));
    let _ = std::fs::remove_file(&missing);
    assert!(!missing.exists(), "前置条件：夹具必须不存在");
    assert!(
        matches!(
            icon_extraction_route(&missing.to_string_lossy()),
            IconExtractionRoute::ByExtension(extension) if extension == "tmp"
        ),
        "缺失的绝对路径应路由到小写扩展名共享"
    );
    // 引号包裹的目标与裸路径一致。
    let quoted = format!("\"{}\"", missing.to_string_lossy());
    assert!(matches!(
        icon_extraction_route(&quoted),
        IconExtractionRoute::ByExtension(extension) if extension == "tmp"
    ));
}

#[test]
fn existing_and_non_path_targets_keep_their_own_extraction_route() {
    let root = std::env::temp_dir().join(format!("flux-icon-route-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("临时目录应可创建");
    let file = root.join("present.txt");
    std::fs::write(&file, b"").expect("临时文件应可写入");
    // 已存在的文件始终按路径提取：它可能有自定义图标。
    assert!(matches!(
        icon_extraction_route(&file.to_string_lossy()),
        IconExtractionRoute::ByTarget
    ));
    // 已存在的含点目录名不能被判为共享扩展名，否则目录会被绑到文件图标。
    let dotted_dir = root.join("v1.0");
    std::fs::create_dir_all(&dotted_dir).expect("含点目录应可创建");
    assert!(matches!(
        icon_extraction_route(&dotted_dir.to_string_lossy()),
        IconExtractionRoute::ByTarget
    ));
    // 无扩展名的缺失目标（如已删除的文件夹）回退按目标提取。
    let ghost_dir = root.join(format!("flux-icon-ghostdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ghost_dir);
    assert!(matches!(
        icon_extraction_route(&ghost_dir.to_string_lossy()),
        IconExtractionRoute::ByTarget
    ));
    // 相对路径与内置 / URL 类目标不参与共享，避免误路由。
    assert!(matches!(
        icon_extraction_route("relative.txt"),
        IconExtractionRoute::ByTarget
    ));
    assert!(matches!(
        icon_extraction_route("builtin:google-search"),
        IconExtractionRoute::ByTarget
    ));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn extension_icon_cache_shares_one_extraction_per_extension() {
    let mut cache = ShellIconCache::new();
    assert!(cache.get_extension("tmp").is_none());
    cache.insert_extension(String::from("tmp"), Some(vec![7_u8; 4]));
    assert_eq!(cache.get_extension("tmp"), Some(Some(vec![7_u8; 4])));
    // 共享失败也入缓存：幽灵行不再重复支付 Shell。
    cache.insert_extension(String::from("ghost"), None);
    assert_eq!(cache.get_extension("ghost"), Some(None));
    // 不同扩展名彼此隔离。
    assert!(cache.get_extension("mkv").is_none());
    // 扩展名缓存与路径缓存互不串键。
    assert!(cache.get("tmp").is_none());
}

#[test]
fn extension_icon_cache_evicts_oldest_entries() {
    let mut cache = ShellIconCache::new();
    for index in 0..=MAX_SHELL_EXTENSION_ICON_CACHE_ENTRIES {
        cache.insert_extension(format!("ext-{index}"), Some(vec![index as u8; 4]));
    }
    assert_eq!(
        cache.extension_entries.len(),
        MAX_SHELL_EXTENSION_ICON_CACHE_ENTRIES
    );
    assert!(cache.get_extension("ext-0").is_none());
    assert!(cache
        .get_extension(&format!("ext-{MAX_SHELL_EXTENSION_ICON_CACHE_ENTRIES}"))
        .is_some());
}

#[test]
fn shell_icon_cache_touch_preserves_recent_entry_during_eviction() {
    let mut cache = ShellIconCache::new();
    for index in 0..MAX_SHELL_ICON_CACHE_ENTRIES {
        cache.insert(format!("target-{index}"), Some(vec![index as u8; 4]));
    }
    assert!(cache.get("target-0").is_some());
    cache.insert(String::from("new-target"), None);
    assert!(cache.get("target-0").is_some());
    assert!(cache.get("target-1").is_none());
    assert!(cache.get("new-target").is_some_and(|icon| icon.is_none()));
}

#[test]
fn shell_icon_cache_retains_negative_results_without_unbounded_growth() {
    let mut cache = ShellIconCache::new();
    cache.insert(String::from("missing-target"), None);
    assert!(cache
        .get("missing-target")
        .is_some_and(|icon| icon.is_none()));
}

#[test]
fn icon_generation_changes_only_after_completion() {
    assert!(!icon_completion_generation_changed(4, 4));
    assert!(icon_completion_generation_changed(4, 5));
    assert!(icon_completion_generation_changed(u64::MAX, 0));
}

#[test]
fn parses_steam_internet_shortcut_icon_file_and_index() {
    let shortcut = "[InternetShortcut]\nURL=steam://rungameid/730\nIconFile=C:\\Program Files (x86)\\Steam\\steam\\games\\730.ico\nIconIndex=0\n";
    assert_eq!(
        parse_internet_shortcut_icon_location(shortcut),
        Some((
            String::from(r"C:\Program Files (x86)\Steam\steam\games\730.ico"),
            0
        ))
    );
}

#[test]
fn parses_shortcut_icon_keys_case_insensitively_and_defaults_index() {
    let shortcut = "[InternetShortcut]\nurl=steam://rungameid/10\niconfile=game.ico\n";
    assert_eq!(
        parse_internet_shortcut_icon_location(shortcut),
        Some((String::from("game.ico"), 0))
    );
}

#[test]
fn resolves_relative_shortcut_icon_file_against_shortcut_directory() {
    let expected = std::path::Path::new("/tmp/Steam")
        .join("icons/game.ico")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        resolve_shortcut_icon_path("/tmp/Steam/Game.url", "icons/game.ico"),
        Some(expected)
    );
}

#[test]
fn result_icon_view_accepts_valid_cached_rgba_and_keeps_placeholder_on_miss() {
    let generation = windui::prelude::signal(0_u64);
    let selected = windui::prelude::signal(0_usize);
    let valid = [255_u8, 128, 64, 255].repeat(32 * 32);
    let loaded = ResultIconView::new(
        Some(String::from(r"C:\Program Files\Demo\demo.exe")),
        String::from("▣"),
        LAUNCHER_FONT_FAMILY,
        Some(valid),
        generation,
        0,
        selected,
    );
    assert!(loaded.image.is_some());

    let pending = ResultIconView::new(
        Some(String::from(r"C:\Program Files\Pending\pending.exe")),
        String::from("▣"),
        LAUNCHER_FONT_FAMILY,
        None,
        generation,
        0,
        selected,
    );
    assert!(pending.image.is_none());
}

#[test]
fn icon_request_eager_window_covers_visible_rows_and_selection_neighbourhood() {
    let eager = crate::EAGER_ICON_ROW_COUNT;
    // 首屏：前 K 行（可见行 + 余量）始终立即请求。
    assert!(icon_row_within_eager_window(0, 0));
    assert!(icon_row_within_eager_window(eager - 1, 0));
    assert!(!icon_row_within_eager_window(eager, 0));
    // 键盘 / 悬停移动选区：选区行及其下 K 行进入窗口，允许选区上方一行余量。
    let selected = 20;
    assert!(icon_row_within_eager_window(selected, selected));
    assert!(icon_row_within_eager_window(selected - 1, selected));
    assert!(icon_row_within_eager_window(selected + eager - 1, selected));
    assert!(!icon_row_within_eager_window(selected + eager, selected));
    assert!(!icon_row_within_eager_window(eager + 7, selected));
}

#[test]
fn prewarm_icon_targets_dedupes_skips_targetless_and_limits() {
    use crate::icons::prewarm_icon_targets;
    let application = |id: &str, target: Option<&str>| SearchResult {
        id: String::from(id),
        title: String::from(id),
        subtitle: String::from("Application"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: target.map(String::from),
    };
    let results = vec![
        application("a", Some(r"C:\Tools\demo.exe")),
        application("b", Some(r"C:\Tools\demo.exe")), // 同目标，去重
        application("c", None),                       // 无目标，跳过
        application("d", Some(r"C:\Tools\other.exe")),
        application("e", Some(r"C:\Tools\third.exe")),
    ];
    let targets = prewarm_icon_targets(&results, 2);
    assert_eq!(
        targets,
        vec![
            String::from(r"C:\Tools\demo.exe"),
            String::from(r"C:\Tools\other.exe"),
        ]
    );
    assert!(prewarm_icon_targets(&results, 16).len() == 3);
    assert!(prewarm_icon_targets(&[], 8).is_empty());
}

#[test]
fn everything_file_order_is_preserved_after_app_first_ranking() {
    let application = SearchResult {
        id: String::from("application:report-viewer"),
        title: String::from("Report Viewer"),
        subtitle: String::from("Application"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(String::from(r"C:\\ReportViewer.lnk")),
    };
    let newest = SearchResult::file(
        String::from(r"C:\\workspace\\report-z.txt"),
        String::from("report-z.txt"),
        String::from(r"C:\\workspace"),
    );
    let older = SearchResult::file(
        String::from(r"C:\\workspace\\report-a.txt"),
        String::from("report-a.txt"),
        String::from(r"C:\\workspace"),
    );
    let newest_id = newest.id.clone();
    let older_id = older.id.clone();
    let mut merged = vec![application.clone(), older, newest];
    let provider_order = vec![merged[2].clone(), merged[1].clone()];

    preserve_everything_file_order(&mut merged, &provider_order);

    assert_eq!(merged[0].id, application.id);
    assert_eq!(merged[1].id, newest_id);
    assert_eq!(merged[2].id, older_id);
}

#[test]
fn application_duplicates_merge_by_canonical_target_and_prefer_start_menu() {
    let app_paths = SearchResult {
        id: String::from("application:app-paths:chrome"),
        title: String::from("chrome"),
        subtitle: String::from("Application • App Paths"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(String::from(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        )),
    };
    let start_menu = SearchResult {
        id: String::from("application:start-menu:google-chrome"),
        title: String::from("Google Chrome"),
        subtitle: String::from("Application • Start Menu"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(String::from(
            r"C:/Program Files/Google/Chrome/Application/chrome.exe",
        )),
    };
    let everything = SearchResult::file(
        String::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        String::from("chrome.exe"),
        String::from(r"C:\Program Files\Google\Chrome\Application"),
    );
    let merged = merge_application_duplicates(vec![app_paths, everything, start_menu]);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].title, "Google Chrome");
    assert!(merged[0].subtitle.contains("Start Menu"));
}

#[cfg(windows)]
#[test]
fn system_power_shell_merges_only_with_the_same_real_executable_path() {
    let powershell_path =
        resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
    let system = SearchResult {
        id: String::from("system:powershell"),
        title: String::from("PowerShell"),
        subtitle: String::from("Windows PowerShell"),
        kind: ResultKind::Command,
        source: ResultSource::BuiltIn,
        target: Some(String::from("powershell.exe")),
    };
    let app_path = SearchResult {
        id: canonical_application_id(&powershell_path).unwrap(),
        title: String::from("PowerShell"),
        subtitle: String::from("Application • App Paths"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(powershell_path),
    };
    let powershell_7 = SearchResult {
        id: String::from(r"application:target:c:\\program files\\powershell\\7\\pwsh.exe"),
        title: String::from("PowerShell 7"),
        subtitle: String::from("Application • App Paths"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(String::from(r"C:\\Program Files\\PowerShell\\7\\pwsh.exe")),
    };

    let merged = merge_application_duplicates(vec![system, app_path, powershell_7]);
    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|result| result.title == "PowerShell"));
    assert!(merged.iter().any(|result| result.title == "PowerShell 7"));
}

#[cfg(windows)]
#[test]
fn post_merge_exact_console_identity_survives_catalog_collision_and_ranks_first() {
    let powershell_path =
        resolve_bare_executable_path("powershell.exe").expect("PowerShell should resolve");
    let system = SearchResult {
        id: String::from("system:powershell"),
        title: String::from("PowerShell"),
        subtitle: String::from("Windows PowerShell"),
        kind: ResultKind::Command,
        source: ResultSource::BuiltIn,
        target: Some(powershell_path.clone()),
    };
    let catalog = SearchResult {
        id: canonical_application_id(&powershell_path).unwrap(),
        title: String::from("Windows PowerShell"),
        subtitle: String::from("Application • Start Menu"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(powershell_path),
    };

    let mut merged = merge_application_duplicates(vec![system, catalog]);
    rank_results_with_priorities("powershell", &mut merged, &[]);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].id, "system:powershell");
}

#[test]
fn executable_icon_target_detection_accepts_shell_executables_only() {
    assert!(is_executable_icon_target(
        r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
    ));
    assert!(is_executable_icon_target(
        r"C:\\Program Files\\PowerShell\\7\\pwsh.exe"
    ));
    assert!(!is_executable_icon_target(
        r"C:\\Users\\m1nus\\PowerShell.lnk"
    ));
    assert!(!is_executable_icon_target("ms-settings:network-wifi"));
}

#[cfg(windows)]
#[test]
fn builtin_power_shell_target_is_resolved_before_merge_and_icon_loading() {
    let mut results = vec![SearchResult {
        id: String::from("system:powershell"),
        title: String::from("PowerShell"),
        subtitle: String::from("Windows PowerShell"),
        kind: ResultKind::Command,
        source: ResultSource::BuiltIn,
        target: Some(String::from("powershell.exe")),
    }];

    normalize_built_in_executable_targets(&mut results);

    let target = results[0].target.as_deref().unwrap().to_ascii_lowercase();
    assert!(target.ends_with(r"\powershell.exe"));
    assert!(target.contains(r"\windowspowershell\"));
}

#[test]
fn icon_target_preserves_explicit_paths_and_resolves_bare_names_on_windows() {
    let explicit = r"C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    assert_eq!(icon_target_for_path(explicit), explicit);
}

#[cfg(windows)]
#[test]
fn icon_target_resolves_bare_powershell_to_a_real_path() {
    let resolved = icon_target_for_path("powershell.exe").to_ascii_lowercase();
    assert!(resolved.ends_with(r"\powershell.exe"));
    assert!(resolved.contains(r"\windowspowershell\"));
}

#[test]
fn application_results_offer_priority_and_launch_actions_in_order() {
    let result = SearchResult {
        id: String::from("app:probe"),
        title: String::from("Result Mouse Probe"),
        subtitle: String::from("Application • Start Menu"),
        kind: ResultKind::Application,
        source: ResultSource::ApplicationCatalog,
        target: Some(String::from(r"C:\ResultMouseProbe.lnk")),
    };
    let actions = actions_for_result(&result, &std::collections::HashMap::new());
    let labels: Vec<_> = actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Set as priority (move to top)",
            "Open",
            "Run as admin",
            "Open file location",
            "Copy file",
            "Copy folder path",
        ]
    );
    assert!(matches!(actions[0].kind, ActionKind::SetPriority));
    assert!(matches!(actions[1].kind, ActionKind::Open));
    assert!(matches!(actions[2].kind, ActionKind::RunAsAdmin));
    assert!(matches!(actions[3].kind, ActionKind::OpenLocation));
    assert!(matches!(actions[4].kind, ActionKind::CopyFile));
    assert!(matches!(actions[5].kind, ActionKind::CopyFolderPath));
}

#[test]
fn system_results_only_offer_open_and_copy_name_actions() {
    let result = SearchResult {
        id: String::from("system:settings"),
        title: String::from("Settings"),
        subtitle: String::from("Windows Settings"),
        kind: ResultKind::Command,
        source: ResultSource::BuiltIn,
        target: Some(String::from("ms-settings:")),
    };
    let actions = actions_for_result(&result, &std::collections::HashMap::new());
    assert_eq!(actions.len(), 2);
    assert!(matches!(actions[0].kind, ActionKind::Open));
    assert!(matches!(actions[1].kind, ActionKind::CopyName));
}

#[test]
fn copy_path_always_uses_one_pair_of_quotes() {
    let result = SearchResult {
        id: String::from("file:test"),
        title: String::from("Roaming"),
        subtitle: String::new(),
        kind: ResultKind::File,
        source: ResultSource::Everything,
        target: Some(String::from(r#"C:\Users\m1nus\AppData\Roaming"#)),
    };
    assert_eq!(
        quoted_result_path(&result).as_deref(),
        Some(r#""C:\Users\m1nus\AppData\Roaming""#)
    );

    let mut already_quoted = result.clone();
    already_quoted.target = Some(String::from(r#""C:\Users\m1nus\AppData\Roaming""#));
    assert_eq!(
        quoted_result_path(&already_quoted).as_deref(),
        Some(r#""C:\Users\m1nus\AppData\Roaming""#)
    );
}

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
fn history_cursor_walks_older_and_newer_queries() {
    let mut cursor = None;
    cursor = history_cursor_step(4, cursor, Key::Up);
    assert_eq!(cursor, Some(3));
    cursor = history_cursor_step(4, cursor, Key::Up);
    assert_eq!(cursor, Some(2));
    cursor = history_cursor_step(4, cursor, Key::Up);
    assert_eq!(cursor, Some(1));
    cursor = history_cursor_step(4, cursor, Key::Down);
    assert_eq!(cursor, Some(2));
    cursor = history_cursor_step(4, cursor, Key::Down);
    assert_eq!(cursor, Some(3));
    cursor = history_cursor_step(4, cursor, Key::Down);
    assert_eq!(cursor, Some(3));
    assert_eq!(history_cursor_step(0, cursor, Key::Up), None);
}

#[test]
fn stationary_pointer_after_enter_does_not_trigger_hover_selection() {
    let mut last = None;
    assert!(hover_position_changed(&mut last, (240, 120)));
    assert!(!hover_position_changed(&mut last, (240, 120)));
    assert!(hover_position_changed(&mut last, (241, 120)));
}

#[test]
fn extension_aliases_normalize_only_the_everything_query_prefix() {
    assert_eq!(normalize_everything_query(".zip"), "ext:zip");
    assert_eq!(
        normalize_everything_query(".mp4 something"),
        "ext:mp4 something"
    );
    assert_eq!(
        normalize_everything_query("  .pdf  report  "),
        "ext:pdf report"
    );
    assert_eq!(normalize_everything_query("ext:zip"), "ext:zip");
    assert_eq!(normalize_everything_query("settings"), "settings");
    assert_eq!(normalize_everything_query("."), ".");
}


#[test]
fn activation_shows_while_window_is_hidden() {
    // 决定切换方向的是窗口实际可见性，而不是前台归属：隐藏但仍持前台的 launcher
    // 必须被唤起，否则用户按 Alt+Space 看不到任何反应。
    assert!(should_show_launcher(false));
    assert!(!should_show_launcher(true));
}

#[test]
fn automatic_update_always_restarts_hidden() {
    assert_eq!(
        relaunch_mode_for_auto_install(),
        super::updater::RelaunchMode::Hidden
    );
}

#[test]
fn pending_non_empty_query_keeps_previous_result_list_visible() {
    assert!(!should_publish_initial_query_results(true, true, false));
    assert!(should_publish_initial_query_results(true, true, true));
}

#[test]
fn synchronous_built_in_results_can_replace_list_immediately() {
    assert!(should_publish_initial_query_results(true, false, false));
    assert!(should_publish_initial_query_results(false, true, false));
}

#[test]
fn core_provider_snapshot_waits_for_both_search_providers() {
    let mut providers = ProviderResults::default();
    providers.reset(7, Vec::new(), true);
    assert!(!providers.core_ready());

    providers.applications_ready = true;
    assert!(!providers.core_ready());

    providers.everything_ready = true;
    assert!(providers.core_ready());
}

#[test]
fn disabled_everything_does_not_delay_application_snapshot() {
    let mut providers = ProviderResults::default();
    providers.reset(8, Vec::new(), false);
    providers.applications_ready = true;
    assert!(providers.core_ready());
}

#[test]
fn builtin_snapshot_does_not_wait_for_everything() {
    let mut providers = ProviderResults::default();
    providers.reset(
        9,
        vec![SearchResult {
            id: String::from("system:wifi"),
            title: String::from("Wi-Fi"),
            subtitle: String::from("Windows Settings"),
            kind: ResultKind::Command,
            source: ResultSource::BuiltIn,
            target: Some(String::from("ms-settings:network-wifi")),
        }],
        true,
    );
    providers.applications_ready = true;
    assert!(providers.core_ready());
    assert!(!providers.everything_ready);
}

#[test]
fn dimension_sliders_round_trip_at_safe_bounds() {
    assert_eq!(
        dimension_slider_fraction(MIN_LAUNCHER_WIDTH, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        0.0
    );
    assert_eq!(
        dimension_slider_fraction(MAX_LAUNCHER_WIDTH, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        1.0
    );
    assert_eq!(
        dimension_from_slider(0.0, MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT),
        MIN_LAUNCHER_HEIGHT
    );
    assert_eq!(
        dimension_from_slider(1.0, MIN_LAUNCHER_HEIGHT, MAX_LAUNCHER_HEIGHT),
        MAX_LAUNCHER_HEIGHT
    );
    assert_eq!(
        dimension_from_slider(0.5, MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        640
    );
}

#[test]
fn dimension_input_clamps_out_of_range_values_and_rejects_partial_input() {
    assert_eq!(
        parse_dimension_input("100", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        Some(MIN_LAUNCHER_WIDTH)
    );
    assert_eq!(
        parse_dimension_input("1200", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        Some(MAX_LAUNCHER_WIDTH)
    );
    assert_eq!(
        parse_dimension_input("", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        None
    );
    assert_eq!(
        parse_dimension_input("abc", MIN_LAUNCHER_WIDTH, MAX_LAUNCHER_WIDTH),
        None
    );
}

#[test]
fn settings_canvas_stays_fixed_while_visual_values_change() {
    // This is the geometry contract used by the Windows slider smoke: changing
    // either visual value must not resize or drift the Settings HWND itself.
    assert_eq!(
        launcher_window_geometry_with_sizes(true, true, 640, 520),
        (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
    );
    assert_eq!(
        launcher_window_geometry_with_sizes(true, false, 380, 300),
        (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
    );
    assert_eq!(
        launcher_window_geometry_with_sizes(false, true, 640, 520),
        (640, 520)
    );
    assert_eq!(
        launcher_window_geometry_with_sizes(false, false, 640, 520),
        (640, COMPACT_WINDOW_HEIGHT)
    );
}

#[test]
fn custom_geometry_uses_visual_dimensions_and_keeps_compact_height() {
    assert_eq!(
        launcher_window_geometry_with_sizes(false, true, 640, 520),
        (640, 520)
    );
    assert_eq!(
        launcher_window_geometry_with_sizes(false, false, 640, 520),
        (640, COMPACT_WINDOW_HEIGHT)
    );
    assert_eq!(
        launcher_window_geometry_with_sizes(true, true, 640, 520),
        (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
    );
}

#[test]
fn activation_clear_uses_compact_geometry_after_expanded_query() {
    assert_eq!(
        launcher_window_geometry(false, true),
        (
            DEFAULT_LAUNCHER_WIDTH as i32,
            DEFAULT_LAUNCHER_HEIGHT as i32,
        )
    );
    assert_eq!(
        launcher_window_geometry(false, false),
        (DEFAULT_LAUNCHER_WIDTH as i32, COMPACT_WINDOW_HEIGHT)
    );
}

#[test]
fn query_cleanup_keeps_open_settings_at_full_geometry() {
    // hide-on-deactivate clears the query asynchronously. The following
    // query transition must not resize the already-open Settings panel.
    assert_eq!(
        launcher_window_geometry_with_sizes(true, false, 420, 56),
        (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
    );
}

#[test]
fn missing_everything_prompt_uses_visible_dialog_geometry() {
    assert_eq!(
        launcher_window_geometry_with_prompt(false, true, false, 420, 382),
        (
            super::EVERYTHING_PROMPT_WINDOW_WIDTH,
            super::EVERYTHING_PROMPT_WINDOW_HEIGHT
        )
    );
    assert_eq!(
        launcher_window_geometry_with_prompt(true, true, false, 420, 382),
        (super::SETTINGS_WINDOW_WIDTH, super::SETTINGS_WINDOW_HEIGHT)
    );
}

#[test]
fn missing_everything_prompt_does_not_override_normal_launcher_geometry() {
    assert_eq!(
        launcher_window_geometry_with_prompt(false, false, true, 640, 520),
        (640, 520)
    );
    assert_eq!(
        launcher_window_geometry_with_prompt(false, false, false, 640, 520),
        (640, COMPACT_WINDOW_HEIGHT)
    );
}

#[test]
fn everything_prompt_requires_missing_auto_enabled_and_unseen_state() {
    assert!(should_show_everything_install_prompt(
        false, true, false, false
    ));
    assert!(!should_show_everything_install_prompt(
        true, true, false, false
    ));
    assert!(!should_show_everything_install_prompt(
        false, false, false, false
    ));
    assert!(!should_show_everything_install_prompt(
        false, true, true, false
    ));
    assert!(!should_show_everything_install_prompt(
        false, true, false, true
    ));
}

#[test]
fn drive_letter_directory_query_synthesizes_top_result() {
    let dir = std::env::temp_dir().join(format!("flux-path-pin-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let query = dir.to_string_lossy().into_owned();

    let result = synthesize_path_result(&query).expect("existing directory must synthesize");
    assert_eq!(result.kind, ResultKind::File);
    assert_eq!(result.title, query);
    assert_eq!(result.target.as_deref(), Some(query.as_str()));

    let providers = ProviderResults {
        everything: vec![SearchResult::file(
            query.clone(),
            String::from("dir"),
            String::new(),
        )],
        ..ProviderResults::default()
    };
    let merged = providers.merged(&query, &[]);
    assert_eq!(merged[0].id, result.id);
    assert_eq!(
        merged.iter().filter(|item| item.id == result.id).count(),
        1,
        "同一路径在结果中只允许出现一次"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn drive_letter_query_deduplicates_everything_path_case_insensitively() {
    let dir = std::env::temp_dir().join(format!("flux-path-pin-case-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let query = dir.to_string_lossy().into_owned();
    let lower = query.to_ascii_lowercase();

    let providers = ProviderResults {
        everything: vec![SearchResult::file(
            lower,
            String::from("dir"),
            String::new(),
        )],
        ..ProviderResults::default()
    };
    let merged = providers.merged(&query, &[]);
    let pinned = &merged[0];
    assert_eq!(
        pinned
            .target
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        query.to_ascii_lowercase()
    );
    let same_path_count = merged
        .iter()
        .filter(|item| {
            item.target.as_deref().map(|target| {
                target.eq_ignore_ascii_case(pinned.target.as_deref().unwrap_or_default())
            }) == Some(true)
        })
        .count();
    assert_eq!(same_path_count, 1, "大小写不同的同一路径不得重复出现");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn quoted_directory_query_with_trailing_separator_still_synthesizes() {
    let dir = std::env::temp_dir().join(format!("flux-path-pin-quote-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bare = dir.to_string_lossy().into_owned();
    let query = format!(r#""{bare}\""#);

    let result = synthesize_path_result(&query)
        .expect("quoted directory with trailing separator must synthesize");
    assert_eq!(result.title, bare, "标题不得保留结尾分隔符");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn drive_letter_file_query_synthesizes_with_parent_subtitle() {
    let dir = std::env::temp_dir().join(format!("flux-path-pin-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file_path = dir.join("note.txt");
    std::fs::write(&file_path, b"flux").unwrap();
    let query = file_path.to_string_lossy().into_owned();

    let result = synthesize_path_result(&query).expect("existing file must synthesize");
    assert_eq!(result.title, String::from("note.txt"));
    assert_eq!(result.subtitle, dir.to_string_lossy());

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn non_path_or_missing_queries_do_not_synthesize() {
    assert!(synthesize_path_result("steam").is_none());
    assert!(synthesize_path_result("C:").is_none());
    assert!(synthesize_path_result(r"Q:\definitely-missing\path").is_none());
}
